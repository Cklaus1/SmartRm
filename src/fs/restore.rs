use std::io;
use std::path::Path;

use super::Filesystem;
use crate::models::ObjectType;

pub struct RestoreMetadata {
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub mtime_ns: Option<i64>,
}

pub struct RestoreOutcome {
    pub mode_restored: bool,
    pub ownership_restored: bool,
    pub timestamps_restored: bool,
}

/// Restore a filesystem object from archive to the target location.
///
/// For files and directories: tries same-fs rename first, falls back to copy+delete.
/// For symlinks: recreates the symlink at target using the stored `link_target`.
///
/// After content placement, restores metadata in order: mode -> mtime -> uid/gid.
pub fn restore_object(
    fs: &dyn Filesystem,
    archived_path: &Path,
    target_path: &Path,
    object_type: ObjectType,
    link_target: Option<&str>,
    meta: &RestoreMetadata,
    create_parents: bool,
) -> io::Result<RestoreOutcome> {
    // Create parent directories if requested
    if create_parents {
        if let Some(parent) = target_path.parent() {
            fs.create_dir_all(parent)?;
        }
    }

    // Move content from archive to target
    match object_type {
        ObjectType::Symlink => {
            let sym_target = link_target.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "symlink restore requires link_target",
                )
            })?;
            fs.create_symlink(Path::new(sym_target), target_path)?;
            // Remove the archived symlink
            let _ = fs.remove_file(archived_path);
        }
        ObjectType::Dir => {
            let same_fs = fs
                .is_same_filesystem(archived_path, target_path)
                .unwrap_or(false);
            if same_fs {
                fs.rename(archived_path, target_path)?;
            } else {
                fs.copy_dir_recursive(archived_path, target_path)?;
                fs.remove_dir_all(archived_path)?;
            }
        }
        _ => {
            // Regular file
            let same_fs = fs
                .is_same_filesystem(archived_path, target_path)
                .unwrap_or(false);
            if same_fs {
                fs.rename(archived_path, target_path)?;
            } else {
                fs.copy_file(archived_path, target_path)?;
                fs.remove_file(archived_path)?;
            }
        }
    }

    // Restore metadata
    let mut outcome = RestoreOutcome {
        mode_restored: false,
        ownership_restored: false,
        timestamps_restored: false,
    };

    // Symlinks: skip mode/mtime restoration (lchown is handled below)
    if object_type == ObjectType::Symlink {
        // Attempt ownership restoration only
        outcome.ownership_restored = restore_ownership(target_path, meta.uid, meta.gid, true);
        return Ok(outcome);
    }

    // 1. Mode
    if let Some(mode) = meta.mode {
        outcome.mode_restored = restore_mode(target_path, mode);
    }

    // 2. Timestamps (mtime)
    if let Some(mtime_ns) = meta.mtime_ns {
        outcome.timestamps_restored = restore_mtime(target_path, mtime_ns);
    }

    // 3. Ownership (uid/gid) — may fail if not root
    outcome.ownership_restored = restore_ownership(target_path, meta.uid, meta.gid, false);

    Ok(outcome)
}

fn restore_mode(path: &Path, mode: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, perms).is_ok()
}

fn restore_mtime(path: &Path, mtime_ns: i64) -> bool {
    use std::ffi::CString;
    let path_c = match CString::new(path.to_string_lossy().as_bytes().to_vec()) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let sec = mtime_ns / 1_000_000_000;
    let nsec = mtime_ns % 1_000_000_000;

    let times = [
        // atime: set to current time (UTIME_NOW)
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_NOW,
        },
        // mtime: set to stored value
        libc::timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        },
    ];

    let ret = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path_c.as_ptr(),
            times.as_ptr(),
            0, // follow symlinks
        )
    };
    ret == 0
}

fn restore_ownership(path: &Path, uid: Option<u32>, gid: Option<u32>, is_symlink: bool) -> bool {
    use std::ffi::CString;

    if uid.is_none() && gid.is_none() {
        return false;
    }

    let path_c = match CString::new(path.to_string_lossy().as_bytes().to_vec()) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let uid_val = uid.map(|u| u as libc::uid_t).unwrap_or(u32::MAX);
    let gid_val = gid.map(|g| g as libc::gid_t).unwrap_or(u32::MAX);

    let ret = if is_symlink {
        unsafe { libc::lchown(path_c.as_ptr(), uid_val, gid_val) }
    } else {
        unsafe { libc::chown(path_c.as_ptr(), uid_val, gid_val) }
    };

    if ret != 0 {
        // Log warning but don't fail — likely not root
        eprintln!(
            "smartrm: warning: could not restore ownership on '{}': {}",
            path.display(),
            io::Error::last_os_error()
        );
    }

    ret == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_mode_sets_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        assert!(restore_mode(&file, 0o755));
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&file).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o755);
    }

    #[test]
    fn restore_mtime_sets_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "hello").unwrap();

        // Set mtime to a known value (1 second, 0 ns)
        let mtime_ns = 1_000_000_000i64;
        assert!(restore_mtime(&file, mtime_ns));

        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(&file).unwrap();
        assert_eq!(m.mtime(), 1);
    }
}

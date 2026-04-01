pub mod archive;
pub mod disk_space;
pub mod hashing;
pub mod metadata;
pub mod restore;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

pub trait Filesystem {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn read_link(&self, path: &Path) -> io::Result<PathBuf>;
    fn exists(&self, path: &Path) -> bool;
    fn statvfs(&self, path: &Path) -> io::Result<DiskSpace>;
    fn is_same_filesystem(&self, a: &Path, b: &Path) -> io::Result<bool>;
    // For symlink creation during restore
    fn create_symlink(&self, original: &Path, link: &Path) -> io::Result<()>;
    // Recursive directory copy (for cross-fs)
    fn copy_dir_recursive(&self, from: &Path, to: &Path) -> io::Result<u64>;
}

pub struct RealFilesystem;

impl Filesystem for RealFilesystem {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    fn copy_file(&self, from: &Path, to: &Path) -> io::Result<u64> {
        fs::copy(from, to)
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
    fn metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::metadata(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        fs::read_link(path)
    }
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn statvfs(&self, path: &Path) -> io::Result<DiskSpace> {
        disk_space::statvfs_real(path)
    }
    fn is_same_filesystem(&self, a: &Path, b: &Path) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;
        let ma = fs::metadata(a)?;
        // b might not exist yet (archive dir); use parent
        let b_check = if b.exists() {
            b.to_path_buf()
        } else {
            b.parent().unwrap_or(Path::new("/")).to_path_buf()
        };
        let mb = fs::metadata(&b_check)?;
        Ok(ma.dev() == mb.dev())
    }
    fn create_symlink(&self, original: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(original, link)
    }
    fn copy_dir_recursive(&self, from: &Path, to: &Path) -> io::Result<u64> {
        let mut total = 0u64;
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let src = entry.path();
            let dst = to.join(entry.file_name());
            let ft = entry.metadata()?.file_type();
            if ft.is_dir() {
                total += self.copy_dir_recursive(&src, &dst)?;
            } else if ft.is_symlink() {
                let target = fs::read_link(&src)?;
                std::os::unix::fs::symlink(&target, &dst)?;
            } else {
                total += fs::copy(&src, &dst)?;
            }
        }
        Ok(total)
    }
}

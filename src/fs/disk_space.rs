use std::ffi::CString;
use std::io;
use std::path::Path;

use super::DiskSpace;

pub fn statvfs_real(path: &Path) -> io::Result<DiskSpace> {
    let c_path = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(DiskSpace {
            free_bytes: stat.f_bavail as u64 * stat.f_frsize as u64,
            total_bytes: stat.f_blocks as u64 * stat.f_frsize as u64,
        })
    }
}

/// Check if there's enough disk space. Returns Ok(()) if sufficient.
pub fn check_disk_space(
    free_bytes: u64,
    needed_bytes: u64,
    min_free: u64,
) -> Result<(), crate::error::SmartrmError> {
    if free_bytes.saturating_sub(needed_bytes) < min_free {
        return Err(crate::error::SmartrmError::DiskSpaceLow {
            needed: needed_bytes,
            available: free_bytes,
            min_free,
        });
    }
    Ok(())
}

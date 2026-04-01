use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::models::ObjectType;

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub object_type: ObjectType,
    pub size_bytes: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub mtime_ns: i64,
    pub ctime_ns: i64,
    pub link_target: Option<String>,
}

pub fn read_metadata(path: &Path) -> io::Result<FileMetadata> {
    let meta = std::fs::symlink_metadata(path)?;
    let file_type = meta.file_type();

    let object_type = if file_type.is_symlink() {
        ObjectType::Symlink
    } else if file_type.is_dir() {
        ObjectType::Dir
    } else if file_type.is_file() {
        ObjectType::File
    } else {
        ObjectType::Other
    };

    let link_target = if file_type.is_symlink() {
        std::fs::read_link(path)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    // mtime_ns: seconds * 1_000_000_000 + nanoseconds
    let mtime_ns = meta.mtime() * 1_000_000_000 + meta.mtime_nsec();
    let ctime_ns = meta.ctime() * 1_000_000_000 + meta.ctime_nsec();

    Ok(FileMetadata {
        object_type,
        size_bytes: meta.len(),
        mode: meta.mode(),
        uid: meta.uid(),
        gid: meta.gid(),
        mtime_ns,
        ctime_ns,
        link_target,
    })
}

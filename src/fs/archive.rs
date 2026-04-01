use std::io;
use std::path::Path;

use super::Filesystem;
use crate::fs::hashing;

#[derive(Debug)]
pub struct ArchiveResult {
    pub bytes_archived: u64,
    pub content_hash: Option<String>,
}

/// Archive a filesystem object (file, directory, or symlink) into the archive location.
///
/// For same-filesystem: uses rename (instant).
/// For cross-filesystem: copies content, computes SHA-256 during copy, then removes original.
///
/// `source` is the original file path.
/// `archive_path` is the target path in the archive (e.g., `<root>/archive/<ulid>/payload`).
/// The parent directory of archive_path is created if needed.
pub fn archive_object(
    fs: &dyn Filesystem,
    source: &Path,
    archive_path: &Path,
    meta: &crate::fs::metadata::FileMetadata,
) -> io::Result<ArchiveResult> {
    // Ensure parent directory exists
    if let Some(parent) = archive_path.parent() {
        fs.create_dir_all(parent)?;
    }

    // Try same-fs rename first
    let same_fs = fs.is_same_filesystem(source, archive_path).unwrap_or(false);

    if same_fs {
        // Same filesystem — instant rename, no hash
        fs.rename(source, archive_path)?;
        Ok(ArchiveResult {
            bytes_archived: meta.size_bytes,
            content_hash: None,
        })
    } else {
        // Cross-filesystem — need to copy+delete
        match meta.object_type {
            crate::models::ObjectType::Symlink => {
                // Archive symlink: recreate the symlink at archive location
                let target = std::fs::read_link(source)?;
                fs.create_symlink(&target, archive_path)?;
                fs.remove_file(source)?;
                Ok(ArchiveResult {
                    bytes_archived: 0,
                    content_hash: None,
                })
            }
            crate::models::ObjectType::Dir => {
                // Cross-fs directory copy
                let bytes = fs.copy_dir_recursive(source, archive_path)?;
                fs.remove_dir_all(source)?;
                // No hash for directories
                Ok(ArchiveResult {
                    bytes_archived: bytes,
                    content_hash: None,
                })
            }
            _ => {
                // Regular file — copy with hash
                let bytes = fs.copy_file(source, archive_path)?;
                let hash = hashing::hash_file(archive_path)?;
                fs.remove_file(source)?;
                Ok(ArchiveResult {
                    bytes_archived: bytes,
                    content_hash: Some(hash),
                })
            }
        }
    }
}

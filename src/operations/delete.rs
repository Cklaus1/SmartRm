use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{Result, SmartrmError};
use crate::fs::Filesystem;
use crate::id;
use crate::models::*;
use crate::output::HumanOutput;
use crate::policy::{classifier, config::SmartrmConfig, resolver};

pub struct DeleteContext<'a> {
    pub conn: &'a Connection,
    pub fs: &'a dyn Filesystem,
    pub config: &'a SmartrmConfig,
    pub paths: Vec<PathBuf>,
    pub recursive: bool,
    pub force: bool,
    pub interactive_each: bool,
    pub interactive_once: bool,
    pub dir: bool,
    pub verbose: bool,
    pub one_file_system: bool,
    pub permanent: bool,
    pub yes_i_am_sure: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteResult {
    pub batch_id: String,
    pub operation_type: String,
    pub status: String,
    pub requested: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub items: Vec<DeleteItemResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteItemResult {
    pub input_path: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl HumanOutput for DeleteResult {
    fn format_human(&self) -> String {
        // In human mode, only show errors.  Verbose per-file output is
        // emitted to stderr during the archive loop (matching rm behaviour:
        // silent on success unless -v is given).
        let mut out = String::new();
        for item in &self.items {
            if item.status == "failed" {
                if let Some(ref msg) = item.error_message {
                    // Error messages from the operation layer are already
                    // formatted in rm style ("cannot remove 'x': reason").
                    // Just prefix with the program name.
                    out.push_str(&format!("smartrm: {}\n", msg));
                }
            }
        }
        out
    }
}

/// Prompt the user on stderr and read a y/n answer from stdin.
/// Returns true if the user confirms (y/Y/yes), false otherwise.
fn prompt_user(message: &str) -> bool {
    eprint!("{}", message);
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => {
            let trimmed = input.trim().to_lowercase();
            trimmed == "y" || trimmed == "yes"
        }
        Err(_) => false,
    }
}

pub fn execute_delete(ctx: &DeleteContext) -> Result<DeleteResult> {
    let now = chrono::Utc::now().to_rfc3339();
    let batch_id = id::new_id();

    let interactive_mode = ctx.interactive_each || ctx.interactive_once;

    // Create batch
    let batch = Batch {
        batch_id: batch_id.clone(),
        operation_type: OperationType::Delete,
        status: BatchStatus::InProgress,
        requested_by: std::env::var("USER").ok(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        hostname: None,
        command_line: Some(std::env::args().collect::<Vec<_>>().join(" ")),
        total_objects_requested: ctx.paths.len() as i64,
        total_objects_processed: 0,
        total_objects_succeeded: 0,
        total_objects_failed: 0,
        total_bytes: 0,
        interactive_mode,
        used_force: ctx.force,
        started_at: now.clone(),
        completed_at: None,
        summary_message: None,
    };
    db::operations::insert_batch(ctx.conn, &batch)?;

    let data_dir = crate::policy::config::resolve_data_dir(ctx.config);
    let archive_root = data_dir.join("archive");

    // -I: prompt once before bulk operations (>3 files or recursive)
    // -f overrides -i/-I (never prompt when force is set)
    if !ctx.force && ctx.interactive_once && (ctx.paths.len() > 3 || ctx.recursive) {
        let msg = format!("smartrm: archive {} files? ", ctx.paths.len());
        if !prompt_user(&msg) {
            // User declined — return empty success result
            db::operations::update_batch_status(
                ctx.conn,
                &batch_id,
                BatchStatus::Complete,
                0,
                0,
                0,
                0,
            )?;
            db::operations::update_batch_completed(ctx.conn, &batch_id, BatchStatus::Complete, None)?;
            return Ok(DeleteResult {
                batch_id,
                operation_type: "delete".to_string(),
                status: "complete".to_string(),
                requested: ctx.paths.len(),
                succeeded: 0,
                failed: 0,
                items: Vec::new(),
            });
        }
    }

    let mut items = Vec::new();
    let mut succeeded = 0i64;
    let mut failed = 0i64;
    let total_bytes = 0i64;

    // Resolve delete flags for policy
    let flags = resolver::DeleteFlags {
        permanent: ctx.permanent,
        force: ctx.force,
    };

    for input_path in &ctx.paths {
        let result = archive_single_path(ctx, &batch_id, &archive_root, input_path, &flags, &now);

        match result {
            Ok(item) => {
                if item.status == "succeeded" {
                    succeeded += 1;
                } else {
                    // skipped items don't count as failures
                    if item.status == "failed" {
                        failed += 1;
                    }
                }
                items.push(item);
            }
            Err(e) => {
                failed += 1;
                items.push(DeleteItemResult {
                    input_path: input_path.to_string_lossy().to_string(),
                    status: "failed".to_string(),
                    archive_id: None,
                    error_code: Some("error".to_string()),
                    error_message: Some(e.to_string()),
                });
            }
        }
    }

    // Update batch
    let batch_status = if failed == 0 {
        BatchStatus::Complete
    } else if succeeded == 0 {
        BatchStatus::Failed
    } else {
        BatchStatus::Partial
    };

    db::operations::update_batch_status(
        ctx.conn,
        &batch_id,
        batch_status,
        succeeded,
        failed,
        succeeded + failed,
        total_bytes,
    )?;
    db::operations::update_batch_completed(ctx.conn, &batch_id, batch_status, None)?;

    // Record effective policies at batch level
    let sample_classification = if let Some(first) = ctx.paths.first() {
        classifier::classify(first)
    } else {
        classifier::classify(Path::new(""))
    };
    let policy = resolver::resolve_delete_policy(ctx.config, &flags, &sample_classification);
    for src in &policy.source_info {
        let ep = EffectivePolicy {
            effective_policy_id: id::new_id(),
            batch_id: Some(batch_id.clone()),
            archive_id: None,
            setting_key: src.setting_key.clone(),
            setting_value: Some(src.setting_value.clone()),
            source_type: src.source_type,
            source_ref: src.source_ref.clone(),
            created_at: now.clone(),
        };
        db::operations::insert_effective_policy(ctx.conn, &ep)?;
    }

    Ok(DeleteResult {
        batch_id,
        operation_type: "delete".to_string(),
        status: batch_status.as_str().to_string(),
        requested: ctx.paths.len(),
        succeeded: succeeded as usize,
        failed: failed as usize,
        items,
    })
}

fn archive_single_path(
    ctx: &DeleteContext,
    batch_id: &str,
    archive_root: &Path,
    input_path: &Path,
    flags: &resolver::DeleteFlags,
    now: &str,
) -> Result<DeleteItemResult> {
    let path_str = input_path.to_string_lossy().to_string();

    // Resolve to absolute path
    let resolved = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(input_path)
    };

    // Check if path exists
    let meta_result = crate::fs::metadata::read_metadata(&resolved);
    let meta = match meta_result {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && ctx.force => {
            // -f: silently ignore nonexistent files
            return Ok(DeleteItemResult {
                input_path: path_str,
                status: "skipped".to_string(),
                archive_id: None,
                error_code: None,
                error_message: None,
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SmartrmError::NotFound(format!(
                "cannot remove '{}': No such file or directory",
                path_str
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(SmartrmError::Io(e));
        }
        Err(e) => return Err(SmartrmError::Io(e)),
    };

    // Check if directory without -r
    if meta.object_type == ObjectType::Dir && !ctx.recursive {
        if ctx.dir {
            // -d: only allow empty directories
            let entries = std::fs::read_dir(&resolved)
                .map_err(SmartrmError::Io)?
                .count();
            if entries > 0 {
                return Err(SmartrmError::NotFound(format!(
                    "cannot remove '{}': Directory not empty",
                    path_str
                )));
            }
            // Empty directory — allow archive to proceed
        } else {
            return Err(SmartrmError::NotFound(format!(
                "cannot remove '{}': Is a directory",
                path_str
            )));
        }
    }

    // -i: prompt before each file (force overrides interactive)
    if !ctx.force && ctx.interactive_each {
        let msg = format!("smartrm: archive '{}'? ", path_str);
        if !prompt_user(&msg) {
            return Ok(DeleteItemResult {
                input_path: path_str,
                status: "skipped".to_string(),
                archive_id: None,
                error_code: None,
                error_message: None,
            });
        }
    }

    // --one-file-system: warn if cross-filesystem situation detected
    if ctx.one_file_system && ctx.recursive && meta.object_type == ObjectType::Dir {
        if let Ok(same_fs) = ctx.fs.is_same_filesystem(&resolved, archive_root) {
            if !same_fs {
                eprintln!(
                    "smartrm: warning: skipping cross-filesystem content in '{}'",
                    path_str
                );
            }
        }
    }

    // Classify
    let classification = classifier::classify(&resolved);

    // Check danger level
    match &classification.danger_level {
        DangerLevel::Blocked(msg) => {
            return Err(SmartrmError::DangerBlocked(msg.clone()));
        }
        DangerLevel::Warning(msg) if !ctx.yes_i_am_sure && !ctx.force => {
            return Err(SmartrmError::DangerBlocked(format!(
                "{}: use --yes-i-am-sure to proceed",
                msg
            )));
        }
        _ => {}
    }

    // Check disk space
    let disk = ctx
        .fs
        .statvfs(archive_root)
        .unwrap_or(crate::fs::DiskSpace {
            free_bytes: u64::MAX,
            total_bytes: u64::MAX,
        });
    crate::fs::disk_space::check_disk_space(
        disk.free_bytes,
        meta.size_bytes,
        ctx.config.min_free_space_bytes,
    )?;

    // Resolve policy
    let policy = resolver::resolve_delete_policy(ctx.config, flags, &classification);

    // --- Permanent delete path ---
    // Records metadata for audit trail but does not archive content.
    if policy.delete_mode == "permanent" {
        return permanent_delete(ctx, batch_id, &path_str, &resolved, &meta, &policy, now);
    }

    // --- Archive path (default) ---

    // Generate IDs
    let archive_id = id::new_id();
    let batch_item_id = id::new_id();
    let archive_path = archive_root.join(&archive_id).join("payload");

    // Build model objects
    let obj = ArchiveObject {
        archive_id: archive_id.clone(),
        batch_id: batch_id.to_string(),
        parent_archive_id: None,
        object_type: meta.object_type,
        state: LifecycleState::Archived,
        original_path: resolved.to_string_lossy().to_string(),
        archived_path: Some(archive_path.to_string_lossy().to_string()),
        storage_mount_id: None,
        original_mount_id: None,
        size_bytes: Some(meta.size_bytes as i64),
        content_hash: None, // filled after archive
        link_target: meta.link_target.clone(),
        mode: Some(meta.mode),
        uid: Some(meta.uid),
        gid: Some(meta.gid),
        mtime_ns: Some(meta.mtime_ns),
        ctime_ns: Some(meta.ctime_ns),
        delete_intent: policy.delete_intent.clone(),
        ttl_seconds: policy.ttl_seconds,
        policy_id: None,
        delete_reason: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
        restored_at: None,
        expired_at: None,
        purged_at: None,
        failure_code: None,
        failure_message: None,
    };

    let item = BatchItem {
        batch_item_id: batch_item_id.clone(),
        batch_id: batch_id.to_string(),
        input_path: path_str.clone(),
        resolved_path: Some(resolved.to_string_lossy().to_string()),
        archive_id: Some(archive_id.clone()),
        status: BatchItemStatus::Pending,
        error_code: None,
        error_message: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };

    // Per-file transaction: DB write -> FS move -> commit
    ctx.conn.execute("BEGIN IMMEDIATE", [])?;

    match (|| -> Result<()> {
        db::operations::insert_archive_object(ctx.conn, &obj)?;
        db::operations::insert_batch_item(ctx.conn, &item)?;
        Ok(())
    })() {
        Ok(()) => {}
        Err(e) => {
            ctx.conn.execute("ROLLBACK", []).ok();
            return Err(e);
        }
    }

    // FS move
    let archive_result =
        crate::fs::archive::archive_object(ctx.fs, &resolved, &archive_path, &meta);

    match archive_result {
        Ok(ar) => {
            // Update hash if we got one
            if let Some(ref hash) = ar.content_hash {
                ctx.conn
                    .execute(
                        "UPDATE archive_objects SET content_hash = ?1 WHERE archive_id = ?2",
                        rusqlite::params![hash, archive_id],
                    )
                    .ok();
            }

            // Update batch item to succeeded
            ctx.conn
                .execute(
                    "UPDATE batch_items SET status = 'succeeded', updated_at = ?1 WHERE batch_item_id = ?2",
                    rusqlite::params![now, batch_item_id],
                )
                .ok();

            // Commit
            match ctx.conn.execute("COMMIT", []) {
                Ok(_) => {
                    if ctx.verbose {
                        eprintln!(
                            "archived '{}' ({})",
                            path_str,
                            id::short_id(&archive_id)
                        );
                    }
                    Ok(DeleteItemResult {
                        input_path: path_str,
                        status: "succeeded".to_string(),
                        archive_id: Some(archive_id),
                        error_code: None,
                        error_message: None,
                    })
                }
                Err(e) => {
                    // Commit failed after FS move — compensating rollback
                    // Move file back from archive to original location
                    let _ = ctx.fs.rename(&archive_path, &resolved);
                    Err(SmartrmError::Db(e))
                }
            }
        }
        Err(e) => {
            // FS move failed — rollback DB transaction
            ctx.conn.execute("ROLLBACK", []).ok();

            // Record failure separately (outside the rolled-back transaction)
            let failed_obj = ArchiveObject {
                state: LifecycleState::Failed,
                failure_code: Some("io_error".to_string()),
                failure_message: Some(e.to_string()),
                ..obj
            };
            // Try to record the failure — best effort
            let _ = db::operations::insert_archive_object(ctx.conn, &failed_obj);
            let failed_item = BatchItem {
                status: BatchItemStatus::Failed,
                error_code: Some("io_error".to_string()),
                error_message: Some(e.to_string()),
                ..item
            };
            let _ = db::operations::insert_batch_item(ctx.conn, &failed_item);

            Ok(DeleteItemResult {
                input_path: path_str,
                status: "failed".to_string(),
                archive_id: None,
                error_code: Some("io_error".to_string()),
                error_message: Some(e.to_string()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx<'a>(
        conn: &'a Connection,
        fs: &'a dyn Filesystem,
        config: &'a crate::policy::config::SmartrmConfig,
        paths: Vec<PathBuf>,
    ) -> DeleteContext<'a> {
        DeleteContext {
            conn,
            fs,
            config,
            paths,
            recursive: false,
            force: false,
            interactive_each: false,
            interactive_once: false,
            dir: false,
            verbose: false,
            one_file_system: false,
            permanent: false,
            yes_i_am_sure: false,
            json: false,
        }
    }

    #[test]
    fn force_skips_missing_files() {
        let conn = crate::db::open_memory_database().unwrap();
        let config = crate::policy::config::SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let mut ctx = make_ctx(
            &conn,
            &fs,
            &config,
            vec![PathBuf::from("/nonexistent/file/that/does/not/exist.txt")],
        );
        ctx.force = true;

        let result = execute_delete(&ctx).unwrap();
        assert_eq!(result.failed, 0);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].status, "skipped");
    }

    #[test]
    fn missing_file_without_force_fails() {
        let conn = crate::db::open_memory_database().unwrap();
        let config = crate::policy::config::SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let ctx = make_ctx(
            &conn,
            &fs,
            &config,
            vec![PathBuf::from("/nonexistent/file/that/does/not/exist.txt")],
        );

        let result = execute_delete(&ctx).unwrap();
        assert_eq!(result.failed, 1);
        assert_eq!(result.items[0].status, "failed");
    }

    #[test]
    fn dir_flag_rejects_nonempty_directory() {
        // Create a temp directory with a file inside
        let tmp = std::env::temp_dir().join("smartrm_test_nonempty_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("file.txt"), "content").unwrap();

        let conn = crate::db::open_memory_database().unwrap();
        let config = crate::policy::config::SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let mut ctx = make_ctx(&conn, &fs, &config, vec![tmp.clone()]);
        ctx.dir = true;

        let result = execute_delete(&ctx).unwrap();
        assert_eq!(result.failed, 1);
        assert!(result.items[0]
            .error_message
            .as_ref()
            .unwrap()
            .contains("Directory not empty"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dir_flag_allows_empty_directory() {
        // Create an empty temp directory
        let tmp = std::env::temp_dir().join("smartrm_test_empty_dir");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let conn = crate::db::open_memory_database().unwrap();
        let mut config = crate::policy::config::SmartrmConfig::default();
        // Use a temp archive root
        let archive_tmp = std::env::temp_dir().join("smartrm_test_archive_empty_dir");
        let _ = std::fs::remove_dir_all(&archive_tmp);
        std::fs::create_dir_all(&archive_tmp).unwrap();
        config.archive_root = Some(archive_tmp.to_string_lossy().to_string());

        let fs = crate::fs::RealFilesystem;

        let mut ctx = make_ctx(&conn, &fs, &config, vec![tmp.clone()]);
        ctx.dir = true;

        let result = execute_delete(&ctx).unwrap();
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.items[0].status, "succeeded");

        // Cleanup
        let _ = std::fs::remove_dir_all(&archive_tmp);
    }

    #[test]
    fn directory_without_r_or_d_fails() {
        let tmp = std::env::temp_dir().join("smartrm_test_dir_no_flags");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let conn = crate::db::open_memory_database().unwrap();
        let config = crate::policy::config::SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let ctx = make_ctx(&conn, &fs, &config, vec![tmp.clone()]);

        let result = execute_delete(&ctx).unwrap();
        assert_eq!(result.failed, 1);
        assert!(result.items[0]
            .error_message
            .as_ref()
            .unwrap()
            .contains("Is a directory"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn format_human_only_shows_failures() {
        let result = DeleteResult {
            batch_id: "test".to_string(),
            operation_type: "delete".to_string(),
            status: "partial".to_string(),
            requested: 2,
            succeeded: 1,
            failed: 1,
            items: vec![
                DeleteItemResult {
                    input_path: "good.txt".to_string(),
                    status: "succeeded".to_string(),
                    archive_id: Some("abc123".to_string()),
                    error_code: None,
                    error_message: None,
                },
                DeleteItemResult {
                    input_path: "bad.txt".to_string(),
                    status: "failed".to_string(),
                    archive_id: None,
                    error_code: Some("io_error".to_string()),
                    error_message: Some("cannot remove 'bad.txt': Permission denied".to_string()),
                },
            ],
        };

        let human = result.format_human();
        // Should NOT contain succeeded item
        assert!(!human.contains("good.txt"));
        // Should contain failed item
        assert!(human.contains("bad.txt"));
        assert!(human.contains("Permission denied"));
    }

    #[test]
    fn format_human_empty_on_all_success() {
        let result = DeleteResult {
            batch_id: "test".to_string(),
            operation_type: "delete".to_string(),
            status: "complete".to_string(),
            requested: 1,
            succeeded: 1,
            failed: 0,
            items: vec![DeleteItemResult {
                input_path: "file.txt".to_string(),
                status: "succeeded".to_string(),
                archive_id: Some("abc123".to_string()),
                error_code: None,
                error_message: None,
            }],
        };

        let human = result.format_human();
        assert!(human.is_empty(), "success-only output should be empty (silent like rm)");
    }
}

/// Permanently delete a file: remove from disk, record metadata with state=purged.
/// No archive content is created — this is a true delete with audit trail.
fn permanent_delete(
    ctx: &DeleteContext,
    batch_id: &str,
    path_str: &str,
    resolved: &Path,
    meta: &crate::fs::metadata::FileMetadata,
    policy: &resolver::ResolvedPolicy,
    now: &str,
) -> Result<DeleteItemResult> {
    let archive_id = id::new_id();
    let batch_item_id = id::new_id();

    // Build archive_object with state=purged and no archived_path
    let obj = ArchiveObject {
        archive_id: archive_id.clone(),
        batch_id: batch_id.to_string(),
        parent_archive_id: None,
        object_type: meta.object_type,
        state: LifecycleState::Purged,
        original_path: resolved.to_string_lossy().to_string(),
        archived_path: None, // no content archived
        storage_mount_id: None,
        original_mount_id: None,
        size_bytes: Some(meta.size_bytes as i64),
        content_hash: None,
        link_target: meta.link_target.clone(),
        mode: Some(meta.mode),
        uid: Some(meta.uid),
        gid: Some(meta.gid),
        mtime_ns: Some(meta.mtime_ns),
        ctime_ns: Some(meta.ctime_ns),
        delete_intent: policy.delete_intent.clone(),
        ttl_seconds: None,
        policy_id: None,
        delete_reason: Some("permanent delete".to_string()),
        created_at: now.to_string(),
        updated_at: now.to_string(),
        restored_at: None,
        expired_at: None,
        purged_at: Some(now.to_string()),
        failure_code: None,
        failure_message: None,
    };

    let item = BatchItem {
        batch_item_id: batch_item_id.clone(),
        batch_id: batch_id.to_string(),
        input_path: path_str.to_string(),
        resolved_path: Some(resolved.to_string_lossy().to_string()),
        archive_id: Some(archive_id.clone()),
        status: BatchItemStatus::Pending,
        error_code: None,
        error_message: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };

    // DB write
    ctx.conn.execute("BEGIN IMMEDIATE", [])?;
    match (|| -> Result<()> {
        db::operations::insert_archive_object(ctx.conn, &obj)?;
        db::operations::insert_batch_item(ctx.conn, &item)?;
        Ok(())
    })() {
        Ok(()) => {}
        Err(e) => {
            ctx.conn.execute("ROLLBACK", []).ok();
            return Err(e);
        }
    }

    // FS delete (true removal)
    let fs_result = if meta.object_type == ObjectType::Dir {
        ctx.fs.remove_dir_all(resolved).map_err(SmartrmError::Io)
    } else {
        ctx.fs.remove_file(resolved).map_err(SmartrmError::Io)
    };

    match fs_result {
        Ok(()) => {
            // Update batch item to succeeded
            ctx.conn
                .execute(
                    "UPDATE batch_items SET status = 'succeeded', updated_at = ?1 WHERE batch_item_id = ?2",
                    rusqlite::params![now, batch_item_id],
                )
                .ok();

            match ctx.conn.execute("COMMIT", []) {
                Ok(_) => {
                    if ctx.verbose {
                        eprintln!("permanently deleted '{}'", path_str);
                    }
                    Ok(DeleteItemResult {
                        input_path: path_str.to_string(),
                        status: "succeeded".to_string(),
                        archive_id: Some(archive_id),
                        error_code: None,
                        error_message: None,
                    })
                }
                Err(e) => {
                    // File is already gone — can't roll back FS delete
                    Err(SmartrmError::Db(e))
                }
            }
        }
        Err(e) => {
            ctx.conn.execute("ROLLBACK", []).ok();

            // Record failure
            let failed_obj = ArchiveObject {
                state: LifecycleState::Failed,
                failure_code: Some("io_error".to_string()),
                failure_message: Some(e.to_string()),
                ..obj
            };
            let _ = db::operations::insert_archive_object(ctx.conn, &failed_obj);
            let failed_item = BatchItem {
                status: BatchItemStatus::Failed,
                error_code: Some("io_error".to_string()),
                error_message: Some(e.to_string()),
                ..item
            };
            let _ = db::operations::insert_batch_item(ctx.conn, &failed_item);

            Ok(DeleteItemResult {
                input_path: path_str.to_string(),
                status: "failed".to_string(),
                archive_id: None,
                error_code: Some("io_error".to_string()),
                error_message: Some(e.to_string()),
            })
        }
    }
}

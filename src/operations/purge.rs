use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{Result, SmartrmError};
use crate::fs::Filesystem;
use crate::id;
use crate::models::*;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

// ---------------------------------------------------------------------------
// Context & result types
// ---------------------------------------------------------------------------

pub struct PurgeContext<'a> {
    pub conn: &'a Connection,
    pub fs: &'a dyn Filesystem,
    pub config: &'a SmartrmConfig,
    pub expired_only: bool,
    pub all: bool,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeResult {
    pub purged_count: usize,
    pub bytes_freed: u64,
    pub db_deleted: bool,
}

impl HumanOutput for PurgeResult {
    fn format_human(&self) -> String {
        if self.purged_count == 0 {
            return "nothing to purge\n".to_string();
        }

        let mut out = format!(
            "purged {} objects, {} freed\n",
            self.purged_count,
            crate::output::human::format_bytes(self.bytes_freed),
        );

        if self.db_deleted {
            out.push_str("database deleted\n");
        }

        out
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn execute_purge(ctx: &PurgeContext) -> Result<PurgeResult> {
    // If expired_only, delegate to cleanup logic
    if ctx.expired_only {
        let cleanup_ctx = super::cleanup::CleanupContext {
            conn: ctx.conn,
            fs: ctx.fs,
            config: ctx.config,
            older_than: None,
            expired_only: true,
            dry_run: false,
            force: ctx.force,
            json: ctx.json,
        };
        let cleanup_result = super::cleanup::execute_cleanup(&cleanup_ctx)?;
        return Ok(PurgeResult {
            purged_count: cleanup_result.purged,
            bytes_freed: cleanup_result.bytes_freed,
            db_deleted: false,
        });
    }

    // For "all" or no filter: purge everything
    if !ctx.all && !ctx.force {
        // Without --all or --force, show summary and refuse
        let (count, total_bytes) = db::queries::count_all_archived(ctx.conn)?;
        if count == 0 {
            return Ok(PurgeResult {
                purged_count: 0,
                bytes_freed: 0,
                db_deleted: false,
            });
        }

        return Err(SmartrmError::GateDenied(format!(
            "purge would destroy {} objects ({}). Use --all --force to confirm.",
            count,
            crate::output::human::format_bytes(total_bytes as u64),
        )));
    }

    // Count what we'll purge
    let (count, total_bytes) = db::queries::count_all_archived(ctx.conn)?;

    if count == 0 {
        return Ok(PurgeResult {
            purged_count: 0,
            bytes_freed: 0,
            db_deleted: false,
        });
    }

    // Create batch
    let now = chrono::Utc::now().to_rfc3339();
    let batch_id = id::new_id();
    let batch = Batch {
        batch_id: batch_id.clone(),
        operation_type: OperationType::Purge,
        status: BatchStatus::InProgress,
        requested_by: std::env::var("USER").ok(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        hostname: None,
        command_line: Some(std::env::args().collect::<Vec<_>>().join(" ")),
        total_objects_requested: count,
        total_objects_processed: 0,
        total_objects_succeeded: 0,
        total_objects_failed: 0,
        total_bytes: 0,
        interactive_mode: false,
        used_force: ctx.force,
        started_at: now.clone(),
        completed_at: None,
        summary_message: None,
    };
    db::operations::insert_batch(ctx.conn, &batch)?;

    // Delete the entire archive directory
    let archive_dir = crate::policy::config::archive_dir(ctx.config);
    if ctx.fs.exists(&archive_dir) {
        ctx.fs
            .remove_dir_all(&archive_dir)
            .map_err(SmartrmError::Io)?;
    }

    // Update all archived/expired objects to purged
    db::queries::purge_all_archived(ctx.conn)?;

    // Complete the batch
    db::operations::update_batch_status(
        ctx.conn,
        &batch_id,
        BatchStatus::Complete,
        count,
        0,
        count,
        total_bytes,
    )?;
    db::operations::update_batch_completed(ctx.conn, &batch_id, BatchStatus::Complete, None)?;

    Ok(PurgeResult {
        purged_count: count as usize,
        bytes_freed: total_bytes as u64,
        db_deleted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_empty_archive() {
        let conn = crate::db::open_memory_database().unwrap();
        let config = SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let ctx = PurgeContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            expired_only: false,
            all: true,
            force: true,
            json: false,
        };

        let result = execute_purge(&ctx).unwrap();
        assert_eq!(result.purged_count, 0);
        assert_eq!(result.bytes_freed, 0);
    }

    #[test]
    fn purge_requires_all_or_force() {
        let conn = crate::db::open_memory_database().unwrap();

        // Insert something to purge
        let batch = Batch {
            batch_id: "batch_purge_test".to_string(),
            operation_type: OperationType::Delete,
            status: BatchStatus::Complete,
            requested_by: None,
            cwd: None,
            hostname: None,
            command_line: None,
            total_objects_requested: 1,
            total_objects_processed: 1,
            total_objects_succeeded: 1,
            total_objects_failed: 0,
            total_bytes: 0,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        let obj = ArchiveObject {
            archive_id: "obj_purge_test".to_string(),
            batch_id: "batch_purge_test".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/test.txt".to_string(),
            archived_path: Some("/archive/obj_purge_test/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(1024),
            content_hash: None,
            link_target: None,
            mode: None,
            uid: None,
            gid: None,
            mtime_ns: None,
            ctime_ns: None,
            delete_intent: None,
            ttl_seconds: None,
            policy_id: None,
            delete_reason: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            restored_at: None,
            expired_at: None,
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj).unwrap();

        let config = SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        // Without --all, should get GateDenied
        let ctx = PurgeContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            expired_only: false,
            all: false,
            force: false,
            json: false,
        };

        let result = execute_purge(&ctx);
        assert!(result.is_err());
        match result.unwrap_err() {
            SmartrmError::GateDenied(_) => {} // expected
            other => panic!("expected GateDenied, got: {}", other),
        }
    }

    #[test]
    fn purge_all_force_transitions_to_purged() {
        let conn = crate::db::open_memory_database().unwrap();

        let batch = Batch {
            batch_id: "batch_purge_all".to_string(),
            operation_type: OperationType::Delete,
            status: BatchStatus::Complete,
            requested_by: None,
            cwd: None,
            hostname: None,
            command_line: None,
            total_objects_requested: 1,
            total_objects_processed: 1,
            total_objects_succeeded: 1,
            total_objects_failed: 0,
            total_bytes: 0,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        let obj = ArchiveObject {
            archive_id: "obj_purge_all".to_string(),
            batch_id: "batch_purge_all".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/test.txt".to_string(),
            archived_path: Some("/nonexistent/obj_purge_all/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(2048),
            content_hash: None,
            link_target: None,
            mode: None,
            uid: None,
            gid: None,
            mtime_ns: None,
            ctime_ns: None,
            delete_intent: None,
            ttl_seconds: None,
            policy_id: None,
            delete_reason: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            restored_at: None,
            expired_at: None,
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj).unwrap();

        // Use a config with a non-existent archive_root so remove_dir_all doesn't fail on real dirs
        let mut config = SmartrmConfig::default();
        config.archive_root = Some("/tmp/smartrm-purge-test-nonexistent".to_string());
        let fs = crate::fs::RealFilesystem;

        let ctx = PurgeContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            expired_only: false,
            all: true,
            force: true,
            json: false,
        };

        let result = execute_purge(&ctx).unwrap();
        assert_eq!(result.purged_count, 1);
        assert_eq!(result.bytes_freed, 2048);

        // Verify state transition
        let fetched =
            db::queries::get_archive_object(&conn, "obj_purge_all").unwrap().unwrap();
        assert_eq!(fetched.state, LifecycleState::Purged);
        assert!(fetched.purged_at.is_some());
    }
}

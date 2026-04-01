use std::path::PathBuf;

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
// Duration parsing
// ---------------------------------------------------------------------------

/// Parse a human-readable duration string into seconds.
///
/// Supported formats: `Nd` (days), `Nh` (hours).
/// Examples: `"30d"`, `"7d"`, `"24h"`, `"12h"`.
pub fn parse_duration_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_part, unit) = s.split_at(s.len() - 1);
    let value: i64 = num_part.parse().ok()?;
    if value < 0 {
        return None;
    }

    match unit {
        "d" => Some(value * 86400),
        "h" => Some(value * 3600),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Context & result types
// ---------------------------------------------------------------------------

pub struct CleanupContext<'a> {
    pub conn: &'a Connection,
    pub fs: &'a dyn Filesystem,
    pub config: &'a SmartrmConfig,
    pub older_than: Option<String>,
    pub expired_only: bool,
    pub dry_run: bool,
    pub force: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupResult {
    pub batch_id: String,
    pub status: String,
    pub purged: usize,
    pub skipped: usize,
    pub protected_skipped: usize,
    pub bytes_freed: u64,
    pub dry_run: bool,
    pub items: Vec<CleanupItemResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupItemResult {
    pub archive_id: String,
    pub original_path: String,
    pub size_bytes: i64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HumanOutput for CleanupResult {
    fn format_human(&self) -> String {
        let mut out = String::new();

        if self.dry_run {
            out.push_str("DRY RUN — no changes made\n");
        }

        for item in &self.items {
            match item.status.as_str() {
                "purged" => {
                    let size = crate::output::human::format_bytes(item.size_bytes as u64);
                    out.push_str(&format!(
                        "purged '{}' ({}, {})\n",
                        item.original_path,
                        id::short_id(&item.archive_id),
                        size,
                    ));
                }
                "would_purge" => {
                    let size = crate::output::human::format_bytes(item.size_bytes as u64);
                    out.push_str(&format!(
                        "would purge '{}' ({}, {})\n",
                        item.original_path,
                        id::short_id(&item.archive_id),
                        size,
                    ));
                }
                "skipped" => {
                    out.push_str(&format!(
                        "skipped '{}' ({})\n",
                        item.original_path,
                        item.reason.as_deref().unwrap_or("protected"),
                    ));
                }
                _ => {}
            }
        }

        let verb = if self.dry_run { "would purge" } else { "purged" };
        out.push_str(&format!(
            "{} {} objects, {} skipped ({} protected), {} freed\n",
            verb,
            self.purged,
            self.skipped,
            self.protected_skipped,
            crate::output::human::format_bytes(self.bytes_freed),
        ));

        out
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn execute_cleanup(ctx: &CleanupContext) -> Result<CleanupResult> {
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();
    let batch_id = id::new_id();

    // 1. Compute cutoff timestamp from older_than
    let cutoff_timestamp = match &ctx.older_than {
        Some(dur_str) => {
            let secs = parse_duration_secs(dur_str).ok_or_else(|| {
                SmartrmError::Config(format!(
                    "invalid duration '{}': use Nd (days) or Nh (hours), e.g. 30d, 24h",
                    dur_str
                ))
            })?;
            let cutoff = now - chrono::Duration::seconds(secs);
            Some(cutoff.to_rfc3339())
        }
        None => None,
    };

    // 2. Query eligible objects
    let objects = db::queries::get_objects_for_cleanup(
        ctx.conn,
        cutoff_timestamp.as_deref(),
        ctx.expired_only,
    )?;

    if objects.is_empty() {
        return Ok(CleanupResult {
            batch_id,
            status: "complete".to_string(),
            purged: 0,
            skipped: 0,
            protected_skipped: 0,
            bytes_freed: 0,
            dry_run: ctx.dry_run,
            items: Vec::new(),
        });
    }

    // 3. Create batch (unless dry run)
    if !ctx.dry_run {
        let batch = Batch {
            batch_id: batch_id.clone(),
            operation_type: OperationType::Cleanup,
            status: BatchStatus::InProgress,
            requested_by: std::env::var("USER").ok(),
            cwd: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            hostname: None,
            command_line: Some(std::env::args().collect::<Vec<_>>().join(" ")),
            total_objects_requested: objects.len() as i64,
            total_objects_processed: 0,
            total_objects_succeeded: 0,
            total_objects_failed: 0,
            total_bytes: 0,
            interactive_mode: false,
            used_force: ctx.force,
            started_at: now_str.clone(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(ctx.conn, &batch)?;
    }

    // 4. Process each object
    let mut items = Vec::new();
    let mut purged = 0usize;
    let mut skipped = 0usize;
    let mut protected_skipped = 0usize;
    let mut bytes_freed = 0u64;

    for obj in &objects {
        let size = obj.size_bytes.unwrap_or(0).max(0) as u64;

        // Check if protected (re-classify from original_path)
        let classification =
            crate::policy::classifier::classify(std::path::Path::new(&obj.original_path));
        let is_protected = classification
            .tags
            .contains(&crate::models::Tag::Protected);

        if is_protected && !ctx.force {
            protected_skipped += 1;
            skipped += 1;
            items.push(CleanupItemResult {
                archive_id: obj.archive_id.clone(),
                original_path: obj.original_path.clone(),
                size_bytes: obj.size_bytes.unwrap_or(0),
                status: "skipped".to_string(),
                reason: Some("protected (use --force to override)".to_string()),
            });
            continue;
        }

        if ctx.dry_run {
            purged += 1;
            bytes_freed += size;
            items.push(CleanupItemResult {
                archive_id: obj.archive_id.clone(),
                original_path: obj.original_path.clone(),
                size_bytes: obj.size_bytes.unwrap_or(0),
                status: "would_purge".to_string(),
                reason: None,
            });
            continue;
        }

        // Delete archive content from disk
        if let Some(ref archived_path) = obj.archived_path {
            let archive_payload = PathBuf::from(archived_path);
            // The archive dir is the parent of "payload"
            let archive_dir = archive_payload
                .parent()
                .unwrap_or(&archive_payload);
            if ctx.fs.exists(archive_dir) {
                ctx.fs
                    .remove_dir_all(archive_dir)
                    .map_err(SmartrmError::Io)?;
            }
        }

        // Update state to purged
        db::operations::update_archive_object_state(
            ctx.conn,
            &obj.archive_id,
            LifecycleState::Purged,
            None,
        )?;

        purged += 1;
        bytes_freed += size;
        items.push(CleanupItemResult {
            archive_id: obj.archive_id.clone(),
            original_path: obj.original_path.clone(),
            size_bytes: obj.size_bytes.unwrap_or(0),
            status: "purged".to_string(),
            reason: None,
        });
    }

    // 5. Update batch
    if !ctx.dry_run {
        let batch_status = if skipped > 0 && purged > 0 {
            BatchStatus::Partial
        } else if purged > 0 {
            BatchStatus::Complete
        } else {
            BatchStatus::Complete
        };
        db::operations::update_batch_status(
            ctx.conn,
            &batch_id,
            batch_status,
            purged as i64,
            0,
            (purged + skipped) as i64,
            bytes_freed as i64,
        )?;
        db::operations::update_batch_completed(ctx.conn, &batch_id, batch_status, None)?;
    }

    Ok(CleanupResult {
        batch_id,
        status: "complete".to_string(),
        purged,
        skipped,
        protected_skipped,
        bytes_freed,
        dry_run: ctx.dry_run,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_days() {
        assert_eq!(parse_duration_secs("30d"), Some(30 * 86400));
        assert_eq!(parse_duration_secs("7d"), Some(7 * 86400));
        assert_eq!(parse_duration_secs("1d"), Some(86400));
    }

    #[test]
    fn parse_duration_hours() {
        assert_eq!(parse_duration_secs("24h"), Some(24 * 3600));
        assert_eq!(parse_duration_secs("12h"), Some(12 * 3600));
        assert_eq!(parse_duration_secs("1h"), Some(3600));
    }

    #[test]
    fn parse_duration_invalid() {
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_duration_secs("abc"), None);
        assert_eq!(parse_duration_secs("30m"), None); // minutes not supported
        assert_eq!(parse_duration_secs("d"), None);
    }

    #[test]
    fn parse_duration_zero() {
        assert_eq!(parse_duration_secs("0d"), Some(0));
        assert_eq!(parse_duration_secs("0h"), Some(0));
    }

    #[test]
    fn cleanup_empty_archive() {
        let conn = crate::db::open_memory_database().unwrap();
        let config = SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        let ctx = CleanupContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            older_than: Some("30d".to_string()),
            expired_only: false,
            dry_run: true,
            force: false,
            json: false,
        };

        let result = execute_cleanup(&ctx).unwrap();
        assert_eq!(result.purged, 0);
        assert_eq!(result.skipped, 0);
        assert!(result.dry_run);
    }

    #[test]
    fn cleanup_filters_expired_only() {
        let conn = crate::db::open_memory_database().unwrap();

        // Insert a batch
        let batch = Batch {
            batch_id: "batch_cleanup".to_string(),
            operation_type: OperationType::Delete,
            status: BatchStatus::Complete,
            requested_by: None,
            cwd: None,
            hostname: None,
            command_line: None,
            total_objects_requested: 2,
            total_objects_processed: 2,
            total_objects_succeeded: 2,
            total_objects_failed: 0,
            total_bytes: 0,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-01-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        // Insert an archived object (not expired)
        let obj1 = ArchiveObject {
            archive_id: "obj_archived_1".to_string(),
            batch_id: "batch_cleanup".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/normal.txt".to_string(),
            archived_path: Some("/archive/obj_archived_1/payload".to_string()),
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
        db::operations::insert_archive_object(&conn, &obj1).unwrap();

        // Insert an expired object
        let obj2 = ArchiveObject {
            archive_id: "obj_expired_1".to_string(),
            state: LifecycleState::Expired,
            original_path: "/tmp/expired.txt".to_string(),
            archived_path: Some("/archive/obj_expired_1/payload".to_string()),
            expired_at: Some("2026-03-01T00:00:00Z".to_string()),
            ..obj1.clone()
        };
        db::operations::insert_archive_object(&conn, &obj2).unwrap();

        let config = SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        // Expired-only dry run should only see the expired object
        let ctx = CleanupContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            older_than: None,
            expired_only: true,
            dry_run: true,
            force: false,
            json: false,
        };

        let result = execute_cleanup(&ctx).unwrap();
        assert_eq!(result.purged, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].original_path, "/tmp/expired.txt");
    }

    #[test]
    fn cleanup_skips_protected() {
        let conn = crate::db::open_memory_database().unwrap();

        let batch = Batch {
            batch_id: "batch_prot".to_string(),
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

        // .env file is classified as protected
        let obj = ArchiveObject {
            archive_id: "obj_protected_1".to_string(),
            batch_id: "batch_prot".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Expired,
            original_path: "/project/.env".to_string(),
            archived_path: Some("/archive/obj_protected_1/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(256),
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
            expired_at: Some("2026-03-01T00:00:00Z".to_string()),
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj).unwrap();

        let config = SmartrmConfig::default();
        let fs = crate::fs::RealFilesystem;

        // Without --force, protected is skipped
        let ctx = CleanupContext {
            conn: &conn,
            fs: &fs,
            config: &config,
            older_than: None,
            expired_only: true,
            dry_run: true,
            force: false,
            json: false,
        };

        let result = execute_cleanup(&ctx).unwrap();
        assert_eq!(result.purged, 0);
        assert_eq!(result.protected_skipped, 1);

        // With --force, protected is purged
        let ctx_force = CleanupContext {
            force: true,
            ..ctx
        };

        let result_force = execute_cleanup(&ctx_force).unwrap();
        assert_eq!(result_force.purged, 1);
        assert_eq!(result_force.protected_skipped, 0);
    }
}

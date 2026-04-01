use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{Result, SmartrmError};
use crate::fs::restore::RestoreMetadata;
use crate::fs::Filesystem;
use crate::id;
use crate::models::*;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

// ---------------------------------------------------------------------------
// Context & targets
// ---------------------------------------------------------------------------

pub struct RestoreContext<'a> {
    pub conn: &'a Connection,
    pub fs: &'a dyn Filesystem,
    pub config: &'a SmartrmConfig,
    pub target: RestoreTarget,
    pub to: Option<PathBuf>,
    pub conflict_policy: ConflictPolicy,
    pub create_parents: bool,
    pub json: bool,
}

pub enum RestoreTarget {
    /// Single archive ID or short prefix
    ById(String),
    /// All objects from a specific batch
    ByBatch(String),
    /// Most recent delete batch
    Last,
    /// All archived/expired objects
    All,
    /// Last N delete batches (used by undo)
    LastN(u32),
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RestoreResult {
    pub batch_id: String,
    pub operation_type: String,
    pub status: String,
    pub requested: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub items: Vec<RestoreItemResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RestoreItemResult {
    pub archive_id: String,
    pub original_path: String,
    pub restored_to: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub mode_restored: bool,
    pub ownership_restored: bool,
    pub timestamps_restored: bool,
}

impl HumanOutput for RestoreResult {
    fn format_human(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            match item.status.as_str() {
                "succeeded" => {
                    let dest = item
                        .restored_to
                        .as_deref()
                        .unwrap_or(&item.original_path);
                    out.push_str(&format!(
                        "restored '{}' -> '{}'\n",
                        id::short_id(&item.archive_id),
                        dest,
                    ));
                }
                "skipped" => {
                    out.push_str(&format!(
                        "skipped '{}' ({})\n",
                        item.original_path,
                        item.error_message.as_deref().unwrap_or("conflict"),
                    ));
                }
                "failed" => {
                    out.push_str(&format!(
                        "smartrm: restore failed for '{}': {}\n",
                        item.original_path,
                        item.error_message.as_deref().unwrap_or("unknown error"),
                    ));
                }
                _ => {}
            }
        }
        if self.succeeded > 0 || self.failed > 0 {
            out.push_str(&format!(
                "restored {} of {} objects",
                self.succeeded, self.requested,
            ));
            if self.skipped > 0 {
                out.push_str(&format!(", {} skipped", self.skipped));
            }
            if self.failed > 0 {
                out.push_str(&format!(", {} failed", self.failed));
            }
            out.push('\n');
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn execute_restore(ctx: &RestoreContext) -> Result<RestoreResult> {
    let now = chrono::Utc::now().to_rfc3339();
    let batch_id = id::new_id();

    // 1. Resolve target -> list of ArchiveObject records
    let objects = resolve_target(ctx.conn, &ctx.target)?;

    if objects.is_empty() {
        return Err(SmartrmError::NotFound(
            "no archived objects found matching the given criteria".to_string(),
        ));
    }

    // Filter: only archived or expired are eligible
    let eligible: Vec<&ArchiveObject> = objects
        .iter()
        .filter(|o| {
            matches!(
                o.state,
                LifecycleState::Archived | LifecycleState::Expired
            )
        })
        .collect();

    if eligible.is_empty() {
        return Err(SmartrmError::NotFound(
            "no restorable objects found (all already restored or purged)".to_string(),
        ));
    }

    // 2. Create a restore batch
    let batch = Batch {
        batch_id: batch_id.clone(),
        operation_type: OperationType::Restore,
        status: BatchStatus::InProgress,
        requested_by: std::env::var("USER").ok(),
        cwd: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
        hostname: None,
        command_line: Some(std::env::args().collect::<Vec<_>>().join(" ")),
        total_objects_requested: eligible.len() as i64,
        total_objects_processed: 0,
        total_objects_succeeded: 0,
        total_objects_failed: 0,
        total_bytes: 0,
        interactive_mode: false,
        used_force: ctx.conflict_policy == ConflictPolicy::Overwrite,
        started_at: now.clone(),
        completed_at: None,
        summary_message: None,
    };
    db::operations::insert_batch(ctx.conn, &batch)?;

    // 3. Process each object
    let mut items = Vec::new();
    let mut succeeded = 0i64;
    let mut failed = 0i64;
    let mut skipped = 0i64;

    for obj in &eligible {
        let result = restore_single_object(ctx, &batch_id, obj, &now);

        match result {
            Ok(item) => {
                match item.status.as_str() {
                    "succeeded" => succeeded += 1,
                    "skipped" => skipped += 1,
                    _ => failed += 1,
                }
                items.push(item);
            }
            Err(e) => {
                failed += 1;
                items.push(RestoreItemResult {
                    archive_id: obj.archive_id.clone(),
                    original_path: obj.original_path.clone(),
                    restored_to: None,
                    status: "failed".to_string(),
                    error_message: Some(e.to_string()),
                    mode_restored: false,
                    ownership_restored: false,
                    timestamps_restored: false,
                });
            }
        }
    }

    // 4. Update batch
    let batch_status = if failed == 0 && skipped == 0 {
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
        succeeded + failed + skipped,
        0,
    )?;
    db::operations::update_batch_completed(ctx.conn, &batch_id, batch_status, None)?;

    Ok(RestoreResult {
        batch_id,
        operation_type: "restore".to_string(),
        status: batch_status.as_str().to_string(),
        requested: eligible.len(),
        succeeded: succeeded as usize,
        failed: failed as usize,
        skipped: skipped as usize,
        items,
    })
}

// ---------------------------------------------------------------------------
// Target resolution
// ---------------------------------------------------------------------------

fn resolve_target(conn: &Connection, target: &RestoreTarget) -> Result<Vec<ArchiveObject>> {
    match target {
        RestoreTarget::ById(id_or_prefix) => {
            let normalized = id_or_prefix.to_lowercase();

            // Try exact match first
            if let Some(obj) = db::queries::get_archive_object(conn, &normalized)? {
                return Ok(vec![obj]);
            }

            // Try prefix match
            let matches = db::queries::get_archive_object_by_prefix(conn, &normalized)?;
            match matches.len() {
                0 => Err(SmartrmError::NotFound(format!(
                    "no archive object found matching '{}'",
                    id_or_prefix
                ))),
                1 => Ok(matches),
                n => {
                    let ids: Vec<String> = matches
                        .iter()
                        .map(|o| format!("  {} ({})", id::short_id(&o.archive_id), o.original_path))
                        .collect();
                    Err(SmartrmError::NotFound(format!(
                        "ambiguous ID prefix '{}' matches {} objects:\n{}",
                        id_or_prefix,
                        n,
                        ids.join("\n"),
                    )))
                }
            }
        }

        RestoreTarget::ByBatch(batch_id) => {
            let objects = db::queries::get_archive_objects_for_batch(conn, batch_id)?;
            if objects.is_empty() {
                return Err(SmartrmError::NotFound(format!(
                    "no objects found for batch '{}'",
                    batch_id
                )));
            }
            Ok(objects)
        }

        RestoreTarget::Last => {
            let batch = db::queries::get_latest_delete_batch(conn)?;
            match batch {
                Some(b) => db::queries::get_archive_objects_for_batch(conn, &b.batch_id),
                None => Err(SmartrmError::NotFound(
                    "no delete batches found".to_string(),
                )),
            }
        }

        RestoreTarget::All => db::queries::get_all_archived_objects(conn),

        RestoreTarget::LastN(n) => {
            let batches = db::queries::get_latest_delete_batches(conn, *n)?;
            if batches.is_empty() {
                return Err(SmartrmError::NotFound(
                    "no delete batches found".to_string(),
                ));
            }
            let mut all_objects = Vec::new();
            for batch in &batches {
                let objects =
                    db::queries::get_archive_objects_for_batch(conn, &batch.batch_id)?;
                all_objects.extend(objects);
            }
            Ok(all_objects)
        }
    }
}

// ---------------------------------------------------------------------------
// Single object restore
// ---------------------------------------------------------------------------

fn restore_single_object(
    ctx: &RestoreContext,
    restore_batch_id: &str,
    obj: &ArchiveObject,
    now: &str,
) -> Result<RestoreItemResult> {
    // Determine target path
    let original = PathBuf::from(&obj.original_path);
    let filename = original
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));

    let target_path = match &ctx.to {
        Some(to_dir) => to_dir.join(filename),
        None => original.clone(),
    };

    // Check for conflict
    let (final_path, restore_mode, was_skipped) =
        resolve_conflict(ctx.fs, &target_path, ctx.conflict_policy)?;

    if was_skipped {
        // Record a skipped restore event
        let event = RestoreEvent {
            restore_event_id: id::new_id(),
            archive_id: obj.archive_id.clone(),
            restore_batch_id: restore_batch_id.to_string(),
            restore_mode: RestoreMode::Original,
            requested_target_path: Some(target_path.to_string_lossy().to_string()),
            final_restored_path: None,
            status: RestoreEventStatus::Failed,
            conflict_policy: ctx.conflict_policy,
            mode_restored: false,
            ownership_restored: false,
            timestamps_restored: false,
            error_code: Some("conflict_skipped".to_string()),
            error_message: Some("target exists, skipped per conflict policy".to_string()),
            created_at: now.to_string(),
        };
        db::operations::insert_restore_event(ctx.conn, &event)?;

        return Ok(RestoreItemResult {
            archive_id: obj.archive_id.clone(),
            original_path: obj.original_path.clone(),
            restored_to: None,
            status: "skipped".to_string(),
            error_message: Some("target already exists".to_string()),
            mode_restored: false,
            ownership_restored: false,
            timestamps_restored: false,
        });
    }

    // Get archived path
    let archived_path = match &obj.archived_path {
        Some(p) => PathBuf::from(p),
        None => {
            return Err(SmartrmError::NotFound(format!(
                "archive object '{}' has no archived_path",
                obj.archive_id
            )))
        }
    };

    // Build restore metadata
    let meta = RestoreMetadata {
        mode: obj.mode,
        uid: obj.uid,
        gid: obj.gid,
        mtime_ns: obj.mtime_ns,
    };

    // Perform FS restore
    let outcome = crate::fs::restore::restore_object(
        ctx.fs,
        &archived_path,
        &final_path,
        obj.object_type,
        obj.link_target.as_deref(),
        &meta,
        ctx.create_parents,
    )
    .map_err(SmartrmError::Io)?;

    // Record restore event in DB
    let event = RestoreEvent {
        restore_event_id: id::new_id(),
        archive_id: obj.archive_id.clone(),
        restore_batch_id: restore_batch_id.to_string(),
        restore_mode,
        requested_target_path: Some(target_path.to_string_lossy().to_string()),
        final_restored_path: Some(final_path.to_string_lossy().to_string()),
        status: RestoreEventStatus::Succeeded,
        conflict_policy: ctx.conflict_policy,
        mode_restored: outcome.mode_restored,
        ownership_restored: outcome.ownership_restored,
        timestamps_restored: outcome.timestamps_restored,
        error_code: None,
        error_message: None,
        created_at: now.to_string(),
    };
    db::operations::insert_restore_event(ctx.conn, &event)?;

    // Update archive object state to restored
    db::operations::update_archive_object_state(
        ctx.conn,
        &obj.archive_id,
        LifecycleState::Restored,
        None,
    )?;

    // Insert batch item
    let batch_item = BatchItem {
        batch_item_id: id::new_id(),
        batch_id: restore_batch_id.to_string(),
        input_path: obj.original_path.clone(),
        resolved_path: Some(final_path.to_string_lossy().to_string()),
        archive_id: Some(obj.archive_id.clone()),
        status: BatchItemStatus::Succeeded,
        error_code: None,
        error_message: None,
        created_at: now.to_string(),
        updated_at: now.to_string(),
    };
    db::operations::insert_batch_item(ctx.conn, &batch_item)?;

    Ok(RestoreItemResult {
        archive_id: obj.archive_id.clone(),
        original_path: obj.original_path.clone(),
        restored_to: Some(final_path.to_string_lossy().to_string()),
        status: "succeeded".to_string(),
        error_message: None,
        mode_restored: outcome.mode_restored,
        ownership_restored: outcome.ownership_restored,
        timestamps_restored: outcome.timestamps_restored,
    })
}

// ---------------------------------------------------------------------------
// Conflict resolution
// ---------------------------------------------------------------------------

/// Returns (final_path, restore_mode, was_skipped).
fn resolve_conflict(
    fs: &dyn Filesystem,
    target: &Path,
    policy: ConflictPolicy,
) -> Result<(PathBuf, RestoreMode, bool)> {
    if !fs.exists(target) {
        let mode = RestoreMode::Original;
        return Ok((target.to_path_buf(), mode, false));
    }

    match policy {
        ConflictPolicy::Fail => Err(SmartrmError::NotFound(format!(
            "target '{}' already exists (use --conflict rename|overwrite|skip)",
            target.display(),
        ))),
        ConflictPolicy::Skip => Ok((target.to_path_buf(), RestoreMode::Original, true)),
        ConflictPolicy::Overwrite => {
            Ok((target.to_path_buf(), RestoreMode::Overwrite, false))
        }
        ConflictPolicy::Rename => {
            let renamed = find_non_conflicting_name(target);
            Ok((renamed, RestoreMode::RenameOnConflict, false))
        }
    }
}

/// Generate a non-conflicting filename.
///
/// Pattern: `file (restored).ext`, `file (restored 2).ext`, etc.
/// For files without extension: `file (restored)`, `file (restored 2)`, etc.
pub fn find_non_conflicting_name(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());

    // Try "file (restored).ext" first
    let candidate = make_candidate(parent, &stem, ext.as_deref(), None);
    if !candidate.exists() {
        return candidate;
    }

    // Try "file (restored 2).ext", "file (restored 3).ext", etc.
    for n in 2u32.. {
        let candidate = make_candidate(parent, &stem, ext.as_deref(), Some(n));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("could not find a non-conflicting name")
}

fn make_candidate(parent: &Path, stem: &str, ext: Option<&str>, n: Option<u32>) -> PathBuf {
    let suffix = match n {
        None => " (restored)".to_string(),
        Some(num) => format!(" (restored {})", num),
    };

    let filename = match ext {
        Some(e) => format!("{}{}.{}", stem, suffix, e),
        None => format!("{}{}", stem, suffix),
    };

    parent.join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_non_conflicting_name_no_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");

        let result = find_non_conflicting_name(&path);
        assert_eq!(
            result.file_name().unwrap().to_string_lossy(),
            "test (restored).txt"
        );
    }

    #[test]
    fn find_non_conflicting_name_first_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");

        // Create the first candidate
        std::fs::write(dir.path().join("test (restored).txt"), "").unwrap();

        let result = find_non_conflicting_name(&path);
        assert_eq!(
            result.file_name().unwrap().to_string_lossy(),
            "test (restored 2).txt"
        );
    }

    #[test]
    fn find_non_conflicting_name_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Makefile");

        let result = find_non_conflicting_name(&path);
        assert_eq!(
            result.file_name().unwrap().to_string_lossy(),
            "Makefile (restored)"
        );
    }

    #[test]
    fn find_non_conflicting_name_multiple_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");

        std::fs::write(dir.path().join("test (restored).txt"), "").unwrap();
        std::fs::write(dir.path().join("test (restored 2).txt"), "").unwrap();
        std::fs::write(dir.path().join("test (restored 3).txt"), "").unwrap();

        let result = find_non_conflicting_name(&path);
        assert_eq!(
            result.file_name().unwrap().to_string_lossy(),
            "test (restored 4).txt"
        );
    }

    #[test]
    fn resolve_target_by_id_exact_match() {
        let conn = crate::db::open_memory_database().unwrap();

        // Insert a batch and object
        let batch = Batch {
            batch_id: "batch_test".to_string(),
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
            total_bytes: 100,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-04-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        let obj = ArchiveObject {
            archive_id: "01abc123def456789012345678".to_string(),
            batch_id: "batch_test".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/test.txt".to_string(),
            archived_path: Some("/archive/01abc123/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(100),
            content_hash: None,
            link_target: None,
            mode: Some(0o644),
            uid: Some(1000),
            gid: Some(1000),
            mtime_ns: Some(1000000),
            ctime_ns: None,
            delete_intent: None,
            ttl_seconds: None,
            policy_id: None,
            delete_reason: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-01T00:00:00Z".to_string(),
            restored_at: None,
            expired_at: None,
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj).unwrap();

        let target = RestoreTarget::ById("01abc123def456789012345678".to_string());
        let result = resolve_target(&conn, &target).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].archive_id, "01abc123def456789012345678");
    }

    #[test]
    fn resolve_target_by_prefix() {
        let conn = crate::db::open_memory_database().unwrap();

        let batch = Batch {
            batch_id: "batch_pfx".to_string(),
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
            total_bytes: 100,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-04-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        let obj = ArchiveObject {
            archive_id: "01xyz999aaa000000000000000".to_string(),
            batch_id: "batch_pfx".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/test.txt".to_string(),
            archived_path: Some("/archive/01xyz999/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(100),
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
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-01T00:00:00Z".to_string(),
            restored_at: None,
            expired_at: None,
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj).unwrap();

        let target = RestoreTarget::ById("01xyz999".to_string());
        let result = resolve_target(&conn, &target).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn resolve_target_no_match() {
        let conn = crate::db::open_memory_database().unwrap();

        let target = RestoreTarget::ById("nonexistent".to_string());
        let result = resolve_target(&conn, &target);
        assert!(result.is_err());
    }

    #[test]
    fn restore_filters_non_restorable_states() {
        let conn = crate::db::open_memory_database().unwrap();

        let batch = Batch {
            batch_id: "batch_filter".to_string(),
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
            total_bytes: 200,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-04-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        };
        db::operations::insert_batch(&conn, &batch).unwrap();

        // Insert one archived and one already-restored object
        let mut obj1 = ArchiveObject {
            archive_id: "obj_archived".to_string(),
            batch_id: "batch_filter".to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/a.txt".to_string(),
            archived_path: Some("/archive/obj_archived/payload".to_string()),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(100),
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
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-01T00:00:00Z".to_string(),
            restored_at: None,
            expired_at: None,
            purged_at: None,
            failure_code: None,
            failure_message: None,
        };
        db::operations::insert_archive_object(&conn, &obj1).unwrap();

        obj1.archive_id = "obj_restored".to_string();
        obj1.state = LifecycleState::Restored;
        obj1.original_path = "/tmp/b.txt".to_string();
        obj1.archived_path = Some("/archive/obj_restored/payload".to_string());
        db::operations::insert_archive_object(&conn, &obj1).unwrap();

        // Resolve by batch should return both, but only archived is eligible
        let objects = resolve_target(
            &conn,
            &RestoreTarget::ByBatch("batch_filter".to_string()),
        )
        .unwrap();
        assert_eq!(objects.len(), 2);

        let eligible: Vec<&ArchiveObject> = objects
            .iter()
            .filter(|o| {
                matches!(
                    o.state,
                    LifecycleState::Archived | LifecycleState::Expired
                )
            })
            .collect();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].archive_id, "obj_archived");
    }
}

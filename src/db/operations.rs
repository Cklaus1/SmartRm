use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::{
    ArchiveObject, Batch, BatchItem, BatchItemStatus, BatchStatus, EffectivePolicy, LifecycleState,
    RestoreEvent,
};

// ---------------------------------------------------------------------------
// Batches
// ---------------------------------------------------------------------------

pub fn insert_batch(conn: &Connection, batch: &Batch) -> Result<()> {
    conn.execute(
        "INSERT INTO batches (
            batch_id, operation_type, status, requested_by, cwd, hostname,
            command_line, total_objects_requested, total_objects_processed,
            total_objects_succeeded, total_objects_failed, total_bytes,
            interactive_mode, used_force, started_at, completed_at, summary_message
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            batch.batch_id,
            batch.operation_type.as_str(),
            batch.status.as_str(),
            batch.requested_by,
            batch.cwd,
            batch.hostname,
            batch.command_line,
            batch.total_objects_requested,
            batch.total_objects_processed,
            batch.total_objects_succeeded,
            batch.total_objects_failed,
            batch.total_bytes,
            batch.interactive_mode as i32,
            batch.used_force as i32,
            batch.started_at,
            batch.completed_at,
            batch.summary_message,
        ],
    )?;
    Ok(())
}

pub fn update_batch_status(
    conn: &Connection,
    batch_id: &str,
    status: BatchStatus,
    succeeded: i64,
    failed: i64,
    processed: i64,
    bytes: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE batches SET
            status = ?1,
            total_objects_succeeded = ?2,
            total_objects_failed = ?3,
            total_objects_processed = ?4,
            total_bytes = ?5
        WHERE batch_id = ?6",
        params![status.as_str(), succeeded, failed, processed, bytes, batch_id],
    )?;
    Ok(())
}

pub fn update_batch_completed(
    conn: &Connection,
    batch_id: &str,
    status: BatchStatus,
    summary: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE batches SET
            status = ?1,
            completed_at = ?2,
            summary_message = ?3
        WHERE batch_id = ?4",
        params![status.as_str(), now, summary, batch_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Batch items
// ---------------------------------------------------------------------------

pub fn insert_batch_item(conn: &Connection, item: &BatchItem) -> Result<()> {
    conn.execute(
        "INSERT INTO batch_items (
            batch_item_id, batch_id, input_path, resolved_path, archive_id,
            status, error_code, error_message, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            item.batch_item_id,
            item.batch_id,
            item.input_path,
            item.resolved_path,
            item.archive_id,
            item.status.as_str(),
            item.error_code,
            item.error_message,
            item.created_at,
            item.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_batch_item_status(
    conn: &Connection,
    batch_item_id: &str,
    status: BatchItemStatus,
    archive_id: Option<&str>,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE batch_items SET
            status = ?1,
            archive_id = ?2,
            error_code = ?3,
            error_message = ?4,
            updated_at = ?5
        WHERE batch_item_id = ?6",
        params![
            status.as_str(),
            archive_id,
            error_code,
            error_message,
            now,
            batch_item_id,
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Archive objects
// ---------------------------------------------------------------------------

pub fn insert_archive_object(conn: &Connection, obj: &ArchiveObject) -> Result<()> {
    conn.execute(
        "INSERT INTO archive_objects (
            archive_id, batch_id, parent_archive_id, object_type, state,
            original_path, archived_path, storage_mount_id, original_mount_id,
            size_bytes, content_hash, link_target, mode, uid, gid,
            mtime_ns, ctime_ns, delete_intent, ttl_seconds, policy_id,
            delete_reason, created_at, updated_at, restored_at, expired_at,
            purged_at, failure_code, failure_message
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
        )",
        params![
            obj.archive_id,
            obj.batch_id,
            obj.parent_archive_id,
            obj.object_type.as_str(),
            obj.state.as_str(),
            obj.original_path,
            obj.archived_path,
            obj.storage_mount_id,
            obj.original_mount_id,
            obj.size_bytes,
            obj.content_hash,
            obj.link_target,
            obj.mode.map(|m| m as i64),
            obj.uid.map(|u| u as i64),
            obj.gid.map(|g| g as i64),
            obj.mtime_ns,
            obj.ctime_ns,
            obj.delete_intent,
            obj.ttl_seconds,
            obj.policy_id,
            obj.delete_reason,
            obj.created_at,
            obj.updated_at,
            obj.restored_at,
            obj.expired_at,
            obj.purged_at,
            obj.failure_code,
            obj.failure_message,
        ],
    )?;
    Ok(())
}

/// Transition an archive object to a new lifecycle state.
///
/// When the state changes to `Restored`, `Expired`, or `Purged` the
/// corresponding timestamp column (`restored_at`, `expired_at`, `purged_at`)
/// is also set to the current UTC time. The `updated_at` column is always
/// refreshed.
pub fn update_archive_object_state(
    conn: &Connection,
    archive_id: &str,
    state: LifecycleState,
    timestamp_field: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    // Determine which timestamp column to set based on state or explicit override.
    let ts_col = timestamp_field.unwrap_or_else(|| match state {
        LifecycleState::Restored => "restored_at",
        LifecycleState::Expired => "expired_at",
        LifecycleState::Purged => "purged_at",
        _ => "",
    });

    if ts_col.is_empty() {
        // No extra timestamp column — just update state + updated_at.
        conn.execute(
            "UPDATE archive_objects SET state = ?1, updated_at = ?2 WHERE archive_id = ?3",
            params![state.as_str(), now, archive_id],
        )?;
    } else {
        // We build a safe SQL string using only known-good column names.
        let sql = match ts_col {
            "restored_at" => {
                "UPDATE archive_objects SET state = ?1, updated_at = ?2, restored_at = ?2 WHERE archive_id = ?3"
            }
            "expired_at" => {
                "UPDATE archive_objects SET state = ?1, updated_at = ?2, expired_at = ?2 WHERE archive_id = ?3"
            }
            "purged_at" => {
                "UPDATE archive_objects SET state = ?1, updated_at = ?2, purged_at = ?2 WHERE archive_id = ?3"
            }
            _ => {
                // Unknown column — fall back to state + updated_at only.
                "UPDATE archive_objects SET state = ?1, updated_at = ?2 WHERE archive_id = ?3"
            }
        };
        conn.execute(sql, params![state.as_str(), now, archive_id])?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Restore events
// ---------------------------------------------------------------------------

pub fn insert_restore_event(conn: &Connection, event: &RestoreEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO restore_events (
            restore_event_id, archive_id, restore_batch_id, restore_mode,
            requested_target_path, final_restored_path, status, conflict_policy,
            mode_restored, ownership_restored, timestamps_restored,
            error_code, error_message, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            event.restore_event_id,
            event.archive_id,
            event.restore_batch_id,
            event.restore_mode.as_str(),
            event.requested_target_path,
            event.final_restored_path,
            event.status.as_str(),
            event.conflict_policy.as_str(),
            event.mode_restored as i32,
            event.ownership_restored as i32,
            event.timestamps_restored as i32,
            event.error_code,
            event.error_message,
            event.created_at,
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Effective policies
// ---------------------------------------------------------------------------

pub fn insert_effective_policy(conn: &Connection, policy: &EffectivePolicy) -> Result<()> {
    conn.execute(
        "INSERT INTO effective_policies (
            effective_policy_id, batch_id, archive_id, setting_key,
            setting_value, source_type, source_ref, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            policy.effective_policy_id,
            policy.batch_id,
            policy.archive_id,
            policy.setting_key,
            policy.setting_value,
            policy.source_type.as_str(),
            policy.source_ref,
            policy.created_at,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::*;

    fn test_db() -> Connection {
        db::open_memory_database().unwrap()
    }

    fn make_batch(id: &str) -> Batch {
        Batch {
            batch_id: id.to_string(),
            operation_type: OperationType::Delete,
            status: BatchStatus::Pending,
            requested_by: None,
            cwd: Some("/tmp".to_string()),
            hostname: None,
            command_line: Some("smartrm test.txt".to_string()),
            total_objects_requested: 1,
            total_objects_processed: 0,
            total_objects_succeeded: 0,
            total_objects_failed: 0,
            total_bytes: 0,
            interactive_mode: false,
            used_force: false,
            started_at: "2026-04-01T00:00:00Z".to_string(),
            completed_at: None,
            summary_message: None,
        }
    }

    fn make_archive_object(id: &str, batch_id: &str) -> ArchiveObject {
        ArchiveObject {
            archive_id: id.to_string(),
            batch_id: batch_id.to_string(),
            parent_archive_id: None,
            object_type: ObjectType::File,
            state: LifecycleState::Archived,
            original_path: "/tmp/test.txt".to_string(),
            archived_path: Some(format!("/archive/{}/payload", id)),
            storage_mount_id: None,
            original_mount_id: None,
            size_bytes: Some(1024),
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
        }
    }

    #[test]
    fn insert_and_get_batch_round_trip() {
        let conn = test_db();
        let batch = make_batch("batch001");
        insert_batch(&conn, &batch).unwrap();

        let fetched = crate::db::queries::get_batch(&conn, "batch001").unwrap().unwrap();
        assert_eq!(fetched.batch_id, "batch001");
        assert_eq!(fetched.operation_type, OperationType::Delete);
        assert_eq!(fetched.status, BatchStatus::Pending);
        assert_eq!(fetched.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn update_batch_status_works() {
        let conn = test_db();
        insert_batch(&conn, &make_batch("batch002")).unwrap();

        update_batch_status(&conn, "batch002", BatchStatus::Complete, 1, 0, 1, 1024).unwrap();

        let fetched = crate::db::queries::get_batch(&conn, "batch002").unwrap().unwrap();
        assert_eq!(fetched.status, BatchStatus::Complete);
        assert_eq!(fetched.total_objects_succeeded, 1);
        assert_eq!(fetched.total_bytes, 1024);
    }

    #[test]
    fn insert_and_get_archive_object_round_trip() {
        let conn = test_db();
        insert_batch(&conn, &make_batch("batch003")).unwrap();

        let obj = make_archive_object("obj001", "batch003");
        insert_archive_object(&conn, &obj).unwrap();

        let fetched = crate::db::queries::get_archive_object(&conn, "obj001").unwrap().unwrap();
        assert_eq!(fetched.archive_id, "obj001");
        assert_eq!(fetched.object_type, ObjectType::File);
        assert_eq!(fetched.state, LifecycleState::Archived);
        assert_eq!(fetched.original_path, "/tmp/test.txt");
        assert_eq!(fetched.size_bytes, Some(1024));
        assert_eq!(fetched.mode, Some(0o644));
    }

    #[test]
    fn update_archive_object_state_sets_timestamp() {
        let conn = test_db();
        insert_batch(&conn, &make_batch("batch004")).unwrap();
        insert_archive_object(&conn, &make_archive_object("obj002", "batch004")).unwrap();

        update_archive_object_state(&conn, "obj002", LifecycleState::Restored, None).unwrap();

        let fetched = crate::db::queries::get_archive_object(&conn, "obj002").unwrap().unwrap();
        assert_eq!(fetched.state, LifecycleState::Restored);
        assert!(fetched.restored_at.is_some());
    }

    #[test]
    fn insert_and_get_batch_items() {
        let conn = test_db();
        insert_batch(&conn, &make_batch("batch005")).unwrap();
        insert_archive_object(&conn, &make_archive_object("obj003", "batch005")).unwrap();

        let item = BatchItem {
            batch_item_id: "item001".to_string(),
            batch_id: "batch005".to_string(),
            input_path: "test.txt".to_string(),
            resolved_path: Some("/tmp/test.txt".to_string()),
            archive_id: Some("obj003".to_string()),
            status: BatchItemStatus::Succeeded,
            error_code: None,
            error_message: None,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            updated_at: "2026-04-01T00:00:00Z".to_string(),
        };
        insert_batch_item(&conn, &item).unwrap();

        let items = crate::db::queries::get_batch_items_for_batch(&conn, "batch005").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].input_path, "test.txt");
        assert_eq!(items[0].status, BatchItemStatus::Succeeded);
    }

    #[test]
    fn get_archive_objects_for_batch_works() {
        let conn = test_db();
        insert_batch(&conn, &make_batch("batch006")).unwrap();
        insert_archive_object(&conn, &make_archive_object("obj004", "batch006")).unwrap();

        let mut obj2 = make_archive_object("obj005", "batch006");
        obj2.original_path = "/tmp/other.txt".to_string();
        insert_archive_object(&conn, &obj2).unwrap();

        let objects = crate::db::queries::get_archive_objects_for_batch(&conn, "batch006").unwrap();
        assert_eq!(objects.len(), 2);
    }

    #[test]
    fn get_latest_delete_batch_returns_most_recent() {
        let conn = test_db();

        let mut b1 = make_batch("batch007");
        b1.started_at = "2026-04-01T00:00:00Z".to_string();
        insert_batch(&conn, &b1).unwrap();

        let mut b2 = make_batch("batch008");
        b2.started_at = "2026-04-01T01:00:00Z".to_string();
        insert_batch(&conn, &b2).unwrap();

        let latest = crate::db::queries::get_latest_delete_batch(&conn).unwrap().unwrap();
        assert_eq!(latest.batch_id, "batch008");
    }
}

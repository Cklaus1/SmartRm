//! DB error path tests — FK violations, PK conflicts, transaction rollback.

use smartrm::db;
use smartrm::db::operations;
use smartrm::models::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Test 1: FK constraint violation
// ---------------------------------------------------------------------------

#[test]
fn fk_constraint_violation_on_missing_batch() {
    let conn = db::open_memory_database().unwrap();

    // Try to insert archive_object with a batch_id that doesn't exist
    let obj = make_archive_object("obj_orphan", "nonexistent_batch");
    let result = operations::insert_archive_object(&conn, &obj);

    assert!(result.is_err(), "insert with missing batch FK should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("FOREIGN KEY") || err_msg.contains("constraint"),
        "error should mention FK violation, got: {}",
        err_msg
    );
}

// ---------------------------------------------------------------------------
// Test 2: Duplicate PK violation
// ---------------------------------------------------------------------------

#[test]
fn duplicate_pk_violation() {
    let conn = db::open_memory_database().unwrap();
    operations::insert_batch(&conn, &make_batch("batch_dup")).unwrap();

    let obj = make_archive_object("test123", "batch_dup");
    operations::insert_archive_object(&conn, &obj).unwrap();

    // Try to insert another with same id "test123"
    let obj2 = ArchiveObject {
        original_path: "/tmp/other.txt".to_string(),
        ..make_archive_object("test123", "batch_dup")
    };
    let result = operations::insert_archive_object(&conn, &obj2);

    assert!(result.is_err(), "duplicate PK insert should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("UNIQUE") || err_msg.contains("PRIMARY KEY") || err_msg.contains("constraint"),
        "error should mention uniqueness, got: {}",
        err_msg
    );
}

// ---------------------------------------------------------------------------
// Test 3: State transition validity (app-level check)
// ---------------------------------------------------------------------------
//
// The DB itself does not enforce state transition ordering (any valid enum
// value is accepted). This test verifies the valid enum values are enforced
// by the CHECK constraint and that the app can transition between states.

#[test]
fn state_transition_updates_correctly() {
    let conn = db::open_memory_database().unwrap();
    operations::insert_batch(&conn, &make_batch("batch_state")).unwrap();

    let mut obj = make_archive_object("obj_state", "batch_state");
    obj.state = LifecycleState::Purged;
    obj.purged_at = Some("2026-04-01T00:00:00Z".to_string());
    operations::insert_archive_object(&conn, &obj).unwrap();

    // Verify initial state
    let fetched = db::queries::get_archive_object(&conn, "obj_state").unwrap().unwrap();
    assert_eq!(fetched.state, LifecycleState::Purged);

    // Transition purged -> archived (DB allows it; app would normally prevent)
    operations::update_archive_object_state(
        &conn,
        "obj_state",
        LifecycleState::Archived,
        None,
    )
    .unwrap();

    let fetched2 = db::queries::get_archive_object(&conn, "obj_state").unwrap().unwrap();
    assert_eq!(fetched2.state, LifecycleState::Archived);
    // purged_at should remain from original insert (update doesn't clear it)
    assert!(fetched2.purged_at.is_some());
}

#[test]
fn invalid_state_value_rejected_by_check_constraint() {
    let conn = db::open_memory_database().unwrap();
    operations::insert_batch(&conn, &make_batch("batch_check")).unwrap();

    // Try to insert with an invalid state value via raw SQL
    let result = conn.execute(
        "INSERT INTO archive_objects (
            archive_id, batch_id, object_type, state, original_path,
            created_at, updated_at
        ) VALUES ('obj_bad_state', 'batch_check', 'file', 'INVALID_STATE',
                  '/tmp/bad.txt', '2026-04-01T00:00:00Z', '2026-04-01T00:00:00Z')",
        [],
    );
    assert!(
        result.is_err(),
        "invalid state value should be rejected by CHECK constraint"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Concurrent writes with file-based DB (WAL mode)
// ---------------------------------------------------------------------------

#[test]
fn concurrent_writes_on_file_db() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("test.sqlite3");

    // Open two connections to the same file-based DB
    let conn1 = db::open_database(&db_path).unwrap();
    let conn2 = db::open_database(&db_path).unwrap();

    // Insert from connection 1
    operations::insert_batch(&conn1, &make_batch("batch_c1")).unwrap();
    operations::insert_archive_object(
        &conn1,
        &make_archive_object("obj_c1", "batch_c1"),
    )
    .unwrap();

    // Insert from connection 2
    operations::insert_batch(&conn2, &make_batch("batch_c2")).unwrap();
    operations::insert_archive_object(
        &conn2,
        &make_archive_object("obj_c2", "batch_c2"),
    )
    .unwrap();

    // Both should be visible from either connection
    let obj1 = db::queries::get_archive_object(&conn1, "obj_c2").unwrap();
    assert!(obj1.is_some(), "conn1 should see conn2's insert");

    let obj2 = db::queries::get_archive_object(&conn2, "obj_c1").unwrap();
    assert!(obj2.is_some(), "conn2 should see conn1's insert");
}

// ---------------------------------------------------------------------------
// Test 5: Transaction rollback on error
// ---------------------------------------------------------------------------

#[test]
fn transaction_rollback_on_error() {
    let conn = db::open_memory_database().unwrap();

    // Begin transaction
    conn.execute("BEGIN", []).unwrap();

    // Insert batch
    operations::insert_batch(&conn, &make_batch("batch_rollback")).unwrap();

    // Insert archive object
    operations::insert_archive_object(
        &conn,
        &make_archive_object("obj_rollback", "batch_rollback"),
    )
    .unwrap();

    // Cause an error: insert duplicate archive object
    let dup_result = operations::insert_archive_object(
        &conn,
        &make_archive_object("obj_rollback", "batch_rollback"),
    );
    assert!(dup_result.is_err(), "duplicate insert should fail");

    // Rollback
    conn.execute("ROLLBACK", []).unwrap();

    // Verify: batch was also rolled back (not in DB)
    let batch = db::queries::get_batch(&conn, "batch_rollback").unwrap();
    assert!(
        batch.is_none(),
        "batch should be rolled back along with the failed insert"
    );

    let obj = db::queries::get_archive_object(&conn, "obj_rollback").unwrap();
    assert!(
        obj.is_none(),
        "archive object should be rolled back too"
    );
}

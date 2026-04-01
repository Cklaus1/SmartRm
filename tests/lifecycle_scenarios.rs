//! End-to-end lifecycle scenario tests.
//!
//! Each test exercises the full delete -> restore cycle using real filesystem
//! operations (tempdir) and an in-memory SQLite database.

use std::path::PathBuf;

use smartrm::db;
use smartrm::db::queries;
use smartrm::fs::RealFilesystem;
use smartrm::models::*;
use smartrm::operations::cleanup::{execute_cleanup, CleanupContext};
use smartrm::operations::delete::{execute_delete, DeleteContext};
use smartrm::operations::restore::{execute_restore, RestoreContext, RestoreTarget};
use smartrm::policy::config::SmartrmConfig;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup() -> (rusqlite::Connection, TempDir, TempDir, SmartrmConfig) {
    let conn = db::open_memory_database().unwrap();
    let source_dir = TempDir::new().unwrap();
    let archive_dir = TempDir::new().unwrap();
    let mut config = SmartrmConfig::default();
    config.archive_root = Some(archive_dir.path().to_string_lossy().to_string());
    config.min_free_space_bytes = 0; // disable disk space check for tests
    (conn, source_dir, archive_dir, config)
}

fn make_delete_ctx<'a>(
    conn: &'a rusqlite::Connection,
    fs: &'a dyn smartrm::fs::Filesystem,
    config: &'a SmartrmConfig,
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

fn make_restore_ctx<'a>(
    conn: &'a rusqlite::Connection,
    fs: &'a dyn smartrm::fs::Filesystem,
    config: &'a SmartrmConfig,
    target: RestoreTarget,
) -> RestoreContext<'a> {
    RestoreContext {
        conn,
        fs,
        config,
        target,
        to: None,
        conflict_policy: ConflictPolicy::Overwrite,
        create_parents: true,
        json: false,
    }
}

// ---------------------------------------------------------------------------
// Scenario 1: delete -> restore -> delete again -> restore second version
// ---------------------------------------------------------------------------

#[test]
fn scenario_1_delete_restore_delete_restore_second_version() {
    let (conn, source_dir, _archive_dir, config) = setup();
    let fs = RealFilesystem;

    let file_path = source_dir.path().join("data.txt");

    // -- Step 1: Create file with content "v1" and delete it --
    std::fs::write(&file_path, "v1").unwrap();
    let del1 = execute_delete(&make_delete_ctx(&conn, &fs, &config, vec![file_path.clone()])).unwrap();
    assert_eq!(del1.succeeded, 1);
    assert!(!file_path.exists(), "file should be gone after delete");

    // DB: 1 archive_object with state=archived
    let all = queries::list_archive_objects(&conn, Some("archived"), 100, None).unwrap();
    assert_eq!(all.len(), 1);
    let first_archive_id = all[0].archive_id.clone();

    // -- Step 2: Restore it --
    let res1 = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ById(first_archive_id.clone()),
    ))
    .unwrap();
    assert_eq!(res1.succeeded, 1);
    assert!(file_path.exists(), "file should be back after restore");
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "v1");

    // DB: first object should now be state=restored
    let obj1 = queries::get_archive_object(&conn, &first_archive_id).unwrap().unwrap();
    assert_eq!(obj1.state, LifecycleState::Restored);

    // -- Step 3: Overwrite with "v2" and delete again --
    std::fs::write(&file_path, "v2").unwrap();
    let del2 = execute_delete(&make_delete_ctx(&conn, &fs, &config, vec![file_path.clone()])).unwrap();
    assert_eq!(del2.succeeded, 1);
    assert!(!file_path.exists());

    // DB: 2 archive_objects total — one restored, one archived
    let history = queries::get_history_for_path(
        &conn,
        &file_path.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(history.len(), 2, "history should have 2 versions");
    // Most recent first
    assert_eq!(history[0].state, LifecycleState::Archived);
    assert_eq!(history[1].state, LifecycleState::Restored);
    // Different archive IDs
    assert_ne!(history[0].archive_id, history[1].archive_id);

    // -- Step 4: Restore the second version --
    let second_archive_id = history[0].archive_id.clone();
    let res2 = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ById(second_archive_id),
    ))
    .unwrap();
    assert_eq!(res2.succeeded, 1);
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "v2");

    // Both versions in history
    let final_history = queries::get_history_for_path(
        &conn,
        &file_path.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(final_history.len(), 2);
}

// ---------------------------------------------------------------------------
// Scenario 2: delete -> partial restore -> undo (restore last batch)
// ---------------------------------------------------------------------------

#[test]
fn scenario_2_delete_partial_restore_then_undo() {
    let (conn, source_dir, _archive_dir, config) = setup();
    let fs = RealFilesystem;

    // Create 3 files
    let paths: Vec<PathBuf> = (1..=3)
        .map(|i| {
            let p = source_dir.path().join(format!("file{}.txt", i));
            std::fs::write(&p, format!("content{}", i)).unwrap();
            p
        })
        .collect();

    // Delete all 3 in one batch
    let del = execute_delete(&make_delete_ctx(&conn, &fs, &config, paths.clone())).unwrap();
    assert_eq!(del.succeeded, 3);
    let delete_batch_id = del.batch_id.clone();
    for p in &paths {
        assert!(!p.exists());
    }

    // Restore 1 by ID
    let objs = queries::get_archive_objects_for_batch(&conn, &delete_batch_id).unwrap();
    assert_eq!(objs.len(), 3);
    let first_id = objs[0].archive_id.clone();

    let res1 = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ById(first_id),
    ))
    .unwrap();
    assert_eq!(res1.succeeded, 1);

    // Verify: 1 file is back, other 2 still archived
    let archived = queries::list_archive_objects(&conn, Some("archived"), 100, None).unwrap();
    assert_eq!(archived.len(), 2);
    let restored = queries::list_archive_objects(&conn, Some("restored"), 100, None).unwrap();
    assert_eq!(restored.len(), 1);

    // Undo: restore last delete batch — should restore only the 2 still archived
    let undo = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ByBatch(delete_batch_id),
    ))
    .unwrap();
    assert_eq!(undo.succeeded, 2, "undo should restore only the 2 still-archived objects");

    // All 3 files should now exist
    for (i, p) in paths.iter().enumerate() {
        assert!(p.exists(), "file {} should exist", i + 1);
        assert_eq!(
            std::fs::read_to_string(p).unwrap(),
            format!("content{}", i + 1)
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 3: delete -> cleanup (purge) -> attempt restore (should fail)
// ---------------------------------------------------------------------------

#[test]
fn scenario_3_delete_cleanup_restore_fails() {
    let (conn, source_dir, _archive_dir, config) = setup();
    let fs = RealFilesystem;

    let file_path = source_dir.path().join("ephemeral.txt");
    std::fs::write(&file_path, "gone forever").unwrap();

    // Delete
    let del = execute_delete(&make_delete_ctx(&conn, &fs, &config, vec![file_path.clone()])).unwrap();
    assert_eq!(del.succeeded, 1);
    let archive_id = del.items[0].archive_id.as_ref().unwrap().clone();

    // Run cleanup (transition to purged)
    let cleanup_ctx = CleanupContext {
        conn: &conn,
        fs: &fs,
        config: &config,
        older_than: None,
        expired_only: false,
        dry_run: false,
        force: true,
        json: false,
    };
    let cleanup = execute_cleanup(&cleanup_ctx).unwrap();
    assert_eq!(cleanup.purged, 1);

    // Verify state=purged
    let obj = queries::get_archive_object(&conn, &archive_id).unwrap().unwrap();
    assert_eq!(obj.state, LifecycleState::Purged);

    // Attempt restore -> should fail (not eligible)
    let restore_result = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ById(archive_id),
    ));
    assert!(
        restore_result.is_err(),
        "restore of a purged object should fail"
    );
    let err_msg = restore_result.unwrap_err().to_string();
    assert!(
        err_msg.contains("no restorable objects") || err_msg.contains("purged"),
        "error should mention non-restorability, got: {}",
        err_msg
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: multi-file delete -> undo -> verify all restored
// ---------------------------------------------------------------------------

#[test]
fn scenario_4_multi_file_delete_and_undo() {
    let (conn, source_dir, _archive_dir, config) = setup();
    let fs = RealFilesystem;

    // Create 5 files
    let paths: Vec<PathBuf> = (1..=5)
        .map(|i| {
            let p = source_dir.path().join(format!("multi{}.txt", i));
            std::fs::write(&p, format!("data-{}", i)).unwrap();
            p
        })
        .collect();

    // Delete all 5 in one call
    let del = execute_delete(&make_delete_ctx(&conn, &fs, &config, paths.clone())).unwrap();
    assert_eq!(del.succeeded, 5);
    assert_eq!(del.failed, 0);
    let batch_id = del.batch_id.clone();

    // Verify batch has 5 items, all succeeded
    let items = queries::get_batch_items_for_batch(&conn, &batch_id).unwrap();
    assert_eq!(items.len(), 5);
    for item in &items {
        assert_eq!(item.status, BatchItemStatus::Succeeded);
    }

    // Undo (restore last delete batch)
    let undo = execute_restore(&make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::Last,
    ))
    .unwrap();
    assert_eq!(undo.succeeded, 5);

    // Verify all 5 files exist with original content
    for (i, p) in paths.iter().enumerate() {
        assert!(p.exists(), "file {} should exist after undo", i + 1);
        assert_eq!(
            std::fs::read_to_string(p).unwrap(),
            format!("data-{}", i + 1)
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 5: delete -> restore to alternate path -> delete original again
// ---------------------------------------------------------------------------

#[test]
fn scenario_5_restore_to_alternate_path() {
    let (conn, source_dir, _archive_dir, config) = setup();
    let fs = RealFilesystem;

    let path_a = source_dir.path().join("original.txt");
    let alt_dir = source_dir.path().join("restored_here");
    std::fs::create_dir_all(&alt_dir).unwrap();

    // Create file at path A with content "hello"
    std::fs::write(&path_a, "hello").unwrap();

    // Delete it
    let del1 = execute_delete(&make_delete_ctx(&conn, &fs, &config, vec![path_a.clone()])).unwrap();
    assert_eq!(del1.succeeded, 1);
    let archive_id_1 = del1.items[0].archive_id.as_ref().unwrap().clone();

    // Restore to path B (alt_dir)
    let mut restore_ctx = make_restore_ctx(
        &conn,
        &fs,
        &config,
        RestoreTarget::ById(archive_id_1),
    );
    restore_ctx.to = Some(alt_dir.clone());
    let res = execute_restore(&restore_ctx).unwrap();
    assert_eq!(res.succeeded, 1);

    // Verify: file at alt_dir/original.txt with content "hello"
    let path_b = alt_dir.join("original.txt");
    assert!(path_b.exists(), "file should exist at alternate path");
    assert_eq!(std::fs::read_to_string(&path_b).unwrap(), "hello");
    assert!(!path_a.exists(), "original path should still not exist");

    // Create new file at path A with content "world"
    std::fs::write(&path_a, "world").unwrap();

    // Delete it
    let del2 = execute_delete(&make_delete_ctx(&conn, &fs, &config, vec![path_a.clone()])).unwrap();
    assert_eq!(del2.succeeded, 1);

    // History for path A should show 2 entries
    let history = queries::get_history_for_path(
        &conn,
        &path_a.to_string_lossy(),
    )
    .unwrap();
    assert_eq!(history.len(), 2, "history for path A should have 2 entries");
}

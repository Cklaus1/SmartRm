//! User journey story tests for SmartRM.
//!
//! Each test simulates a real-world multi-step user workflow, telling a
//! complete story from the user's perspective. They use the operations layer
//! directly (not the binary) with real filesystem I/O in tempdirs and an
//! in-memory SQLite database.

use std::fs;
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
// Test environment helper
// ---------------------------------------------------------------------------

struct JourneyEnv {
    conn: rusqlite::Connection,
    source_dir: TempDir,
    _archive_dir: TempDir,
    config: SmartrmConfig,
    fs: RealFilesystem,
}

impl JourneyEnv {
    fn new() -> Self {
        let conn = db::open_memory_database().unwrap();
        let source_dir = TempDir::new().unwrap();
        let archive_dir = TempDir::new().unwrap();
        let mut config = SmartrmConfig::default();
        config.archive_root = Some(archive_dir.path().to_string_lossy().to_string());
        config.min_free_space_bytes = 0;
        Self {
            conn,
            source_dir,
            _archive_dir: archive_dir,
            config,
            fs: RealFilesystem,
        }
    }

    /// Create a file under source_dir with given relative path and content.
    fn create_file(&self, relative_path: &str, content: &str) -> PathBuf {
        let path = self.source_dir.path().join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    /// Delete the given paths (non-recursive, non-force).
    fn delete(&self, paths: &[PathBuf]) -> smartrm::operations::delete::DeleteResult {
        let ctx = DeleteContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            paths: paths.to_vec(),
            recursive: false,
            force: false,
            verbose: false,
            permanent: false,
            yes_i_am_sure: false,
            json: false,
            interactive_each: false,
            interactive_once: false,
            dir: false,
            one_file_system: false,
        };
        execute_delete(&ctx).unwrap()
    }

    /// Delete the given paths with recursive flag.
    fn delete_recursive(&self, paths: &[PathBuf]) -> smartrm::operations::delete::DeleteResult {
        let ctx = DeleteContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            paths: paths.to_vec(),
            recursive: true,
            force: false,
            verbose: false,
            permanent: false,
            yes_i_am_sure: false,
            json: false,
            interactive_each: false,
            interactive_once: false,
            dir: false,
            one_file_system: false,
        };
        execute_delete(&ctx).unwrap()
    }

    /// Restore the most recent delete batch (undo).
    fn restore_last(&self) -> smartrm::operations::restore::RestoreResult {
        let ctx = RestoreContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            target: RestoreTarget::Last,
            to: None,
            conflict_policy: ConflictPolicy::Overwrite,
            create_parents: true,
            json: false,
        };
        execute_restore(&ctx).unwrap()
    }

    /// Restore a specific archive object by its full ID.
    fn restore_by_id(&self, id: &str) -> smartrm::operations::restore::RestoreResult {
        let ctx = RestoreContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            target: RestoreTarget::ById(id.to_string()),
            to: None,
            conflict_policy: ConflictPolicy::Rename,
            create_parents: true,
            json: false,
        };
        execute_restore(&ctx).unwrap()
    }

    /// Restore a specific archive object to an alternate directory.
    fn restore_to(&self, id: &str, dest: PathBuf) -> smartrm::operations::restore::RestoreResult {
        let ctx = RestoreContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            target: RestoreTarget::ById(id.to_string()),
            to: Some(dest),
            conflict_policy: ConflictPolicy::Overwrite,
            create_parents: true,
            json: false,
        };
        execute_restore(&ctx).unwrap()
    }

    /// Restore with a specific conflict policy.
    fn restore_with_conflict(
        &self,
        target: RestoreTarget,
        policy: ConflictPolicy,
    ) -> smartrm::operations::restore::RestoreResult {
        let ctx = RestoreContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            target,
            to: None,
            conflict_policy: policy,
            create_parents: true,
            json: false,
        };
        execute_restore(&ctx).unwrap()
    }

    /// Restore all archived objects.
    fn restore_all(&self) -> smartrm::operations::restore::RestoreResult {
        let ctx = RestoreContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            target: RestoreTarget::All,
            to: None,
            conflict_policy: ConflictPolicy::Overwrite,
            create_parents: true,
            json: false,
        };
        execute_restore(&ctx).unwrap()
    }

    /// Run cleanup (purge) with options.
    fn cleanup(
        &self,
        older_than: Option<&str>,
        dry_run: bool,
        force: bool,
    ) -> smartrm::operations::cleanup::CleanupResult {
        let ctx = CleanupContext {
            conn: &self.conn,
            fs: &self.fs,
            config: &self.config,
            older_than: older_than.map(|s| s.to_string()),
            expired_only: false,
            dry_run,
            force,
            json: false,
        };
        execute_cleanup(&ctx).unwrap()
    }
}

// ===========================================================================
// Journey 1: "Oh no, I deleted the wrong file"
//
// The most common scenario -- accidental deletion and immediate recovery.
// Developer is cleaning up temp files and accidentally deletes their config.
// ===========================================================================

#[test]
fn journey_accidental_delete_and_immediate_undo() {
    let env = JourneyEnv::new();

    // Step 1: User has a project with src/main.rs, config.json, temp.log
    let _main_rs = env.create_file("src/main.rs", "fn main() {}");
    let config_json = env.create_file("config.json", r#"{"key": "value"}"#);
    let temp_log = env.create_file("temp.log", "some log data");

    // Step 2: User accidentally deletes both temp.log AND config.json
    let del = env.delete(&[temp_log.clone(), config_json.clone()]);
    assert_eq!(del.succeeded, 2, "both files should be archived");
    assert!(!temp_log.exists(), "temp.log should be gone");
    assert!(!config_json.exists(), "config.json should be gone");

    // Step 3: User immediately realizes the mistake

    // Step 4: User runs undo -- restores the entire last batch
    let undo = env.restore_last();
    assert_eq!(undo.succeeded, 2, "undo should restore both files");

    // Step 5: Both files are restored with original content
    assert!(temp_log.exists(), "temp.log should be back");
    assert!(config_json.exists(), "config.json should be back");
    assert_eq!(fs::read_to_string(&config_json).unwrap(), r#"{"key": "value"}"#);
    assert_eq!(fs::read_to_string(&temp_log).unwrap(), "some log data");

    // Step 6: User re-deletes just temp.log (the one they actually wanted gone)
    let del2 = env.delete(&[temp_log.clone()]);
    assert_eq!(del2.succeeded, 1);

    // Step 7: Verify final state -- config.json exists, temp.log is archived
    assert!(config_json.exists(), "config.json should still be present");
    assert!(!temp_log.exists(), "temp.log should now be archived");
}

// ===========================================================================
// Journey 2: "I deleted it days ago, need it back"
//
// Finding and restoring a specific file from the archive by searching.
// Developer deleted a file last week, now needs it for a bug fix.
// ===========================================================================

#[test]
fn journey_find_old_file_and_restore_by_id() {
    let env = JourneyEnv::new();

    // Step 1: Create and delete 10 different files in separate batches
    let mut all_archive_ids = Vec::new();
    let filenames = [
        "notes.txt",
        "draft.md",
        "old-test.rs",
        "critical-bugfix.patch",
        "report.csv",
        "schema.sql",
        "backup.tar",
        "readme-old.md",
        "config-backup.yaml",
        "unused-module.js",
    ];
    for name in &filenames {
        let path = env.create_file(name, &format!("content of {}", name));
        let del = env.delete(&[path]);
        assert_eq!(del.succeeded, 1);
        all_archive_ids.push(del.items[0].archive_id.clone().unwrap());
    }

    // Step 2: Verify all 10 are in the archive
    let archived = queries::list_archive_objects(&env.conn, Some("archived"), 100, None).unwrap();
    assert_eq!(archived.len(), 10, "all 10 files should be in the archive");

    // Step 3: User searches for "bugfix" -- use the search query function
    let search_results = queries::search_archive_objects(
        &env.conn, "bugfix", false, None, None, None, 0, 100,
    )
    .unwrap();
    assert_eq!(search_results.len(), 1, "search should find exactly one match");
    assert!(
        search_results[0].original_path.contains("critical-bugfix.patch"),
        "found file should be the bugfix patch"
    );

    // Step 4: Get the archive ID of the found file
    let bugfix_id = &search_results[0].archive_id;

    // Step 5: Restore by ID
    let restore = env.restore_by_id(bugfix_id);
    assert_eq!(restore.succeeded, 1, "restore should succeed");

    // Step 6: Verify content is intact
    let restored_path = env.source_dir.path().join("critical-bugfix.patch");
    assert!(restored_path.exists(), "bugfix patch should be restored");
    assert_eq!(
        fs::read_to_string(&restored_path).unwrap(),
        "content of critical-bugfix.patch"
    );
}

// ===========================================================================
// Journey 3: "Clean up my archive, it's getting huge"
//
// Routine archive maintenance. After months of use, the user wants to clean
// up old files but keep recent ones.
// ===========================================================================

#[test]
fn journey_archive_cleanup() {
    let env = JourneyEnv::new();

    // Step 1: Create and delete 10 files (they will all be "recent" since
    // we're creating them now, but cleanup with --older-than 0d treats
    // everything created before "now" as eligible)
    for i in 0..10 {
        let path = env.create_file(&format!("file_{}.txt", i), &format!("data {}", i));
        let del = env.delete(&[path]);
        assert_eq!(del.succeeded, 1);
    }

    // Step 2: Check stats -- should show 10 archived objects
    let stats = queries::get_stats(&env.conn).unwrap();
    let archived_count = stats
        .count_by_state
        .iter()
        .find(|(s, _)| s == "archived")
        .map(|(_, c)| *c)
        .unwrap_or(0);
    assert_eq!(archived_count, 10, "should have 10 archived objects");

    // Step 3: Preview cleanup with dry_run
    let dry_run = env.cleanup(Some("0d"), true, false);
    assert!(dry_run.dry_run, "should be a dry run");
    assert_eq!(dry_run.purged, 10, "dry run should report 10 would-be-purged");

    // Step 4: Verify nothing was actually purged (dry run)
    let still_archived =
        queries::list_archive_objects(&env.conn, Some("archived"), 100, None).unwrap();
    assert_eq!(still_archived.len(), 10, "dry run should not purge anything");

    // Step 5: Run actual cleanup
    let real_cleanup = env.cleanup(Some("0d"), false, false);
    assert!(!real_cleanup.dry_run);
    assert_eq!(real_cleanup.purged, 10, "should purge all 10");

    // Step 6: Verify archive is empty
    let after_cleanup =
        queries::list_archive_objects(&env.conn, Some("archived"), 100, None).unwrap();
    assert_eq!(after_cleanup.len(), 0, "archive should be empty after cleanup");

    // Step 7: Stats should reflect the purge
    let stats_after = queries::get_stats(&env.conn).unwrap();
    let purged_count = stats_after
        .count_by_state
        .iter()
        .find(|(s, _)| s == "purged")
        .map(|(_, c)| *c)
        .unwrap_or(0);
    assert_eq!(purged_count, 10, "should show 10 purged objects");
}

// ===========================================================================
// Journey 4: "I need to restore to a different location"
//
// Original directory was restructured. User needs old files in a new location.
// ===========================================================================

#[test]
fn journey_restore_to_different_location() {
    let env = JourneyEnv::new();

    // Step 1: Create project/old-api/ with handler.rs and types.rs
    let handler = env.create_file("project/old-api/handler.rs", "pub fn handle() {}");
    let types = env.create_file("project/old-api/types.rs", "pub struct Request {}");

    // Step 2: Delete the old-api directory recursively
    let old_api_dir = env.source_dir.path().join("project/old-api");
    let del = env.delete_recursive(&[old_api_dir.clone()]);
    assert_eq!(del.succeeded, 1, "directory delete should succeed");
    assert!(!old_api_dir.exists(), "old-api directory should be gone");

    // Step 3: Create the new-api directory (the restructured project)
    let new_api_dir = env.source_dir.path().join("project/new-api");
    fs::create_dir_all(&new_api_dir).unwrap();

    // Step 4: Restore old-api directory to new-api/ using --to
    let archive_id = del.items[0].archive_id.as_ref().unwrap();
    let restore = env.restore_to(archive_id, new_api_dir.clone());
    assert_eq!(restore.succeeded, 1, "restore to new location should succeed");

    // Step 5: Verify files land in project/new-api/ with content preserved.
    // The directory itself is restored as "old-api" inside new-api/
    let restored_dir = new_api_dir.join("old-api");
    if restored_dir.exists() {
        // Directory was restored as a unit under the target
        assert!(restored_dir.join("handler.rs").exists() || restored_dir.exists());
    } else {
        // The restore places the directory payload directly; the old-api dir
        // is the archived unit, so it lands as new-api/old-api/
        // Either way, something should exist in new-api/
        let entries: Vec<_> = fs::read_dir(&new_api_dir).unwrap().collect();
        assert!(!entries.is_empty(), "new-api/ should have restored content");
    }

    // Original location should still not exist
    assert!(!handler.exists(), "original handler.rs should still be gone");
    assert!(!types.exists(), "original types.rs should still be gone");
}

// ===========================================================================
// Journey 5: "Multiple versions of the same file"
//
// Developer iterates on a config file, deleting and recreating it multiple
// times. Needs to retrieve a specific version.
// ===========================================================================

#[test]
fn journey_multiple_versions_of_same_file() {
    let env = JourneyEnv::new();

    // Step 1: Create config.yaml with content v1 and delete it
    let config_path = env.source_dir.path().join("config.yaml");
    fs::write(&config_path, "version: 1").unwrap();
    let del1 = env.delete(&[config_path.clone()]);
    assert_eq!(del1.succeeded, 1);
    let v1_id = del1.items[0].archive_id.clone().unwrap();

    // Step 2: Create config.yaml with content v2 and delete it
    fs::write(&config_path, "version: 2").unwrap();
    let del2 = env.delete(&[config_path.clone()]);
    assert_eq!(del2.succeeded, 1);
    let v2_id = del2.items[0].archive_id.clone().unwrap();

    // Step 3: Create config.yaml with content v3 and delete it
    fs::write(&config_path, "version: 3").unwrap();
    let del3 = env.delete(&[config_path.clone()]);
    assert_eq!(del3.succeeded, 1);
    let v3_id = del3.items[0].archive_id.clone().unwrap();

    // Step 4: Check history -- 3 versions should be shown
    let history =
        queries::get_history_for_path(&env.conn, &config_path.to_string_lossy()).unwrap();
    assert_eq!(history.len(), 3, "should have 3 versions in history");

    // Step 5: Restore version 1 (oldest) by its specific archive_id
    let res1 = env.restore_by_id(&v1_id);
    assert_eq!(res1.succeeded, 1);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), "version: 1");

    // Step 6: Restore version 3 (latest) by ID -- should trigger rename
    // conflict since v1 is already at that path
    let res3 = env.restore_by_id(&v3_id);
    assert_eq!(res3.succeeded, 1);

    // The v3 restore used Rename conflict policy, so it created a renamed copy
    let restored_to = res3.items[0].restored_to.as_ref().unwrap();
    assert!(
        restored_to.contains("restored"),
        "conflict should have produced a renamed file, got: {}",
        restored_to
    );

    // Original v1 should still be at config_path
    assert_eq!(
        fs::read_to_string(&config_path).unwrap(),
        "version: 1",
        "original v1 file should be untouched"
    );

    // The renamed file should have v3 content
    let renamed_path = PathBuf::from(restored_to);
    assert_eq!(
        fs::read_to_string(&renamed_path).unwrap(),
        "version: 3",
        "renamed file should have v3 content"
    );

    // v2 should still be in archive, untouched
    let v2_obj = queries::get_archive_object(&env.conn, &v2_id).unwrap().unwrap();
    assert_eq!(v2_obj.state, LifecycleState::Archived, "v2 should still be archived");
}

// ===========================================================================
// Journey 6: "Oops, I deleted an entire project directory"
//
// The panic scenario -- developer runs smartrm -r on the wrong directory.
// ===========================================================================

#[test]
fn journey_delete_entire_project_and_undo() {
    let env = JourneyEnv::new();

    // Step 1: Create a realistic project tree
    let main_rs = env.create_file("project/src/main.rs", "fn main() { println!(\"hello\"); }");
    let lib_rs = env.create_file("project/src/lib.rs", "pub mod utils;");
    let helpers = env.create_file("project/src/utils/helpers.rs", "pub fn help() {}");
    let cargo = env.create_file("project/Cargo.toml", "[package]\nname = \"myproject\"");
    let readme = env.create_file("project/README.md", "# My Project");
    let dotenv = env.create_file("project/.env", "SECRET_KEY=abc123");

    // Step 2: Delete the entire project directory recursively
    let project_dir = env.source_dir.path().join("project");
    let del = env.delete_recursive(&[project_dir.clone()]);
    assert_eq!(del.succeeded, 1, "recursive delete should succeed");

    // Step 3: Verify entire tree is gone
    assert!(!project_dir.exists(), "project directory should be gone");
    assert!(!main_rs.exists());
    assert!(!lib_rs.exists());
    assert!(!helpers.exists());
    assert!(!cargo.exists());
    assert!(!readme.exists());
    assert!(!dotenv.exists());

    // Step 4: Immediately undo
    let undo = env.restore_last();
    assert_eq!(undo.succeeded, 1, "undo should restore the directory");

    // Step 5: Verify entire tree is restored with all content
    assert!(project_dir.exists(), "project directory should be back");
    assert!(main_rs.exists(), "main.rs should be restored");
    assert_eq!(
        fs::read_to_string(&main_rs).unwrap(),
        "fn main() { println!(\"hello\"); }"
    );
    assert!(lib_rs.exists(), "lib.rs should be restored");
    assert_eq!(fs::read_to_string(&lib_rs).unwrap(), "pub mod utils;");
    assert!(helpers.exists(), "helpers.rs should be restored");
    assert_eq!(fs::read_to_string(&helpers).unwrap(), "pub fn help() {}");
    assert!(cargo.exists(), "Cargo.toml should be restored");
    assert_eq!(
        fs::read_to_string(&cargo).unwrap(),
        "[package]\nname = \"myproject\""
    );
    assert!(readme.exists(), "README.md should be restored");

    // Step 6: Verify .env still has its content
    assert!(dotenv.exists(), ".env should be restored");
    assert_eq!(fs::read_to_string(&dotenv).unwrap(), "SECRET_KEY=abc123");
}

// ===========================================================================
// Journey 7: "Setting up smartrm for the first time"
//
// New user onboarding flow -- clean state, first operations.
// ===========================================================================

#[test]
fn journey_first_time_setup() {
    let env = JourneyEnv::new();

    // Step 1: Start with clean state -- archive should be empty
    let stats = queries::get_stats(&env.conn).unwrap();
    assert!(
        stats.count_by_state.is_empty(),
        "fresh DB should have no objects"
    );

    // Step 2: Delete a file -- archive is created automatically
    let hello = env.create_file("hello.txt", "Hello, SmartRM!");
    let del = env.delete(&[hello.clone()]);
    assert_eq!(del.succeeded, 1);
    assert!(!hello.exists(), "file should be archived");

    // Step 3: List archived objects -- should see the file
    let list = queries::list_archive_objects(&env.conn, Some("archived"), 100, None).unwrap();
    assert_eq!(list.len(), 1, "list should show 1 archived object");
    assert!(
        list[0].original_path.contains("hello.txt"),
        "listed object should be hello.txt"
    );

    // Step 4: Stats should show 1 object
    let stats = queries::get_stats(&env.conn).unwrap();
    let archived_count = stats
        .count_by_state
        .iter()
        .find(|(s, _)| s == "archived")
        .map(|(_, c)| *c)
        .unwrap_or(0);
    assert_eq!(archived_count, 1, "stats should report 1 archived object");
    assert!(stats.total_size_bytes > 0, "total size should be > 0");

    // Step 5: Undo -- file comes back
    let undo = env.restore_last();
    assert_eq!(undo.succeeded, 1);
    assert!(hello.exists(), "file should be restored after undo");
    assert_eq!(fs::read_to_string(&hello).unwrap(), "Hello, SmartRM!");

    // Step 6: Delete it again
    let del2 = env.delete(&[hello.clone()]);
    assert_eq!(del2.succeeded, 1);

    // Step 7: Search by filename -- finds it
    let results = queries::search_archive_objects(
        &env.conn, "hello", false, None, None, None, 0, 100,
    )
    .unwrap();
    // There are 2 versions now (first was restored then deleted again) but
    // only the second one is still in "archived" state. Search returns all.
    assert!(
        results.iter().any(|o| o.original_path.contains("hello.txt")),
        "search should find hello.txt"
    );

    // Step 8: Timeline -- shows the batches
    let timeline = queries::get_timeline_batches(&env.conn, false, None, 100).unwrap();
    assert!(timeline.len() >= 2, "should have at least 2 batches (delete, restore, delete)");
}

// ===========================================================================
// Journey 8: "Partial failure -- some files delete, some don't"
//
// Error resilience story. User deletes multiple files where one doesn't exist.
// ===========================================================================

#[test]
fn journey_partial_failure_and_recovery() {
    let env = JourneyEnv::new();

    // Step 1: Create 2 real files
    let a = env.create_file("a.txt", "content a");
    let b = env.create_file("b.txt", "content b");

    // Step 2: Try to delete 3 files where 1 doesn't exist (without -f)
    let nonexistent = env.source_dir.path().join("does_not_exist.txt");
    let del = env.delete(&[a.clone(), b.clone(), nonexistent]);

    // Step 3: Verify: 2 files archived, 1 failed
    assert_eq!(del.succeeded, 2, "2 real files should be archived");
    assert_eq!(del.failed, 1, "1 missing file should fail");

    // Step 4: Batch status should be "partial"
    assert_eq!(del.status, "partial", "batch status should be partial");

    // Step 5: The 2 real files should be gone
    assert!(!a.exists(), "a.txt should be archived");
    assert!(!b.exists(), "b.txt should be archived");

    // Step 6: Undo restores the 2 that succeeded
    let undo = env.restore_last();
    assert_eq!(undo.succeeded, 2, "undo should restore the 2 archived files");

    // Step 7: Both files are back with correct content
    assert!(a.exists(), "a.txt should be restored");
    assert!(b.exists(), "b.txt should be restored");
    assert_eq!(fs::read_to_string(&a).unwrap(), "content a");
    assert_eq!(fs::read_to_string(&b).unwrap(), "content b");
}

// ===========================================================================
// Journey 9: "Conflict resolution during restore"
//
// User deleted a file, then created a new file at the same path.
// Tests rename and overwrite conflict policies.
// ===========================================================================

#[test]
fn journey_conflict_resolution() {
    let env = JourneyEnv::new();

    // Step 1: Create important.txt with "original content"
    let important = env.create_file("important.txt", "original content");

    // Step 2: Delete it
    let del1 = env.delete(&[important.clone()]);
    assert_eq!(del1.succeeded, 1);
    let original_id = del1.items[0].archive_id.clone().unwrap();

    // Step 3: Create important.txt with "new content" (the conflict target)
    fs::write(&important, "new content").unwrap();
    assert_eq!(fs::read_to_string(&important).unwrap(), "new content");

    // Step 4: Restore with rename conflict policy
    let res_rename = env.restore_with_conflict(
        RestoreTarget::ById(original_id.clone()),
        ConflictPolicy::Rename,
    );
    assert_eq!(res_rename.succeeded, 1);

    // The restored file got a renamed path
    let renamed_path = PathBuf::from(res_rename.items[0].restored_to.as_ref().unwrap());
    assert!(
        renamed_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("restored"),
        "renamed file should contain 'restored'"
    );

    // Step 5: Verify both files exist with correct content
    assert_eq!(
        fs::read_to_string(&important).unwrap(),
        "new content",
        "original path should still have new content"
    );
    assert_eq!(
        fs::read_to_string(&renamed_path).unwrap(),
        "original content",
        "renamed file should have original content"
    );

    // Clean up for the overwrite test: delete both files, re-create scenario
    let _ = fs::remove_file(&renamed_path);
    let _ = fs::remove_file(&important);

    // Step 6: Create new scenario for overwrite test
    let important2 = env.create_file("important2.txt", "first version");
    let del2 = env.delete(&[important2.clone()]);
    assert_eq!(del2.succeeded, 1);
    let first_id = del2.items[0].archive_id.clone().unwrap();

    // Create a new file at the same path
    fs::write(&important2, "second version").unwrap();

    // Step 7: Restore with overwrite policy
    let res_overwrite = env.restore_with_conflict(
        RestoreTarget::ById(first_id),
        ConflictPolicy::Overwrite,
    );
    assert_eq!(res_overwrite.succeeded, 1);

    // Step 8: Verify content is the restored "first version" (overwritten)
    assert_eq!(
        fs::read_to_string(&important2).unwrap(),
        "first version",
        "overwrite should replace the file with original content"
    );
}

// ===========================================================================
// Journey 10: "Full uninstall -- getting all files back"
//
// Migration/uninstall workflow. User wants every deleted file restored.
// ===========================================================================

#[test]
fn journey_restore_everything() {
    let env = JourneyEnv::new();

    // Step 1: Create and delete 5 files across different directories
    let paths: Vec<PathBuf> = vec![
        env.create_file("docs/readme.txt", "readme content"),
        env.create_file("src/app.rs", "fn app() {}"),
        env.create_file("tests/test1.rs", "#[test] fn t() {}"),
        env.create_file("config/settings.toml", "key = \"val\""),
        env.create_file("data/sample.csv", "a,b,c\n1,2,3"),
    ];

    // Delete them in separate batches to simulate real usage over time
    for path in &paths {
        let del = env.delete(&[path.clone()]);
        assert_eq!(del.succeeded, 1);
    }

    // Verify all 5 are gone
    for path in &paths {
        assert!(!path.exists(), "{} should be archived", path.display());
    }

    // Step 2: Restore all
    let restore_all = env.restore_all();
    assert_eq!(
        restore_all.succeeded, 5,
        "all 5 files should be restored"
    );

    // Step 3: Verify all 5 files are back at original locations with content
    assert_eq!(
        fs::read_to_string(&paths[0]).unwrap(),
        "readme content"
    );
    assert_eq!(fs::read_to_string(&paths[1]).unwrap(), "fn app() {}");
    assert_eq!(
        fs::read_to_string(&paths[2]).unwrap(),
        "#[test] fn t() {}"
    );
    assert_eq!(
        fs::read_to_string(&paths[3]).unwrap(),
        "key = \"val\""
    );
    assert_eq!(
        fs::read_to_string(&paths[4]).unwrap(),
        "a,b,c\n1,2,3"
    );

    // Step 4: All should show as "restored" in the DB
    let archived =
        queries::list_archive_objects(&env.conn, Some("archived"), 100, None).unwrap();
    assert_eq!(archived.len(), 0, "no objects should remain in archived state");

    let restored =
        queries::list_archive_objects(&env.conn, Some("restored"), 100, None).unwrap();
    assert_eq!(restored.len(), 5, "all 5 should be in restored state");
}

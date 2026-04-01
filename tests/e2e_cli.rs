//! E2E CLI tests for SmartRM.
//!
//! These tests spawn the actual `smartrm` binary as a subprocess and verify
//! stdout, stderr, exit codes, and filesystem state. This is the highest-
//! fidelity test layer -- it tests exactly what a user experiences.

use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

fn smartrm_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_smartrm"))
}

fn run_smartrm(
    args: &[&str],
    work_dir: &std::path::Path,
    data_dir: &std::path::Path,
) -> std::process::Output {
    Command::new(smartrm_bin())
        .args(args)
        .current_dir(work_dir)
        .env("SMARTRM_HOME", data_dir) // Isolate archive/DB to tempdir
        .output()
        .expect("failed to execute smartrm")
}

fn run_smartrm_status(
    args: &[&str],
    work_dir: &std::path::Path,
    data_dir: &std::path::Path,
) -> (i32, String, String) {
    let output = run_smartrm(args, work_dir, data_dir);
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (code, stdout, stderr)
}

struct TestEnv {
    work_dir: TempDir,
    data_dir: TempDir,
}

impl TestEnv {
    fn new() -> Self {
        Self {
            work_dir: TempDir::new().unwrap(),
            data_dir: TempDir::new().unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> (i32, String, String) {
        run_smartrm_status(args, self.work_dir.path(), self.data_dir.path())
    }

    fn create_file(&self, name: &str, content: &str) -> PathBuf {
        let path = self.work_dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        path
    }

    fn file_exists(&self, name: &str) -> bool {
        self.work_dir.path().join(name).exists()
    }

    #[allow(dead_code)]
    fn read_file(&self, name: &str) -> String {
        fs::read_to_string(self.work_dir.path().join(name)).unwrap()
    }
}

// ---------------------------------------------------------------------------
// Basic delete operations
// ---------------------------------------------------------------------------

#[test]
fn e2e_delete_single_file() {
    let env = TestEnv::new();
    env.create_file("file.txt", "hello world");

    let (code, _stdout, _stderr) = env.run(&["file.txt"]);

    assert_eq!(code, 0, "exit code should be 0 on success");
    assert!(!env.file_exists("file.txt"), "file should be gone after delete");
}

#[test]
fn e2e_delete_multiple_files() {
    let env = TestEnv::new();
    env.create_file("a.txt", "aaa");
    env.create_file("b.txt", "bbb");
    env.create_file("c.txt", "ccc");

    let (code, _stdout, _stderr) = env.run(&["a.txt", "b.txt", "c.txt"]);

    assert_eq!(code, 0, "exit code should be 0 on success");
    assert!(!env.file_exists("a.txt"), "a.txt should be gone");
    assert!(!env.file_exists("b.txt"), "b.txt should be gone");
    assert!(!env.file_exists("c.txt"), "c.txt should be gone");
}

#[test]
fn e2e_delete_directory() {
    let env = TestEnv::new();
    env.create_file("dir/file1.txt", "one");
    env.create_file("dir/file2.txt", "two");

    let (code, _stdout, _stderr) = env.run(&["-r", "dir"]);

    assert_eq!(code, 0, "exit code should be 0 on recursive dir delete");
    assert!(!env.file_exists("dir"), "dir should be gone");
}

#[test]
fn e2e_delete_directory_without_r_fails() {
    let env = TestEnv::new();
    env.create_file("dir/file.txt", "content");

    let (code, stdout, _stderr) = env.run(&["dir"]);

    assert_eq!(code, 1, "exit code should be 1 when deleting dir without -r");
    let combined = format!("{}{}", stdout, _stderr);
    assert!(
        combined.contains("Is a directory"),
        "error should mention 'Is a directory', got: {}",
        combined
    );
}

#[test]
fn e2e_delete_nonexistent_file_fails() {
    let env = TestEnv::new();

    let (code, stdout, stderr) = env.run(&["nosuchfile"]);

    assert_eq!(code, 1, "exit code should be 1 for nonexistent file");
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("No such file"),
        "error should mention 'No such file', got: {}",
        combined
    );
}

#[test]
fn e2e_delete_nonexistent_with_force() {
    let env = TestEnv::new();

    let (code, stdout, stderr) = env.run(&["-f", "nosuchfile"]);

    assert_eq!(code, 0, "exit code should be 0 with -f for nonexistent");
    assert!(
        stdout.is_empty() || !stdout.contains("error"),
        "should be silent with -f, got stdout: {}",
        stdout
    );
    assert!(
        !stderr.contains("error"),
        "should be silent with -f, got stderr: {}",
        stderr
    );
}

#[test]
fn e2e_delete_with_verbose() {
    let env = TestEnv::new();
    env.create_file("file.txt", "content");

    let (code, _stdout, stderr) = env.run(&["-v", "file.txt"]);

    assert_eq!(code, 0, "exit code should be 0");
    assert!(
        stderr.contains("archived") && stderr.contains("file.txt"),
        "verbose stderr should contain 'archived' and filename, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// JSON output
// ---------------------------------------------------------------------------

#[test]
fn e2e_delete_json_output() {
    let env = TestEnv::new();
    env.create_file("file.txt", "json test");

    let (code, stdout, _stderr) = env.run(&["--json", "file.txt"]);

    assert_eq!(code, 0, "exit code should be 0");

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert!(
        json.get("batch_id").is_some(),
        "JSON should have batch_id field"
    );
    assert_eq!(
        json.get("status").and_then(|v| v.as_str()),
        Some("complete"),
        "status should be 'complete'"
    );
    assert!(
        json.get("items").and_then(|v| v.as_array()).is_some(),
        "JSON should have items array"
    );
}

#[test]
fn e2e_list_json_output() {
    let env = TestEnv::new();
    env.create_file("forlist.txt", "list json test");
    env.run(&["forlist.txt"]);

    let (code, stdout, _stderr) = env.run(&["list", "--json"]);

    assert_eq!(code, 0, "list after delete should exit 0");

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");

    assert!(
        json.get("objects").and_then(|v| v.as_array()).is_some(),
        "JSON should have objects array"
    );
}

// ---------------------------------------------------------------------------
// List command
// ---------------------------------------------------------------------------

#[test]
fn e2e_list_shows_archived() {
    let env = TestEnv::new();
    env.create_file("listed.txt", "listed content");

    env.run(&["listed.txt"]);

    let (code, stdout, _stderr) = env.run(&["list"]);

    assert_eq!(code, 0, "list should exit 0 when items exist");
    assert!(
        stdout.contains("listed.txt"),
        "list should show the filename, got: {}",
        stdout
    );
    assert!(
        stdout.contains("archived"),
        "list should show 'archived' state, got: {}",
        stdout
    );
}

#[test]
fn e2e_list_empty() {
    let env = TestEnv::new();

    let (code, stdout, _stderr) = env.run(&["list"]);

    assert_eq!(code, 2, "list with nothing should exit 2");
    assert!(
        stdout.contains("No archived objects") || stdout.contains("0 objects"),
        "should indicate empty archive, got: {}",
        stdout
    );
}

#[test]
fn e2e_list_with_state_filter() {
    let env = TestEnv::new();
    env.create_file("filtered.txt", "filter test");
    env.run(&["filtered.txt"]);

    // --state archived should show the file
    let (code, stdout, _stderr) = env.run(&["list", "--state", "archived"]);
    assert_eq!(code, 0, "list --state archived should find the item");
    assert!(
        stdout.contains("filtered.txt"),
        "archived filter should show file, got: {}",
        stdout
    );

    // --state purged should show nothing
    let (code2, _stdout2, _stderr2) = env.run(&["list", "--state", "purged"]);
    assert_eq!(code2, 2, "list --state purged should exit 2 (nothing found)");
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

#[test]
fn e2e_undo_restores_file() {
    let env = TestEnv::new();
    env.create_file("undome.txt", "hello");

    env.run(&["undome.txt"]);
    assert!(!env.file_exists("undome.txt"), "file should be gone after delete");

    let (code, _stdout, _stderr) = env.run(&["undo"]);
    assert_eq!(code, 0, "undo should exit 0");
    assert!(env.file_exists("undome.txt"), "file should be restored after undo");
    assert_eq!(
        env.read_file("undome.txt"),
        "hello",
        "file content should match original"
    );
}

#[test]
fn e2e_undo_nothing_to_undo() {
    let env = TestEnv::new();

    let (code, _stdout, stderr) = env.run(&["undo"]);
    // When there are no batches at all, the restore operation returns an error
    // ("no delete batches found"), which the main function maps to exit code 1.
    assert_eq!(code, 1, "undo with nothing to undo should exit 1 (error)");
    assert!(
        stderr.contains("no delete batches") || stderr.contains("nothing to undo"),
        "stderr should explain nothing to undo, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

#[test]
fn e2e_restore_by_short_id() {
    let env = TestEnv::new();
    env.create_file("restorebyid.txt", "restore me");

    // Delete with JSON to get archive_id
    let (code, stdout, _stderr) = env.run(&["--json", "restorebyid.txt"]);
    assert_eq!(code, 0);

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let items = json["items"].as_array().unwrap();
    let archive_id = items[0]["archive_id"].as_str().unwrap();
    // Use first 8 chars as short ID
    let short_id = &archive_id[..8];

    let (code, _stdout, _stderr) = env.run(&["restore", short_id]);
    assert_eq!(code, 0, "restore by short ID should exit 0");
    assert!(
        env.file_exists("restorebyid.txt"),
        "file should be restored"
    );
}

#[test]
fn e2e_restore_to_alternate_path() {
    let env = TestEnv::new();
    env.create_file("original.txt", "alt restore");

    // Delete with JSON
    let (code, stdout, _stderr) = env.run(&["--json", "original.txt"]);
    assert_eq!(code, 0);

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let items = json["items"].as_array().unwrap();
    let archive_id = items[0]["archive_id"].as_str().unwrap();
    let short_id = &archive_id[..8];

    // Create alt dir
    let alt_dir = env.work_dir.path().join("alt_dest");
    fs::create_dir_all(&alt_dir).unwrap();

    let alt_str = alt_dir.to_str().unwrap();
    let (code, _stdout, _stderr) = env.run(&["restore", short_id, "--to", alt_str]);
    assert_eq!(code, 0, "restore --to should exit 0");
    assert!(
        alt_dir.join("original.txt").exists(),
        "file should appear at alternate destination"
    );
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[test]
fn e2e_search_glob() {
    let env = TestEnv::new();
    env.create_file("alpha.txt", "a");
    env.create_file("beta.txt", "b");
    env.create_file("gamma.log", "g");

    env.run(&["alpha.txt", "beta.txt", "gamma.log"]);

    let (code, stdout, _stderr) = env.run(&["search", "*.txt"]);
    assert_eq!(code, 0, "search with glob should find results");
    assert!(
        stdout.contains("alpha.txt") && stdout.contains("beta.txt"),
        "search should find .txt files, got: {}",
        stdout
    );
}

#[test]
fn e2e_search_substring() {
    let env = TestEnv::new();
    env.create_file("my_config.yaml", "key: value");
    env.run(&["my_config.yaml"]);

    let (code, stdout, _stderr) = env.run(&["search", "config"]);
    assert_eq!(code, 0, "substring search should find results");
    assert!(
        stdout.contains("config"),
        "search should show matching file, got: {}",
        stdout
    );
}

#[test]
fn e2e_search_no_results() {
    let env = TestEnv::new();

    let (code, _stdout, _stderr) = env.run(&["search", "nonexistent_pattern_xyz"]);
    assert_eq!(code, 2, "search with no results should exit 2");
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

#[test]
fn e2e_history_shows_versions() {
    let env = TestEnv::new();
    let file_path = env.create_file("versioned.txt", "v1");

    // Delete v1
    env.run(&["versioned.txt"]);
    // Undo to restore
    env.run(&["undo"]);
    // Modify and delete again (v2)
    fs::write(&file_path, "v2").unwrap();
    env.run(&["versioned.txt"]);

    // Use the absolute path for history lookup
    let abs_path = env.work_dir.path().join("versioned.txt");
    let abs_str = abs_path.to_str().unwrap();

    let (code, stdout, _stderr) = env.run(&["history", abs_str]);
    assert_eq!(code, 0, "history should exit 0");
    assert!(
        stdout.contains("2") || stdout.contains("version"),
        "history should show version info, got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

#[test]
fn e2e_timeline_shows_batches() {
    let env = TestEnv::new();
    env.create_file("t1.txt", "timeline1");
    env.create_file("t2.txt", "timeline2");

    // Two separate delete calls = 2 batches
    env.run(&["t1.txt"]);
    env.run(&["t2.txt"]);

    let (code, stdout, _stderr) = env.run(&["timeline"]);
    assert_eq!(code, 0, "timeline should exit 0");
    assert!(
        stdout.contains("2 batch"),
        "timeline should show 2 batches, got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

#[test]
fn e2e_stats_shows_counts() {
    let env = TestEnv::new();
    env.create_file("s1.txt", "stat1");
    env.create_file("s2.txt", "stat2");
    env.create_file("s3.txt", "stat3");

    env.run(&["s1.txt", "s2.txt", "s3.txt"]);

    let (code, stdout, _stderr) = env.run(&["stats"]);
    assert_eq!(code, 0, "stats should exit 0");
    assert!(
        stdout.contains("Total objects:") && stdout.contains("3"),
        "stats should show total objects 3, got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

#[test]
fn e2e_cleanup_dry_run() {
    let env = TestEnv::new();
    env.create_file("cleanup_dry.txt", "dry run content");
    env.run(&["cleanup_dry.txt"]);

    let (code, stdout, _stderr) = env.run(&["cleanup", "--older-than", "0d", "--dry-run"]);
    assert_eq!(code, 0, "cleanup dry run should exit 0");
    assert!(
        stdout.contains("DRY RUN"),
        "output should contain 'DRY RUN', got: {}",
        stdout
    );
}

#[test]
fn e2e_cleanup_actually_purges_is_gated() {
    let env = TestEnv::new();
    env.create_file("cleanup_real.txt", "purge me");
    env.run(&["cleanup_real.txt"]);

    // Verify it's listed before cleanup
    let (code1, _stdout1, _stderr1) = env.run(&["list"]);
    assert_eq!(code1, 0, "should have items before cleanup");

    // Cleanup is destructive — blocked without TTY
    let (code, _stdout, stderr) = env.run(&["cleanup", "--older-than", "0d"]);
    assert_eq!(code, 1, "cleanup should be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("gate denied"),
        "should be gate-denied, got: {}", stderr
    );

    // Archive should still be intact
    let (code2, stdout2, _stderr2) = env.run(&["list", "--state", "archived"]);
    assert_eq!(code2, 0, "archived objects should still exist");
    assert!(stdout2.contains("archived"), "should still show archived state");
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[test]
fn e2e_config_shows_defaults() {
    let env = TestEnv::new();

    let (code, stdout, _stderr) = env.run(&["config"]);
    assert_eq!(code, 0, "config should exit 0");
    assert!(
        stdout.contains("default_delete_mode"),
        "config output should show default_delete_mode, got: {}",
        stdout
    );
}

#[test]
fn e2e_config_set_and_show() {
    let env = TestEnv::new();

    let (code, _stdout, _stderr) = env.run(&["config", "set", "danger_protection", "false"]);
    assert_eq!(code, 0, "config set should exit 0");

    let (code, stdout, _stderr) = env.run(&["config"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("false"),
        "config should show updated value 'false', got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Danger detection
// ---------------------------------------------------------------------------

#[test]
fn e2e_delete_root_blocked() {
    let env = TestEnv::new();

    let (code, stdout, stderr) = env.run(&["-rf", "/"]);
    let combined = format!("{}{}", stdout, stderr);

    assert_eq!(code, 1, "deleting / should exit 1");
    assert!(
        combined.contains("Cannot delete"),
        "error should contain 'Cannot delete', got: {}",
        combined
    );
}

#[test]
fn e2e_delete_git_dir_warns() {
    let env = TestEnv::new();
    env.create_file(".git/HEAD", "ref: refs/heads/main");
    env.create_file(".git/config", "[core]");

    let (code, stdout, stderr) = env.run(&["-r", ".git"]);
    let combined = format!("{}{}", stdout, stderr);

    assert_eq!(code, 1, "deleting .git without --yes-i-am-sure should exit 1");
    assert!(
        combined.contains("git history"),
        "error should mention 'git history', got: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

#[test]
fn e2e_exit_code_0_on_success() {
    let env = TestEnv::new();
    env.create_file("success.txt", "ok");

    let (code, _stdout, _stderr) = env.run(&["success.txt"]);
    assert_eq!(code, 0);
}

#[test]
fn e2e_exit_code_1_on_error() {
    let env = TestEnv::new();

    let (code, _stdout, _stderr) = env.run(&["doesnotexist.txt"]);
    assert_eq!(code, 1);
}

#[test]
fn e2e_exit_code_2_on_empty_search() {
    let env = TestEnv::new();

    let (code, _stdout, _stderr) = env.run(&["search", "nothing_here_at_all"]);
    assert_eq!(code, 2);
}

// ---------------------------------------------------------------------------
// Completions
// ---------------------------------------------------------------------------

#[test]
fn e2e_completions_bash() {
    let env = TestEnv::new();

    let (code, stdout, _stderr) = env.run(&["completions", "bash"]);
    assert_eq!(code, 0, "completions bash should exit 0");
    assert!(
        !stdout.is_empty(),
        "completions output should not be empty"
    );
}

// ---------------------------------------------------------------------------
// Explain policy
// ---------------------------------------------------------------------------

#[test]
fn e2e_explain_policy() {
    let env = TestEnv::new();

    let (code, stdout, _stderr) = env.run(&["explain-policy", ".env"]);
    assert_eq!(code, 0, "explain-policy should exit 0");
    assert!(
        stdout.to_lowercase().contains("protected"),
        "explain-policy for .env should mention 'protected', got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Symlinks
// ---------------------------------------------------------------------------

#[test]
fn e2e_delete_symlink() {
    let env = TestEnv::new();
    let target = env.create_file("target.txt", "I am the target");

    let link_path = env.work_dir.path().join("symlink.txt");
    unix_fs::symlink(&target, &link_path).unwrap();

    let (code, _stdout, _stderr) = env.run(&["symlink.txt"]);

    assert_eq!(code, 0, "deleting symlink should exit 0");
    assert!(
        !link_path.exists() && !link_path.symlink_metadata().is_ok(),
        "symlink should be gone"
    );
    assert!(target.exists(), "target file should still exist");
}

#[test]
fn e2e_delete_broken_symlink() {
    let env = TestEnv::new();

    // Create a symlink pointing to a nonexistent target
    let link_path = env.work_dir.path().join("broken_link");
    unix_fs::symlink("/nonexistent/target/path", &link_path).unwrap();

    let (code, _stdout, _stderr) = env.run(&["broken_link"]);

    assert_eq!(code, 0, "deleting broken symlink should exit 0");
    assert!(
        !link_path.symlink_metadata().is_ok(),
        "broken symlink should be gone"
    );
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn e2e_double_dash_literal_filename() {
    let env = TestEnv::new();
    env.create_file("-rf", "not a flag");

    let (code, _stdout, _stderr) = env.run(&["--", "-rf"]);

    assert_eq!(code, 0, "-- should allow literal filename starting with dash");
    assert!(
        !env.file_exists("-rf"),
        "file named -rf should be archived"
    );
}

#[test]
fn e2e_empty_dir_with_d_flag() {
    let env = TestEnv::new();
    let empty_dir = env.work_dir.path().join("emptydir");
    fs::create_dir_all(&empty_dir).unwrap();

    let (code, _stdout, _stderr) = env.run(&["-d", "emptydir"]);

    assert_eq!(code, 0, "-d should archive empty directory");
    assert!(
        !env.file_exists("emptydir"),
        "empty dir should be gone"
    );
}

// ---------------------------------------------------------------------------
// Destructive gate tests — verify agent/non-TTY blocking
// ---------------------------------------------------------------------------

#[test]
fn e2e_permanent_blocked_without_tty() {
    let env = TestEnv::new();
    env.create_file("precious.txt", "do not delete me");

    let (code, _stdout, stderr) = env.run(&["--permanent", "precious.txt"]);

    assert_eq!(code, 1, "--permanent should be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("terminal") || stderr.contains("gate denied"),
        "should mention TTY requirement, got: {}", stderr
    );
    assert!(
        env.file_exists("precious.txt"),
        "file must survive when gate blocks"
    );
    assert_eq!(
        env.read_file("precious.txt"),
        "do not delete me",
        "file content must be intact"
    );
}

#[test]
fn e2e_permanent_force_still_blocked_without_tty() {
    let env = TestEnv::new();
    env.create_file("victim.txt", "still here");

    // --force should NOT bypass the destructive gate
    let (code, _stdout, stderr) = env.run(&["--permanent", "-f", "victim.txt"]);

    assert_eq!(code, 1, "--permanent -f should still be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("terminal") || stderr.contains("gate denied"),
        "should mention TTY requirement, got: {}", stderr
    );
    assert!(
        env.file_exists("victim.txt"),
        "file must survive even with --force"
    );
}

#[test]
fn e2e_purge_blocked_without_tty() {
    let env = TestEnv::new();
    env.create_file("archived.txt", "in archive");

    // Archive the file first
    let (code, _, _) = env.run(&["archived.txt"]);
    assert_eq!(code, 0);
    assert!(!env.file_exists("archived.txt"));

    // Now try to purge — should be blocked
    let (code, _stdout, stderr) = env.run(&["purge", "--all", "--force"]);

    assert_eq!(code, 1, "purge should be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("terminal") || stderr.contains("gate denied"),
        "should mention TTY requirement, got: {}", stderr
    );

    // Archive should still be intact
    let (code, stdout, _) = env.run(&["list"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("archived"),
        "archive should still contain the file"
    );
}

#[test]
fn e2e_purge_expired_blocked_without_tty() {
    let env = TestEnv::new();
    env.create_file("temp.txt", "temp");
    let (code, _, _) = env.run(&["temp.txt"]);
    assert_eq!(code, 0);

    let (code, _stdout, stderr) = env.run(&["purge", "--expired"]);
    assert_eq!(code, 1, "purge --expired should be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("terminal") || stderr.contains("gate denied"),
        "got: {}", stderr
    );
}

#[test]
fn e2e_normal_archive_not_blocked() {
    let env = TestEnv::new();
    env.create_file("normal.txt", "safe to archive");

    // Normal archive should NOT be gated
    let (code, _stdout, _stderr) = env.run(&["normal.txt"]);

    assert_eq!(code, 0, "normal archive should work without TTY");
    assert!(!env.file_exists("normal.txt"), "file should be archived");
}

#[test]
fn e2e_cleanup_dryrun_not_blocked() {
    let env = TestEnv::new();
    env.create_file("old.txt", "old data");
    let (code, _, _) = env.run(&["old.txt"]);
    assert_eq!(code, 0);

    // Cleanup --dry-run is read-only, should NOT be gated
    let (code, stdout, _stderr) = env.run(&["cleanup", "--older-than", "0d", "--dry-run"]);
    assert_eq!(code, 0, "cleanup dry-run should work without TTY");
    assert!(stdout.contains("DRY RUN"), "should indicate dry run");
}

#[test]
fn e2e_cleanup_blocked_without_tty() {
    let env = TestEnv::new();
    env.create_file("archived.txt", "important data");
    let (code, _, _) = env.run(&["archived.txt"]);
    assert_eq!(code, 0);

    // Actual cleanup is destructive — should be blocked without TTY
    let (code, _stdout, stderr) = env.run(&["cleanup", "--older-than", "0d"]);
    assert_eq!(code, 1, "cleanup should be blocked without TTY");
    assert!(
        stderr.contains("TTY") || stderr.contains("terminal") || stderr.contains("gate denied"),
        "should mention TTY requirement, got: {}", stderr
    );

    // Archive should still be intact — content not destroyed
    let (list_code, list_stdout, _) = env.run(&["list"]);
    assert_eq!(list_code, 0);
    assert!(list_stdout.contains("archived"), "file should still be archived");

    // Undo should still work — proving content was not destroyed
    let (undo_code, _, _) = env.run(&["undo"]);
    assert_eq!(undo_code, 0, "undo should work since content wasn't purged");
    assert!(env.file_exists("archived.txt"), "file should be restored");
    assert_eq!(env.read_file("archived.txt"), "important data");
}

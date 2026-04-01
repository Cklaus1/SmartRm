use rusqlite::Connection;
use crate::error::Result;

/// Current schema version. Bump when adding migrations.
const SCHEMA_VERSION: i64 = 1;

/// Initialize the database schema. Creates all tables, indexes, and the
/// schema_version tracking row if they do not already exist.
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(CREATE_ALL)?;

    // Ensure the version row exists.
    let existing: Option<i64> = conn
        .query_row(
            "SELECT version FROM schema_version LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    if existing.is_none() {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// DDL — all CREATE TABLE / CREATE INDEX statements
// ---------------------------------------------------------------------------

const CREATE_ALL: &str = "
-- Version tracking (single row)
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

-- 1. batches
CREATE TABLE IF NOT EXISTS batches (
    batch_id TEXT PRIMARY KEY,
    operation_type TEXT NOT NULL CHECK (operation_type IN ('delete', 'restore', 'cleanup', 'purge')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'complete', 'partial', 'failed', 'rolled_back')),
    requested_by TEXT,
    cwd TEXT,
    hostname TEXT,
    command_line TEXT,
    total_objects_requested INTEGER NOT NULL DEFAULT 0,
    total_objects_processed INTEGER NOT NULL DEFAULT 0,
    total_objects_succeeded INTEGER NOT NULL DEFAULT 0,
    total_objects_failed INTEGER NOT NULL DEFAULT 0,
    total_bytes INTEGER NOT NULL DEFAULT 0,
    interactive_mode INTEGER NOT NULL DEFAULT 0 CHECK (interactive_mode IN (0, 1)),
    used_force INTEGER NOT NULL DEFAULT 0 CHECK (used_force IN (0, 1)),
    started_at TEXT NOT NULL,
    completed_at TEXT,
    summary_message TEXT
);
CREATE INDEX IF NOT EXISTS idx_batches_started_at ON batches(started_at);
CREATE INDEX IF NOT EXISTS idx_batches_status ON batches(status);
CREATE INDEX IF NOT EXISTS idx_batches_operation_type ON batches(operation_type);

-- 2. mounts
CREATE TABLE IF NOT EXISTS mounts (
    mount_id TEXT PRIMARY KEY,
    device_name TEXT,
    mount_point TEXT NOT NULL,
    fs_type TEXT,
    archive_root TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mounts_mount_point ON mounts(mount_point);

-- 3. policies
CREATE TABLE IF NOT EXISTS policies (
    policy_id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'user', 'path_prefix', 'project', 'mount')),
    scope_value TEXT,
    priority INTEGER NOT NULL,
    intent_default TEXT,
    ttl_seconds_default INTEGER,
    min_free_space_bytes INTEGER,
    auto_cleanup_enabled INTEGER NOT NULL DEFAULT 0 CHECK (auto_cleanup_enabled IN (0, 1)),
    protect_patterns_json TEXT,
    exclude_patterns_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_policies_scope ON policies(scope_type, scope_value);
CREATE INDEX IF NOT EXISTS idx_policies_priority ON policies(priority DESC);

-- 4. archive_objects (references batches, mounts, policies)
CREATE TABLE IF NOT EXISTS archive_objects (
    archive_id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL,
    parent_archive_id TEXT,
    object_type TEXT NOT NULL CHECK (object_type IN ('file', 'dir', 'symlink', 'other')),
    state TEXT NOT NULL CHECK (state IN ('archived', 'restored', 'expired', 'purged', 'failed')),
    original_path TEXT NOT NULL,
    archived_path TEXT,
    storage_mount_id TEXT,
    original_mount_id TEXT,
    size_bytes INTEGER,
    content_hash TEXT,
    link_target TEXT,
    mode INTEGER,
    uid INTEGER,
    gid INTEGER,
    mtime_ns INTEGER,
    ctime_ns INTEGER,
    xattrs_json TEXT,
    acl_blob BLOB,
    delete_intent TEXT,
    ttl_seconds INTEGER,
    policy_id TEXT,
    delete_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    restored_at TEXT,
    expired_at TEXT,
    purged_at TEXT,
    failure_code TEXT,
    failure_message TEXT,
    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE RESTRICT,
    FOREIGN KEY (parent_archive_id) REFERENCES archive_objects(archive_id) ON DELETE SET NULL,
    FOREIGN KEY (storage_mount_id) REFERENCES mounts(mount_id) ON DELETE SET NULL,
    FOREIGN KEY (original_mount_id) REFERENCES mounts(mount_id) ON DELETE SET NULL,
    FOREIGN KEY (policy_id) REFERENCES policies(policy_id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_ao_batch_id ON archive_objects(batch_id);
CREATE INDEX IF NOT EXISTS idx_ao_parent_archive_id ON archive_objects(parent_archive_id);
CREATE INDEX IF NOT EXISTS idx_ao_state ON archive_objects(state);
CREATE INDEX IF NOT EXISTS idx_ao_original_path ON archive_objects(original_path);
CREATE INDEX IF NOT EXISTS idx_ao_created_at ON archive_objects(created_at);
CREATE INDEX IF NOT EXISTS idx_ao_content_hash ON archive_objects(content_hash);
CREATE INDEX IF NOT EXISTS idx_ao_delete_intent ON archive_objects(delete_intent);
CREATE INDEX IF NOT EXISTS idx_ao_policy_id ON archive_objects(policy_id);

-- 5. batch_items (references batches, archive_objects)
CREATE TABLE IF NOT EXISTS batch_items (
    batch_item_id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL,
    input_path TEXT NOT NULL,
    resolved_path TEXT,
    archive_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed', 'skipped')),
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE CASCADE,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_batch_items_batch_id ON batch_items(batch_id);
CREATE INDEX IF NOT EXISTS idx_batch_items_status ON batch_items(status);
CREATE INDEX IF NOT EXISTS idx_batch_items_input_path ON batch_items(input_path);

-- 6. restore_events (references archive_objects, batches)
CREATE TABLE IF NOT EXISTS restore_events (
    restore_event_id TEXT PRIMARY KEY,
    archive_id TEXT NOT NULL,
    restore_batch_id TEXT NOT NULL,
    restore_mode TEXT NOT NULL CHECK (restore_mode IN ('original', 'alternate_path', 'overwrite', 'rename_on_conflict')),
    requested_target_path TEXT,
    final_restored_path TEXT,
    status TEXT NOT NULL CHECK (status IN ('succeeded', 'failed', 'partial')),
    conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('fail', 'rename', 'overwrite', 'skip')),
    mode_restored INTEGER NOT NULL DEFAULT 0 CHECK (mode_restored IN (0, 1)),
    ownership_restored INTEGER NOT NULL DEFAULT 0 CHECK (ownership_restored IN (0, 1)),
    timestamps_restored INTEGER NOT NULL DEFAULT 0 CHECK (timestamps_restored IN (0, 1)),
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE RESTRICT,
    FOREIGN KEY (restore_batch_id) REFERENCES batches(batch_id) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS idx_re_archive_id ON restore_events(archive_id);
CREATE INDEX IF NOT EXISTS idx_re_restore_batch_id ON restore_events(restore_batch_id);
CREATE INDEX IF NOT EXISTS idx_re_created_at ON restore_events(created_at);

-- 7. effective_policies (references batches, archive_objects)
CREATE TABLE IF NOT EXISTS effective_policies (
    effective_policy_id TEXT PRIMARY KEY,
    batch_id TEXT,
    archive_id TEXT,
    setting_key TEXT NOT NULL,
    setting_value TEXT,
    source_type TEXT NOT NULL CHECK (source_type IN ('cli', 'interactive', 'user_rule', 'project_rule', 'system_rule', 'learned', 'default', 'hard_safety')),
    source_ref TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (batch_id) REFERENCES batches(batch_id) ON DELETE CASCADE,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ep_batch_id ON effective_policies(batch_id);
CREATE INDEX IF NOT EXISTS idx_ep_archive_id ON effective_policies(archive_id);
CREATE INDEX IF NOT EXISTS idx_ep_setting_key ON effective_policies(setting_key);

-- 8. hash_jobs (references archive_objects)
CREATE TABLE IF NOT EXISTS hash_jobs (
    hash_job_id TEXT PRIMARY KEY,
    archive_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (archive_id) REFERENCES archive_objects(archive_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_hj_status ON hash_jobs(status);
CREATE INDEX IF NOT EXISTS idx_hj_archive_id ON hash_jobs(archive_id);

-- 9. destructive_audit_log (no foreign keys)
CREATE TABLE IF NOT EXISTS destructive_audit_log (
    attempt_id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    os_user TEXT,
    hostname TEXT,
    cwd TEXT,
    command TEXT NOT NULL,
    arguments TEXT,
    interactive_tty_present INTEGER NOT NULL CHECK (interactive_tty_present IN (0, 1)),
    scope_count INTEGER,
    scope_bytes INTEGER,
    protected_paths_affected INTEGER,
    result TEXT NOT NULL CHECK (result IN ('allowed', 'denied', 'locked_out', 'no_tty', 'blocked_agent')),
    failure_reason TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dal_timestamp ON destructive_audit_log(timestamp);
CREATE INDEX IF NOT EXISTS idx_dal_result ON destructive_audit_log(result);
CREATE INDEX IF NOT EXISTS idx_dal_os_user ON destructive_audit_log(os_user);
";

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn initialize_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        initialize(&conn).unwrap();

        // Verify all 10 tables exist by querying sqlite_master.
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let expected = vec![
            "archive_objects",
            "batch_items",
            "batches",
            "destructive_audit_log",
            "effective_policies",
            "hash_jobs",
            "mounts",
            "policies",
            "restore_events",
            "schema_version",
        ];
        assert_eq!(tables, expected);
    }

    #[test]
    fn initialize_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        initialize(&conn).unwrap();
        initialize(&conn).unwrap(); // second call must not fail

        let version: i64 = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
}

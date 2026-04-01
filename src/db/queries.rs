use rusqlite::{params, Connection, Row};
use serde::Serialize;

use crate::error::Result;
use crate::models::{
    ArchiveObject, Batch, BatchItem, BatchItemStatus, BatchStatus, EffectivePolicy, LifecycleState,
    ObjectType, OperationType, SourceType,
};

// ---------------------------------------------------------------------------
// Row-mapping helpers
// ---------------------------------------------------------------------------

fn row_to_batch(row: &Row) -> rusqlite::Result<Batch> {
    Ok(Batch {
        batch_id: row.get("batch_id")?,
        operation_type: {
            let s: String = row.get("operation_type")?;
            s.parse::<OperationType>().unwrap_or(OperationType::Delete)
        },
        status: {
            let s: String = row.get("status")?;
            s.parse::<BatchStatus>().unwrap_or(BatchStatus::Pending)
        },
        requested_by: row.get("requested_by")?,
        cwd: row.get("cwd")?,
        hostname: row.get("hostname")?,
        command_line: row.get("command_line")?,
        total_objects_requested: row.get("total_objects_requested")?,
        total_objects_processed: row.get("total_objects_processed")?,
        total_objects_succeeded: row.get("total_objects_succeeded")?,
        total_objects_failed: row.get("total_objects_failed")?,
        total_bytes: row.get("total_bytes")?,
        interactive_mode: {
            let v: i64 = row.get("interactive_mode")?;
            v != 0
        },
        used_force: {
            let v: i64 = row.get("used_force")?;
            v != 0
        },
        started_at: row.get("started_at")?,
        completed_at: row.get("completed_at")?,
        summary_message: row.get("summary_message")?,
    })
}

fn row_to_batch_item(row: &Row) -> rusqlite::Result<BatchItem> {
    Ok(BatchItem {
        batch_item_id: row.get("batch_item_id")?,
        batch_id: row.get("batch_id")?,
        input_path: row.get("input_path")?,
        resolved_path: row.get("resolved_path")?,
        archive_id: row.get("archive_id")?,
        status: {
            let s: String = row.get("status")?;
            s.parse::<BatchItemStatus>().unwrap_or(BatchItemStatus::Pending)
        },
        error_code: row.get("error_code")?,
        error_message: row.get("error_message")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_archive_object(row: &Row) -> rusqlite::Result<ArchiveObject> {
    Ok(ArchiveObject {
        archive_id: row.get("archive_id")?,
        batch_id: row.get("batch_id")?,
        parent_archive_id: row.get("parent_archive_id")?,
        object_type: {
            let s: String = row.get("object_type")?;
            s.parse::<ObjectType>().unwrap_or(ObjectType::File)
        },
        state: {
            let s: String = row.get("state")?;
            s.parse::<LifecycleState>().unwrap_or(LifecycleState::Archived)
        },
        original_path: row.get("original_path")?,
        archived_path: row.get("archived_path")?,
        storage_mount_id: row.get("storage_mount_id")?,
        original_mount_id: row.get("original_mount_id")?,
        size_bytes: row.get("size_bytes")?,
        content_hash: row.get("content_hash")?,
        link_target: row.get("link_target")?,
        mode: {
            let v: Option<i64> = row.get("mode")?;
            v.map(|n| n as u32)
        },
        uid: {
            let v: Option<i64> = row.get("uid")?;
            v.map(|n| n as u32)
        },
        gid: {
            let v: Option<i64> = row.get("gid")?;
            v.map(|n| n as u32)
        },
        mtime_ns: row.get("mtime_ns")?,
        ctime_ns: row.get("ctime_ns")?,
        delete_intent: row.get("delete_intent")?,
        ttl_seconds: row.get("ttl_seconds")?,
        policy_id: row.get("policy_id")?,
        delete_reason: row.get("delete_reason")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        restored_at: row.get("restored_at")?,
        expired_at: row.get("expired_at")?,
        purged_at: row.get("purged_at")?,
        failure_code: row.get("failure_code")?,
        failure_message: row.get("failure_message")?,
    })
}

// ---------------------------------------------------------------------------
// Batch queries
// ---------------------------------------------------------------------------

pub fn get_batch(conn: &Connection, batch_id: &str) -> Result<Option<Batch>> {
    let mut stmt = conn.prepare("SELECT * FROM batches WHERE batch_id = ?1")?;
    let mut rows = stmt.query_map(params![batch_id], row_to_batch)?;
    match rows.next() {
        Some(Ok(batch)) => Ok(Some(batch)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_latest_delete_batch(conn: &Connection) -> Result<Option<Batch>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM batches
         WHERE operation_type = 'delete'
         ORDER BY started_at DESC
         LIMIT 1",
    )?;
    let mut rows = stmt.query_map([], row_to_batch)?;
    match rows.next() {
        Some(Ok(batch)) => Ok(Some(batch)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Archive object queries
// ---------------------------------------------------------------------------

pub fn get_archive_object(
    conn: &Connection,
    archive_id: &str,
) -> Result<Option<ArchiveObject>> {
    let mut stmt = conn.prepare("SELECT * FROM archive_objects WHERE archive_id = ?1")?;
    let mut rows = stmt.query_map(params![archive_id], row_to_archive_object)?;
    match rows.next() {
        Some(Ok(obj)) => Ok(Some(obj)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn get_archive_objects_for_batch(
    conn: &Connection,
    batch_id: &str,
) -> Result<Vec<ArchiveObject>> {
    let mut stmt =
        conn.prepare("SELECT * FROM archive_objects WHERE batch_id = ?1 ORDER BY original_path")?;
    let rows = stmt.query_map(params![batch_id], row_to_archive_object)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Batch item queries
// ---------------------------------------------------------------------------

pub fn get_batch_items_for_batch(
    conn: &Connection,
    batch_id: &str,
) -> Result<Vec<BatchItem>> {
    let mut stmt =
        conn.prepare("SELECT * FROM batch_items WHERE batch_id = ?1 ORDER BY created_at")?;
    let rows = stmt.query_map(params![batch_id], row_to_batch_item)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Archive object queries — extended for restore / list
// ---------------------------------------------------------------------------

/// List archive objects with optional state filter and keyset pagination.
///
/// Results are ordered by `created_at DESC`. When `cursor` is provided, only
/// objects with `created_at < cursor` are returned (keyset pagination).
pub fn list_archive_objects(
    conn: &Connection,
    state_filter: Option<&str>,
    limit: u32,
    cursor: Option<&str>,
) -> Result<Vec<ArchiveObject>> {
    let rows: Vec<ArchiveObject> = match (state_filter, cursor) {
        (Some(state), Some(cur)) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM archive_objects WHERE state = ?1 AND created_at < ?2
                 ORDER BY created_at DESC LIMIT ?3",
            )?;
            let mapped = stmt.query_map(params![state, cur, limit], row_to_archive_object)?;
            mapped.filter_map(|r| r.ok()).collect()
        }
        (Some(state), None) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM archive_objects WHERE state = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![state, limit], row_to_archive_object)?;
            mapped.filter_map(|r| r.ok()).collect()
        }
        (None, Some(cur)) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM archive_objects WHERE created_at < ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![cur, limit], row_to_archive_object)?;
            mapped.filter_map(|r| r.ok()).collect()
        }
        (None, None) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM archive_objects
                 ORDER BY created_at DESC LIMIT ?1",
            )?;
            let mapped = stmt.query_map(params![limit], row_to_archive_object)?;
            mapped.filter_map(|r| r.ok()).collect()
        }
    };

    Ok(rows)
}

/// Count archive objects, optionally filtered by state.
pub fn count_archive_objects(conn: &Connection, state_filter: Option<&str>) -> Result<i64> {
    let count: i64 = match state_filter {
        Some(state) => conn.query_row(
            "SELECT COUNT(*) FROM archive_objects WHERE state = ?1",
            params![state],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM archive_objects",
            [],
            |row| row.get(0),
        )?,
    };
    Ok(count)
}

/// Get the last N delete batches by started_at DESC.
pub fn get_latest_delete_batches(conn: &Connection, n: u32) -> Result<Vec<Batch>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM batches
         WHERE operation_type = 'delete'
         ORDER BY started_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![n], row_to_batch)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Find archive objects whose ID starts with `prefix`.
pub fn get_archive_object_by_prefix(
    conn: &Connection,
    prefix: &str,
) -> Result<Vec<ArchiveObject>> {
    let pattern = format!("{}%", prefix);
    let mut stmt = conn.prepare(
        "SELECT * FROM archive_objects WHERE archive_id LIKE ?1 ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map(params![pattern], row_to_archive_object)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// ArchiveStats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveStats {
    pub count_by_state: Vec<(String, i64)>,
    pub total_size_bytes: i64,
    pub top_directories: Vec<(String, i64)>,
}

// ---------------------------------------------------------------------------
// Row-mapping: effective_policies
// ---------------------------------------------------------------------------

fn row_to_effective_policy(row: &Row) -> rusqlite::Result<EffectivePolicy> {
    Ok(EffectivePolicy {
        effective_policy_id: row.get("effective_policy_id")?,
        batch_id: row.get("batch_id")?,
        archive_id: row.get("archive_id")?,
        setting_key: row.get("setting_key")?,
        setting_value: row.get("setting_value")?,
        source_type: {
            let s: String = row.get("source_type")?;
            s.parse::<SourceType>().unwrap_or(SourceType::Default)
        },
        source_ref: row.get("source_ref")?,
        created_at: row.get("created_at")?,
    })
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search archive objects with optional glob/substring, date, size, and directory filters.
pub fn search_archive_objects(
    conn: &Connection,
    pattern: &str,
    is_glob: bool,
    after: Option<&str>,
    min_size: Option<i64>,
    dir_filter: Option<&str>,
    offset: u32,
    limit: u32,
) -> Result<Vec<ArchiveObject>> {
    let mut sql = String::from("SELECT * FROM archive_objects WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    // Pattern filter
    if is_glob {
        let like_pattern = crate::commands::search::glob_to_sql_like(pattern);
        sql.push_str(" AND original_path LIKE ?");
        param_values.push(Box::new(like_pattern));
    } else {
        let substr = format!("%{}%", pattern);
        sql.push_str(" AND original_path LIKE ?");
        param_values.push(Box::new(substr));
    }

    // Date filter
    if let Some(after_date) = after {
        sql.push_str(" AND created_at >= ?");
        param_values.push(Box::new(after_date.to_string()));
    }

    // Size filter
    if let Some(min_bytes) = min_size {
        sql.push_str(" AND size_bytes >= ?");
        param_values.push(Box::new(min_bytes));
    }

    // Directory filter
    if let Some(dir) = dir_filter {
        let dir_pattern = if dir.ends_with('/') {
            format!("{}%", dir)
        } else {
            format!("{}/%", dir)
        };
        sql.push_str(" AND original_path LIKE ?");
        param_values.push(Box::new(dir_pattern));
    }

    sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), row_to_archive_object)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// Get all versions of a file at a given path, ordered newest first.
///
/// If the path contains no `/`, it matches as a filename suffix.
pub fn get_history_for_path(conn: &Connection, path: &str) -> Result<Vec<ArchiveObject>> {
    let is_bare = !path.contains('/');

    let (sql, param): (&str, String) = if is_bare {
        (
            "SELECT * FROM archive_objects WHERE original_path LIKE ?1 ORDER BY created_at DESC",
            format!("%/{}", path),
        )
    } else {
        (
            "SELECT * FROM archive_objects WHERE original_path = ?1 ORDER BY created_at DESC",
            path.to_string(),
        )
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![param], row_to_archive_object)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

/// Get batches ordered by started_at DESC, with optional filters.
pub fn get_timeline_batches(
    conn: &Connection,
    today_only: bool,
    dir_filter: Option<&str>,
    limit: u32,
) -> Result<Vec<Batch>> {
    if let Some(dir) = dir_filter {
        // Join with archive_objects to filter by directory
        let dir_pattern = if dir.ends_with('/') {
            format!("{}%", dir)
        } else {
            format!("{}/%", dir)
        };

        let mut sql = String::from(
            "SELECT DISTINCT b.* FROM batches b
             INNER JOIN archive_objects ao ON ao.batch_id = b.batch_id
             WHERE ao.original_path LIKE ?1",
        );

        if today_only {
            sql.push_str(" AND b.started_at >= date('now')");
        }

        sql.push_str(" ORDER BY b.started_at DESC LIMIT ?2");

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![dir_pattern, limit], row_to_batch)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    } else if today_only {
        let mut stmt = conn.prepare(
            "SELECT * FROM batches WHERE started_at >= date('now')
             ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_batch)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    } else {
        let mut stmt = conn.prepare(
            "SELECT * FROM batches ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], row_to_batch)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Get aggregate archive statistics.
pub fn get_stats(conn: &Connection) -> Result<ArchiveStats> {
    // Count by state
    let mut stmt = conn.prepare(
        "SELECT state, COUNT(*) as cnt FROM archive_objects GROUP BY state ORDER BY cnt DESC",
    )?;
    let count_by_state: Vec<(String, i64)> = stmt
        .query_map([], |row| {
            let state: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((state, count))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Total size
    let total_size_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM archive_objects",
            [],
            |row| row.get(0),
        )?;

    // Top directories: extract directory from original_path
    // Use SQLite's ability to find the last '/' and take everything before it.
    let _stmt = conn.prepare(
        "SELECT
            CASE
                WHEN INSTR(original_path, '/') > 0
                THEN SUBSTR(original_path, 1, LENGTH(original_path) - LENGTH(REPLACE(SUBSTR(original_path, INSTR(original_path, '/')), '/', '')) - 1 + INSTR(original_path, '/') - 1)
                ELSE '.'
            END AS dir,
            COUNT(*) as cnt
         FROM archive_objects
         GROUP BY dir
         ORDER BY cnt DESC
         LIMIT 10",
    )?;

    // The above SQL is complex. Use a simpler approach: rtrim after last '/'.
    drop(stmt);

    // Simpler: use a subquery with SUBSTR + reverse-find approach
    // SQLite doesn't have REVERSE, so we use: everything up to the last '/'
    let mut stmt = conn.prepare(
        "SELECT dir, COUNT(*) as cnt FROM (
            SELECT
                CASE
                    WHEN original_path LIKE '%/%'
                    THEN RTRIM(original_path, REPLACE(original_path, '/', ''))
                    ELSE '.'
                END AS dir
            FROM archive_objects
        )
        GROUP BY dir
        ORDER BY cnt DESC
        LIMIT 10",
    )?;

    let top_directories: Vec<(String, i64)> = stmt
        .query_map([], |row| {
            let dir: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            // Trim trailing slash
            let dir = dir.trim_end_matches('/').to_string();
            Ok((dir, count))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(ArchiveStats {
        count_by_state,
        total_size_bytes,
        top_directories,
    })
}

// ---------------------------------------------------------------------------
// Effective policies
// ---------------------------------------------------------------------------

/// Get effective policies for a given batch.
pub fn get_effective_policies_for_batch(
    conn: &Connection,
    batch_id: &str,
) -> Result<Vec<EffectivePolicy>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM effective_policies WHERE batch_id = ?1 ORDER BY setting_key",
    )?;
    let rows = stmt.query_map(params![batch_id], row_to_effective_policy)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Get all archive objects with state 'archived' or 'expired'.
pub fn get_all_archived_objects(conn: &Connection) -> Result<Vec<ArchiveObject>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM archive_objects WHERE state IN ('archived', 'expired')
         ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_archive_object)?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Cleanup / purge queries
// ---------------------------------------------------------------------------

/// Get objects eligible for cleanup.
///
/// If `expired_only` is true, returns only objects with state `'expired'`.
/// If `older_than_timestamp` is provided, returns archived objects created before
/// that timestamp. If neither is set, returns all archived + expired objects.
pub fn get_objects_for_cleanup(
    conn: &Connection,
    older_than_timestamp: Option<&str>,
    expired_only: bool,
) -> Result<Vec<ArchiveObject>> {
    let objects = if expired_only {
        let mut stmt = conn.prepare(
            "SELECT * FROM archive_objects WHERE state = 'expired'
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_archive_object)?;
        rows.filter_map(|r| r.ok()).collect()
    } else if let Some(ts) = older_than_timestamp {
        let mut stmt = conn.prepare(
            "SELECT * FROM archive_objects WHERE state = 'archived' AND created_at < ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![ts], row_to_archive_object)?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT * FROM archive_objects WHERE state IN ('archived', 'expired')
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_archive_object)?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(objects)
}

/// Count all archived + expired objects and their total size in bytes.
///
/// Returns `(count, total_bytes)`.
pub fn count_all_archived(conn: &Connection) -> Result<(i64, i64)> {
    let (count, total_bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(COALESCE(size_bytes, 0)), 0)
         FROM archive_objects WHERE state IN ('archived', 'expired')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((count, total_bytes))
}

/// Transition all archived and expired objects to purged state.
pub fn purge_all_archived(conn: &Connection) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE archive_objects SET state = 'purged', purged_at = ?1, updated_at = ?1
         WHERE state IN ('archived', 'expired')",
        params![now],
    )?;
    Ok(())
}

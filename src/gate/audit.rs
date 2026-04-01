use rusqlite::{params, Connection};

use super::GateScope;
use crate::error::Result;
use crate::id;

/// Write a record to the destructive_audit_log table.
///
/// `result` must be one of: "allowed", "denied", "locked_out", "no_tty", "blocked_agent".
pub fn log_attempt(
    conn: &Connection,
    command: &str,
    arguments: &str,
    tty_present: bool,
    scope: &GateScope,
    result: &str,
    failure_reason: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let attempt_id = id::new_id();
    let os_user = std::env::var("USER").ok();
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    conn.execute(
        "INSERT INTO destructive_audit_log (
            attempt_id, timestamp, os_user, hostname, cwd,
            command, arguments, interactive_tty_present,
            scope_count, scope_bytes, protected_paths_affected,
            result, failure_reason, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            attempt_id,
            now,
            os_user,
            Option::<String>::None, // hostname
            cwd,
            command,
            arguments,
            tty_present as i32,
            scope.object_count as i64,
            scope.total_bytes as i64,
            scope.protected_count as i64,
            result,
            failure_reason,
            now,
        ],
    )?;
    Ok(())
}

/// Count audit log entries matching a given result type.
pub fn count_audit_entries(conn: &Connection, result_filter: Option<&str>) -> Result<i64> {
    let count: i64 = match result_filter {
        Some(result) => conn.query_row(
            "SELECT COUNT(*) FROM destructive_audit_log WHERE result = ?1",
            params![result],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM destructive_audit_log",
            [],
            |row| row.get(0),
        )?,
    };
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn log_and_count_attempts() {
        let conn = db::open_memory_database().unwrap();

        let scope = GateScope {
            action: "purge".to_string(),
            object_count: 10,
            total_bytes: 1024,
            protected_count: 0,
            examples: vec![],
        };

        // Log an allowed attempt
        log_attempt(&conn, "purge", "--all", true, &scope, "allowed", None).unwrap();

        // Log a denied attempt
        log_attempt(
            &conn,
            "purge",
            "--all",
            true,
            &scope,
            "denied",
            Some("wrong phrase"),
        )
        .unwrap();

        // Log a no_tty attempt
        log_attempt(
            &conn,
            "purge",
            "--all",
            false,
            &scope,
            "no_tty",
            Some("no terminal"),
        )
        .unwrap();

        assert_eq!(count_audit_entries(&conn, None).unwrap(), 3);
        assert_eq!(count_audit_entries(&conn, Some("allowed")).unwrap(), 1);
        assert_eq!(count_audit_entries(&conn, Some("denied")).unwrap(), 1);
        assert_eq!(count_audit_entries(&conn, Some("no_tty")).unwrap(), 1);
    }
}

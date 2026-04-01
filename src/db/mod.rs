pub mod schema;
pub mod operations;
pub mod queries;

use std::path::Path;
use rusqlite::Connection;
use crate::error::Result;

pub fn open_database(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA synchronous = NORMAL;
    ")?;
    schema::initialize(&conn)?;
    Ok(conn)
}

pub fn open_memory_database() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA synchronous = NORMAL;
    ")?;
    schema::initialize(&conn)?;
    Ok(conn)
}

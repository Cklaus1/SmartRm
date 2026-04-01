use std::process::ExitCode;

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::Result;
use crate::id;
use crate::models::ArchiveObject;
use crate::output;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the history command.
pub struct HistoryArgs {
    pub path: String,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryResult {
    pub path: String,
    pub versions: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub version: usize,
    pub archive_id: String,
    pub short_id: String,
    pub state: String,
    pub size_bytes: Option<i64>,
    pub size_human: String,
    pub created_at: String,
}

impl HumanOutput for HistoryResult {
    fn format_human(&self) -> String {
        if self.versions.is_empty() {
            return format!("No history found for {}\n", self.path);
        }

        let mut out = String::new();
        out.push_str(&format!("Version history for {}:\n\n", self.path));

        out.push_str(&format!(
            "  {:<4} {:<10} {:<10} {:<22} {:>10}\n",
            "#", "ID", "State", "Deleted", "Size"
        ));
        out.push_str(&format!("  {}\n", "-".repeat(58)));

        for entry in &self.versions {
            let date_display = format_date(&entry.created_at);
            out.push_str(&format!(
                "  {:<4} {:<10} {:<10} {:<22} {:>10}\n",
                entry.version, entry.short_id, entry.state, date_display, entry.size_human,
            ));
        }

        out.push_str(&format!("\n{} version(s)\n", self.versions.len()));
        out
    }
}

pub fn run(args: &HistoryArgs, conn: &Connection, _config: &SmartrmConfig) -> Result<ExitCode> {
    let objects = db::queries::get_history_for_path(conn, &args.path)?;

    let versions: Vec<HistoryEntry> = objects
        .iter()
        .enumerate()
        .map(|(i, obj)| to_history_entry(i + 1, obj))
        .collect();

    let display_path = if !objects.is_empty() {
        objects[0].original_path.clone()
    } else {
        args.path.clone()
    };

    let result = HistoryResult {
        path: display_path,
        versions,
    };

    output::print_output(&result, args.json);

    let exit_code = if result.versions.is_empty() { 2 } else { 0 };
    Ok(ExitCode::from(exit_code))
}

fn to_history_entry(version: usize, obj: &ArchiveObject) -> HistoryEntry {
    HistoryEntry {
        version,
        archive_id: obj.archive_id.clone(),
        short_id: id::short_id(&obj.archive_id).to_string(),
        state: obj.state.as_str().to_string(),
        size_bytes: obj.size_bytes,
        size_human: format_size(obj.size_bytes),
        created_at: obj.created_at.clone(),
    }
}

fn format_size(bytes: Option<i64>) -> String {
    match bytes {
        None => "-".to_string(),
        Some(b) if b < 0 => "-".to_string(),
        Some(b) => {
            let b = b as f64;
            if b < 1024.0 {
                format!("{} B", b as i64)
            } else if b < 1024.0 * 1024.0 {
                format!("{:.1} KB", b / 1024.0)
            } else if b < 1024.0 * 1024.0 * 1024.0 {
                format!("{:.1} MB", b / (1024.0 * 1024.0))
            } else {
                format!("{:.1} GB", b / (1024.0 * 1024.0 * 1024.0))
            }
        }
    }
}

fn format_date(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| {
            if iso.len() > 19 {
                iso[..19].to_string()
            } else {
                iso.to_string()
            }
        })
}

/// Determine whether a path looks like a bare filename (no directory separators).
pub fn is_bare_filename(path: &str) -> bool {
    !path.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_filename_detection() {
        assert!(is_bare_filename("test.txt"));
        assert!(!is_bare_filename("/path/to/test.txt"));
        assert!(!is_bare_filename("relative/test.txt"));
    }
}

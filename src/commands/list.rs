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

/// Parameters extracted from the CLI layer for the list command.
pub struct ListArgs {
    pub state: Option<String>,
    pub limit: u32,
    pub cursor: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListResult {
    pub objects: Vec<ListEntry>,
    pub total_count: i64,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListEntry {
    pub archive_id: String,
    pub short_id: String,
    pub original_path: String,
    pub object_type: String,
    pub state: String,
    pub size_bytes: Option<i64>,
    pub size_human: String,
    pub created_at: String,
}

impl HumanOutput for ListResult {
    fn format_human(&self) -> String {
        if self.objects.is_empty() {
            return "No archived objects found.\n".to_string();
        }

        let mut out = String::new();

        // Header
        out.push_str(&format!(
            "{:<10} {:<40} {:<20} {:>10} {:<10}\n",
            "ID", "Original Path", "Deleted", "Size", "State"
        ));
        out.push_str(&format!("{}\n", "-".repeat(92)));

        for entry in &self.objects {
            let path_display = truncate_path(&entry.original_path, 40);
            let date_display = format_date(&entry.created_at);

            out.push_str(&format!(
                "{:<10} {:<40} {:<20} {:>10} {:<10}\n",
                entry.short_id,
                path_display,
                date_display,
                entry.size_human,
                entry.state,
            ));
        }

        if self.has_more {
            if let Some(ref cursor) = self.next_cursor {
                out.push_str(&format!(
                    "\n({} of {} shown, use --cursor={} for next page)\n",
                    self.objects.len(),
                    self.total_count,
                    cursor,
                ));
            }
        } else {
            out.push_str(&format!("\n{} objects total\n", self.total_count));
        }

        out
    }
}

pub fn run(args: &ListArgs, conn: &Connection, _config: &SmartrmConfig) -> Result<ExitCode> {
    let state_filter = args.state.as_deref();

    let objects = db::queries::list_archive_objects(
        conn,
        state_filter,
        args.limit,
        args.cursor.as_deref(),
    )?;

    let total_count = db::queries::count_archive_objects(conn, state_filter)?;

    let has_more = objects.len() == args.limit as usize;
    let next_cursor = if has_more {
        objects.last().map(|o| o.created_at.clone())
    } else {
        None
    };

    let entries: Vec<ListEntry> = objects.iter().map(|o| to_list_entry(o)).collect();

    let result = ListResult {
        objects: entries,
        total_count,
        has_more,
        next_cursor,
    };

    output::print_output(&result, args.json);

    let exit_code = if result.objects.is_empty() { 2 } else { 0 };
    Ok(ExitCode::from(exit_code))
}

fn to_list_entry(obj: &ArchiveObject) -> ListEntry {
    ListEntry {
        archive_id: obj.archive_id.clone(),
        short_id: id::short_id(&obj.archive_id).to_string(),
        original_path: obj.original_path.clone(),
        object_type: obj.object_type.as_str().to_string(),
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

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        let prefix = "...";
        let available = max_len.saturating_sub(prefix.len());
        let start = path.len() - available;
        format!("{}{}", prefix, &path[start..])
    }
}

fn format_date(iso: &str) -> String {
    // Try to parse and format nicely; fall back to raw string
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(512)), "512 B");
        assert_eq!(format_size(Some(1024)), "1.0 KB");
        assert_eq!(format_size(Some(4300)), "4.2 KB");
        assert_eq!(format_size(Some(1_048_576)), "1.0 MB");
        assert_eq!(format_size(Some(1_073_741_824)), "1.0 GB");
        assert_eq!(format_size(None), "-");
    }

    #[test]
    fn truncate_path_short() {
        assert_eq!(truncate_path("/tmp/test.txt", 40), "/tmp/test.txt");
    }

    #[test]
    fn truncate_path_long() {
        let long_path = "/very/long/path/to/some/deeply/nested/file/in/the/filesystem/test.txt";
        let result = truncate_path(long_path, 40);
        assert!(result.len() <= 40);
        assert!(result.starts_with("..."));
    }

    #[test]
    fn format_date_rfc3339() {
        assert_eq!(
            format_date("2026-04-01T10:00:00+00:00"),
            "2026-04-01 10:00"
        );
    }

    #[test]
    fn format_date_fallback() {
        assert_eq!(format_date("not-a-date"), "not-a-date");
    }
}

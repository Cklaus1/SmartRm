use std::process::ExitCode;

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{Result, SmartrmError};
use crate::id;
use crate::models::ArchiveObject;
use crate::output;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the search command.
pub struct SearchArgs {
    pub pattern: String,
    pub after: Option<String>,
    pub larger_than: Option<String>,
    pub dir: Option<String>,
    pub limit: u32,
    pub offset: u32,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub objects: Vec<SearchEntry>,
    pub total_shown: usize,
    pub offset: u32,
    pub is_glob: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchEntry {
    pub archive_id: String,
    pub short_id: String,
    pub original_path: String,
    pub object_type: String,
    pub state: String,
    pub size_bytes: Option<i64>,
    pub size_human: String,
    pub created_at: String,
}

impl HumanOutput for SearchResult {
    fn format_human(&self) -> String {
        if self.objects.is_empty() {
            return "No matching objects found.\n".to_string();
        }

        let mut out = String::new();

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
                entry.short_id, path_display, date_display, entry.size_human, entry.state,
            ));
        }

        out.push_str(&format!("\n{} results shown", self.total_shown));
        if self.offset > 0 {
            out.push_str(&format!(" (offset {})", self.offset));
        }
        out.push('\n');

        out
    }
}

pub fn run(args: &SearchArgs, conn: &Connection, _config: &SmartrmConfig) -> Result<ExitCode> {
    let is_glob = is_glob_pattern(&args.pattern);

    let min_size = match &args.larger_than {
        Some(s) => Some(parse_size(s)?),
        None => None,
    };

    let objects = db::queries::search_archive_objects(
        conn,
        &args.pattern,
        is_glob,
        args.after.as_deref(),
        min_size,
        args.dir.as_deref(),
        args.offset,
        args.limit,
    )?;

    let entries: Vec<SearchEntry> = objects.iter().map(to_search_entry).collect();
    let total_shown = entries.len();

    let result = SearchResult {
        objects: entries,
        total_shown,
        offset: args.offset,
        is_glob,
    };

    output::print_output(&result, args.json);

    let exit_code = if result.objects.is_empty() { 2 } else { 0 };
    Ok(ExitCode::from(exit_code))
}

fn to_search_entry(obj: &ArchiveObject) -> SearchEntry {
    SearchEntry {
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

/// Detect whether the pattern is a glob (contains * or ?) or a plain substring.
pub fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Parse a human-readable size string into bytes.
///
/// Supports: "10M", "1G", "500K", "1024" (raw bytes).
pub fn parse_size(s: &str) -> Result<i64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(SmartrmError::Config("empty size string".to_string()));
    }

    let (num_part, multiplier) = if let Some(n) = s.strip_suffix('G').or_else(|| s.strip_suffix('g'))
    {
        (n, 1024_i64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        (n, 1024_i64 * 1024)
    } else if let Some(n) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        (n, 1024_i64)
    } else if let Some(n) = s.strip_suffix("GB").or_else(|| s.strip_suffix("gb")) {
        (n, 1024_i64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MB").or_else(|| s.strip_suffix("mb")) {
        (n, 1024_i64 * 1024)
    } else if let Some(n) = s.strip_suffix("KB").or_else(|| s.strip_suffix("kb")) {
        (n, 1024_i64)
    } else {
        (s, 1_i64)
    };

    let num: f64 = num_part
        .trim()
        .parse()
        .map_err(|_| SmartrmError::Config(format!("invalid size: {s}")))?;

    Ok((num * multiplier as f64) as i64)
}

/// Convert a glob pattern (e.g., "*.log") to a SQL LIKE pattern.
///
/// `*` -> `%`, `?` -> `_`. Escapes existing `%` and `_` in the input.
pub fn glob_to_sql_like(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len() + 4);
    for ch in pattern.chars() {
        match ch {
            '%' => result.push_str("\\%"),
            '_' => result.push_str("\\_"),
            '*' => result.push('%'),
            '?' => result.push('_'),
            _ => result.push(ch),
        }
    }
    result
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
    fn is_glob_with_asterisk() {
        assert!(is_glob_pattern("*.log"));
        assert!(is_glob_pattern("test*"));
    }

    #[test]
    fn is_glob_with_question_mark() {
        assert!(is_glob_pattern("file?.txt"));
    }

    #[test]
    fn is_not_glob_for_plain_string() {
        assert!(!is_glob_pattern("config"));
        assert!(!is_glob_pattern("my-file.txt"));
    }

    #[test]
    fn parse_size_bytes() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("0").unwrap(), 0);
    }

    #[test]
    fn parse_size_kilobytes() {
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("500k").unwrap(), 500 * 1024);
        assert_eq!(parse_size("2KB").unwrap(), 2 * 1024);
    }

    #[test]
    fn parse_size_megabytes() {
        assert_eq!(parse_size("10M").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_size("1m").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("5MB").unwrap(), 5 * 1024 * 1024);
    }

    #[test]
    fn parse_size_gigabytes() {
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("2g").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_invalid() {
        assert!(parse_size("abc").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn glob_to_sql_like_asterisk() {
        assert_eq!(glob_to_sql_like("*.log"), "%.log");
    }

    #[test]
    fn glob_to_sql_like_question_mark() {
        assert_eq!(glob_to_sql_like("file?.txt"), "file_.txt");
    }

    #[test]
    fn glob_to_sql_like_escapes_percent() {
        assert_eq!(glob_to_sql_like("100%"), "100\\%");
    }

    #[test]
    fn glob_to_sql_like_escapes_underscore() {
        assert_eq!(glob_to_sql_like("my_file"), "my\\_file");
    }
}

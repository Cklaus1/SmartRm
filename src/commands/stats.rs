use std::process::ExitCode;

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::Result;
use crate::output;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the stats command.
pub struct StatsArgs {
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsResult {
    pub total_objects: i64,
    pub count_by_state: Vec<(String, i64)>,
    pub total_size_bytes: i64,
    pub total_size_human: String,
    pub database_size_bytes: Option<u64>,
    pub database_size_human: String,
    pub top_directories: Vec<(String, i64)>,
}

impl HumanOutput for StatsResult {
    fn format_human(&self) -> String {
        let mut out = String::new();

        out.push_str("SmartRM Archive Statistics\n\n");

        out.push_str(&format!("Total objects:    {}\n", self.total_objects));
        for (state, count) in &self.count_by_state {
            out.push_str(&format!("  {:<14} {}\n", format!("{}:", capitalize(state)), count));
        }

        out.push_str(&format!(
            "\nTotal archive size: {}\n",
            self.total_size_human
        ));
        out.push_str(&format!("Database size:      {}\n", self.database_size_human));

        if !self.top_directories.is_empty() {
            out.push_str("\nTop deleted directories:\n");
            for (dir, count) in &self.top_directories {
                let label = if count == &1 { "object" } else { "objects" };
                out.push_str(&format!("  {:<40} {} {}\n", dir, count, label));
            }
        }

        out
    }
}

pub fn run(args: &StatsArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    let archive_stats = db::queries::get_stats(conn)?;

    let total_objects: i64 = archive_stats.count_by_state.iter().map(|(_, c)| c).sum();

    let db_file = crate::policy::config::db_path(config);
    let database_size_bytes = std::fs::metadata(&db_file).ok().map(|m| m.len());

    let result = StatsResult {
        total_objects,
        count_by_state: archive_stats.count_by_state,
        total_size_bytes: archive_stats.total_size_bytes,
        total_size_human: format_size(archive_stats.total_size_bytes),
        database_size_bytes,
        database_size_human: match database_size_bytes {
            Some(b) => format_size(b as i64),
            None => "-".to_string(),
        },
        top_directories: archive_stats.top_directories,
    };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

fn format_size(bytes: i64) -> String {
    if bytes < 0 {
        return "-".to_string();
    }
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{} B", bytes)
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

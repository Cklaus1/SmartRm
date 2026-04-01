use std::process::ExitCode;

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::Result;
use crate::id;
use crate::models::Batch;
use crate::output;
use crate::output::HumanOutput;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the timeline command.
pub struct TimelineArgs {
    pub today: bool,
    pub dir: Option<String>,
    pub limit: u32,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineResult {
    pub batches: Vec<TimelineEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub batch_id: String,
    pub short_id: String,
    pub operation_type: String,
    pub status: String,
    pub total_objects: i64,
    pub total_bytes: i64,
    pub size_human: String,
    pub started_at: String,
}

impl HumanOutput for TimelineResult {
    fn format_human(&self) -> String {
        if self.batches.is_empty() {
            return "No batches found.\n".to_string();
        }

        let mut out = String::new();

        out.push_str(&format!(
            "{:<10} {:<10} {:<10} {:>6} {:>10} {:<20}\n",
            "Batch", "Type", "Status", "Files", "Size", "Started"
        ));
        out.push_str(&format!("{}\n", "-".repeat(68)));

        for entry in &self.batches {
            let date_display = format_date(&entry.started_at);
            out.push_str(&format!(
                "{:<10} {:<10} {:<10} {:>6} {:>10} {:<20}\n",
                entry.short_id,
                entry.operation_type,
                entry.status,
                entry.total_objects,
                entry.size_human,
                date_display,
            ));
        }

        out.push_str(&format!("\n{} batch(es)\n", self.batches.len()));
        out
    }
}

pub fn run(args: &TimelineArgs, conn: &Connection, _config: &SmartrmConfig) -> Result<ExitCode> {
    let batches =
        db::queries::get_timeline_batches(conn, args.today, args.dir.as_deref(), args.limit)?;

    let entries: Vec<TimelineEntry> = batches.iter().map(to_timeline_entry).collect();

    let result = TimelineResult { batches: entries };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

fn to_timeline_entry(batch: &Batch) -> TimelineEntry {
    TimelineEntry {
        batch_id: batch.batch_id.clone(),
        short_id: id::short_id(&batch.batch_id).to_string(),
        operation_type: batch.operation_type.as_str().to_string(),
        status: batch.status.as_str().to_string(),
        total_objects: batch.total_objects_succeeded,
        total_bytes: batch.total_bytes,
        size_human: format_size(batch.total_bytes),
        started_at: batch.started_at.clone(),
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

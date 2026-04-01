use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    pub batch_id: String,
    pub operation_type: OperationType,
    pub status: BatchStatus,
    pub requested_by: Option<String>,
    pub cwd: Option<String>,
    pub hostname: Option<String>,
    pub command_line: Option<String>,
    pub total_objects_requested: i64,
    pub total_objects_processed: i64,
    pub total_objects_succeeded: i64,
    pub total_objects_failed: i64,
    pub total_bytes: i64,
    pub interactive_mode: bool,
    pub used_force: bool,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub summary_message: Option<String>,
}

// ---------------------------------------------------------------------------
// OperationType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Delete,
    Restore,
    Cleanup,
    Purge,
}

impl OperationType {
    pub fn as_str(self) -> &'static str {
        match self {
            OperationType::Delete => "delete",
            OperationType::Restore => "restore",
            OperationType::Cleanup => "cleanup",
            OperationType::Purge => "purge",
        }
    }
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OperationType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "delete" => Ok(OperationType::Delete),
            "restore" => Ok(OperationType::Restore),
            "cleanup" => Ok(OperationType::Cleanup),
            "purge" => Ok(OperationType::Purge),
            other => Err(format!("unknown operation type: {other}")),
        }
    }
}

impl TryFrom<&str> for OperationType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// BatchStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Pending,
    InProgress,
    Complete,
    Partial,
    Failed,
    RolledBack,
}

impl BatchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BatchStatus::Pending => "pending",
            BatchStatus::InProgress => "in_progress",
            BatchStatus::Complete => "complete",
            BatchStatus::Partial => "partial",
            BatchStatus::Failed => "failed",
            BatchStatus::RolledBack => "rolled_back",
        }
    }
}

impl fmt::Display for BatchStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BatchStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(BatchStatus::Pending),
            "in_progress" => Ok(BatchStatus::InProgress),
            "complete" => Ok(BatchStatus::Complete),
            "partial" => Ok(BatchStatus::Partial),
            "failed" => Ok(BatchStatus::Failed),
            "rolled_back" => Ok(BatchStatus::RolledBack),
            other => Err(format!("unknown batch status: {other}")),
        }
    }
}

impl TryFrom<&str> for BatchStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

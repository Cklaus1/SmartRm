use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct BatchItem {
    pub batch_item_id: String,
    pub batch_id: String,
    pub input_path: String,
    pub resolved_path: Option<String>,
    pub archive_id: Option<String>,
    pub status: BatchItemStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// BatchItemStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchItemStatus {
    Pending,
    Succeeded,
    Failed,
    Skipped,
}

impl BatchItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            BatchItemStatus::Pending => "pending",
            BatchItemStatus::Succeeded => "succeeded",
            BatchItemStatus::Failed => "failed",
            BatchItemStatus::Skipped => "skipped",
        }
    }
}

impl fmt::Display for BatchItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BatchItemStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(BatchItemStatus::Pending),
            "succeeded" => Ok(BatchItemStatus::Succeeded),
            "failed" => Ok(BatchItemStatus::Failed),
            "skipped" => Ok(BatchItemStatus::Skipped),
            unknown => Err(format!("unknown batch item status: {unknown}")),
        }
    }
}

impl TryFrom<&str> for BatchItemStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

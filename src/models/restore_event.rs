use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct RestoreEvent {
    pub restore_event_id: String,
    pub archive_id: String,
    pub restore_batch_id: String,
    pub restore_mode: RestoreMode,
    pub requested_target_path: Option<String>,
    pub final_restored_path: Option<String>,
    pub status: RestoreEventStatus,
    pub conflict_policy: ConflictPolicy,
    pub mode_restored: bool,
    pub ownership_restored: bool,
    pub timestamps_restored: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// RestoreMode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreMode {
    Original,
    AlternatePath,
    Overwrite,
    RenameOnConflict,
}

impl RestoreMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RestoreMode::Original => "original",
            RestoreMode::AlternatePath => "alternate_path",
            RestoreMode::Overwrite => "overwrite",
            RestoreMode::RenameOnConflict => "rename_on_conflict",
        }
    }
}

impl fmt::Display for RestoreMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RestoreMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "original" => Ok(RestoreMode::Original),
            "alternate_path" => Ok(RestoreMode::AlternatePath),
            "overwrite" => Ok(RestoreMode::Overwrite),
            "rename_on_conflict" => Ok(RestoreMode::RenameOnConflict),
            unknown => Err(format!("unknown restore mode: {unknown}")),
        }
    }
}

impl TryFrom<&str> for RestoreMode {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// RestoreEventStatus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreEventStatus {
    Succeeded,
    Failed,
    Partial,
}

impl RestoreEventStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RestoreEventStatus::Succeeded => "succeeded",
            RestoreEventStatus::Failed => "failed",
            RestoreEventStatus::Partial => "partial",
        }
    }
}

impl fmt::Display for RestoreEventStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RestoreEventStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "succeeded" => Ok(RestoreEventStatus::Succeeded),
            "failed" => Ok(RestoreEventStatus::Failed),
            "partial" => Ok(RestoreEventStatus::Partial),
            unknown => Err(format!("unknown restore event status: {unknown}")),
        }
    }
}

impl TryFrom<&str> for RestoreEventStatus {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// ConflictPolicy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    Fail,
    Rename,
    Overwrite,
    Skip,
}

impl ConflictPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictPolicy::Fail => "fail",
            ConflictPolicy::Rename => "rename",
            ConflictPolicy::Overwrite => "overwrite",
            ConflictPolicy::Skip => "skip",
        }
    }
}

impl fmt::Display for ConflictPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ConflictPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fail" => Ok(ConflictPolicy::Fail),
            "rename" => Ok(ConflictPolicy::Rename),
            "overwrite" => Ok(ConflictPolicy::Overwrite),
            "skip" => Ok(ConflictPolicy::Skip),
            unknown => Err(format!("unknown conflict policy: {unknown}")),
        }
    }
}

impl TryFrom<&str> for ConflictPolicy {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

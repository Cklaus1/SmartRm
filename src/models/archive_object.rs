use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveObject {
    pub archive_id: String,
    pub batch_id: String,
    pub parent_archive_id: Option<String>,
    pub object_type: ObjectType,
    pub state: LifecycleState,
    pub original_path: String,
    pub archived_path: Option<String>,
    pub storage_mount_id: Option<String>,
    pub original_mount_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub content_hash: Option<String>,
    pub link_target: Option<String>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub mtime_ns: Option<i64>,
    pub ctime_ns: Option<i64>,
    pub delete_intent: Option<String>,
    pub ttl_seconds: Option<i64>,
    pub policy_id: Option<String>,
    pub delete_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub restored_at: Option<String>,
    pub expired_at: Option<String>,
    pub purged_at: Option<String>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
}

// ---------------------------------------------------------------------------
// ObjectType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    File,
    Dir,
    Symlink,
    Other,
}

impl ObjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectType::File => "file",
            ObjectType::Dir => "dir",
            ObjectType::Symlink => "symlink",
            ObjectType::Other => "other",
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ObjectType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file" => Ok(ObjectType::File),
            "dir" => Ok(ObjectType::Dir),
            "symlink" => Ok(ObjectType::Symlink),
            "other" => Ok(ObjectType::Other),
            unknown => Err(format!("unknown object type: {unknown}")),
        }
    }
}

impl TryFrom<&str> for ObjectType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// LifecycleState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Archived,
    Restored,
    Expired,
    Purged,
    Failed,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleState::Archived => "archived",
            LifecycleState::Restored => "restored",
            LifecycleState::Expired => "expired",
            LifecycleState::Purged => "purged",
            LifecycleState::Failed => "failed",
        }
    }
}

impl fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LifecycleState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "archived" => Ok(LifecycleState::Archived),
            "restored" => Ok(LifecycleState::Restored),
            "expired" => Ok(LifecycleState::Expired),
            "purged" => Ok(LifecycleState::Purged),
            "failed" => Ok(LifecycleState::Failed),
            unknown => Err(format!("unknown lifecycle state: {unknown}")),
        }
    }
}

impl TryFrom<&str> for LifecycleState {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

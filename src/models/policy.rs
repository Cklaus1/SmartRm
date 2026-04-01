use serde::Serialize;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize)]
pub struct EffectivePolicy {
    pub effective_policy_id: String,
    pub batch_id: Option<String>,
    pub archive_id: Option<String>,
    pub setting_key: String,
    pub setting_value: Option<String>,
    pub source_type: SourceType,
    pub source_ref: Option<String>,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// SourceType
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Cli,
    Interactive,
    UserRule,
    ProjectRule,
    SystemRule,
    Learned,
    Default,
    HardSafety,
}

impl SourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceType::Cli => "cli",
            SourceType::Interactive => "interactive",
            SourceType::UserRule => "user_rule",
            SourceType::ProjectRule => "project_rule",
            SourceType::SystemRule => "system_rule",
            SourceType::Learned => "learned",
            SourceType::Default => "default",
            SourceType::HardSafety => "hard_safety",
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SourceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cli" => Ok(SourceType::Cli),
            "interactive" => Ok(SourceType::Interactive),
            "user_rule" => Ok(SourceType::UserRule),
            "project_rule" => Ok(SourceType::ProjectRule),
            "system_rule" => Ok(SourceType::SystemRule),
            "learned" => Ok(SourceType::Learned),
            "default" => Ok(SourceType::Default),
            "hard_safety" => Ok(SourceType::HardSafety),
            unknown => Err(format!("unknown source type: {unknown}")),
        }
    }
}

impl TryFrom<&str> for SourceType {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub tags: Vec<Tag>,
    pub danger_level: DangerLevel,
}

// ---------------------------------------------------------------------------
// Tag
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tag {
    Build,
    Temp,
    Content,
    Protected,
}

impl Tag {
    pub fn as_str(self) -> &'static str {
        match self {
            Tag::Build => "build",
            Tag::Temp => "temp",
            Tag::Content => "content",
            Tag::Protected => "protected",
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Tag {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "build" => Ok(Tag::Build),
            "temp" => Ok(Tag::Temp),
            "content" => Ok(Tag::Content),
            "protected" => Ok(Tag::Protected),
            unknown => Err(format!("unknown tag: {unknown}")),
        }
    }
}

impl TryFrom<&str> for Tag {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// ---------------------------------------------------------------------------
// DangerLevel
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DangerLevel {
    Safe,
    Warning(String),
    Blocked(String),
}

impl DangerLevel {
    /// Returns the variant name as a snake_case string (without the payload).
    pub fn as_str(&self) -> &'static str {
        match self {
            DangerLevel::Safe => "safe",
            DangerLevel::Warning(_) => "warning",
            DangerLevel::Blocked(_) => "blocked",
        }
    }
}

impl fmt::Display for DangerLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DangerLevel::Safe => f.write_str("safe"),
            DangerLevel::Warning(msg) => write!(f, "warning: {msg}"),
            DangerLevel::Blocked(msg) => write!(f, "blocked: {msg}"),
        }
    }
}

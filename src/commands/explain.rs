use std::path::Path;
use std::process::ExitCode;

use rusqlite::Connection;
use serde::Serialize;

use crate::db;
use crate::error::{Result, SmartrmError};
use crate::id;
use crate::models::EffectivePolicy;
use crate::output;
use crate::output::HumanOutput;
use crate::policy::classifier;
use crate::policy::config::SmartrmConfig;
use crate::policy::resolver::{self, DeleteFlags};

// ---------------------------------------------------------------------------
// explain <archive_id>
// ---------------------------------------------------------------------------

pub struct ExplainArgs {
    pub archive_id: String,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainResult {
    pub archive_id: String,
    pub short_id: String,
    pub original_path: String,
    pub policies: Vec<PolicyEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEntry {
    pub setting_key: String,
    pub setting_value: Option<String>,
    pub source_type: String,
    pub source_ref: Option<String>,
}

impl HumanOutput for ExplainResult {
    fn format_human(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!(
            "Archive {} ({})\n",
            self.short_id, self.original_path
        ));

        if self.policies.is_empty() {
            out.push_str("  No effective policies recorded.\n");
        } else {
            for p in &self.policies {
                let value = p.setting_value.as_deref().unwrap_or("-");
                let source_ref = match &p.source_ref {
                    Some(r) => format!(", ref: {}", r),
                    None => String::new(),
                };
                out.push_str(&format!(
                    "  {}: {} (source: {}{})\n",
                    p.setting_key, value, p.source_type, source_ref,
                ));
            }
        }

        out
    }
}

pub fn run(args: &ExplainArgs, conn: &Connection, _config: &SmartrmConfig) -> Result<ExitCode> {
    // Resolve the archive object (support prefix matching)
    let matches = db::queries::get_archive_object_by_prefix(conn, &args.archive_id)?;

    let obj = match matches.len() {
        0 => {
            return Err(SmartrmError::NotFound(format!(
                "no archive object matching '{}'",
                args.archive_id
            )));
        }
        1 => &matches[0],
        _ => {
            return Err(SmartrmError::Config(format!(
                "ambiguous ID '{}': {} matches",
                args.archive_id,
                matches.len()
            )));
        }
    };

    let policies = db::queries::get_effective_policies_for_batch(conn, &obj.batch_id)?;

    let entries: Vec<PolicyEntry> = policies.iter().map(to_policy_entry).collect();

    let result = ExplainResult {
        archive_id: obj.archive_id.clone(),
        short_id: id::short_id(&obj.archive_id).to_string(),
        original_path: obj.original_path.clone(),
        policies: entries,
    };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

fn to_policy_entry(ep: &EffectivePolicy) -> PolicyEntry {
    PolicyEntry {
        setting_key: ep.setting_key.clone(),
        setting_value: ep.setting_value.clone(),
        source_type: ep.source_type.as_str().to_string(),
        source_ref: ep.source_ref.clone(),
    }
}

// ---------------------------------------------------------------------------
// explain-policy <path>
// ---------------------------------------------------------------------------

pub struct ExplainPolicyArgs {
    pub path: String,
    pub json: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExplainPolicyResult {
    pub path: String,
    pub classification: ClassificationInfo,
    pub resolved_policy: ResolvedPolicyInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClassificationInfo {
    pub tags: Vec<String>,
    pub danger_level: String,
    pub danger_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPolicyInfo {
    pub delete_mode: String,
    pub delete_intent: Option<String>,
    pub ttl_seconds: Option<i64>,
    pub sources: Vec<PolicyEntry>,
}

impl HumanOutput for ExplainPolicyResult {
    fn format_human(&self) -> String {
        let mut out = String::new();

        out.push_str(&format!("Policy for: {}\n\n", self.path));

        // Classification
        out.push_str("Classification:\n");
        if self.classification.tags.is_empty() {
            out.push_str("  tags: (none)\n");
        } else {
            out.push_str(&format!("  tags: {}\n", self.classification.tags.join(", ")));
        }
        out.push_str(&format!(
            "  danger_level: {}\n",
            self.classification.danger_level
        ));
        if let Some(ref msg) = self.classification.danger_message {
            out.push_str(&format!("  danger_message: {}\n", msg));
        }

        // Policy
        out.push_str("\nEffective policy:\n");
        for source in &self.resolved_policy.sources {
            let value = source.setting_value.as_deref().unwrap_or("-");
            let source_ref = match &source.source_ref {
                Some(r) => format!(", ref: {}", r),
                None => String::new(),
            };
            out.push_str(&format!(
                "  {}: {} (source: {}{})\n",
                source.setting_key, value, source.source_type, source_ref,
            ));
        }

        if let Some(ttl) = self.resolved_policy.ttl_seconds {
            out.push_str(&format!("  ttl_seconds: {}\n", ttl));
        }

        out
    }
}

pub fn run_explain_policy(
    args: &ExplainPolicyArgs,
    _conn: &Connection,
    config: &SmartrmConfig,
) -> Result<ExitCode> {
    let path = Path::new(&args.path);
    let classification = classifier::classify(path);

    let flags = DeleteFlags {
        permanent: false,
        force: false,
    };
    let resolved = resolver::resolve_delete_policy(config, &flags, &classification);

    let (danger_level_str, danger_message) = match &classification.danger_level {
        crate::models::DangerLevel::Safe => ("safe".to_string(), None),
        crate::models::DangerLevel::Warning(msg) => ("warning".to_string(), Some(msg.clone())),
        crate::models::DangerLevel::Blocked(msg) => ("blocked".to_string(), Some(msg.clone())),
    };

    let sources: Vec<PolicyEntry> = resolved
        .source_info
        .iter()
        .map(|s| PolicyEntry {
            setting_key: s.setting_key.clone(),
            setting_value: Some(s.setting_value.clone()),
            source_type: s.source_type.as_str().to_string(),
            source_ref: s.source_ref.clone(),
        })
        .collect();

    let result = ExplainPolicyResult {
        path: args.path.clone(),
        classification: ClassificationInfo {
            tags: classification.tags.iter().map(|t| t.as_str().to_string()).collect(),
            danger_level: danger_level_str,
            danger_message,
        },
        resolved_policy: ResolvedPolicyInfo {
            delete_mode: resolved.delete_mode,
            delete_intent: resolved.delete_intent,
            ttl_seconds: resolved.ttl_seconds,
            sources,
        },
    };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

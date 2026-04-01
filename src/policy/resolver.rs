use crate::models::policy::{Classification, SourceType, Tag};
use crate::policy::config::SmartrmConfig;

#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    pub delete_mode: String, // "archive" or "permanent"
    pub ttl_seconds: Option<i64>,
    pub min_free_space_bytes: u64,
    pub delete_intent: Option<String>,
    pub source_info: Vec<PolicySource>,
}

#[derive(Debug, Clone)]
pub struct PolicySource {
    pub setting_key: String,
    pub setting_value: String,
    pub source_type: SourceType,
    pub source_ref: Option<String>,
}

pub struct DeleteFlags {
    pub permanent: bool,
    pub force: bool,
}

pub fn resolve_delete_policy(
    config: &SmartrmConfig,
    flags: &DeleteFlags,
    classification: &Classification,
) -> ResolvedPolicy {
    let mut sources = Vec::new();

    // Delete mode
    let delete_mode = if flags.permanent {
        sources.push(PolicySource {
            setting_key: "delete_mode".to_string(),
            setting_value: "permanent".to_string(),
            source_type: SourceType::Cli,
            source_ref: Some("--permanent".to_string()),
        });
        "permanent".to_string()
    } else {
        sources.push(PolicySource {
            setting_key: "delete_mode".to_string(),
            setting_value: config.default_delete_mode.clone(),
            source_type: SourceType::Default,
            source_ref: None,
        });
        config.default_delete_mode.clone()
    };

    // Infer intent from classification
    let delete_intent = if classification.tags.contains(&Tag::Build) {
        Some("cleanup".to_string())
    } else if classification.tags.contains(&Tag::Temp) {
        Some("temp".to_string())
    } else {
        None
    };

    if let Some(ref intent) = delete_intent {
        sources.push(PolicySource {
            setting_key: "delete_intent".to_string(),
            setting_value: intent.clone(),
            source_type: SourceType::Default,
            source_ref: Some("classifier".to_string()),
        });
    }

    ResolvedPolicy {
        delete_mode,
        ttl_seconds: config.default_ttl_seconds,
        min_free_space_bytes: config.min_free_space_bytes,
        delete_intent,
        source_info: sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::policy::{Classification, DangerLevel, Tag};

    fn default_config() -> SmartrmConfig {
        SmartrmConfig::default()
    }

    fn safe_classification(tags: Vec<Tag>) -> Classification {
        Classification {
            tags,
            danger_level: DangerLevel::Safe,
        }
    }

    #[test]
    fn default_policy_uses_archive_mode() {
        let config = default_config();
        let flags = DeleteFlags {
            permanent: false,
            force: false,
        };
        let classification = safe_classification(vec![]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.delete_mode, "archive");
    }

    #[test]
    fn permanent_flag_overrides_config() {
        let config = default_config();
        let flags = DeleteFlags {
            permanent: true,
            force: false,
        };
        let classification = safe_classification(vec![]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.delete_mode, "permanent");
        assert!(policy
            .source_info
            .iter()
            .any(|s| s.source_type == SourceType::Cli));
    }

    #[test]
    fn build_tag_sets_cleanup_intent() {
        let config = default_config();
        let flags = DeleteFlags {
            permanent: false,
            force: false,
        };
        let classification = safe_classification(vec![Tag::Build]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.delete_intent, Some("cleanup".to_string()));
    }

    #[test]
    fn temp_tag_sets_temp_intent() {
        let config = default_config();
        let flags = DeleteFlags {
            permanent: false,
            force: false,
        };
        let classification = safe_classification(vec![Tag::Temp]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.delete_intent, Some("temp".to_string()));
    }

    #[test]
    fn content_tag_has_no_intent() {
        let config = default_config();
        let flags = DeleteFlags {
            permanent: false,
            force: false,
        };
        let classification = safe_classification(vec![Tag::Content]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.delete_intent, None);
    }

    #[test]
    fn config_ttl_is_propagated() {
        let mut config = default_config();
        config.default_ttl_seconds = Some(86400);
        let flags = DeleteFlags {
            permanent: false,
            force: false,
        };
        let classification = safe_classification(vec![]);
        let policy = resolve_delete_policy(&config, &flags, &classification);
        assert_eq!(policy.ttl_seconds, Some(86400));
    }
}

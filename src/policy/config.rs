use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartrmConfig {
    #[serde(default = "default_delete_mode")]
    pub default_delete_mode: String,
    #[serde(default = "default_min_free_space")]
    pub min_free_space_bytes: u64,
    #[serde(default = "default_conflict_mode")]
    pub default_restore_conflict_mode: String,
    #[serde(default)]
    pub default_ttl_seconds: Option<i64>,
    #[serde(default = "default_protected_patterns")]
    pub protected_patterns: Vec<String>,
    #[serde(default)]
    pub excluded_patterns: Vec<String>,
    #[serde(default)]
    pub archive_root: Option<String>,
    #[serde(default = "default_true")]
    pub danger_protection: bool,
    #[serde(default)]
    pub auto_cleanup: bool,
    #[serde(default = "default_gate_method")]
    pub destructive_gate_method: String,
    #[serde(default = "default_allow_destructive")]
    pub allow_destructive_commands: String,
    #[serde(default = "default_true")]
    pub agent_detection: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase_hash: Option<String>,
}

fn default_delete_mode() -> String {
    "archive".to_string()
}
fn default_min_free_space() -> u64 {
    1_073_741_824 // 1GB
}
fn default_conflict_mode() -> String {
    "rename".to_string()
}
fn default_protected_patterns() -> Vec<String> {
    vec![".env".to_string(), ".env.*".to_string()]
}
fn default_true() -> bool {
    true
}
fn default_gate_method() -> String {
    "confirmation_phrase".to_string()
}
fn default_allow_destructive() -> String {
    "interactive_with_confirmation".to_string()
}

impl Default for SmartrmConfig {
    fn default() -> Self {
        // Use serde default deserialization from empty JSON
        serde_json::from_str("{}").unwrap()
    }
}

/// Resolve the data directory (where archive + DB live)
pub fn resolve_data_dir(config: &SmartrmConfig) -> PathBuf {
    if let Some(home) = std::env::var_os("SMARTRM_HOME") {
        return PathBuf::from(home);
    }
    if let Some(ref root) = config.archive_root {
        return PathBuf::from(root);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("smartrm")
}

/// Resolve the config file path
pub fn resolve_config_path() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("SMARTRM_HOME") {
        let p = PathBuf::from(home).join("config.json");
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(config_dir) = dirs::config_dir() {
        let p = config_dir.join("smartrm").join("config.json");
        if p.exists() {
            return Some(p);
        }
    }
    let system = Path::new("/etc/smartrm/config.json");
    if system.exists() {
        return Some(system.to_path_buf());
    }
    None
}

/// Load configuration with layered resolution
pub fn load_config() -> SmartrmConfig {
    match resolve_config_path() {
        Some(path) => match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => SmartrmConfig::default(),
        },
        None => SmartrmConfig::default(),
    }
}

/// Get the user config file path (creating parent dir if needed)
pub fn user_config_path() -> PathBuf {
    if let Some(home) = std::env::var_os("SMARTRM_HOME") {
        return PathBuf::from(home).join("config.json");
    }
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("smartrm");
    config_dir.join("config.json")
}

/// Save config to user config file
pub fn save_config(config: &SmartrmConfig) -> crate::error::Result<()> {
    let path = user_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::error::SmartrmError::Io)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| crate::error::SmartrmError::Config(e.to_string()))?;
    std::fs::write(&path, json).map_err(crate::error::SmartrmError::Io)?;
    Ok(())
}

/// Get the archive directory path
pub fn archive_dir(config: &SmartrmConfig) -> PathBuf {
    resolve_data_dir(config).join("archive")
}

/// Get the database file path
pub fn db_path(config: &SmartrmConfig) -> PathBuf {
    resolve_data_dir(config).join("db.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_defaults() {
        let config = SmartrmConfig::default();
        assert_eq!(config.default_delete_mode, "archive");
        assert_eq!(config.min_free_space_bytes, 1_073_741_824);
        assert_eq!(config.default_restore_conflict_mode, "rename");
        assert!(config.danger_protection);
        assert!(!config.auto_cleanup);
        assert!(config.agent_detection);
        assert_eq!(config.protected_patterns.len(), 2);
    }

    #[test]
    fn config_deserializes_from_partial_json() {
        let json = r#"{"default_delete_mode": "permanent"}"#;
        let config: SmartrmConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_delete_mode, "permanent");
        // Other fields should have defaults
        assert_eq!(config.min_free_space_bytes, 1_073_741_824);
        assert!(config.danger_protection);
    }

    #[test]
    fn db_path_uses_data_dir() {
        let config = SmartrmConfig::default();
        let path = db_path(&config);
        assert!(path.to_string_lossy().ends_with("db.sqlite3"));
    }

    #[test]
    fn archive_dir_uses_data_dir() {
        let config = SmartrmConfig::default();
        let path = archive_dir(&config);
        assert!(path.to_string_lossy().ends_with("archive"));
    }

    #[test]
    fn resolve_data_dir_respects_env() {
        std::env::set_var("SMARTRM_HOME", "/tmp/smartrm-test-env");
        let config = SmartrmConfig::default();
        let dir = resolve_data_dir(&config);
        assert_eq!(dir, PathBuf::from("/tmp/smartrm-test-env"));
        std::env::remove_var("SMARTRM_HOME");
    }

    #[test]
    fn resolve_data_dir_respects_archive_root() {
        // Ensure SMARTRM_HOME is not set for this test
        std::env::remove_var("SMARTRM_HOME");
        let mut config = SmartrmConfig::default();
        config.archive_root = Some("/custom/archive".to_string());
        let dir = resolve_data_dir(&config);
        assert_eq!(dir, PathBuf::from("/custom/archive"));
    }
}

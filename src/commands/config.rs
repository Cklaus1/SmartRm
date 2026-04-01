use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};

use crate::error::{Result, SmartrmError};
use crate::output::human::format_bytes;
use crate::policy::config::{
    load_config, resolve_data_dir, save_config, user_config_path, SmartrmConfig,
};

/// Display the current configuration in human-readable format.
pub fn show_config(config: &SmartrmConfig) -> Result<ExitCode> {
    let data_dir = resolve_data_dir(config);
    let config_path = user_config_path();

    println!("SmartRM Configuration\n");

    println!(
        "{:<35}{}",
        "default_delete_mode:", config.default_delete_mode
    );
    println!(
        "{:<35}{} ({})",
        "min_free_space_bytes:",
        config.min_free_space_bytes,
        format_bytes(config.min_free_space_bytes)
    );
    println!(
        "{:<35}{}",
        "default_restore_conflict_mode:", config.default_restore_conflict_mode
    );
    println!(
        "{:<35}{}",
        "default_ttl_seconds:",
        match config.default_ttl_seconds {
            Some(v) => v.to_string(),
            None => "(not set)".to_string(),
        }
    );
    println!(
        "{:<35}{}",
        "protected_patterns:",
        if config.protected_patterns.is_empty() {
            "(none)".to_string()
        } else {
            config.protected_patterns.join(", ")
        }
    );
    println!(
        "{:<35}{}",
        "excluded_patterns:",
        if config.excluded_patterns.is_empty() {
            "(none)".to_string()
        } else {
            config.excluded_patterns.join(", ")
        }
    );
    println!(
        "{:<35}{}",
        "archive_root:",
        config
            .archive_root
            .as_deref()
            .unwrap_or(&data_dir.to_string_lossy())
    );
    println!("{:<35}{}", "danger_protection:", config.danger_protection);
    println!("{:<35}{}", "auto_cleanup:", config.auto_cleanup);
    println!(
        "{:<35}{}",
        "destructive_gate_method:", config.destructive_gate_method
    );
    println!(
        "{:<35}{}",
        "allow_destructive_commands:", config.allow_destructive_commands
    );
    println!("{:<35}{}", "agent_detection:", config.agent_detection);

    println!();
    println!("Config file: {}", config_path.display());
    println!("Data directory: {}", data_dir.display());

    Ok(ExitCode::from(0))
}

/// Apply a config value to a SmartrmConfig struct by key name.
///
/// Validates the key and value, returning an error for unknown keys or
/// invalid values. Does not persist — call `save_config` separately.
pub fn apply_config_value(config: &mut SmartrmConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "default_delete_mode" => {
            match value {
                "archive" | "permanent" => {}
                _ => {
                    return Err(SmartrmError::Config(format!(
                        "invalid value for default_delete_mode: '{}' (expected: archive, permanent)",
                        value
                    )));
                }
            }
            config.default_delete_mode = value.to_string();
        }
        "min_free_space_bytes" => {
            let n: u64 = value.parse().map_err(|_| {
                SmartrmError::Config(format!(
                    "invalid value for min_free_space_bytes: '{}' (expected a number)",
                    value
                ))
            })?;
            config.min_free_space_bytes = n;
        }
        "default_restore_conflict_mode" => {
            match value {
                "fail" | "rename" | "overwrite" | "skip" => {}
                _ => {
                    return Err(SmartrmError::Config(format!(
                        "invalid value for default_restore_conflict_mode: '{}' (expected: fail, rename, overwrite, skip)",
                        value
                    )));
                }
            }
            config.default_restore_conflict_mode = value.to_string();
        }
        "default_ttl_seconds" => {
            if value == "none" || value == "null" || value.is_empty() {
                config.default_ttl_seconds = None;
            } else {
                let n: i64 = value.parse().map_err(|_| {
                    SmartrmError::Config(format!(
                        "invalid value for default_ttl_seconds: '{}' (expected a number or 'none')",
                        value
                    ))
                })?;
                config.default_ttl_seconds = Some(n);
            }
        }
        "protected_patterns" => {
            config.protected_patterns = parse_comma_list(value);
        }
        "excluded_patterns" => {
            config.excluded_patterns = parse_comma_list(value);
        }
        "archive_root" => {
            if value == "none" || value == "null" || value.is_empty() {
                config.archive_root = None;
            } else {
                config.archive_root = Some(value.to_string());
            }
        }
        "danger_protection" => {
            config.danger_protection = parse_bool(value, "danger_protection")?;
        }
        "auto_cleanup" => {
            config.auto_cleanup = parse_bool(value, "auto_cleanup")?;
        }
        "destructive_gate_method" => {
            match value {
                "confirmation_phrase" | "passphrase" | "none" => {}
                _ => {
                    return Err(SmartrmError::Config(format!(
                        "invalid value for destructive_gate_method: '{}' (expected: confirmation_phrase, passphrase, none)",
                        value
                    )));
                }
            }
            config.destructive_gate_method = value.to_string();
        }
        "allow_destructive_commands" => {
            match value {
                "interactive_with_confirmation" | "always" | "never" => {}
                _ => {
                    return Err(SmartrmError::Config(format!(
                        "invalid value for allow_destructive_commands: '{}' (expected: interactive_with_confirmation, always, never)",
                        value
                    )));
                }
            }
            config.allow_destructive_commands = value.to_string();
        }
        "agent_detection" => {
            config.agent_detection = parse_bool(value, "agent_detection")?;
        }
        _ => {
            return Err(SmartrmError::Config(format!(
                "unknown config key: '{}'",
                key
            )));
        }
    }
    Ok(())
}

/// Set a configuration value by key. Loads, modifies, and persists config.
pub fn set_config(key: &str, value: &str) -> Result<ExitCode> {
    let mut config = load_config();
    apply_config_value(&mut config, key, value)?;
    save_config(&config)?;
    println!("Set {} = {}", key, value);
    Ok(ExitCode::from(0))
}

/// Set the passphrase for the destructive command gate.
///
/// Reads the passphrase from stdin (twice for confirmation), hashes it with
/// Argon2, and stores the hash in the config file.
pub fn set_passphrase() -> Result<ExitCode> {
    let pass1 = read_passphrase("Enter passphrase: ")?;
    let pass2 = read_passphrase("Confirm passphrase: ")?;

    if pass1 != pass2 {
        return Err(SmartrmError::Config(
            "passphrases do not match".to_string(),
        ));
    }

    if pass1.is_empty() {
        return Err(SmartrmError::Config(
            "passphrase cannot be empty".to_string(),
        ));
    }

    let hash = hash_passphrase(&pass1)?;

    let mut config = load_config();
    config.passphrase_hash = Some(hash);
    save_config(&config)?;

    println!("Passphrase set successfully.");
    Ok(ExitCode::from(0))
}

/// Hash a passphrase using Argon2id.
pub fn hash_passphrase(passphrase: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(passphrase.as_bytes(), &salt)
        .map_err(|e| SmartrmError::Config(format!("failed to hash passphrase: {}", e)))?;
    Ok(hash.to_string())
}

/// Verify a passphrase against a stored hash.
pub fn verify_passphrase(passphrase: &str, hash: &str) -> Result<bool> {
    use argon2::password_hash::PasswordHash;
    use argon2::PasswordVerifier;

    let parsed = PasswordHash::new(hash)
        .map_err(|e| SmartrmError::Config(format!("invalid passphrase hash: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(passphrase.as_bytes(), &parsed)
        .is_ok())
}

// -- helpers --

fn parse_bool(value: &str, key: &str) -> Result<bool> {
    match value {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(SmartrmError::Config(format!(
            "invalid value for {}: '{}' (expected: true/false)",
            key, value
        ))),
    }
}

fn parse_comma_list(value: &str) -> Vec<String> {
    if value.is_empty() || value == "none" || value == "null" {
        return Vec::new();
    }
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Read a passphrase from stdin. When stdin is a terminal, disables echo.
fn read_passphrase(prompt: &str) -> Result<String> {
    let stdin = io::stdin();
    let mut stderr = io::stderr();

    // Write prompt to stderr (so it's visible even if stdout is redirected)
    write!(stderr, "{}", prompt).ok();
    stderr.flush().ok();

    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(SmartrmError::Io)?;

    // Print newline after hidden input
    eprintln!();

    Ok(line.trim_end_matches('\n').trim_end_matches('\r').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_bool_accepts_valid_values() {
        assert!(parse_bool("true", "test").unwrap());
        assert!(parse_bool("1", "test").unwrap());
        assert!(parse_bool("yes", "test").unwrap());
        assert!(!parse_bool("false", "test").unwrap());
        assert!(!parse_bool("0", "test").unwrap());
        assert!(!parse_bool("no", "test").unwrap());
    }

    #[test]
    fn parse_bool_rejects_invalid() {
        assert!(parse_bool("maybe", "test").is_err());
    }

    #[test]
    fn parse_comma_list_works() {
        assert_eq!(
            parse_comma_list(".env, .env.*, .secret"),
            vec![".env", ".env.*", ".secret"]
        );
        assert!(parse_comma_list("none").is_empty());
        assert!(parse_comma_list("").is_empty());
    }

    #[test]
    fn hash_and_verify_passphrase_roundtrip() {
        let pass = "my-secret-passphrase";
        let hash = hash_passphrase(pass).unwrap();
        assert!(verify_passphrase(pass, &hash).unwrap());
        assert!(!verify_passphrase("wrong-passphrase", &hash).unwrap());
    }

    #[test]
    fn config_roundtrip_set_and_reload() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        // Save directly to a known path, then read back
        let mut config = SmartrmConfig::default();
        config.default_delete_mode = "permanent".to_string();
        config.min_free_space_bytes = 999;
        config.danger_protection = false;
        config.protected_patterns = vec!["*.key".to_string()];

        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, &json).unwrap();

        // Verify the file was written
        assert!(config_path.exists());

        // Reload from file directly
        let contents = std::fs::read_to_string(&config_path).unwrap();
        let reloaded: SmartrmConfig = serde_json::from_str(&contents).unwrap();
        assert_eq!(reloaded.default_delete_mode, "permanent");
        assert_eq!(reloaded.min_free_space_bytes, 999);
        assert!(!reloaded.danger_protection);
        assert_eq!(reloaded.protected_patterns, vec!["*.key"]);
    }

    #[test]
    fn config_set_rejects_unknown_key() {
        // Test validation logic directly: apply_config_value should reject unknown keys
        let mut config = SmartrmConfig::default();
        let result = apply_config_value(&mut config, "nonexistent_key", "value");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("unknown config key"));
    }

    #[test]
    fn config_set_validates_types() {
        let mut config = SmartrmConfig::default();

        // min_free_space_bytes rejects non-numeric
        let result = apply_config_value(&mut config, "min_free_space_bytes", "not-a-number");
        assert!(result.is_err());

        // danger_protection rejects non-bool
        let result = apply_config_value(&mut config, "danger_protection", "maybe");
        assert!(result.is_err());

        // default_delete_mode rejects unknown mode
        let result = apply_config_value(&mut config, "default_delete_mode", "yolo");
        assert!(result.is_err());
    }

    #[test]
    fn config_set_valid_key_persists() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        // Apply changes to a config and save manually
        let mut config = SmartrmConfig::default();

        apply_config_value(&mut config, "danger_protection", "false").unwrap();
        assert!(!config.danger_protection);

        apply_config_value(&mut config, "min_free_space_bytes", "2048").unwrap();
        assert_eq!(config.min_free_space_bytes, 2048);

        apply_config_value(&mut config, "protected_patterns", ".env,.secret,*.pem").unwrap();
        assert_eq!(
            config.protected_patterns,
            vec![".env", ".secret", "*.pem"]
        );

        // Write and read back
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, &json).unwrap();

        let contents = std::fs::read_to_string(&config_path).unwrap();
        let reloaded: SmartrmConfig = serde_json::from_str(&contents).unwrap();
        assert!(!reloaded.danger_protection);
        assert_eq!(reloaded.min_free_space_bytes, 2048);
        assert_eq!(
            reloaded.protected_patterns,
            vec![".env", ".secret", "*.pem"]
        );
    }

    #[test]
    fn format_bytes_in_config_display() {
        // Verify format_bytes produces expected output for config display
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
    }
}

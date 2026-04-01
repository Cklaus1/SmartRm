pub mod agent_detection;
pub mod audit;
pub mod auth;
pub mod cooldown;
pub mod scope_preview;
pub mod tty;

use crate::error::{Result, SmartrmError};
use crate::policy::config::SmartrmConfig;

/// Abstraction over TTY/env for testability.
pub trait GateEnvironment {
    fn is_stdin_tty(&self) -> bool;
    fn is_stderr_tty(&self) -> bool;
    fn read_line_from_tty(&self, prompt: &str) -> std::io::Result<String>;
    fn get_env(&self, key: &str) -> Option<String>;
    fn now(&self) -> chrono::DateTime<chrono::Utc>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateTier {
    /// y/N only (--permanent on non-protected)
    SimpleConfirm,
    /// TTY + scope preview + confirmation phrase or passphrase
    Standard,
    /// TTY + scope preview + type phrase "PURGE PROTECTED" + phrase/passphrase
    Elevated,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GateDecision {
    Allowed,
    Denied(String),
}

pub struct GateScope {
    pub action: String,
    pub object_count: usize,
    pub total_bytes: u64,
    pub protected_count: usize,
    pub examples: Vec<String>,
}

/// Main gate check. Returns Ok(Allowed) or Ok(Denied(reason)).
/// Called before executing any destructive command.
pub fn check_gate(
    env: &dyn GateEnvironment,
    config: &SmartrmConfig,
    tier: GateTier,
    scope: &GateScope,
    conn: &rusqlite::Connection,
) -> Result<GateDecision> {
    let mode = config.allow_destructive_commands.as_str();

    // 1. Check allow_destructive_commands config
    if mode == "disabled" {
        let reason = "destructive commands are disabled in configuration".to_string();
        audit::log_attempt(
            conn,
            &scope.action,
            "",
            env.is_stdin_tty(),
            scope,
            "denied",
            Some(&reason),
        )?;
        return Ok(GateDecision::Denied(reason));
    }

    // 2. Verify TTY
    if !env.is_stdin_tty() || !env.is_stderr_tty() {
        let reason = "destructive commands require an interactive terminal (TTY)".to_string();
        audit::log_attempt(
            conn,
            &scope.action,
            "",
            false,
            scope,
            "no_tty",
            Some(&reason),
        )?;
        return Ok(GateDecision::Denied(reason));
    }

    // 3. Agent detection
    if config.agent_detection && agent_detection::is_agent_environment(env) {
        let reason =
            "destructive commands are blocked in automated/agent environments".to_string();
        audit::log_attempt(
            conn,
            &scope.action,
            "",
            true,
            scope,
            "blocked_agent",
            Some(&reason),
        )?;
        return Ok(GateDecision::Denied(reason));
    }

    // 4. For SimpleConfirm tier: just y/N prompt
    if tier == GateTier::SimpleConfirm {
        let prompt = format!(
            "{}\nProceed? [y/N] ",
            scope_preview::format_scope_preview(scope)
        );
        match env.read_line_from_tty(&prompt) {
            Ok(line) => {
                let answer = line.trim().to_lowercase();
                if answer == "y" || answer == "yes" {
                    audit::log_attempt(
                        conn,
                        &scope.action,
                        "",
                        true,
                        scope,
                        "allowed",
                        None,
                    )?;
                    return Ok(GateDecision::Allowed);
                }
                let reason = "user declined confirmation".to_string();
                audit::log_attempt(
                    conn,
                    &scope.action,
                    "",
                    true,
                    scope,
                    "denied",
                    Some(&reason),
                )?;
                return Ok(GateDecision::Denied(reason));
            }
            Err(e) => {
                return Err(SmartrmError::Io(e));
            }
        }
    }

    // For interactive_only mode: use y/N even for Standard/Elevated tiers
    if mode == "interactive_only" {
        let prompt = format!(
            "{}\nProceed? [y/N] ",
            scope_preview::format_scope_preview(scope)
        );
        match env.read_line_from_tty(&prompt) {
            Ok(line) => {
                let answer = line.trim().to_lowercase();
                if answer == "y" || answer == "yes" {
                    audit::log_attempt(
                        conn,
                        &scope.action,
                        "",
                        true,
                        scope,
                        "allowed",
                        None,
                    )?;
                    return Ok(GateDecision::Allowed);
                }
                let reason = "user declined confirmation".to_string();
                audit::log_attempt(
                    conn,
                    &scope.action,
                    "",
                    true,
                    scope,
                    "denied",
                    Some(&reason),
                )?;
                return Ok(GateDecision::Denied(reason));
            }
            Err(e) => {
                return Err(SmartrmError::Io(e));
            }
        }
    }

    // 5. Display scope preview
    let preview = scope_preview::format_scope_preview(scope);

    // 6. For Elevated tier: require typing "PURGE PROTECTED"
    if tier == GateTier::Elevated {
        let elevated_phrase = "PURGE PROTECTED";
        let prompt = format!(
            "{}\nType \"{}\" to continue: ",
            preview, elevated_phrase
        );
        match env.read_line_from_tty(&prompt) {
            Ok(line) => {
                if line.trim() != elevated_phrase {
                    let reason = format!(
                        "elevated confirmation failed: expected \"{}\"",
                        elevated_phrase
                    );
                    audit::log_attempt(
                        conn,
                        &scope.action,
                        "",
                        true,
                        scope,
                        "denied",
                        Some(&reason),
                    )?;
                    return Ok(GateDecision::Denied(reason));
                }
            }
            Err(e) => {
                return Err(SmartrmError::Io(e));
            }
        }
    } else {
        // Standard tier: just show the preview
        // The preview is shown as part of the phrase prompt below
        let _ = env.read_line_from_tty(&format!("{}\nPress Enter to continue: ", preview));
    }

    // 7. Based on config gate method
    match config.destructive_gate_method.as_str() {
        "passphrase" => {
            // Read passphrase -- for now we just verify against a stored hash
            // The hash would be stored in config; if not set, deny
            let prompt = "Enter passphrase: ".to_string();
            match env.read_line_from_tty(&prompt) {
                Ok(input) => {
                    // We'd need the stored hash from config. For now, we accept
                    // any non-empty passphrase if no hash is stored (config
                    // must set one up via `smartrm config set-passphrase`).
                    // This is a placeholder -- real impl reads from config file.
                    let reason = "passphrase verification not configured; use 'smartrm config set-passphrase' first".to_string();
                    audit::log_attempt(
                        conn,
                        &scope.action,
                        "",
                        true,
                        scope,
                        "denied",
                        Some(&reason),
                    )?;
                    let _ = input; // suppress unused warning
                    Ok(GateDecision::Denied(reason))
                }
                Err(e) => Err(SmartrmError::Io(e)),
            }
        }
        _ => {
            // Default: confirmation_phrase
            let expected = auth::generate_confirmation_phrase(scope);
            let prompt = format!("Type \"{}\" to confirm: ", expected);
            match env.read_line_from_tty(&prompt) {
                Ok(input) => {
                    if auth::verify_phrase(&input, &expected) {
                        audit::log_attempt(
                            conn,
                            &scope.action,
                            "",
                            true,
                            scope,
                            "allowed",
                            None,
                        )?;
                        Ok(GateDecision::Allowed)
                    } else {
                        let reason = format!(
                            "confirmation phrase mismatch: expected \"{}\"",
                            expected
                        );
                        audit::log_attempt(
                            conn,
                            &scope.action,
                            "",
                            true,
                            scope,
                            "denied",
                            Some(&reason),
                        )?;
                        Ok(GateDecision::Denied(reason))
                    }
                }
                Err(e) => Err(SmartrmError::Io(e)),
            }
        }
    }
}

#[cfg(test)]
mod tests;

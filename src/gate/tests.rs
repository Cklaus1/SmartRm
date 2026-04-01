use std::cell::RefCell;
use std::collections::HashMap;

use super::*;
use crate::db;
use crate::gate::audit;
use crate::policy::config::SmartrmConfig;

/// Mock GateEnvironment for testing.
pub struct MockGateEnvironment {
    pub tty: bool,
    pub env_vars: HashMap<String, String>,
    pub input_lines: Vec<String>,
    input_index: RefCell<usize>,
}

impl MockGateEnvironment {
    pub fn new(tty: bool) -> Self {
        Self {
            tty,
            env_vars: HashMap::new(),
            input_lines: Vec::new(),
            input_index: RefCell::new(0),
        }
    }

    pub fn with_inputs(mut self, inputs: Vec<&str>) -> Self {
        self.input_lines = inputs.into_iter().map(String::from).collect();
        self
    }

    pub fn with_env(mut self, key: &str, val: &str) -> Self {
        self.env_vars.insert(key.to_string(), val.to_string());
        self
    }
}

impl GateEnvironment for MockGateEnvironment {
    fn is_stdin_tty(&self) -> bool {
        self.tty
    }
    fn is_stderr_tty(&self) -> bool {
        self.tty
    }
    fn read_line_from_tty(&self, _prompt: &str) -> std::io::Result<String> {
        let mut idx = self.input_index.borrow_mut();
        if *idx < self.input_lines.len() {
            let line = self.input_lines[*idx].clone();
            *idx += 1;
            Ok(line)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "no more mock input",
            ))
        }
    }
    fn get_env(&self, key: &str) -> Option<String> {
        self.env_vars.get(key).cloned()
    }
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

fn test_scope(count: usize) -> GateScope {
    GateScope {
        action: "purge".to_string(),
        object_count: count,
        total_bytes: count as u64 * 1024,
        protected_count: 0,
        examples: vec!["/tmp/example.txt".to_string()],
    }
}

fn test_config() -> SmartrmConfig {
    SmartrmConfig::default()
}

// -----------------------------------------------------------------------
// No TTY -> Denied
// -----------------------------------------------------------------------

#[test]
fn no_tty_denied() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(false);
    let config = test_config();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(
        decision,
        GateDecision::Denied(
            "destructive commands require an interactive terminal (TTY)".to_string()
        )
    );

    // Verify audit log
    assert_eq!(audit::count_audit_entries(&conn, Some("no_tty")).unwrap(), 1);
}

// -----------------------------------------------------------------------
// Agent detected -> Denied
// -----------------------------------------------------------------------

#[test]
fn agent_detected_denied() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true).with_env("CI", "true");
    let config = test_config();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(
        decision,
        GateDecision::Denied(
            "destructive commands are blocked in automated/agent environments".to_string()
        )
    );

    assert_eq!(
        audit::count_audit_entries(&conn, Some("blocked_agent")).unwrap(),
        1
    );
}

// -----------------------------------------------------------------------
// Correct confirmation phrase -> Allowed
// -----------------------------------------------------------------------

#[test]
fn correct_phrase_allowed() {
    let conn = db::open_memory_database().unwrap();
    let scope = test_scope(10);
    // The phrase will be "PURGE 10 OBJECTS"
    // For Standard tier: first input is the "Press Enter" prompt, second is the phrase
    let env = MockGateEnvironment::new(true).with_inputs(vec!["", "PURGE 10 OBJECTS"]);
    let config = test_config();

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(decision, GateDecision::Allowed);

    assert_eq!(
        audit::count_audit_entries(&conn, Some("allowed")).unwrap(),
        1
    );
}

// -----------------------------------------------------------------------
// Wrong confirmation phrase -> Denied
// -----------------------------------------------------------------------

#[test]
fn wrong_phrase_denied() {
    let conn = db::open_memory_database().unwrap();
    let scope = test_scope(10);
    let env = MockGateEnvironment::new(true).with_inputs(vec!["", "PURGE 9 OBJECTS"]);
    let config = test_config();

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    match decision {
        GateDecision::Denied(reason) => {
            assert!(reason.contains("confirmation phrase mismatch"));
        }
        GateDecision::Allowed => panic!("expected Denied"),
    }

    assert_eq!(
        audit::count_audit_entries(&conn, Some("denied")).unwrap(),
        1
    );
}

// -----------------------------------------------------------------------
// Config "disabled" -> all gated commands blocked
// -----------------------------------------------------------------------

#[test]
fn config_disabled_blocks_all() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true);
    let mut config = test_config();
    config.allow_destructive_commands = "disabled".to_string();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(
        decision,
        GateDecision::Denied(
            "destructive commands are disabled in configuration".to_string()
        )
    );
}

// -----------------------------------------------------------------------
// Config "interactive_only" -> y/N only, no phrase
// -----------------------------------------------------------------------

#[test]
fn config_interactive_only_yes() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true).with_inputs(vec!["y"]);
    let mut config = test_config();
    config.allow_destructive_commands = "interactive_only".to_string();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(decision, GateDecision::Allowed);
}

#[test]
fn config_interactive_only_no() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true).with_inputs(vec!["n"]);
    let mut config = test_config();
    config.allow_destructive_commands = "interactive_only".to_string();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(
        decision,
        GateDecision::Denied("user declined confirmation".to_string())
    );
}

// -----------------------------------------------------------------------
// SimpleConfirm tier: y/N only
// -----------------------------------------------------------------------

#[test]
fn simple_confirm_yes() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true).with_inputs(vec!["yes"]);
    let config = test_config();
    let scope = test_scope(5);

    let decision = check_gate(&env, &config, GateTier::SimpleConfirm, &scope, &conn).unwrap();
    assert_eq!(decision, GateDecision::Allowed);
}

#[test]
fn simple_confirm_no() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true).with_inputs(vec!["n"]);
    let config = test_config();
    let scope = test_scope(5);

    let decision = check_gate(&env, &config, GateTier::SimpleConfirm, &scope, &conn).unwrap();
    assert_eq!(
        decision,
        GateDecision::Denied("user declined confirmation".to_string())
    );
}

// -----------------------------------------------------------------------
// Elevated tier requires "PURGE PROTECTED" phrase
// -----------------------------------------------------------------------

#[test]
fn elevated_correct_phrase_then_confirmation() {
    let conn = db::open_memory_database().unwrap();
    let scope = test_scope(3);
    // Elevated: first input is "PURGE PROTECTED", second is the confirmation phrase "PURGE 3 OBJECTS"
    let env =
        MockGateEnvironment::new(true).with_inputs(vec!["PURGE PROTECTED", "PURGE 3 OBJECTS"]);
    let config = test_config();

    let decision = check_gate(&env, &config, GateTier::Elevated, &scope, &conn).unwrap();
    assert_eq!(decision, GateDecision::Allowed);
}

#[test]
fn elevated_wrong_elevated_phrase_denied() {
    let conn = db::open_memory_database().unwrap();
    let scope = test_scope(3);
    let env = MockGateEnvironment::new(true).with_inputs(vec!["WRONG PHRASE"]);
    let config = test_config();

    let decision = check_gate(&env, &config, GateTier::Elevated, &scope, &conn).unwrap();
    match decision {
        GateDecision::Denied(reason) => {
            assert!(reason.contains("elevated confirmation failed"));
        }
        GateDecision::Allowed => panic!("expected Denied"),
    }
}

// -----------------------------------------------------------------------
// Audit log written for both success and failure
// -----------------------------------------------------------------------

#[test]
fn audit_log_records_both_outcomes() {
    let conn = db::open_memory_database().unwrap();
    let config = test_config();

    // Denied (wrong phrase)
    let scope1 = test_scope(10);
    let env1 = MockGateEnvironment::new(true).with_inputs(vec!["", "WRONG"]);
    let _ = check_gate(&env1, &config, GateTier::Standard, &scope1, &conn);

    // Allowed (correct phrase)
    let scope2 = test_scope(5);
    let env2 = MockGateEnvironment::new(true).with_inputs(vec!["", "PURGE 5 OBJECTS"]);
    let _ = check_gate(&env2, &config, GateTier::Standard, &scope2, &conn);

    let total = audit::count_audit_entries(&conn, None).unwrap();
    assert!(total >= 2, "expected at least 2 audit entries, got {}", total);
}

// -----------------------------------------------------------------------
// Agent detection disabled in config -> agent env allowed through
// -----------------------------------------------------------------------

#[test]
fn agent_detection_disabled_allows_ci() {
    let conn = db::open_memory_database().unwrap();
    let env = MockGateEnvironment::new(true)
        .with_env("CI", "true")
        .with_inputs(vec!["y"]);
    let mut config = test_config();
    config.agent_detection = false;
    config.allow_destructive_commands = "interactive_only".to_string();
    let scope = test_scope(10);

    let decision = check_gate(&env, &config, GateTier::Standard, &scope, &conn).unwrap();
    assert_eq!(decision, GateDecision::Allowed);
}

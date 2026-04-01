use super::GateEnvironment;

/// Known environment variable markers that indicate CI/automation environments.
const AGENT_MARKERS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "JENKINS_URL",
    "BUILDKITE",
    "TRAVIS",
    "CIRCLECI",
    "GITLAB_CI",
    "SMARTRM_AGENT_MODE",
];

/// Check whether the current environment looks like a CI/automation agent.
///
/// Returns `true` if any known marker env var is set to a non-empty value,
/// or if `TERM=dumb` (common in non-interactive subprocesses).
pub fn is_agent_environment(env: &dyn GateEnvironment) -> bool {
    for key in AGENT_MARKERS {
        if let Some(val) = env.get_env(key) {
            if !val.is_empty() {
                return true;
            }
        }
    }

    // TERM=dumb is a strong signal of a non-interactive environment
    if let Some(term) = env.get_env("TERM") {
        if term == "dumb" {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestEnv {
        vars: HashMap<String, String>,
    }

    impl GateEnvironment for TestEnv {
        fn is_stdin_tty(&self) -> bool {
            true
        }
        fn is_stderr_tty(&self) -> bool {
            true
        }
        fn read_line_from_tty(&self, _prompt: &str) -> std::io::Result<String> {
            Ok(String::new())
        }
        fn get_env(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }

    #[test]
    fn clean_environment_is_not_agent() {
        let env = TestEnv {
            vars: HashMap::new(),
        };
        assert!(!is_agent_environment(&env));
    }

    #[test]
    fn ci_true_is_agent() {
        let mut vars = HashMap::new();
        vars.insert("CI".to_string(), "true".to_string());
        let env = TestEnv { vars };
        assert!(is_agent_environment(&env));
    }

    #[test]
    fn github_actions_is_agent() {
        let mut vars = HashMap::new();
        vars.insert("GITHUB_ACTIONS".to_string(), "true".to_string());
        let env = TestEnv { vars };
        assert!(is_agent_environment(&env));
    }

    #[test]
    fn term_dumb_is_agent() {
        let mut vars = HashMap::new();
        vars.insert("TERM".to_string(), "dumb".to_string());
        let env = TestEnv { vars };
        assert!(is_agent_environment(&env));
    }

    #[test]
    fn term_xterm_is_not_agent() {
        let mut vars = HashMap::new();
        vars.insert("TERM".to_string(), "xterm-256color".to_string());
        let env = TestEnv { vars };
        assert!(!is_agent_environment(&env));
    }

    #[test]
    fn smartrm_agent_mode_is_agent() {
        let mut vars = HashMap::new();
        vars.insert("SMARTRM_AGENT_MODE".to_string(), "1".to_string());
        let env = TestEnv { vars };
        assert!(is_agent_environment(&env));
    }

    #[test]
    fn empty_ci_var_is_not_agent() {
        let mut vars = HashMap::new();
        vars.insert("CI".to_string(), String::new());
        let env = TestEnv { vars };
        assert!(!is_agent_environment(&env));
    }
}

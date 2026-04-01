use std::time::{Duration, Instant};

/// Tracks failed gate attempts with exponential backoff cooldown.
///
/// Defaults: 3 max attempts, 30s base cooldown, 2x escalation, 5min max lockout.
pub struct CooldownState {
    attempts: u32,
    max_attempts: u32,
    last_failure: Option<Instant>,
    cooldown_base: Duration,
    lockout_count: u32,
    max_cooldown: Duration,
}

impl CooldownState {
    pub fn new() -> Self {
        Self {
            attempts: 0,
            max_attempts: 3,
            last_failure: None,
            cooldown_base: Duration::from_secs(30),
            lockout_count: 0,
            max_cooldown: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Record a failed gate attempt. Increments the attempt counter and,
    /// if the threshold is reached, starts a cooldown period.
    pub fn record_failure(&mut self) {
        self.attempts += 1;
        if self.attempts >= self.max_attempts {
            self.last_failure = Some(Instant::now());
            self.lockout_count += 1;
        }
    }

    /// Record a successful gate pass. Resets the attempt counter.
    pub fn record_success(&mut self) {
        self.attempts = 0;
        self.last_failure = None;
        self.lockout_count = 0;
    }

    /// Check whether the user is currently locked out.
    pub fn is_locked_out(&self) -> bool {
        if self.attempts < self.max_attempts {
            return false;
        }
        match self.last_failure {
            Some(t) => t.elapsed() < self.current_cooldown(),
            None => false,
        }
    }

    /// Number of remaining attempts before lockout.
    pub fn remaining_attempts(&self) -> u32 {
        if self.attempts >= self.max_attempts {
            0
        } else {
            self.max_attempts - self.attempts
        }
    }

    /// Duration remaining in the current cooldown, or None if not locked out.
    pub fn cooldown_remaining(&self) -> Option<Duration> {
        if self.attempts < self.max_attempts {
            return None;
        }
        match self.last_failure {
            Some(t) => {
                let cooldown = self.current_cooldown();
                let elapsed = t.elapsed();
                if elapsed < cooldown {
                    Some(cooldown - elapsed)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn current_cooldown(&self) -> Duration {
        // Exponential backoff: base * 2^(lockout_count - 1), capped at max
        let multiplier = 2u32.saturating_pow(self.lockout_count.saturating_sub(1));
        let cooldown = self.cooldown_base * multiplier;
        if cooldown > self.max_cooldown {
            self.max_cooldown
        } else {
            cooldown
        }
    }
}

impl Default for CooldownState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_not_locked() {
        let state = CooldownState::new();
        assert!(!state.is_locked_out());
        assert_eq!(state.remaining_attempts(), 3);
        assert!(state.cooldown_remaining().is_none());
    }

    #[test]
    fn one_failure_not_locked() {
        let mut state = CooldownState::new();
        state.record_failure();
        assert!(!state.is_locked_out());
        assert_eq!(state.remaining_attempts(), 2);
    }

    #[test]
    fn two_failures_not_locked() {
        let mut state = CooldownState::new();
        state.record_failure();
        state.record_failure();
        assert!(!state.is_locked_out());
        assert_eq!(state.remaining_attempts(), 1);
    }

    #[test]
    fn three_failures_locked() {
        let mut state = CooldownState::new();
        state.record_failure();
        state.record_failure();
        state.record_failure();
        assert!(state.is_locked_out());
        assert_eq!(state.remaining_attempts(), 0);
        assert!(state.cooldown_remaining().is_some());
    }

    #[test]
    fn success_resets_state() {
        let mut state = CooldownState::new();
        state.record_failure();
        state.record_failure();
        state.record_success();
        assert!(!state.is_locked_out());
        assert_eq!(state.remaining_attempts(), 3);
        assert!(state.cooldown_remaining().is_none());
    }

    #[test]
    fn cooldown_expires() {
        let mut state = CooldownState::new();
        state.max_attempts = 1; // Lock after 1 failure
        state.cooldown_base = Duration::from_millis(1); // Very short cooldown
        state.record_failure();
        assert!(state.is_locked_out());

        // Sleep just past the cooldown
        std::thread::sleep(Duration::from_millis(5));
        assert!(!state.is_locked_out());
    }
}

use super::GateEnvironment;

/// Real implementation of GateEnvironment that uses actual TTY and env vars.
pub struct RealGateEnvironment;

impl GateEnvironment for RealGateEnvironment {
    fn is_stdin_tty(&self) -> bool {
        unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
    }

    fn is_stderr_tty(&self) -> bool {
        unsafe { libc::isatty(libc::STDERR_FILENO) != 0 }
    }

    fn read_line_from_tty(&self, prompt: &str) -> std::io::Result<String> {
        use std::io::{BufRead, Write};

        // Write prompt to stderr (always goes to terminal)
        eprint!("{}", prompt);
        std::io::stderr().flush()?;

        // Open /dev/tty for reading -- this reads from the terminal even if
        // stdin is redirected.
        let tty = std::fs::File::open("/dev/tty")?;
        let mut reader = std::io::BufReader::new(tty);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        Ok(line.trim().to_string())
    }

    fn get_env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

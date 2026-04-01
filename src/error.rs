use std::fmt;
use std::io;

#[derive(Debug)]
pub enum SmartrmError {
    Io(io::Error),
    Db(rusqlite::Error),
    NotFound(String),
    DangerBlocked(String),
    DiskSpaceLow {
        needed: u64,
        available: u64,
        min_free: u64,
    },
    GateDenied(String),
    InvalidState {
        expected: String,
        actual: String,
    },
    Config(String),
}

impl fmt::Display for SmartrmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmartrmError::Io(err) => {
                // Format I/O errors in rm-style when possible
                write!(f, "{err}")
            }
            SmartrmError::Db(err) => write!(f, "database error: {err}"),
            SmartrmError::NotFound(msg) => write!(f, "{msg}"),
            SmartrmError::DangerBlocked(reason) => {
                write!(f, "operation blocked by safety gate: {reason}")
            }
            SmartrmError::DiskSpaceLow {
                needed,
                available,
                min_free,
            } => write!(
                f,
                "insufficient disk space: need {needed} bytes, \
                 {available} available, minimum free threshold is {min_free}"
            ),
            SmartrmError::GateDenied(gate) => {
                write!(f, "gate denied: {gate}")
            }
            SmartrmError::InvalidState { expected, actual } => {
                write!(f, "invalid state: expected {expected}, got {actual}")
            }
            SmartrmError::Config(msg) => write!(f, "configuration error: {msg}"),
        }
    }
}

impl std::error::Error for SmartrmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SmartrmError::Io(err) => Some(err),
            SmartrmError::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for SmartrmError {
    fn from(err: io::Error) -> Self {
        SmartrmError::Io(err)
    }
}

impl From<rusqlite::Error> for SmartrmError {
    fn from(err: rusqlite::Error) -> Self {
        SmartrmError::Db(err)
    }
}

pub type Result<T> = std::result::Result<T, SmartrmError>;

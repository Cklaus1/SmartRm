use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "smartrm",
    version,
    about = "File Lifecycle System — intelligent, reversible rm replacement"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    // When no subcommand is given, these are the delete-mode args
    /// Files/directories to archive (delete mode)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub files: Vec<PathBuf>,

    // rm-compatible flags (apply to delete mode)
    /// Remove directories and their contents recursively
    #[arg(short = 'r', long = "recursive", alias = "R")]
    pub recursive: bool,

    /// Ignore nonexistent files and arguments, never prompt
    #[arg(short = 'f', long = "force")]
    pub force: bool,

    /// Prompt before every removal
    #[arg(short = 'i')]
    pub interactive_each: bool,

    /// Prompt once before removing more than three files, or when removing recursively
    #[arg(short = 'I')]
    pub interactive_once: bool,

    /// Remove empty directories
    #[arg(short = 'd', long = "dir")]
    pub dir: bool,

    /// Explain what is being done
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// Do not treat '/' specially
    #[arg(long = "no-preserve-root")]
    pub no_preserve_root: bool,

    /// Do not remove '/' (default)
    #[arg(long = "preserve-root", default_value_t = true)]
    pub preserve_root: bool,

    /// Do not cross filesystem boundaries
    #[arg(long = "one-file-system")]
    pub one_file_system: bool,

    /// Permanently delete (bypass archive)
    #[arg(long = "permanent")]
    pub permanent: bool,

    /// Override dangerous operation warnings
    #[arg(long = "yes-i-am-sure")]
    pub yes_i_am_sure: bool,

    /// Output in JSON format
    #[arg(long = "json", global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Restore the last N delete batches (default: 1)
    Undo {
        /// Number of batches to undo
        #[arg(default_value = "1")]
        count: u32,
        /// Conflict resolution policy
        #[arg(long, value_enum, default_value = "rename")]
        conflict: ConflictPolicyArg,
    },
    /// Restore archived files
    Restore {
        /// Archive ID to restore (first 8+ chars of ULID)
        archive_id: Option<String>,
        /// Restore all objects from a specific batch
        #[arg(long)]
        batch: Option<String>,
        /// Restore the most recent deletion
        #[arg(long)]
        last: bool,
        /// Restore all archived files
        #[arg(long)]
        all: bool,
        /// Restore only this path from a batch
        #[arg(long)]
        only: Option<String>,
        /// Restore to alternate location
        #[arg(long)]
        to: Option<PathBuf>,
        /// Conflict resolution policy
        #[arg(long, value_enum, default_value = "rename")]
        conflict: ConflictPolicyArg,
        /// Shorthand for --conflict overwrite
        #[arg(short = 'f', long = "force")]
        force: bool,
        /// Don't create missing parent directories
        #[arg(long)]
        no_create_parents: bool,
    },
    /// List archived files
    List {
        /// Filter by lifecycle state
        #[arg(long)]
        state: Option<String>,
        /// Maximum number of results
        #[arg(long, default_value = "50")]
        limit: u32,
        /// Cursor for keyset pagination (ULID of last seen item)
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Search archived files
    Search {
        /// Search pattern (glob if contains * or ?, else substring)
        pattern: String,
        /// Filter by date (ISO 8601)
        #[arg(long)]
        after: Option<String>,
        /// Filter by minimum size (e.g., "10M", "1G")
        #[arg(long)]
        larger_than: Option<String>,
        /// Filter by original directory
        #[arg(long)]
        dir: Option<String>,
        /// Maximum number of results
        #[arg(long, default_value = "50")]
        limit: u32,
        /// Skip first N results
        #[arg(long, default_value = "0")]
        offset: u32,
    },
    /// Show version history for a file path
    History {
        /// File path to show history for
        path: String,
    },
    /// Show chronological batch history
    Timeline {
        /// Show only today's activity
        #[arg(long)]
        today: bool,
        /// Filter by directory
        #[arg(long)]
        dir: Option<String>,
        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    /// Remove old archives
    Cleanup {
        /// Remove archives older than duration (e.g., "30d", "7d")
        #[arg(long)]
        older_than: Option<String>,
        /// Only purge expired-state objects
        #[arg(long)]
        expired: bool,
        /// Preview what would be cleaned
        #[arg(long)]
        dry_run: bool,
        /// Force cleanup of protected files
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Delete entire archive (for uninstall)
    Purge {
        /// Only purge expired objects
        #[arg(long)]
        expired: bool,
        /// Purge everything
        #[arg(long)]
        all: bool,
        /// Force without confirmation
        #[arg(short = 'f', long)]
        force: bool,
    },
    /// Show archive statistics
    Stats {
        /// Show storage prediction (Phase 2)
        #[arg(long)]
        predict: bool,
    },
    /// View or modify configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
    /// Explain why an archive object was archived
    Explain {
        /// Archive ID
        archive_id: String,
    },
    /// Explain what policy would apply to a path
    ExplainPolicy {
        /// File path to check
        path: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Set a configuration value
    Set {
        key: String,
        value: String,
    },
    /// Set the passphrase for destructive command gate
    SetPassphrase,
}

#[derive(ValueEnum, Clone, Debug, Copy)]
pub enum ConflictPolicyArg {
    Fail,
    Rename,
    Overwrite,
    Skip,
}

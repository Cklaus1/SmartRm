use std::path::PathBuf;
use std::process::ExitCode;

use rusqlite::Connection;

use crate::error::{Result, SmartrmError};
use crate::fs::RealFilesystem;
use crate::gate::{self, GateDecision, GateScope, GateTier};
use crate::gate::tty::RealGateEnvironment;
use crate::models::policy::Tag;
use crate::operations::delete::{execute_delete, DeleteContext};
use crate::output;
use crate::policy::{classifier, config::SmartrmConfig};

/// Parameters extracted from the CLI layer for the delete command.
/// This avoids depending on the `cli` module (which lives in the binary crate).
pub struct DeleteArgs {
    pub files: Vec<PathBuf>,
    pub recursive: bool,
    pub force: bool,
    pub interactive_each: bool,
    pub interactive_once: bool,
    pub dir: bool,
    pub verbose: bool,
    pub one_file_system: bool,
    pub permanent: bool,
    pub yes_i_am_sure: bool,
    pub json: bool,
}

pub fn run(args: &DeleteArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    // Gate check: --permanent requires the destructive gate
    if args.permanent {
        // Check if any paths are protected
        let has_protected = args.files.iter().any(|p| {
            let c = classifier::classify(p);
            c.tags.contains(&Tag::Protected)
        });

        let tier = if has_protected {
            GateTier::Standard // Full gate for protected paths
        } else {
            GateTier::SimpleConfirm // y/N for non-protected
        };

        let scope = GateScope {
            action: "permanently delete".to_string(),
            object_count: args.files.len(),
            total_bytes: 0, // not yet computed
            protected_count: if has_protected { 1 } else { 0 },
            examples: args.files.iter().take(5).map(|p| p.to_string_lossy().to_string()).collect(),
        };

        let env = RealGateEnvironment;
        let decision = gate::check_gate(&env, config, tier, &scope, conn)?;
        match decision {
            GateDecision::Allowed => {} // proceed
            GateDecision::Denied(reason) => {
                return Err(SmartrmError::GateDenied(reason));
            }
        }
    }

    let ctx = DeleteContext {
        conn,
        fs: &RealFilesystem,
        config,
        paths: args.files.clone(),
        recursive: args.recursive,
        force: args.force,
        interactive_each: args.interactive_each,
        interactive_once: args.interactive_once,
        dir: args.dir,
        verbose: args.verbose,
        one_file_system: args.one_file_system,
        permanent: args.permanent,
        yes_i_am_sure: args.yes_i_am_sure,
        json: args.json,
    };

    let result = execute_delete(&ctx)?;
    let exit_code = if result.failed > 0 { 1 } else { 0 };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(exit_code))
}

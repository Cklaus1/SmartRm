use std::process::ExitCode;

use rusqlite::Connection;

use crate::error::{Result, SmartrmError};
use crate::fs::RealFilesystem;
use crate::gate::{self, GateDecision, GateScope, GateTier};
use crate::gate::tty::RealGateEnvironment;
use crate::operations::cleanup::{execute_cleanup, CleanupContext};
use crate::output;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the cleanup command.
pub struct CleanupArgs {
    pub older_than: Option<String>,
    pub expired: bool,
    pub dry_run: bool,
    pub force: bool,
    pub json: bool,
}

pub fn run(args: &CleanupArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    // Dry-run is read-only — no gate needed
    // Actual cleanup is destructive (permanently deletes archive content) — gate required
    if !args.dry_run {
        let (count, total_bytes) = crate::db::queries::count_all_archived(conn)?;
        if count > 0 {
            let scope = GateScope {
                action: "cleanup".to_string(),
                object_count: count as usize,
                total_bytes: total_bytes as u64,
                protected_count: 0,
                examples: crate::db::queries::list_archive_objects(conn, None, 5, None)
                    .unwrap_or_default()
                    .iter()
                    .map(|o| o.original_path.clone())
                    .collect(),
            };

            let env = RealGateEnvironment;
            let decision = gate::check_gate(&env, config, GateTier::Standard, &scope, conn)?;
            match decision {
                GateDecision::Allowed => {}
                GateDecision::Denied(reason) => {
                    return Err(SmartrmError::GateDenied(reason));
                }
            }
        }
    }

    let ctx = CleanupContext {
        conn,
        fs: &RealFilesystem,
        config,
        older_than: args.older_than.clone(),
        expired_only: args.expired,
        dry_run: args.dry_run,
        force: args.force,
        json: args.json,
    };

    let result = execute_cleanup(&ctx)?;
    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

use std::process::ExitCode;

use rusqlite::Connection;

use crate::error::{Result, SmartrmError};
use crate::fs::RealFilesystem;
use crate::gate::{self, GateDecision, GateScope, GateTier};
use crate::gate::tty::RealGateEnvironment;
use crate::operations::purge::{execute_purge, PurgeContext};
use crate::output;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the purge command.
pub struct PurgeArgs {
    pub expired: bool,
    pub all: bool,
    pub force: bool,
    pub json: bool,
}

pub fn run(args: &PurgeArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    // Determine scope for the gate check
    let (count, total_bytes) = crate::db::queries::count_all_archived(conn)?;

    // Gate ALWAYS runs for purge — no flag bypasses it
    if count > 0 {
        let scope = GateScope {
            action: "purge".to_string(),
            object_count: count as usize,
            total_bytes: total_bytes as u64,
            protected_count: 0,
            examples: build_examples(conn),
        };

        // Choose tier: purge --all is Elevated, everything else is Standard
        let tier = if args.all {
            GateTier::Elevated
        } else {
            GateTier::Standard
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

    let ctx = PurgeContext {
        conn,
        fs: &RealFilesystem,
        config,
        expired_only: args.expired,
        all: args.all,
        force: true, // gate already confirmed above
        json: args.json,
    };

    let result = execute_purge(&ctx)?;
    output::print_output(&result, args.json);

    Ok(ExitCode::from(0))
}

/// Grab a few example paths for the scope preview.
fn build_examples(conn: &Connection) -> Vec<String> {
    match crate::db::queries::list_archive_objects(conn, None, 5, None) {
        Ok(objects) => objects.iter().map(|o| o.original_path.clone()).collect(),
        Err(_) => vec![],
    }
}

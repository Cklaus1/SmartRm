use std::process::ExitCode;

use rusqlite::Connection;

use crate::error::Result;
use crate::fs::RealFilesystem;
use crate::models::ConflictPolicy;
use crate::operations::restore::{execute_restore, RestoreContext, RestoreTarget};
use crate::output;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the undo command.
pub struct UndoArgs {
    pub count: u32,
    pub conflict_policy: ConflictPolicy,
    pub json: bool,
}

pub fn run(args: &UndoArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    let ctx = RestoreContext {
        conn,
        fs: &RealFilesystem,
        config,
        target: RestoreTarget::LastN(args.count),
        to: None,
        conflict_policy: args.conflict_policy,
        create_parents: true,
        json: args.json,
    };

    let result = execute_restore(&ctx)?;
    let exit_code = if result.failed > 0 {
        1
    } else if result.requested == 0 {
        2 // nothing found to undo
    } else {
        0
    };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(exit_code))
}

use std::path::PathBuf;
use std::process::ExitCode;

use rusqlite::Connection;

use crate::error::Result;
use crate::fs::RealFilesystem;
use crate::models::ConflictPolicy;
use crate::operations::restore::{execute_restore, RestoreContext, RestoreTarget};
use crate::output;
use crate::policy::config::SmartrmConfig;

/// Parameters extracted from the CLI layer for the restore command.
pub struct RestoreArgs {
    pub archive_id: Option<String>,
    pub batch: Option<String>,
    pub last: bool,
    pub all: bool,
    pub to: Option<PathBuf>,
    pub conflict_policy: ConflictPolicy,
    pub no_create_parents: bool,
    pub json: bool,
}

pub fn run(args: &RestoreArgs, conn: &Connection, config: &SmartrmConfig) -> Result<ExitCode> {
    let target = resolve_target(args)?;

    let ctx = RestoreContext {
        conn,
        fs: &RealFilesystem,
        config,
        target,
        to: args.to.clone(),
        conflict_policy: args.conflict_policy,
        create_parents: !args.no_create_parents,
        json: args.json,
    };

    let result = execute_restore(&ctx)?;
    let exit_code = if result.failed > 0 {
        1
    } else if result.requested == 0 {
        2 // nothing found to restore
    } else {
        0
    };

    output::print_output(&result, args.json);

    Ok(ExitCode::from(exit_code))
}

fn resolve_target(args: &RestoreArgs) -> Result<RestoreTarget> {
    if let Some(ref id) = args.archive_id {
        return Ok(RestoreTarget::ById(id.clone()));
    }
    if let Some(ref batch) = args.batch {
        return Ok(RestoreTarget::ByBatch(batch.clone()));
    }
    if args.all {
        return Ok(RestoreTarget::All);
    }
    if args.last {
        return Ok(RestoreTarget::Last);
    }

    // Default: restore last delete batch
    Ok(RestoreTarget::Last)
}

mod cli;
pub mod commands;
pub mod db;
pub mod error;
pub mod fs;
pub mod gate;
pub mod id;
pub mod models;
pub mod operations;
pub mod output;
pub mod policy;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();

    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("smartrm: {}", e);
            ExitCode::from(1)
        }
    }
}

fn conflict_arg_to_policy(arg: cli::ConflictPolicyArg) -> models::ConflictPolicy {
    match arg {
        cli::ConflictPolicyArg::Fail => models::ConflictPolicy::Fail,
        cli::ConflictPolicyArg::Rename => models::ConflictPolicy::Rename,
        cli::ConflictPolicyArg::Overwrite => models::ConflictPolicy::Overwrite,
        cli::ConflictPolicyArg::Skip => models::ConflictPolicy::Skip,
    }
}

fn run(cli: cli::Cli) -> error::Result<ExitCode> {
    // Load configuration
    let config = policy::config::load_config();

    // Determine data directory and ensure it exists
    let data_dir = policy::config::resolve_data_dir(&config);
    std::fs::create_dir_all(&data_dir).map_err(error::SmartrmError::Io)?;

    let db_file = policy::config::db_path(&config);
    let conn = db::open_database(&db_file)?;

    match cli.command {
        None if !cli.files.is_empty() => {
            // Delete mode
            let args = commands::delete::DeleteArgs {
                files: cli.files.clone(),
                recursive: cli.recursive,
                force: cli.force,
                interactive_each: cli.interactive_each,
                interactive_once: cli.interactive_once,
                dir: cli.dir,
                verbose: cli.verbose,
                one_file_system: cli.one_file_system,
                permanent: cli.permanent,
                yes_i_am_sure: cli.yes_i_am_sure,
                json: cli.json,
            };
            commands::delete::run(&args, &conn, &config)
        }
        None => {
            // No args, no subcommand — show help
            use clap::CommandFactory;
            cli::Cli::command().print_help().ok();
            println!();
            Ok(ExitCode::from(0))
        }
        Some(cli::Command::Undo { count, conflict }) => {
            let args = commands::undo::UndoArgs {
                count,
                conflict_policy: conflict_arg_to_policy(conflict),
                json: cli.json,
            };
            commands::undo::run(&args, &conn, &config)
        }
        Some(cli::Command::Restore {
            archive_id,
            batch,
            last,
            all,
            only: _,
            to,
            conflict,
            force,
            no_create_parents,
        }) => {
            let conflict_policy = if force {
                models::ConflictPolicy::Overwrite
            } else {
                conflict_arg_to_policy(conflict)
            };
            let args = commands::restore::RestoreArgs {
                archive_id,
                batch,
                last,
                all,
                to,
                conflict_policy,
                no_create_parents,
                json: cli.json,
            };
            commands::restore::run(&args, &conn, &config)
        }
        Some(cli::Command::List {
            state,
            limit,
            cursor,
        }) => {
            let args = commands::list::ListArgs {
                state,
                limit,
                cursor,
                json: cli.json,
            };
            commands::list::run(&args, &conn, &config)
        }
        Some(cli::Command::Search {
            pattern,
            after,
            larger_than,
            dir,
            limit,
            offset,
        }) => {
            let args = commands::search::SearchArgs {
                pattern,
                after,
                larger_than,
                dir,
                limit,
                offset,
                json: cli.json,
            };
            commands::search::run(&args, &conn, &config)
        }
        Some(cli::Command::History { path }) => {
            let args = commands::history::HistoryArgs {
                path,
                json: cli.json,
            };
            commands::history::run(&args, &conn, &config)
        }
        Some(cli::Command::Timeline { today, dir, limit }) => {
            let args = commands::timeline::TimelineArgs {
                today,
                dir,
                limit,
                json: cli.json,
            };
            commands::timeline::run(&args, &conn, &config)
        }
        Some(cli::Command::Cleanup {
            older_than,
            expired,
            dry_run,
            force,
        }) => {
            let args = commands::cleanup::CleanupArgs {
                older_than,
                expired,
                dry_run,
                force,
                json: cli.json,
            };
            commands::cleanup::run(&args, &conn, &config)
        }
        Some(cli::Command::Purge {
            expired,
            all,
            force,
        }) => {
            let args = commands::purge::PurgeArgs {
                expired,
                all,
                force,
                json: cli.json,
            };
            commands::purge::run(&args, &conn, &config)
        }
        Some(cli::Command::Stats { predict: _ }) => {
            let args = commands::stats::StatsArgs { json: cli.json };
            commands::stats::run(&args, &conn, &config)
        }
        Some(cli::Command::Config { action }) => match action {
            None => commands::config::show_config(&config),
            Some(cli::ConfigAction::Set { key, value }) => {
                commands::config::set_config(&key, &value)
            }
            Some(cli::ConfigAction::SetPassphrase) => commands::config::set_passphrase(),
        },
        Some(cli::Command::Completions { shell }) => {
            use clap::CommandFactory;
            clap_complete::generate(
                shell,
                &mut cli::Cli::command(),
                "smartrm",
                &mut std::io::stdout(),
            );
            Ok(ExitCode::from(0))
        }
        Some(cli::Command::Explain { archive_id }) => {
            let args = commands::explain::ExplainArgs {
                archive_id,
                json: cli.json,
            };
            commands::explain::run(&args, &conn, &config)
        }
        Some(cli::Command::ExplainPolicy { path }) => {
            let args = commands::explain::ExplainPolicyArgs {
                path,
                json: cli.json,
            };
            commands::explain::run_explain_policy(&args, &conn, &config)
        }
    }
}

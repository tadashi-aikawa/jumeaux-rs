use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};

use jumeaux::cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => error.exit(),
            _ => {
                eprint!("{error}");
                return ExitCode::FAILURE;
            }
        },
    };
    if let Command::Run(args) = &cli.command {
        let result = jumeaux::config::Config::from_run_args(args).and_then(|config| {
            jumeaux::addons::log2reqs::prepare(&config, &jumeaux::notification::Context::from_env())
        });
        match result {
            Ok(result) => {
                for warning in result.warnings {
                    eprintln!("{warning}");
                }
                for notification in result.notifications {
                    if let Some(error) = notification.error {
                        eprintln!("{error}");
                    }
                }
                if let Some(code) = result.exit_code {
                    return ExitCode::from(code);
                }
            }
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let command = match cli.command {
        Command::Run(_) => "run",
        Command::Retry(_) => "retry",
        Command::Init(_) => "init",
        Command::Server(_) => "server",
        Command::Viewer(_) => "viewer",
    };

    eprintln!("{command}: 未実装");
    ExitCode::FAILURE
}

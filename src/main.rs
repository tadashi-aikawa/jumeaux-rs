use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};

use jumeaux::cli::{Cli, Command};
mod commands;
mod init;
mod logging;

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
    if let Command::Init(args) = &cli.command {
        return match init::run(args.name.as_deref()) {
            Ok(code) => ExitCode::from(code),
            Err(e) => {
                logging::Logger(0).log(jumeaux::notification::Verbosity::Error, e.to_string());
                ExitCode::FAILURE
            }
        };
    }
    if matches!(cli.command, Command::Run(_) | Command::Retry(_)) {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| jumeaux::compat::Error::new("RuntimeError", e.to_string()))
            .and_then(|runtime| {
                runtime.block_on(async {
                    match &cli.command {
                        Command::Run(args) => commands::run(args).await,
                        Command::Retry(args) => commands::retry(args).await,
                        _ => unreachable!(),
                    }
                })
            });
        return match result {
            Ok(code) => ExitCode::from(code),
            Err(e) => {
                logging::Logger(0).log(jumeaux::notification::Verbosity::Error, e.to_string());
                ExitCode::FAILURE
            }
        };
    }
    let command = match cli.command {
        Command::Server(_) => "server",
        Command::Viewer(_) => "viewer",
        _ => unreachable!(),
    };
    eprintln!("{command}: 未実装");
    ExitCode::FAILURE
}

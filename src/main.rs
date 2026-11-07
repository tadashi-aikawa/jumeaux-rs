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
        let result = jumeaux::config::Config::from_run_args(args)
            .map_err(jumeaux::notification::Failure::from)
            .and_then(|config| {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| jumeaux::compat::Error::new("RuntimeError", e.to_string()))?;
                runtime.block_on(jumeaux::addons::log2reqs::prepare(
                    &config,
                    &jumeaux::notification::Context::from_env(),
                ))
            });
        match result {
            Ok(result) => {
                print_logs(&result.logs);
                if let Some(code) = result.exit_code {
                    return ExitCode::from(code);
                }
            }
            Err(error) => {
                print_logs(&error.logs);
                print_logs(&[jumeaux::notification::Log::new(
                    jumeaux::notification::Level::Error,
                    error.error.message,
                )]);
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

fn print_logs(logs: &[jumeaux::notification::Log]) {
    use jumeaux::notification::Level;
    for log in logs {
        match log.level {
            Level::Info => eprintln!("{}", log.message),
            Level::Warning => eprintln!("\x1b[33m[WARNING] {}\x1b[0m", log.message),
            Level::Error => eprintln!("\x1b[31m[ ERROR ] {}\x1b[0m", log.message),
        }
    }
}

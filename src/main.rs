mod cli;

use std::process::ExitCode;

use clap::Parser;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse();
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

use clap::{ArgAction, Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "jumeaux", version, about, infer_long_args = true, override_help = include_str!("../assets/help.txt"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run Jumeaux
    Run(RunArgs),
    /// Retry Jumeaux from report
    Retry(RetryArgs),
    /// Initialize a project template for Jumeaux
    Init(InitArgs),
    /// Boot mock API server
    Server(ServerArgs),
    /// Boot Jumeaux Viewer
    Viewer(ViewerArgs),
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Files written requests
    #[arg(required = true, value_name = "files")]
    pub files: Vec<String>,

    /// Configuration files
    #[arg(long, value_name = "yaml")]
    pub config: Vec<String>,

    /// Skip add-ons loading whose tags have one of this
    #[arg(long, value_name = "skip_addon_tag")]
    pub skip_addon_tag: Vec<String>,

    #[command(flatten)]
    pub options: ReportArgs,
}

#[derive(Debug, Args)]
pub struct RetryArgs {
    /// Report for retry
    #[arg(value_name = "report")]
    pub report: String,

    #[command(flatten)]
    pub options: ReportArgs,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    /// The title of report
    #[arg(long, value_name = "title")]
    pub title: Option<String>,

    /// The description of report
    #[arg(long, value_name = "description")]
    pub description: Option<String>,

    /// Tags
    #[arg(long, value_name = "tag")]
    pub tag: Vec<String>,

    /// The number of threads in challenge
    #[arg(long, value_name = "threads", allow_negative_numbers = true)]
    pub threads: Option<i64>,

    /// The number of processes in challenge
    #[arg(long, value_name = "processes", allow_negative_numbers = true)]
    pub processes: Option<i64>,

    /// The max number of retries which accesses to API
    #[arg(long, value_name = "max_retries", allow_negative_numbers = true)]
    pub max_retries: Option<i64>,

    #[command(flatten)]
    pub logging: LoggingArgs,
}

#[derive(Debug, Args)]
pub struct LoggingArgs {
    /// Logger level (-v, -vv, or -vvv)
    #[arg(short = 'v', action = ArgAction::Count, value_parser = clap::value_parser!(u8).range(0..=3))]
    pub v: u8,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Initialize template name
    #[arg(value_name = "name")]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    /// Running port
    #[arg(
        long,
        value_name = "port",
        default_value_t = 8000,
        allow_negative_numbers = true
    )]
    pub port: i64,

    #[command(flatten)]
    pub logging: LoggingArgs,
}

#[derive(Debug, Args)]
pub struct ViewerArgs {
    /// Running port
    #[arg(
        long,
        value_name = "port",
        default_value_t = 5500,
        allow_negative_numbers = true
    )]
    pub port: i64,

    /// Directory which has responses
    #[arg(long, value_name = "responses_dir", default_value = "responses")]
    pub responses_dir: String,
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, error::ErrorKind};

    use super::*;

    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn run_accepts_all_python_options() {
        let cli = Cli::try_parse_from([
            "jumeaux",
            "run",
            "requests.json",
            "requests.csv",
            "--config=base.yml",
            "--config",
            "override.yml",
            "--title",
            "API comparison",
            "--description=Compare two APIs",
            "--tag=smoke",
            "--tag",
            "daily",
            "--skip-addon-tag=slow",
            "--skip-addon-tag",
            "external",
            "--threads=4",
            "--processes",
            "2",
            "--max-retries=3",
            "-vvv",
        ])
        .unwrap();

        let Command::Run(args) = cli.command else {
            panic!("expected run");
        };
        assert_eq!(args.files, ["requests.json", "requests.csv"]);
        assert_eq!(args.config, ["base.yml", "override.yml"]);
        assert_eq!(args.skip_addon_tag, ["slow", "external"]);
        assert_eq!(args.options.title.as_deref(), Some("API comparison"));
        assert_eq!(
            args.options.description.as_deref(),
            Some("Compare two APIs")
        );
        assert_eq!(args.options.tag, ["smoke", "daily"]);
        assert_eq!(args.options.threads, Some(4));
        assert_eq!(args.options.processes, Some(2));
        assert_eq!(args.options.max_retries, Some(3));
        assert_eq!(args.options.logging.v, 3);
    }

    #[test]
    fn retry_accepts_all_python_options() {
        let cli = Cli::try_parse_from([
            "jumeaux",
            "retry",
            "responses/latest/report.json",
            "--title=Retry",
            "--description",
            "Retry failures",
            "--tag=retry",
            "--tag=nightly",
            "--threads=8",
            "--processes=2",
            "--max-retries=5",
            "-vv",
        ])
        .unwrap();

        let Command::Retry(args) = cli.command else {
            panic!("expected retry");
        };
        assert_eq!(args.report, "responses/latest/report.json");
        assert_eq!(args.options.title.as_deref(), Some("Retry"));
        assert_eq!(args.options.description.as_deref(), Some("Retry failures"));
        assert_eq!(args.options.tag, ["retry", "nightly"]);
        assert_eq!(args.options.threads, Some(8));
        assert_eq!(args.options.processes, Some(2));
        assert_eq!(args.options.max_retries, Some(5));
        assert_eq!(args.options.logging.v, 2);
    }

    #[test]
    fn omitted_run_and_retry_options_remain_unset() {
        let cli = Cli::try_parse_from(["jumeaux", "run", "requests"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run");
        };
        assert!(args.config.is_empty());
        assert!(args.skip_addon_tag.is_empty());
        assert_report_options_unset(&args.options);

        let cli = Cli::try_parse_from(["jumeaux", "retry", "report.json"]).unwrap();
        let Command::Retry(args) = cli.command else {
            panic!("expected retry");
        };
        assert_report_options_unset(&args.options);
    }

    fn assert_report_options_unset(options: &ReportArgs) {
        // Python's [def: ...] annotations are not docopt [default: ...] values.
        // Preserve missing values so the executor can merge configuration later.
        assert_eq!(options.title, None);
        assert_eq!(options.description, None);
        assert!(options.tag.is_empty());
        assert_eq!(options.threads, None);
        assert_eq!(options.processes, None);
        assert_eq!(options.max_retries, None);
        assert_eq!(options.logging.v, 0);
    }

    #[test]
    fn init_name_is_optional() {
        for name in [None, Some("simple")] {
            let mut argv = vec!["jumeaux", "init"];
            argv.extend(name);
            let cli = Cli::try_parse_from(argv).unwrap();
            let Command::Init(args) = cli.command else {
                panic!("expected init");
            };
            assert_eq!(args.name.as_deref(), name);
        }
    }

    #[test]
    fn server_and_viewer_preserve_defaults_and_accept_overrides() {
        let cli = Cli::try_parse_from(["jumeaux", "server"]).unwrap();
        let Command::Server(args) = cli.command else {
            panic!("expected server");
        };
        assert_eq!(args.port, 8000);
        assert_eq!(args.logging.v, 0);

        let cli = Cli::try_parse_from(["jumeaux", "server", "--port", "8080", "-v"]).unwrap();
        let Command::Server(args) = cli.command else {
            panic!("expected server");
        };
        assert_eq!(args.port, 8080);
        assert_eq!(args.logging.v, 1);

        let cli = Cli::try_parse_from(["jumeaux", "viewer"]).unwrap();
        let Command::Viewer(args) = cli.command else {
            panic!("expected viewer");
        };
        assert_eq!(args.port, 5500);
        assert_eq!(args.responses_dir, "responses");

        let cli = Cli::try_parse_from([
            "jumeaux",
            "viewer",
            "--port=5555",
            "--responses-dir=custom-responses",
        ])
        .unwrap();
        let Command::Viewer(args) = cli.command else {
            panic!("expected viewer");
        };
        assert_eq!(args.port, 5555);
        assert_eq!(args.responses_dir, "custom-responses");
    }

    #[test]
    fn run_and_retry_require_their_positional_arguments() {
        for command in ["run", "retry"] {
            let error = Cli::try_parse_from(["jumeaux", command]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        }
    }

    #[test]
    fn numeric_options_reject_non_integer_values() {
        for option in ["--threads", "--processes", "--max-retries"] {
            for command in ["run", "retry"] {
                let error = Cli::try_parse_from(["jumeaux", command, "input", option, "invalid"])
                    .unwrap_err();
                assert_eq!(error.kind(), ErrorKind::ValueValidation);
            }
        }
        for command in ["server", "viewer"] {
            let error = Cli::try_parse_from(["jumeaux", command, "--port", "invalid"]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn verbosity_accepts_zero_through_three_occurrences() {
        for command in ["run", "retry", "server"] {
            for count in 0..=3 {
                let mut argv = vec!["jumeaux", command];
                if command != "server" {
                    argv.push("input");
                }
                argv.extend(std::iter::repeat_n("-v", count));
                let cli = Cli::try_parse_from(argv).unwrap();
                let v = match cli.command {
                    Command::Run(args) => args.options.logging.v,
                    Command::Retry(args) => args.options.logging.v,
                    Command::Server(args) => args.logging.v,
                    _ => panic!("expected run, retry, or server"),
                };
                assert_eq!(usize::from(v), count);
            }
        }
    }

    #[test]
    fn verbosity_rejects_more_than_three_occurrences() {
        for command in ["run", "retry", "server"] {
            for flags in [vec!["-vvvv"], vec!["-vv", "-vv"], vec!["-v"; 5]] {
                let mut argv = vec!["jumeaux", command];
                if command != "server" {
                    argv.push("input");
                }
                argv.extend(flags);
                let error = Cli::try_parse_from(argv).unwrap_err();
                assert_eq!(error.kind(), ErrorKind::ValueValidation);
            }
        }
    }

    #[test]
    fn subcommands_accept_unambiguous_long_option_prefixes() {
        let cli = Cli::try_parse_from(["jumeaux", "retry", "r", "--desc=x", "--thr=1"]).unwrap();
        let Command::Retry(args) = cli.command else {
            panic!("expected retry");
        };
        assert_eq!(args.options.description.as_deref(), Some("x"));
        assert_eq!(args.options.threads, Some(1));

        let cli = Cli::try_parse_from(["jumeaux", "run", "r", "--conf=c.yml"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run");
        };
        assert_eq!(args.config, ["c.yml"]);

        let cli = Cli::try_parse_from(["jumeaux", "server", "--po=8080"]).unwrap();
        let Command::Server(args) = cli.command else {
            panic!("expected server");
        };
        assert_eq!(args.port, 8080);

        let cli = Cli::try_parse_from(["jumeaux", "viewer", "--res=custom"]).unwrap();
        let Command::Viewer(args) = cli.command else {
            panic!("expected viewer");
        };
        assert_eq!(args.responses_dir, "custom");
    }

    #[test]
    fn ambiguous_long_option_prefixes_are_rejected() {
        let error = Cli::try_parse_from(["jumeaux", "retry", "r", "--t=1"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }

    #[test]
    fn negative_integer_values_are_preserved() {
        let cli = Cli::try_parse_from(["jumeaux", "retry", "r", "--threads", "-1"]).unwrap();
        let Command::Retry(args) = cli.command else {
            panic!("expected retry");
        };
        assert_eq!(args.options.threads, Some(-1));
    }

    #[test]
    fn help_and_version_exit_successfully_without_required_arguments() {
        for flag in ["--help", "-h"] {
            let error = Cli::try_parse_from(["jumeaux", flag]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            assert_eq!(error.exit_code(), 0);
            for command in ["run", "retry", "init", "server", "viewer"] {
                let error = Cli::try_parse_from(["jumeaux", command, flag]).unwrap_err();
                assert_eq!(error.kind(), ErrorKind::DisplayHelp);
                assert_eq!(error.exit_code(), 0);
            }
        }
        let error = Cli::try_parse_from(["jumeaux", "--version"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert_eq!(error.exit_code(), 0);
        assert_eq!(error.to_string(), "jumeaux 0.1.0\n");
    }
}

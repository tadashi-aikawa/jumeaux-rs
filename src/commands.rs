use crate::logging::{Logger, config_yaml};
use jumeaux::{
    addons::{
        r#final, log2reqs,
        payload::{FinalPayload, FinalReference},
        reqs2reqs,
    },
    cli::{RetryArgs, RunArgs},
    compat::{
        Error, Result,
        encoding::{self, Errors},
    },
    config::{Config, MergedArgs},
    executor,
    http::HttpTransport,
    models::{Report, Request},
    notification::{Context, Verbosity},
};
use std::sync::Arc;

pub async fn run(args: &RunArgs) -> Result<u8> {
    let config = Config::from_run_args(args)?;
    let merged = MergedArgs {
        files: Some(args.files.clone()),
        ..MergedArgs::from_cli(&args.options)
    };
    execute(config, merged, None, None, Logger(args.options.logging.v)).await
}
pub async fn retry(args: &RetryArgs) -> Result<u8> {
    let logger = Logger(args.options.logging.v);
    let bytes = std::fs::read(&args.report)
        .map_err(|e| Error::new("IOError", format!("{}: {e}", args.report)))?;
    // Python json.loadのbytes入口と同じBOM検出。UTF-8以外は保存encodingを判別できるBOMが必要。
    let encoding =
        if bytes.starts_with(&[0xff, 0xfe, 0, 0]) || bytes.starts_with(&[0, 0, 0xfe, 0xff]) {
            "utf-32"
        } else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
            "utf-16"
        } else if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            "utf-8-sig"
        } else {
            "utf-8"
        };
    let text = encoding::decode(&bytes, encoding, Errors::Strict)?;
    let value = jumeaux::compat::json::parse(&text)?;
    let (report, warnings) = Report::from_value_with_warnings(value)?;
    for w in warnings {
        logger.log(Verbosity::Warning, w);
    }
    let (mut config, warnings) = Config::from_report(&report)?;
    for w in warnings {
        logger.log(Verbosity::Warning, w);
    }
    let merged = MergedArgs::from_retry_args(&args.options);
    config.merge_args(&merged);
    let requests = report.trials.iter().map(|t| t.to_request()).collect();
    execute(config, merged, Some(report.key), Some(requests), logger).await
}
async fn execute(
    config: Config,
    args: MergedArgs,
    retry_hash: Option<String>,
    requests: Option<Vec<Request>>,
    logger: Logger,
) -> Result<u8> {
    let pipeline = Arc::new(executor::Pipeline::build(&config)?);
    let final_pipeline = r#final::Pipeline::build(&config.addons.final_addons)?;
    let key = executor::hash_from_args(&args.args_str()?);
    let context = Context::from_env();
    let requests = match requests {
        Some(requests) => requests,
        None => {
            let mut requests = Vec::new();
            for file in config
                .input_files
                .as_ref()
                .ok_or_else(|| Error::new("RequiredError", "input_files is required"))?
            {
                let bytes = tokio::fs::read(file)
                    .await
                    .map_err(|e| Error::new("IOError", format!("{file}: {e}")))?;
                let loaded = log2reqs::read(&config.addons.log2reqs, &bytes)?;
                if config.addons.log2reqs.name == "plain" && logger.0 >= 3 {
                    for (index, request) in loaded.iter().enumerate() {
                        logger.log(
                            Verbosity::Debug,
                            format!("[log2reqs/plain] ---- {index} ----"),
                        );
                        logger.log(
                            Verbosity::Debug,
                            format!("[log2reqs/plain] [path] {}", request.path),
                        );
                        logger.log(
                            Verbosity::Debug,
                            format!(
                                "[log2reqs/plain] [qs] ({}) {}",
                                request.url_encoding,
                                crate::logging::repr(&serde_json::json!(request.qs))
                            ),
                        );
                    }
                }
                requests.extend(loaded);
            }
            requests
        }
    };
    logger.log(Verbosity::Normal, include_str!("../assets/start.txt"));
    logger.log(
        Verbosity::Normal,
        format!("Version: {}", env!("CARGO_PKG_VERSION")),
    );
    if logger.0 >= 1 {
        logger.log(
            Verbosity::Verbose,
            r"
         ____             __ _
__/\__  / ___|___  _ __  / _(_) __ _  __/\__
\    / | |   / _ \| '_ \| |_| |/ _` | \    /
/_  _\ | |__| (_) | | | |  _| | (_| | /_  _\\
  \/    \____\___/|_| |_|_| |_|\__, |   \/
                               |___/
",
        );
        logger.log(
            Verbosity::Verbose,
            "Merge with yaml files or report, and args",
        );
        logger.log(Verbosity::Verbose, "----");
        logger.log(Verbosity::Verbose, config_yaml(&config)?);
    }
    let prepared = reqs2reqs::Pipeline::build(&config.addons.reqs2reqs)?
        .exec(requests, &config, &context)
        .await;
    let prepared = match prepared {
        Ok(p) => p,
        Err(e) => {
            logger.logs(&e.logs);
            logger.log(Verbosity::Error, e.error.message);
            return Ok(1);
        }
    };
    logger.logs(&prepared.logs);
    if let Some(code) = prepared.exit_code {
        return Ok(code);
    }
    let c = config.concurrency()?;
    if logger.0 >= 3 && config.max_retries != 0 {
        for _ in 0..2 {
            logger.log(Verbosity::Normal,format!("Converted retries value: {} -> Retry(total={}, connect=None, read=None, redirect=None, status=None)",config.max_retries,config.max_retries));
        }
    }
    logger.log(Verbosity::Normal,format!("\n--------------------------------------------------------------------------------\n| {}\n| ({key})\n--------------------------------------------------------------------------------\n| {}\n--------------------------------------------------------------------------------\n| - {} processes\n| - {} threads\n--------------------------------------------------------------------------------\n    ",config.title.as_deref().unwrap_or("No title"),config.description.as_deref().unwrap_or("None"),c.processes,c.threads));
    let transport = Arc::new(HttpTransport::new(
        &config.one,
        &config.other,
        config.max_retries,
    )?);
    let execution = pipeline
        .exec_with_progress(transport, prepared.requests, &key, None, move |p| {
            logger.progress(p)
        })
        .await
        .map_err(|e| e.error)?;
    let report = jumeaux::report::create_report(&config, execution, key, retry_hash);
    let stdout = config.output.stdout()?;
    let result = final_pipeline
        .exec(
            FinalPayload {
                report,
                output_summary: config.output,
            },
            &FinalReference {
                notifiers: config.notifiers,
            },
            &context,
            |s| stdout.write_text(s),
        )
        .await;
    stdout.flush()?;
    match result {
        Ok(result) => {
            for log in result.logs {
                logger.log(log.level, log.message);
            }
            Ok(result.exit_code.unwrap_or(0))
        }
        Err(e) => {
            for log in e.logs {
                logger.log(log.level, log.message);
            }
            Err(e.error)
        }
    }
}

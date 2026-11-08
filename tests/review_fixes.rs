#[path = "support/migration.rs"]
mod migration;

use jumeaux::{
    addons::{self, Layer},
    compat,
    config::{self, Config, HeaderJudgementSettings},
    models::*,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../compat/fixtures/review-fixes.json")).unwrap()
}
fn model<T: DeserializeOwned + Serialize>(input: &Value) -> compat::Result<Value> {
    serde_json::from_value::<T>(input.clone())
        .map(|v| serde_json::to_value(v).unwrap())
        .map_err(|e| compat::Error::new("ConfigError", e.to_string()))
}
fn error_class(error: &compat::Error) -> &str {
    if error.message.contains("unknown field") {
        "UnknownPropertiesError"
    } else if error.message.contains("missing field")
        || error.message.contains("invalid type: null")
    {
        "RequiredError"
    } else if error.message.contains("invalid type") {
        "InvalidTypeError"
    } else {
        error.kind
    }
}

#[test]
fn python_review_regressions() {
    let mut count = 0;
    let mut differences = 0;
    let mut disabled_features = 0;
    for case in cases() {
        let input = &case["input"];
        let actual = match case["operation"].as_str().unwrap() {
            "config" => {
                let paths: Vec<_> = input["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        root()
                            .join("compat/fixtures/config")
                            .join(p.as_str().unwrap())
                    })
                    .collect();
                config::load(&paths, &[]).map(|c| serde_json::to_value(c).unwrap())
            }
            "Request" => model::<Request>(input),
            "StatusCounts" => model::<StatusCounts>(input),
            "Notifier" => model::<Notifier>(input),
            "Report" => {
                let (report, warnings) = Report::from_value_with_warnings(input.clone()).unwrap();
                assert_eq!(
                    warnings.len(),
                    if case["name"] == "report-logger" {
                        1
                    } else {
                        7
                    }
                );
                assert!(warnings.iter().all(|w| w.starts_with("report.")));
                let (config, warnings) =
                    Config::from_report(&migration::without_viewer(&report)).unwrap();
                assert_eq!(warnings.len(), 1);
                assert!(warnings[0].contains("summary.comparison"));
                assert_eq!(config.comparison(), HeaderJudgementSettings::default());
                assert!(config.output.logger.is_none());
                // 通常のserde入口も同じ未知キー処理を通る。
                assert_eq!(
                    model::<Report>(input).unwrap(),
                    serde_json::to_value(&report).unwrap()
                );
                Ok(serde_json::to_value(report).unwrap())
            }
            "addon" => {
                let layer = Layer::from_name(input["layer"].as_str().unwrap()).unwrap();
                let addon: Addon =
                    serde_json::from_value(json!({"name":input["name"],"config":input["config"]}))
                        .unwrap();
                addons::parse_config(layer, &addon, "addon")
                    .map(|c| serde_json::to_value(c).unwrap())
            }
            "process-output" => continue,
            op => panic!("unknown operation {op}"),
        };
        let expected = case.get("rust_expected").unwrap_or(&case["expected"]);
        if !cfg!(feature = "miroir") && input["name"] == "miroir" && expected.get("value").is_some()
        {
            let error = actual.unwrap_err();
            assert_eq!(error.kind, "ConfigError");
            assert!(error.message.contains("--features miroir"));
            disabled_features += 1;
            count += 1;
            continue;
        }
        if case.get("rust_expected").is_some() {
            assert_ne!(expected, &case["expected"]);
            assert!(
                case["difference"]
                    .as_str()
                    .unwrap()
                    .contains("docs/compat-spec.md")
            );
            differences += 1;
        }
        let actual = match actual {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error_class(&error)}),
        };
        assert_eq!(&actual, expected, "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 179);
    assert_eq!(differences, 5);
    assert_eq!(
        disabled_features,
        if cfg!(feature = "miroir") { 0 } else { 2 }
    );
    eprintln!(
        "Review fixtures: {count}/{count}; Python matches {}; explicit differences {differences}; disabled features {disabled_features}",
        count - differences - disabled_features
    );
}

#[test]
fn errors_keep_source_and_do_not_double_dot() {
    let dir = root().join("compat/fixtures/config");
    let error = config::load(
        &[dir.join("base.yml"), dir.join("missing-addon-name.yml")],
        &[],
    )
    .unwrap_err();
    assert!(error.message.contains("missing-addon-name.yml"), "{error}");
    assert!(
        error.message.contains("addons.log2reqs: missing field"),
        "{error}"
    );
    assert!(!error.message.contains(".."), "{error}");
    let error = config::load(
        &[dir.join("base.yml"), dir.join("replaced-invalid.yml")],
        &[],
    )
    .unwrap_err();
    assert!(error.message.contains("replaced-invalid.yml"), "{error}");
}

#[test]
fn retry_round_trips_comparison_and_old_reports_default() {
    let case = cases()
        .into_iter()
        .find(|c| c["name"] == "report-logger")
        .unwrap();
    let mut report: Report = serde_json::from_value(case["input"].clone()).unwrap();
    assert_eq!(
        Config::from_report(&migration::without_viewer(&report))
            .unwrap()
            .0
            .comparison(),
        HeaderJudgementSettings::default()
    );
    let headers = HeaderJudgementSettings {
        judge_response_header: true,
        ignore_response_header_keys: vec!["X-Test".into()],
    };
    report.summary.comparison = Some(headers.clone());
    let mut bytes = Vec::new();
    report
        .summary
        .output
        .write_report(&mut bytes, &report)
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["summary"]["comparison"],
        serde_json::to_value(&headers).unwrap()
    );
    let reread: Report = serde_json::from_slice(&bytes).unwrap();
    let (config, warnings) = Config::from_report(&migration::without_viewer(&reread)).unwrap();
    assert_eq!(config.comparison(), headers);
    assert!(warnings.is_empty());
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn process_streams_keep_bom_and_logs_remain_utf8() {
    let mut count = 0;
    for case in cases()
        .into_iter()
        .filter(|c| c["operation"] == "process-output")
    {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "output_stream_child", "--nocapture"])
            .env(
                "JUMEAUX_TEST_OUTPUT_ENCODING",
                case["input"]["encoding"].as_str().unwrap(),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected = &case["expected"]["value"];
        // libtestの進捗はstdoutへ出るため、固定した本文のbytesを切り出す。
        let actual = hex(&output.stdout);
        let expected_stdout = expected["stdout_hex"].as_str().unwrap();
        assert!(actual.contains(expected_stdout), "{actual}");
        assert_eq!(actual.matches(&expected_stdout[..6]).count(), 1);
        assert_eq!(hex(&output.stderr), expected["stderr_hex"]);
        count += 1;
    }
    assert_eq!(count, 2);
    eprintln!("Process output Python fixtures: {count}/{count}");
}

#[test]
fn output_stream_child() {
    let Ok(encoding) = std::env::var("JUMEAUX_TEST_OUTPUT_ENCODING") else {
        return;
    };
    let output = OutputSummary {
        response_dir: "out".into(),
        encoding,
        logger: None,
    };
    let first_stdout = output.stdout().unwrap();
    let first_stderr = output.stderr().unwrap();
    for chunk in ["日本", "語\n"] {
        // 最初のハンドルを持ったまま再取得する。
        let stdout = output.stdout().unwrap();
        stdout.write_text(chunk).unwrap();
        stdout.flush().unwrap();
        let stderr = output.stderr().unwrap();
        stderr.write_text(chunk).unwrap();
        stderr.flush().unwrap();
    }
    first_stdout.flush().unwrap();
    first_stderr.flush().unwrap();
    eprintln!("ログ");
}

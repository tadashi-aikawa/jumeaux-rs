use jumeaux::{
    config::{self, Config, HeaderJudgementSettings},
    models::{OutputSummary, Report},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../compat/fixtures/review-round2.json")).unwrap()
}

#[test]
fn round2_python_fixtures_and_report_performance() {
    let mut count = 0;
    for case in cases() {
        let input = &case["input"];
        let expected = case.get("rust_expected").unwrap_or(&case["expected"]);
        let actual = match case["operation"].as_str().unwrap() {
            "config" => {
                let paths: Vec<_> = input["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("compat/fixtures/config")
                            .join(p.as_str().unwrap())
                    })
                    .collect();
                match config::load(&paths, &[]) {
                    Ok(config) => json!({"value": serde_json::to_value(config).unwrap()}),
                    Err(error) if case["name"].as_str().unwrap().starts_with("invalid-merge") => {
                        assert!(error.message.contains(".<<:"), "{error}");
                        json!({"error": "ConstructorError"})
                    }
                    Err(error) => json!({"error": error.kind}),
                }
            }
            "Report" => {
                let (report, warnings) = Report::from_value_with_warnings(input.clone()).unwrap();
                assert_eq!(warnings.len(), 20, "{warnings:?}");
                assert!(
                    warnings
                        .iter()
                        .all(|w| w.ends_with(".future: ignored unknown key"))
                );
                json!({"value": serde_json::to_value(report).unwrap()})
            }
            "report-repeat" => {
                let size = input["count"].as_u64().unwrap() as usize;
                let mut source = input["report"].clone();
                source["trials"] = Value::Array(vec![source["trials"][0].clone(); size]);
                let text = serde_json::to_string(&source).unwrap();
                let start = Instant::now();
                let source: Value = serde_json::from_str(&text).unwrap();
                let parse_elapsed = start.elapsed();
                let start = Instant::now();
                let (report, warnings) = Report::from_value_with_warnings(source).unwrap();
                let read_elapsed = start.elapsed();
                assert_eq!(warnings.len(), 16 + 4 * size);
                assert!(
                    read_elapsed < Duration::from_secs(1),
                    "report read: {read_elapsed:?}"
                );
                eprintln!(
                    "8000 trials: JSON parse {parse_elapsed:?}, report read {read_elapsed:?}, total {:?}",
                    parse_elapsed + read_elapsed
                );
                json!({"value": {"count": report.trials.len(),
                    "first": serde_json::to_value(&report.trials[0]).unwrap(),
                    "last": serde_json::to_value(report.trials.last().unwrap()).unwrap()}})
            }
            operation => panic!("unknown operation: {operation}"),
        };
        assert_eq!(&actual, expected, "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 10);
    eprintln!("Round 2 fixtures: {count}/{count}; Python matches 9; explicit difference 1");
}

#[test]
fn warnings_are_returned_and_comparison_is_preserved() {
    let old: Value =
        serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap();
    let (mut report, warnings) = Report::from_value_with_warnings(old).unwrap();
    assert!(warnings.is_empty());
    report.summary.output.logger = Some(json!({"level": "debug"}));
    let (_, warnings) = Config::from_report(&report).unwrap();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].starts_with("report.summary.comparison:"));
    assert!(warnings[1].starts_with("report.summary.output.logger:"));
    let (_, warnings) =
        Config::from_report_with_headers(&report, HeaderJudgementSettings::default()).unwrap();
    assert_eq!(warnings.len(), 1);
    let mut source = serde_json::to_value(&report).unwrap();
    source["summary"]["comparison"] = json!({
        "judge_response_header": true,
        "ignore_response_header_keys": ["X-Test"],
        "future": 1
    });
    let (report, warnings) = Report::from_value_with_warnings(source).unwrap();
    assert_eq!(warnings.len(), 2);
    let (config, warnings) = Config::from_report(&report).unwrap();
    assert!(warnings.is_empty());
    assert!(config.judge_response_header);
    assert_eq!(config.ignore_response_header_keys, ["X-Test"]);
}

#[test]
fn invalid_known_report_fields_still_fail() {
    let mut source = cases()
        .into_iter()
        .find(|c| c["operation"] == "Report")
        .unwrap()["input"]
        .clone();
    source["trials"][0]["seq"] = json!("invalid");
    let error = Report::from_value_with_warnings(source.clone()).unwrap_err();
    assert!(error.message.contains("report.trials[0].seq"));
    source["trials"][0].as_object_mut().unwrap().remove("seq");
    let error = Report::from_value_with_warnings(source).unwrap_err();
    assert!(error.message.contains("missing field `seq`"));
}

#[test]
fn shared_handles_are_reentrant_and_parallel_and_library_is_silent() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "parallel_output_child", "--nocapture"])
        .env("JUMEAUX_ROUND2_OUTPUT_CHILD", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("output handle reacquisition deadlocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut expected = "並列".repeat(16).into_bytes();
    let utf16 = jumeaux::compat::encoding::encode(
        "異",
        "utf-16",
        jumeaux::compat::encoding::Errors::Strict,
    )
    .unwrap();
    expected.extend_from_slice(&utf16[2..]); // pipeのUTF-16はBOMなし。
    expected.extend_from_slice("戻".as_bytes());
    assert_eq!(output.stderr, expected);
}

#[test]
fn parallel_output_child() {
    if std::env::var_os("JUMEAUX_ROUND2_OUTPUT_CHILD").is_none() {
        return;
    }
    let output = OutputSummary {
        response_dir: "out".into(),
        encoding: "utf8".into(),
        logger: None,
    };
    let first = output.stderr().unwrap();
    let second = output.stderr().unwrap();
    let stdout = output.stdout().unwrap();
    let another = OutputSummary {
        encoding: "utf-16".into(),
        ..output.clone()
    };
    let other_stderr = another.stderr().unwrap();
    let _other_stdout = another.stdout().unwrap();
    let mut threads = Vec::new();
    for i in 0..16 {
        let writer = if i % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        threads.push(std::thread::spawn(move || {
            writer.write_text("並列").unwrap()
        }));
    }
    for thread in threads {
        thread.join().unwrap();
    }
    first.flush().unwrap();
    second.flush().unwrap();
    other_stderr.write_text("異").unwrap();
    other_stderr.flush().unwrap();
    first.write_text("戻").unwrap();
    first.flush().unwrap();
    stdout.flush().unwrap();
    let source = cases()
        .into_iter()
        .find(|c| c["operation"] == "Report")
        .unwrap()["input"]
        .clone();
    let _: Report = serde_json::from_value(source).unwrap();
    let report: Report =
        serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap();
    let _ = Config::from_report(&report).unwrap();
    let mut report = report;
    report.summary.output.logger = Some(json!({}));
    let _ = Config::from_report_with_headers(&report, HeaderJudgementSettings::default()).unwrap();
}

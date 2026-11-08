use jumeaux::{
    config::{Config, MergedArgs},
    executor::Execution,
    models::Report,
    report::create_report,
};
use serde_json::json;

#[test]
fn aggregates_every_baseline_report() {
    let mut count = 0;
    for entry in std::fs::read_dir("compat/python-baseline").unwrap() {
        let path = entry.unwrap().path().join("report.json");
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let encoding = if bytes.starts_with(&[0xff, 0xfe]) {
            "utf-16"
        } else {
            "utf-8"
        };
        let text = jumeaux::compat::encoding::decode(
            &bytes,
            encoding,
            jumeaux::compat::encoding::Errors::Strict,
        )
        .unwrap();
        let expected: Report = serde_json::from_str(&text).unwrap();
        let (mut config, _) = Config::from_report(&expected).unwrap();
        config.tags = Some(expected.summary.tags.clone());
        let execution = Execution {
            trials: expected.trials.clone(),
            logs: vec![],
            started_at: chrono::DateTime::parse_from_rfc3339(&expected.summary.time.start).unwrap(),
            ended_at: chrono::DateTime::parse_from_rfc3339(&expected.summary.time.end).unwrap(),
            concurrency: expected.summary.concurrency.clone(),
            comparison: config.comparison(),
        };
        let actual = create_report(
            &config,
            execution,
            expected.key.clone(),
            expected.retry_hash.clone(),
        );
        let mut actual = serde_json::to_value(actual).unwrap();
        actual["version"] = json!(expected.version);
        assert_eq!(
            actual["summary"]["comparison"],
            serde_json::to_value(config.comparison()).unwrap()
        );
        actual["summary"]
            .as_object_mut()
            .unwrap()
            .remove("comparison");
        assert_eq!(
            actual,
            serde_json::to_value(expected).unwrap(),
            "{}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(count, 31);
}

#[test]
fn args_string_keeps_zero_and_empty_lists_but_omits_missing_and_empty_strings() {
    assert_eq!(MergedArgs::default().args_str().unwrap(), "{}");
    let args = MergedArgs {
        files: Some(vec!["日本語".into(), "a".into()]),
        tag: Some(vec![]),
        title: Some("".into()),
        threads: Some(0),
        processes: Some(0),
        max_retries: Some(0),
        description: Some("改行\n\"".into()),
    };
    assert_eq!(
        args.args_str().unwrap(),
        "{\"description\": \"改行\\n\\\"\",\"files\": [\"日本語\",\"a\"],\"max_retries\": 0,\"processes\": 0,\"tag\": [],\"threads\": 0}"
    );
}

#[test]
fn empty_report_and_python_timedelta_seconds() {
    let config: Config = serde_json::from_value(json!({
        "one":{"name":"a", "host":"http://a"}, "other":{"name":"b", "host":"http://b"},
        "output":{"response_dir":"responses"}, "addons":{"log2reqs":{"name":"plain"}}
    }))
    .unwrap();
    let report = create_report(
        &config,
        Execution {
            trials: vec![],
            logs: vec![],
            started_at: "2026-01-01T00:00:00+09:00".parse().unwrap(),
            ended_at: "2026-01-02T00:00:02.123456+09:00".parse().unwrap(),
            concurrency: config.concurrency().unwrap(),
            comparison: config.comparison(),
        },
        "key".into(),
        Some("old".into()),
    );
    assert_eq!(report.title, "No title");
    assert_eq!(report.summary.time.elapsed_sec, 2);
    assert_eq!(report.summary.time.start, "2026-01-01T00:00:00+09:00");
    assert_eq!(report.summary.time.end, "2026-01-02T00:00:02.123456+09:00");
    assert_eq!(
        serde_json::to_value(report.summary.status).unwrap(),
        json!({"same":0,"different":0,"failure":0})
    );
    assert_eq!(report.retry_hash.as_deref(), Some("old"));
}

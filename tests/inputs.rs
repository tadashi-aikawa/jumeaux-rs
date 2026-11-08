use jumeaux::{
    addons::{self, AddonConfig, Layer, reqs2reqs::Pipeline},
    compat::{Error, Result, template},
    config::Config,
    models::{Addon, Request},
    notification::{self, Context},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn config() -> Config {
    serde_json::from_value(json!({"one":{"name":"one","host":"http://one"},"other":{"name":"other","host":"http://other"},"output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"}},"title":"empty test","notifiers":{"local":{"type":"slack","channel":"#local"}}})).unwrap()
}
fn addon(input: &Value) -> Addon {
    serde_json::from_value(json!({"name":input["addon"],"config":input["config"]})).unwrap()
}
fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
        })
        .block_on(future)
}

#[test]
fn conditional_addons_reject_nonfinite_request_contexts() {
    let context = Context {
        webhook_url: None,
        thread_ts: None,
    };
    let config = config();
    for number in ["1e400".to_owned(), "-1e400".to_owned(), "9".repeat(400)] {
        let request = Request {
            path: "/".into(),
            json: Some(serde_json::from_str(&format!("{{\"a\":{number}}}")).unwrap()),
            ..Request::default()
        };
        for expression in ["json.a > 1", "False"] {
            for (name, settings) in [
                ("filter", json!({"when":expression})),
                (
                    "rename",
                    json!({"conditions":[{"when":expression,"name":"renamed"}]}),
                ),
                (
                    "replace",
                    json!({"items":[{"when":expression,"queries":{"x":["1"]}}]}),
                ),
            ] {
                let pipeline =
                    Pipeline::build(&[addon(&json!({"addon":name,"config":settings}))]).unwrap();
                let error =
                    block_on(pipeline.exec(vec![request.clone()], &config, &context)).unwrap_err();
                assert_eq!(error.error.kind, "ValueError", "{name}: {expression}");
                assert_eq!(error.error.message, "non-finite numbers are not supported");
            }
        }
        // add自身にwhenはない。追加されたRequestも後続の条件式の入口で検査する。
        let pipeline = Pipeline::build(&[
            addon(&json!({"addon":"add","config":{"reqs":[request]}})),
            addon(&json!({"addon":"filter","config":{"when":"json.a > 1"}})),
        ])
        .unwrap();
        let error = block_on(pipeline.exec(vec![], &config, &context)).unwrap_err();
        assert_eq!(error.error.kind, "ValueError");
    }
    let request: Request = serde_json::from_value(json!({"path":"/", "json":{"a":2}})).unwrap();
    let pipeline = Pipeline::build(&[addon(
        &json!({"addon":"filter","config":{"when":"json.a > 1"}}),
    )])
    .unwrap();
    assert_eq!(
        block_on(pipeline.exec(vec![request], &config, &context))
            .unwrap()
            .requests
            .len(),
        1
    );
}

fn run_cli(config: &Value, webhook: Option<&str>) -> std::process::Output {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "jumeaux-inputs-review-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("requests");
    std::fs::write(&file, "/discarded\n").unwrap();
    let path = directory.join("config.json");
    std::fs::write(&path, serde_json::to_vec(config).unwrap()).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_jumeaux"));
    command
        .args([
            "run",
            file.to_str().unwrap(),
            "--config",
            path.to_str().unwrap(),
        ])
        .env_remove("SLACK_INCOMING_WEBHOOKS_URL")
        .env_remove("SLACK_THREAD_TS")
        .env("NO_PROXY", "127.0.0.1,localhost");
    if let Some(url) = webhook {
        command.env("SLACK_INCOMING_WEBHOOKS_URL", url);
    }
    let result = command.output().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    result
}

/// 外部Slackへの接続を避け、送信されたHTTP本文とContent-Typeを記録する。
fn receiver(status: u16) -> (Context, mpsc::Receiver<Value>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/webhook", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // macOSではaccept先へO_NONBLOCKが引き継がれる。受信待ちは専用スレッドで行う。
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    let end = loop {
                        let n = stream.read(&mut buffer).unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                        if let Some(pos) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                            break pos + 4;
                        }
                    };
                    let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                    let header = |key: &str| {
                        headers
                            .lines()
                            .filter_map(|l| l.split_once(':'))
                            .find(|(k, _)| k.eq_ignore_ascii_case(key))
                            .map(|(_, v)| v.trim().to_owned())
                    };
                    let len: usize = header("content-length").unwrap().parse().unwrap();
                    while bytes.len() < end + len {
                        let n = stream.read(&mut buffer).unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    let raw = std::str::from_utf8(&bytes[end..end + len]).unwrap();
                    assert_eq!(
                        header("user-agent").as_deref(),
                        Some(concat!("jumeaux/", env!("CARGO_PKG_VERSION")))
                    );
                    let body = if status == 204 { "" } else { "reply" };
                    let response = format!(
                        "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    if status != 0 {
                        stream.write_all(response.as_bytes()).unwrap();
                    }
                    tx.send(json!({"payload":serde_json::from_str::<Value>(raw).unwrap(),"raw":raw,"content_type":header("content-type")})).unwrap();
                    return;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return,
            }
        }
    });
    (
        Context {
            webhook_url: Some(url),
            thread_ts: None,
        },
        rx,
        handle,
    )
}

fn evaluate(input: &Value) -> Result<Value> {
    let layer = input["layer"].as_str().unwrap();
    if layer == "guard_cli" {
        let mut config = serde_json::to_value(config()).unwrap();
        config["notifiers"]["local"]["version"] = input["version"].clone();
        config["addons"]["reqs2reqs"] = json!([
            {"name":"repeat","config":{"times":0}},
            {"name":"empty_guard","config":input["config"]}
        ]);
        let result = run_cli(&config, None);
        assert!(result.stdout.is_empty());
        return Ok(
            json!({"exit_code":result.status.code(),"stderr":String::from_utf8(result.stderr).unwrap()}),
        );
    }
    if layer == "log2reqs" {
        return addons::log2reqs::read(&addon(input), &unhex(input["body_hex"].as_str().unwrap()))
            .map(|v| {
                if let Some(container) = input["scalar"].as_str() {
                    let value = serde_json::to_value(v).unwrap()[0][container]["value"].clone();
                    let kind = match &value {
                        Value::String(_) => "str",
                        Value::Bool(_) => "bool",
                        Value::Number(n) if n.is_i64() || n.is_u64() => "int",
                        Value::Number(_) => "float",
                        Value::Null => "NoneType",
                        _ => unreachable!(),
                    };
                    json!({"type":kind,"value":value})
                } else {
                    json!(v)
                }
            });
    }
    if layer == "notification" {
        let notifier = serde_json::from_value(input["notifier"].clone()).unwrap();
        notification::payload(
            &notifier,
            input["message"].as_str().unwrap(),
            &Context::default(),
        )?;
        let (mut context, rx, handle) = receiver(200);
        context.thread_ts = input["thread_ts"].as_str().map(str::to_owned);
        let outcome = block_on(notification::notify(
            &notifier,
            input["message"].as_str().unwrap(),
            &context,
        ))?;
        let mut received = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        handle.join().unwrap();
        received["error"] = json!(outcome.error);
        return Ok(received);
    }
    let requests: Vec<Request> = if let Some(count) = input["count"].as_u64() {
        (0..count)
            .map(|i| Request {
                path: format!("/{i}"),
                ..Request::default()
            })
            .collect()
    } else {
        serde_json::from_value(input["requests"].clone()).unwrap()
    };
    let addon = addon(input);
    let config = addons::parse_config(Layer::Reqs2Reqs, &addon, "reqs2reqs")?;
    if let AddonConfig::Reqs2ReqsShuffle(c) = config {
        return Ok(json!(
            c.apply(requests, input["seed"].as_u64())
                .into_iter()
                .map(|r| r.path)
                .collect::<Vec<_>>()
        ));
    }
    if let (AddonConfig::Reqs2ReqsReplace(c), Some(now)) = (&config, input["now"].as_str()) {
        let now = chrono::DateTime::parse_from_rfc3339(now).unwrap();
        return c
            .apply_with_datetime(requests, |v| template::parse_datetime_at(v, now))
            .map(|v| json!(v));
    }
    let pipeline = Pipeline::build(&[addon])?;
    let needs_receiver = input["addon"] == "empty_guard"
        && requests.is_empty()
        && input["config"]["notifies"]
            .as_array()
            .is_some_and(|n| !n.is_empty());
    let server = needs_receiver.then(|| receiver(200));
    let context = server.as_ref().map(|s| s.0.clone()).unwrap_or_default();
    let result =
        block_on(pipeline.exec(requests, &self::config(), &context)).map_err(|e| e.error)?;
    if let Some(exit_code) = result.exit_code {
        let notifications = if let Some((_, rx, handle)) = server {
            let record = rx.recv_timeout(Duration::from_secs(10)).unwrap();
            handle.join().unwrap();
            vec![record["payload"].clone()]
        } else {
            vec![]
        };
        Ok(json!({"exit_code":exit_code,"notifications":notifications}))
    } else {
        Ok(json!(result.requests))
    }
}

#[test]
fn python_input_and_notification_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/inputs.json")).unwrap();
    let mut failures = Vec::new();
    for case in &cases {
        let actual = match evaluate(&case["input"]) {
            Ok(v) => json!({"value":v}),
            Err(e) => json!({"error":e.kind}),
        };
        let expected = case.get("rust_expected").unwrap_or(&case["expected"]);
        if actual != *expected {
            failures.push(format!(
                "{}: expected {}, actual {}",
                case["name"], expected, actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} mismatches:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    let differences = cases
        .iter()
        .filter(|c| c.get("rust_expected").is_some())
        .count();
    println!(
        "{} / {} fixtures matched; {} accepted differences",
        cases.len(),
        cases.len(),
        differences
    );
}

#[test]
fn empty_guard_stops_before_following_addon_and_returns_warnings() {
    let addons: Vec<Addon> = serde_json::from_value(json!([
        {"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"{{ title }}"}]}},
        {"name":"add","config":{"reqs":[{"path":"/never"}]}}
    ]))
    .unwrap();
    let result = block_on(Pipeline::build(&addons).unwrap().exec(
        vec![],
        &config(),
        &Context::default(),
    ))
    .unwrap();
    assert_eq!(result.exit_code, Some(1));
    assert!(result.requests.is_empty());
    assert_eq!(result.logs.len(), 5);
    assert_eq!(result.logs[0].level, notification::Level::Warning);
    assert_eq!(
        result.logs[1],
        notification::Log::new(notification::Level::Info, "Notify using slack@v1")
    );
    assert!(result.notifications[0].skipped);
}

#[test]
fn notification_http_errors_are_returned() {
    let notifier = serde_json::from_value(json!({"type":"slack","version":2})).unwrap();
    let (context, rx, handle) = receiver(400);
    let result = block_on(notification::notify(&notifier, "hello", &context)).unwrap();
    assert_eq!(result.error.as_deref(), Some("reply"));
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    handle.join().unwrap();
}

#[test]
fn notification_status_fixtures_match_jumeaux_ok() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/http-review2.json")).unwrap();
    for case in cases
        .iter()
        .filter(|case| case["input"].get("notifier").is_some())
    {
        let notifier = serde_json::from_value(case["input"]["notifier"].clone()).unwrap();
        let (context, rx, handle) = receiver(case["input"]["status"].as_u64().unwrap() as u16);
        let result = block_on(notification::notify(&notifier, "status fixture", &context)).unwrap();
        let expected = &case["expected"];
        assert_eq!(
            json!({"ok": result.error.is_none(), "notification_error": result.error}),
            json!({"ok": expected["ok"], "notification_error": expected["notification_error"]}),
            "{}",
            case["name"]
        );
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        handle.join().unwrap();
    }
}

#[test]
fn missing_webhook_skips_both_versions_and_blocks_parsing() {
    for version in [1, 2] {
        let notifier = serde_json::from_value(
            json!({"type":"slack","version":version,"channel":"#local","use_blocks":true}),
        )
        .unwrap();
        let result = block_on(notification::notify(
            &notifier,
            "invalid JSON",
            &Context::default(),
        ))
        .unwrap();
        assert!(result.skipped);
        assert_eq!(result.logs.len(), 3);
        assert!(result.error.is_none());
    }
}

#[test]
fn seedless_shuffle_preserves_requests() {
    let input: Vec<Request> = (0..100)
        .map(|n| Request {
            path: format!("/{n}"),
            ..Request::default()
        })
        .collect();
    let mut actual = addons::reqs2reqs::shuffle::Config {}
        .apply(input.clone(), None)
        .into_iter()
        .map(|r| r.path)
        .collect::<Vec<_>>();
    let mut expected = input.into_iter().map(|r| r.path).collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn templates_are_validated_before_execution() {
    let addons: Vec<Addon> = serde_json::from_value(json!([{"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"{% invalid %}"}]}}])).unwrap();
    assert!(matches!(
        Pipeline::build(&addons),
        Err(Error {
            kind: "TemplateSyntaxError",
            ..
        })
    ));
}

#[test]
fn cli_empty_guard_notifies_and_exits_with_one() {
    let directory = std::env::temp_dir().join(format!("jumeaux-inputs-cli-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("requests");
    std::fs::write(&file, "/discarded\n").unwrap();
    let path = directory.join("config.json");
    let mut config = serde_json::to_value(config()).unwrap();
    config["addons"]["reqs2reqs"] = json!([
        {"name":"repeat","config":{"times":0}},
        {"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"{{ title }} notify!"}]}},
        {"name":"add","config":{"reqs":[{"path":"/never"}]}}
    ]);
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (context, rx, handle) = receiver(200);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jumeaux"))
        .args([
            "run",
            file.to_str().unwrap(),
            "--config",
            path.to_str().unwrap(),
        ])
        .env("SLACK_INCOMING_WEBHOOKS_URL", context.webhook_url.unwrap())
        .env_remove("SLACK_THREAD_TS")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "\x1b[33m[WARNING] Requests are empty. Exit executor.\x1b[0m\nNotify using slack@v1\n"
    );
    let record = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    handle.join().unwrap();
    assert_eq!(record["payload"]["text"], "empty test notify!");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn input_files_are_concatenated_before_transformations() {
    let mut config = config();
    config.input_files = Some(vec![
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/compat/python-baseline/simple/requests"
        )
        .into(),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/compat/python-baseline/path_empty/requests"
        )
        .into(),
    ]);
    let expected = config
        .input_files
        .as_ref()
        .unwrap()
        .iter()
        .flat_map(|p| {
            addons::log2reqs::read(&config.addons.log2reqs, &std::fs::read(p).unwrap()).unwrap()
        })
        .collect::<Vec<_>>();
    let result = block_on(addons::log2reqs::prepare(&config, &Context::default())).unwrap();
    assert_eq!(
        serde_json::to_value(result.requests).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn notification_failure_preserves_logs_before_error() {
    let mut config = config();
    config.addons.reqs2reqs = serde_json::from_value(json!([{"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"hello"}]}}])).unwrap();
    let (context, rx, handle) = receiver(0);
    let failure = block_on(Pipeline::build(&config.addons.reqs2reqs).unwrap().exec(
        vec![],
        &config,
        &context,
    ))
    .unwrap_err();
    assert_eq!(failure.error.kind, "ConnectionError");
    assert_eq!(
        failure.logs,
        vec![
            notification::Log::new(
                notification::Level::Warning,
                "Requests are empty. Exit executor."
            ),
            notification::Log::new(notification::Level::Info, "Notify using slack@v1"),
        ]
    );
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    handle.join().unwrap();

    let (context, rx, handle) = receiver(0);
    let mut config = serde_json::to_value(config).unwrap();
    config["addons"]["reqs2reqs"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"name":"repeat","config":{"times":0}}));
    let output = run_cli(&config, context.webhook_url.as_deref());
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("\x1b[33m[WARNING] Requests are empty. Exit executor.\x1b[0m\nNotify using slack@v1\n\x1b[31m[ ERROR ] "),"{stderr}");
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    handle.join().unwrap();
}

#[test]
fn notification_info_log_matches_notifier_baselines() {
    for (version, baseline) in [
        (
            1,
            include_str!("../compat/python-baseline/notify_v1/run.stderr"),
        ),
        (
            2,
            include_str!("../compat/python-baseline/notify_v2/run.stderr"),
        ),
        (
            2,
            include_str!("../compat/python-baseline/notify_v2_blocks/run.stderr"),
        ),
    ] {
        let line = baseline
            .lines()
            .find(|l| l.starts_with("Notify using "))
            .unwrap();
        let mut config = config();
        config.notifiers.as_mut().unwrap()["local"].version = version;
        let addons: Vec<Addon> = serde_json::from_value(json!([{"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"hello"}]}}])).unwrap();
        let output = block_on(Pipeline::build(&addons).unwrap().exec(
            vec![],
            &config,
            &Context::default(),
        ))
        .unwrap();
        assert_eq!(
            output.logs[1],
            notification::Log::new(notification::Level::Info, line)
        );
    }
}

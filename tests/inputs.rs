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
                    let response = format!(
                        "HTTP/1.1 {status} Result\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                    );
                    stream.write_all(response.as_bytes()).unwrap();
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
    if layer == "log2reqs" {
        return addons::log2reqs::read(&addon(input), &unhex(input["body_hex"].as_str().unwrap()))
            .map(|v| json!(v));
    }
    if layer == "notification" {
        let notifier = serde_json::from_value(input["notifier"].clone()).unwrap();
        let (mut context, rx, handle) = receiver(200);
        context.thread_ts = input["thread_ts"].as_str().map(str::to_owned);
        let outcome =
            notification::notify(&notifier, input["message"].as_str().unwrap(), &context)?;
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
    let result = pipeline.exec(requests, &self::config(), &context)?;
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
        if actual != case["expected"] {
            failures.push(format!(
                "{}: expected {}, actual {}",
                case["name"], case["expected"], actual
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
    println!("{} / {} Python fixtures matched", cases.len(), cases.len());
}

#[test]
fn empty_guard_stops_before_following_addon_and_returns_warnings() {
    let addons: Vec<Addon> = serde_json::from_value(json!([
        {"name":"empty_guard","config":{"notifies":[{"notifier":"local","message":"{{ title }}"}]}},
        {"name":"add","config":{"reqs":[{"path":"/never"}]}}
    ]))
    .unwrap();
    let result = Pipeline::build(&addons)
        .unwrap()
        .exec(vec![], &config(), &Context::default())
        .unwrap();
    assert_eq!(result.exit_code, Some(1));
    assert!(result.requests.is_empty());
    assert_eq!(result.warnings.len(), 4);
    assert!(result.notifications[0].skipped);
}

#[test]
fn notification_http_errors_are_returned() {
    let notifier = serde_json::from_value(json!({"type":"slack","version":2})).unwrap();
    let (context, rx, handle) = receiver(201);
    let result = notification::notify(&notifier, "hello", &context).unwrap();
    assert_eq!(result.error.as_deref(), Some("ok"));
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    handle.join().unwrap();
}

#[test]
fn missing_webhook_skips_both_versions_and_blocks_parsing() {
    for version in [1, 2] {
        let notifier = serde_json::from_value(
            json!({"type":"slack","version":version,"channel":"#local","use_blocks":true}),
        )
        .unwrap();
        let result = notification::notify(&notifier, "invalid JSON", &Context::default()).unwrap();
        assert!(result.skipped);
        assert_eq!(result.warnings.len(), 3);
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
        "Requests are empty. Exit executor.\n"
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
    let result = addons::log2reqs::prepare(&config, &Context::default()).unwrap();
    assert_eq!(
        serde_json::to_value(result.requests).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

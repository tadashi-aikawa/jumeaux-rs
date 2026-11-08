use jumeaux::{
    addons::{self, AddonExecutor, Chain, Dump, Factory, payload::*},
    compat::{Result, json as compat_json},
    config::Config,
    executor::{self, Pipeline, Storage},
    http::ResponsePair,
    models::{Addon, Addons, Request, Response, Status},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "jumeaux-challenge-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn response(value: &Value) -> Response {
    let mut value = value.clone();
    value["body"] = json!(unhex(value["body_hex"].as_str().unwrap()));
    value.as_object_mut().unwrap().remove("body_hex");
    serde_json::from_value(value).unwrap()
}
fn config(path: &Path) -> Config {
    serde_json::from_value(json!({
        "one":{"name":"one","host":"http://localhost"}, "other":{"name":"other","host":"http://localhost"},
        "output":{"response_dir":path}, "addons":{"log2reqs":{"name":"plain"},"res2dict":[{"name":"json"}],"store_criterion":[{"name":"free"}]}
    })).unwrap()
}
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), out);
            } else {
                out.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .into(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn run(input: &Value) -> Result<Value> {
    Ok(match input["layer"].as_str().unwrap() {
        "store" => {
            let addon =
                serde_json::from_value(json!({"name":"free","config":input["config"]})).unwrap();
            let res = response(&input["response"]);
            let reference = StoreCriterionReference {
                status: Status::Different,
                req: serde_json::from_value(json!({"name":"named","path":"/a","qs":{"q":["1"]}}))
                    .unwrap(),
                res_one: res.clone(),
                res_other: res,
            };
            json!(
                addons::store_criterion::Factory
                    .create(&addon)?
                    .exec(
                        StoreCriterionPayload {
                            stored: input["stored"].as_bool().unwrap()
                        },
                        &reference
                    )?
                    .stored
            )
        }
        "dump" => {
            let addons: Addons =
                serde_json::from_value(json!({"log2reqs":{"name":"plain"},"dump":input["steps"]}))
                    .unwrap();
            let result = Chain::<Dump>::build(&addons, &addons::dump::Factory)?.exec(
                DumpPayload {
                    response: response(&input["response"]),
                    body: unhex(input["body_hex"].as_str().unwrap()),
                    encoding: input["encoding"].as_str().map(str::to_owned),
                },
                &(),
            )?;
            json!({"body_hex":hex(&result.body),"encoding":result.encoding})
        }
        "tag" => {
            let addon: Addon =
                serde_json::from_value(json!({"name":"tag","config":input["config"]})).unwrap();
            let res = response(&input["response"]);
            let reference = DidChallengeReference {
                res_one: res.clone(),
                res_other: res,
                res_one_props: Some(input["props"].clone()),
                res_other_props: None,
            };
            json!(
                addons::did_challenge::Factory
                    .create(&addon)?
                    .exec(
                        DidChallengePayload {
                            trial: serde_json::from_value(input["trial"].clone()).unwrap()
                        },
                        &reference
                    )?
                    .trial
            )
        }
        "sleep" => {
            let c: addons::did_challenge::sleep::Config =
                serde_json::from_value(input["config"].clone()).unwrap();
            json!(c.seconds(input["unit"].as_f64().unwrap())?.as_secs_f64())
        }
        "props" => json!(hex(&executor::prop_bytes(&compat_json::parse(
            input["text"].as_str().unwrap()
        )?)?)),
        "key" => json!(executor::hash_from_args_at(
            input["args"].as_str().unwrap(),
            chrono::DateTime::parse_from_rfc3339(input["now"].as_str().unwrap()).unwrap()
        )),
        "integrated" => {
            let temp = Temp::new();
            let mut config = config(&temp.0);
            config.addons = serde_json::from_value(input["addons"].clone()).unwrap();
            config.judge_response_header = input["headers"].as_bool().unwrap();
            config.ignore_response_header_keys.clear();
            let storage = Storage::create(&config.output, "key")?;
            let trial = Pipeline::build(&config)?.finish(
                1,
                serde_json::from_value(input["request"].clone()).unwrap(),
                ResponsePair {
                    one: response(&input["one"]),
                    other: response(&input["other"]),
                    request_time: "2000-01-01T00:00:00+09:00".into(),
                },
                &storage,
            )?;
            json!({"trial":trial,"files":files(storage.root()).into_iter().map(|(k,v)|(k,hex(&v))).collect::<BTreeMap<_,_>>()})
        }
        "filename" => {
            let temp = Temp::new();
            let config = config(&temp.0);
            let storage = Storage::create(&config.output, "key")?;
            let response = Response {
                body: b"{\"a\":1}".as_slice().into(),
                encoding: Some("utf-8".into()),
                headers: [("Content-Type".into(), "application/json".into())].into(),
                url: "http://localhost/a".into(),
                status_code: 200,
                elapsed: 0.0,
                elapsed_sec: 0.0,
                response_type: "json".into(),
            };
            let request: Request =
                serde_json::from_value(json!({"path":"/a","name":input["name"]})).unwrap();
            let trial = Pipeline::build(&config)?.finish(
                input["seq"].as_u64().unwrap(),
                request,
                ResponsePair {
                    one: response.clone(),
                    other: response,
                    request_time: "2000-01-01T00:00:00+09:00".into(),
                },
                &storage,
            )?;
            json!({"trial":trial,"files":files(storage.root()).keys().collect::<Vec<_>>()})
        }
        _ => unreachable!(),
    })
}

#[test]
fn python_component_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/challenge.json")).unwrap();
    let mut failed = Vec::new();
    for case in &cases {
        let result = match run(&case["input"]) {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error": error.kind}),
        };
        let expected = case.get("rust_expected").unwrap_or(&case["expected"]);
        if &result != expected {
            failed.push(format!(
                "{}\nactual: {result}\nexpected: {}",
                case["name"], expected
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    println!(
        "challenge component fixtures: {}/{} matched",
        cases.len(),
        cases.len()
    );
}

#[test]
fn storage_latest_and_reuse() {
    let temp = Temp::new();
    let config = config(&temp.0);
    let one = Storage::create(&config.output, "first").unwrap();
    one.update_latest().unwrap();
    assert_eq!(
        std::fs::read_link(temp.0.join("latest")).unwrap(),
        Path::new("first")
    );
    assert_eq!(
        Storage::create(&config.output, "first").err().unwrap().kind,
        "FileExistsError"
    );
    let two = Storage::create(&config.output, "second").unwrap();
    two.update_latest().unwrap();
    assert_eq!(
        std::fs::read_link(temp.0.join("latest")).unwrap(),
        Path::new("second")
    );
    assert!(one.root().exists());
    std::fs::remove_dir_all(one.root()).unwrap();
    std::fs::remove_file(temp.0.join("latest")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("missing", temp.0.join("latest")).unwrap();
    two.update_latest().unwrap();
    assert_eq!(
        std::fs::read_link(temp.0.join("latest")).unwrap(),
        Path::new("second")
    );
}

#[test]
fn did_sleep_really_waits() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/challenge.json")).unwrap();
    let input = &cases.iter().find(|c| c["input"]["layer"] == "tag").unwrap()["input"];
    let res = response(&input["response"]);
    let reference = DidChallengeReference {
        res_one: res.clone(),
        res_other: res,
        res_one_props: None,
        res_other_props: None,
    };
    let c = addons::did_challenge::sleep::Config {
        min: 0.02,
        max: 0.02,
    };
    let start = std::time::Instant::now();
    c.exec(
        DidChallengePayload {
            trial: serde_json::from_value(input["trial"].clone()).unwrap(),
        },
        &reference,
    )
    .unwrap();
    assert!(start.elapsed() >= std::time::Duration::from_millis(20));
}

struct Server {
    address: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    peak: std::sync::Arc<AtomicUsize>,
    cookies: std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>,
}
impl Server {
    fn start() -> Self {
        use std::{
            io::{Read, Write},
            sync::{Arc, Mutex, atomic::AtomicBool},
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let cookies = Arc::new(Mutex::new(Vec::new()));
        let (flag, maximum, records) = (Arc::clone(&stop), Arc::clone(&peak), Arc::clone(&cookies));
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !flag.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                let (active, maximum, records) = (
                    Arc::clone(&active),
                    Arc::clone(&maximum),
                    Arc::clone(&records),
                );
                workers.push(std::thread::spawn(move || {
                    stream.set_nonblocking(false).unwrap();
                    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                    let mut bytes = Vec::new();
                    let mut byte = [0];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).unwrap() == 0 { return; }
                        bytes.push(byte[0]);
                    }
                    let text = String::from_utf8(bytes).unwrap();
                    let length = text.lines().filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map_or(0, |(_, value)| value.trim().parse::<usize>().unwrap());
                    stream.read_exact(&mut vec![0; length]).unwrap();
                    let path = text.split_whitespace().nth(1).unwrap().to_owned();
                    let cookie = text.to_ascii_lowercase().contains("cookie: token=ok");
                    records.lock().unwrap().push((path.clone(), cookie));
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    let delay = if path.contains("slow") { 80 } else { 10 };
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    let body = if path.contains("invalid") { "{" } else if path.contains("different") { "{\"a\":2}" } else { "{\"a\":1}" };
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: token=ok; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            address,
            stop,
            thread: Some(thread),
            peak,
            cookies,
        }
    }
    fn config(&self, path: &Path) -> Config {
        let mut config = config(path);
        config.one.host = self.address.clone();
        config.other.host = self.address.clone();
        config
    }
    fn transport(&self, config: &Config) -> std::sync::Arc<jumeaux::http::HttpTransport> {
        std::sync::Arc::new(
            jumeaux::http::HttpTransport::with_options(
                &config.one,
                &config.other,
                0,
                jumeaux::http::TransportOptions {
                    trust_env: false,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn requests(paths: &[&str]) -> Vec<Request> {
    paths
        .iter()
        .map(|path| Request {
            path: (*path).into(),
            ..Default::default()
        })
        .collect()
}

#[tokio::test]
async fn parallelism_is_bounded_and_trials_keep_input_order() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    for (key, threads, processes) in [("threads", 2, None), ("processes", 0, Some(2))] {
        server.peak.store(0, Ordering::Relaxed);
        let mut config = server.config(&temp.0);
        config.threads = threads;
        config.processes = processes;
        let trials = Arc::new(Pipeline::build(&config).unwrap())
            .exec(
                server.transport(&config),
                requests(&["/slow", "/fast", "/next", "/last"]),
                key,
                Some(executor::ProcessCookieScope::PerTrial),
            )
            .await
            .unwrap()
            .trials;
        assert_eq!(
            trials.iter().map(|t| t.seq).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            trials.iter().map(|t| t.path.as_str()).collect::<Vec<_>>(),
            vec!["/slow", "/fast", "/next", "/last"]
        );
        let peak = server.peak.load(Ordering::Relaxed);
        assert!(
            (3..=4).contains(&peak),
            "two trials, two endpoints each; peak={peak}"
        );
    }
}

#[tokio::test]
async fn cookie_scope_defaults_to_per_trial_for_processes_and_shared_for_threads() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    for (key, processes, scope, expected_cookie) in [
        ("threads", None, None, true),
        ("processes-default", Some(1), None, false),
        ("processes-zero", Some(0), None, true),
        (
            "shared",
            Some(1),
            Some(executor::ProcessCookieScope::Shared),
            true,
        ),
        (
            "isolated",
            Some(1),
            Some(executor::ProcessCookieScope::PerTrial),
            false,
        ),
    ] {
        server.cookies.lock().unwrap().clear();
        let mut config = server.config(&temp.0);
        config.processes = processes;
        let pipeline = Arc::new(Pipeline::build(&config).unwrap());
        pipeline
            .exec(
                server.transport(&config),
                requests(&["/first", "/second"]),
                key,
                scope,
            )
            .await
            .unwrap();
        let records = server.cookies.lock().unwrap();
        let second: Vec<_> = records
            .iter()
            .filter(|(path, _)| path.starts_with("/second"))
            .collect();
        assert_eq!(second.len(), 2);
        assert!(
            second.iter().all(|(_, cookie)| *cookie == expected_cookie),
            "{key}: {records:?}"
        );
    }
    assert_eq!(
        executor::ProcessCookieScope::default(),
        executor::ProcessCookieScope::PerTrial
    );
}

#[tokio::test]
async fn addon_error_keeps_previous_latest_and_failure_skips_addons() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    let config = server.config(&temp.0);
    let previous = Storage::create(&config.output, "previous").unwrap();
    previous.update_latest().unwrap();
    let result = Arc::new(Pipeline::build(&config).unwrap())
        .exec(
            server.transport(&config),
            requests(&["/invalid"]),
            "invalid",
            None,
        )
        .await;
    let failure = result.unwrap_err();
    assert_eq!(failure.error.kind, "JSONDecodeError");
    assert!(summaries(&failure.logs).is_empty());
    assert!(files(&temp.0.join("invalid")).is_empty());
    assert_eq!(
        std::fs::read_link(temp.0.join("latest")).unwrap(),
        Path::new("previous")
    );
    let mut config = config;
    let refused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.other.host = format!("http://{}", refused.local_addr().unwrap());
    drop(refused);
    config.addons.did_challenge = serde_json::from_value(
        json!([{"name":"tag","config":{"conditions":[{"when":"1 / 0","tag":"never"}]}}]),
    )
    .unwrap();
    let result = Arc::new(Pipeline::build(&config).unwrap())
        .exec(
            server.transport(&config),
            requests(&["/a"]),
            "failure",
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.trials[0].status, Status::Failure);
    let progress = summaries(&result.logs);
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].name, None);
    assert_eq!(progress[0].status, Status::Failure);
    assert_eq!(progress[0].one.status_code, None);
    assert_eq!(
        json!(result.trials[0].one),
        json!({"url":format!("{}/a?",config.one.host), "type":"unknown"})
    );
    assert!(result.trials[0].tags.is_empty());
    assert!(files(&temp.0.join("failure")).is_empty());
    assert_eq!(
        std::fs::read_link(temp.0.join("latest")).unwrap(),
        Path::new("failure")
    );
}

#[tokio::test]
async fn storage_error_stops_pending_trials_and_drains_running_trials() {
    use std::sync::Arc;
    for threads in [1, 2] {
        let temp = Temp::new();
        let server = Server::start();
        let mut config = server.config(&temp.0);
        config.threads = threads;
        config.addons.did_challenge = serde_json::from_value(json!([
            {"name":"sleep","config":{"min":0.2,"max":0.2}}
        ]))
        .unwrap();
        let previous = Storage::create(&config.output, "previous").unwrap();
        previous.update_latest().unwrap();
        let mut input = requests(&["/first", "/slow", "/3", "/4", "/5", "/6", "/7", "/8"]);
        input[0].name = Some("a/b".into());
        let started = std::time::Instant::now();
        let failure = Arc::new(Pipeline::build(&config).unwrap())
            .exec(server.transport(&config), input, "error", None)
            .await
            .unwrap_err();
        assert_eq!(failure.error.kind, "FileNotFoundError");
        assert!(
            failure
                .error
                .message
                .contains(temp.0.join("error/one/(1)a/b").to_str().unwrap())
        );
        let progress = summaries(&failure.logs);
        assert_eq!(progress.len(), threads as usize);
        assert!(failure.logs.iter().all(|log| log.total == 8));
        assert_eq!(progress[0].name.as_deref(), Some("a/b"));
        let records = server.cookies.lock().unwrap().clone();
        assert_eq!(records.len(), threads as usize * 2);
        assert!(
            records
                .iter()
                .all(|(path, _)| path == "/first" || path == "/slow")
        );
        let saved = files(&temp.0.join("error"));
        if threads == 1 {
            assert!(saved.is_empty());
        } else {
            assert_eq!(
                saved.keys().map(String::as_str).collect::<Vec<_>>(),
                [
                    "one-props/(2)2.json",
                    "one/(2)2",
                    "other-props/(2)2.json",
                    "other/(2)2"
                ]
            );
            assert!(started.elapsed() >= std::time::Duration::from_millis(200));
        }
        assert_eq!(
            std::fs::read_link(temp.0.join("latest")).unwrap(),
            Path::new("previous")
        );
        // execが返った後にHTTP・保存が増えない。
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(files(&temp.0.join("error")), saved);
        assert_eq!(*server.cookies.lock().unwrap(), records);
    }
}

#[tokio::test]
async fn first_error_in_input_order_wins_even_if_it_finishes_later() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    let mut config = server.config(&temp.0);
    config.threads = 2;
    let mut input = requests(&["/slow", "/invalid", "/never"]);
    input[0].name = Some("a/b".into());
    let failure = Arc::new(Pipeline::build(&config).unwrap())
        .exec(server.transport(&config), input, "errors", None)
        .await
        .unwrap_err();
    assert_eq!(failure.error.kind, "FileNotFoundError");
    assert_eq!(summaries(&failure.logs).len(), 1);
    assert_eq!(summaries(&failure.logs)[0].seq, 1);
    assert_eq!(server.cookies.lock().unwrap().len(), 4);
    assert!(files(&temp.0.join("errors")).is_empty());
    assert!(!temp.0.join("latest").exists());
}

#[tokio::test]
async fn execution_times_cover_trials_after_storage_and_before_latest() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    let mut config = server.config(&temp.0);
    config.addons.did_challenge = serde_json::from_value(json!([
        {"name":"sleep","config":{"min":0.02,"max":0.02}}
    ]))
    .unwrap();
    for (key, input) in [("empty", Vec::new()), ("one", requests(&["/a"]))] {
        let before = chrono::Local::now().fixed_offset();
        let execution = Arc::new(Pipeline::build(&config).unwrap())
            .exec(server.transport(&config), input, key, None)
            .await
            .unwrap();
        let after = chrono::Local::now().fixed_offset();
        assert!(before <= execution.started_at);
        assert!(execution.started_at <= execution.ended_at);
        assert!(execution.ended_at <= after);
        // key直下のディレクトリ構成とlatestのmtimeで計測位置を検証する。
        let created: chrono::DateTime<chrono::Utc> = std::fs::metadata(temp.0.join(key))
            .unwrap()
            .modified()
            .unwrap()
            .into();
        let linked: chrono::DateTime<chrono::Utc> =
            std::fs::symlink_metadata(temp.0.join("latest"))
                .unwrap()
                .modified()
                .unwrap()
                .into();
        assert!(created <= execution.started_at);
        assert!(execution.ended_at <= linked);
        assert_eq!(summaries(&execution.logs).len(), execution.trials.len());
        if key == "one" {
            assert!((execution.ended_at - execution.started_at).num_milliseconds() >= 20);
        }
    }
}

#[tokio::test]
async fn latest_error_preserves_progress_logs() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    let config = server.config(&temp.0);
    std::fs::create_dir(temp.0.join("latest")).unwrap();
    let failure = Arc::new(Pipeline::build(&config).unwrap())
        .exec(server.transport(&config), requests(&["/a"]), "key", None)
        .await
        .unwrap_err();
    assert!(
        failure
            .error
            .message
            .contains(temp.0.join("latest").to_str().unwrap())
    );
    assert_eq!(summaries(&failure.logs).len(), 1);
    assert_eq!(summaries(&failure.logs)[0].seq, 1);
    assert_eq!(files(&temp.0.join("key")).len(), 4);
}

// CLI実装前の比較用フォーマッタ。構造化ログだけでPythonの行とverbosityを復元する。
fn summaries(logs: &[executor::Progress]) -> Vec<&executor::TrialProgress> {
    logs.iter()
        .filter_map(executor::Progress::summary)
        .collect()
}
fn progress_line(log: &executor::TrialProgress) -> Value {
    use jumeaux::models::HttpMethod;
    let prefix = format!("[{} / {}]", log.seq, log.total);
    let message = if log.status == Status::Failure {
        assert_eq!(log.one.status_code, None);
        assert_eq!(log.other.status_code, None);
        assert_eq!(log.one.response_sec, None);
        assert_eq!(log.other.response_sec, None);
        format!("{prefix} 💀 {}", log.name.as_deref().unwrap_or("None"))
    } else {
        assert!(log.one.response_sec.unwrap() >= 0.0);
        assert!(log.other.response_sec.unwrap() >= 0.0);
        // ローカルHTTPの実測時間だけをPython fixtureの固定値へ正規化する。
        let symbol = if log.status == Status::Same { "O" } else { "X" };
        let method = match log.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        };
        format!(
            "{prefix} {symbol} ({} - {}) <0.0s - 0.0s> {{HttpMethod.{method}}} {}",
            log.one.status_code.unwrap(),
            log.other.status_code.unwrap(),
            log.name.as_deref().unwrap_or(&log.path)
        )
    };
    json!({"verbosity": if log.status == Status::Same { 2 } else { 1 }, "message": message})
}

fn storage_review(input: &Value) -> Value {
    fn directories(root: &Path, path: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                out.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .into(),
                );
                directories(root, &entry.path(), out);
            }
        }
    }
    let temp = Temp::new();
    let key = temp.0.join("responses/key");
    let setup = input["setup"].as_str().unwrap();
    match setup {
        "missing" => (),
        "file" => {
            std::fs::create_dir(key.parent().unwrap()).unwrap();
            std::fs::write(&key, b"existing").unwrap();
        }
        _ => {
            std::fs::create_dir_all(&key).unwrap();
            match setup {
                "unrelated" => std::fs::write(key.join("keep"), b"existing").unwrap(),
                "one" | "other" => std::fs::create_dir(key.join(setup)).unwrap(),
                "empty" => (),
                _ => unreachable!(),
            }
        }
    }
    let error = Storage::create(&config(&temp.0.join("responses")).output, "key").err();
    let error_path = error.as_ref().map(|error| {
        let path = error
            .message
            .rsplit_once(": '")
            .unwrap()
            .1
            .strip_suffix('\'')
            .unwrap();
        Path::new(path)
            .strip_prefix(&temp.0)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    });
    let mut dirs = Vec::new();
    directories(&temp.0, &temp.0, &mut dirs);
    dirs.sort();
    if setup == "unrelated" {
        assert_eq!(std::fs::read(key.join("keep")).unwrap(), b"existing");
    }
    #[cfg(unix)]
    if error.is_none() {
        use std::os::unix::fs::PermissionsExt;
        for side in ["one", "other", "one-props", "other-props"] {
            assert_eq!(
                std::fs::metadata(key.join(side))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o777
            );
        }
    }
    json!({"error":error.map(|e|e.kind), "error_path":error_path, "directories":dirs})
}

#[tokio::test]
async fn python_review_fixtures() {
    use std::sync::Arc;
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/challenge-review.json")).unwrap();
    for case in &cases {
        let input = &case["input"];
        let actual = if input["layer"] == "storage" {
            storage_review(input)
        } else {
            let temp = Temp::new();
            let server = Server::start();
            let mut config = server.config(&temp.0);
            match input["status"].as_str().unwrap() {
                "different" => config.other.host.push_str("/different"),
                "invalid" => config.one.host.push_str("/invalid"),
                "failure" => config.other.host = "http://127.0.0.1:0".into(),
                "same" => (),
                _ => unreachable!(),
            }
            let request: Request = serde_json::from_value(input["request"].clone()).unwrap();
            let execution = Arc::new(Pipeline::build(&config).unwrap())
                .exec(server.transport(&config), vec![request; 8], "key", None)
                .await;
            let (error, logs) = match execution {
                Ok(result) => {
                    assert_eq!(summaries(&result.logs).len(), 8);
                    for (log, trial) in summaries(&result.logs).into_iter().zip(&result.trials) {
                        assert_eq!(log.seq, trial.seq);
                        assert_eq!(log.total, 8);
                        assert_eq!(log.one.response_sec, trial.one.response_sec);
                        assert_eq!(log.other.response_sec, trial.other.response_sec);
                    }
                    (None, result.logs)
                }
                Err(failure) => (Some(failure.error.kind), failure.logs),
            };
            json!({"error":error, "logs":summaries(&logs).into_iter().take(1).map(progress_line).collect::<Vec<_>>()})
        };
        assert_eq!(actual, case["expected"], "{}", case["name"]);
    }
    println!(
        "challenge review fixtures: {}/{} matched",
        cases.len(),
        cases.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_stream_before_http_and_during_sleep_without_retention() {
    use jumeaux::{
        addons::log::AddonLog,
        executor::{Event, Stage},
        http::{HttpEvent, Side},
        notification::Verbosity,
    };
    use std::{
        sync::{Arc, mpsc},
        time::Duration,
    };
    let temp = Temp::new();
    let server = Server::start();
    let mut config = server.config(&temp.0);
    config.one.headers.insert("X-Endpoint".into(), "one".into());
    config
        .other
        .headers
        .insert("X-Endpoint".into(), "other".into());
    config.one.proxy = Some(server.address.strip_prefix("http://").unwrap().into());
    config.addons.did_challenge = serde_json::from_value(json!([
        {"name":"sleep","config":{"min":0.02,"max":0.02}}
    ]))
    .unwrap();
    let input: Request = serde_json::from_value(json!({
        "path":"/a", "name":"event-test", "method":"POST", "raw":"body",
        "form":{"form":"value"}, "json":{"json":"value"}, "qs":{"q":["a b"]}, "headers":{"X-Local":"request"}
    })).unwrap();
    let (send, receive) = mpsc::channel();
    let (resume, pause) = mpsc::channel();
    let pipeline = Arc::new(Pipeline::build(&config).unwrap());
    let transport = server.transport(&config);
    let job = tokio::spawn(async move {
        pipeline
            .exec_with_progress(transport, vec![input], "stream", None, move |event| {
                send.send(event.clone()).unwrap();
                if matches!(
                    event.event,
                    Event::Started(_)
                        | Event::Addon {
                            log: AddonLog::Sleep { .. },
                            ..
                        }
                ) {
                    pause.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            })
            .await
    });
    let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(&first.event, Event::Started(request) if request.name.as_deref() == Some("event-test"))
    );
    assert_eq!(first.level, Verbosity::VeryVerbose);
    assert!(!job.is_finished());
    assert!(
        server.cookies.lock().unwrap().is_empty(),
        "start is delivered before HTTP"
    );
    assert!(files(&temp.0.join("stream")).is_empty());
    resume.send(()).unwrap();
    let mut streamed = vec![first];
    loop {
        let event = receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("{error}: {streamed:#?}"));
        let sleep = matches!(
            event.event,
            Event::Addon {
                log: AddonLog::Sleep { .. },
                ..
            }
        );
        streamed.push(event);
        if sleep {
            break;
        }
    }
    assert!(
        !job.is_finished(),
        "sleep event must be delivered before the stage returns"
    );
    assert_eq!(files(&temp.0.join("stream")).len(), 4);
    resume.send(()).unwrap();
    let result = job.await.unwrap().unwrap();
    streamed.extend(receive.try_iter());
    assert!(
        result.logs.is_empty(),
        "streamed events must not be retained"
    );
    assert!(streamed.iter().all(|p| p.seq == 1 && p.total == 1));
    assert!(streamed.iter().any(|p| matches!(&p.event, Event::Http(HttpEvent::Url { side: Side::One, url }) if url == &format!("{}/a?q=a+b", server.address))));
    assert!(streamed.iter().any(|p| matches!(&p.event, Event::Http(HttpEvent::Proxy { side: Side::One, host }) if host == &config.one.proxy)));
    let details = streamed
        .iter()
        .find_map(|p| match &p.event {
            Event::Http(HttpEvent::RequestDetails(req)) => Some(req),
            _ => None,
        })
        .unwrap();
    assert_eq!(details.raw.as_deref(), Some("body"));
    assert_eq!(details.form.as_ref().unwrap()["form"], "value");
    assert_eq!(details.json.as_ref().unwrap()["json"], "value");
    for endpoint in [Side::One, Side::Other] {
        let headers = streamed
            .iter()
            .find_map(|p| match &p.event {
                Event::Http(HttpEvent::RequestHeaders { side, headers }) if *side == endpoint => {
                    Some(headers)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(headers["X-Local"], "request");
        assert_eq!(
            headers["X-Endpoint"],
            if endpoint == Side::One {
                "one"
            } else {
                "other"
            }
        );
        assert!(headers.contains_key("User-Agent"));
    }
    let saved: Vec<_> = streamed
        .iter()
        .filter_map(|p| match &p.event {
            Event::Saved {
                path,
                bytes,
                encoding,
                ..
            } => Some((path, bytes, encoding)),
            _ => None,
        })
        .collect();
    assert_eq!(saved.len(), 4);
    for (path, bytes, encoding) in saved {
        assert_eq!(std::fs::read(path).unwrap().len(), *bytes);
        assert_eq!(encoding.as_deref(), Some("utf-8"));
    }
    assert!(
        matches!(&streamed.last().unwrap().event, Event::StageFinished { stage: Stage::DidChallenge, elapsed } if *elapsed >= Duration::from_millis(20))
    );
}

#[tokio::test]
async fn failure_stream_delivers_partial_events_without_retaining_them() {
    use jumeaux::executor::Event;
    use std::sync::{Arc, Mutex};
    let temp = Temp::new();
    let server = Server::start();
    let mut config = server.config(&temp.0);
    config.threads = 2;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observed);
    let mut input = requests(&["/a", "/slow", "/never"]);
    input[0].name = Some("a/b".into());
    let failure = Arc::new(Pipeline::build(&config).unwrap())
        .exec_with_progress(
            server.transport(&config),
            input,
            "error",
            None,
            move |event| captured.lock().unwrap().push(event.clone()),
        )
        .await
        .unwrap_err();
    assert!(
        failure.logs.is_empty(),
        "failed streams must not retain events"
    );
    let logs = observed.lock().unwrap();
    assert!(
        logs.iter()
            .any(|p| matches!(&p.event, Event::Failed(e) if e == &failure.error))
    );
    assert!(!logs.iter().any(|p| p.seq == 3));
    assert!(
        !logs
            .iter()
            .any(|p| p.seq == 1 && matches!(p.event, Event::Saved { .. }))
    );
    assert_eq!(
        logs.iter()
            .filter(|p| p.seq == 2 && matches!(p.event, Event::Saved { .. }))
            .count(),
        4
    );
    assert_eq!(server.cookies.lock().unwrap().len(), 4);
}

#[test]
fn addon_logs_record_the_selected_branch_without_reevaluating_conditions() {
    use jumeaux::addons::{
        Res2Dict, StoreCriterion,
        log::{AddonLog, ConversionReason},
    };
    use std::cell::RefCell;
    let addons: Addons = serde_json::from_value(json!({"log2reqs":{"name":"plain"},
        "res2dict":[{"name":"xml"}, {"name":"json","config":{"force":true}}, {"name":"json"}],
        "store_criterion":[{"name":"free","config":{"when_any":["False", "True", "1 / 0"]}}, {"name":"free","config":{"when_any":["1 / 0"]}}]
    })).unwrap();
    let res = response(
        &json!({"body_hex":"7b2261223a317d","encoding":"utf-8","type":"json","headers":{},"url":"http://localhost/a","status_code":200,"elapsed":0.0,"elapsed_sec":0.0}),
    );
    let logs = RefCell::new(Vec::new());
    Chain::<Res2Dict>::build(&addons, &addons::res2dict::Factory)
        .unwrap()
        .exec_with_log(
            Res2DictPayload {
                response: res.clone(),
                result: None,
            },
            &(),
            &|index, name, event| logs.borrow_mut().push((index, name.to_owned(), event)),
        )
        .unwrap();
    assert_eq!(
        *logs.borrow(),
        vec![
            (
                0,
                "xml".into(),
                AddonLog::Conversion {
                    format: "xml",
                    reason: ConversionReason::DifferentType
                }
            ),
            (
                1,
                "json".into(),
                AddonLog::Conversion {
                    format: "json",
                    reason: ConversionReason::Forced
                }
            ),
            (
                2,
                "json".into(),
                AddonLog::Conversion {
                    format: "json",
                    reason: ConversionReason::AlreadyExists
                }
            ),
        ]
    );
    logs.borrow_mut().clear();
    let result = Chain::<StoreCriterion>::build(&addons, &addons::store_criterion::Factory)
        .unwrap()
        .exec_with_log(
            StoreCriterionPayload { stored: false },
            &StoreCriterionReference {
                status: Status::Same,
                req: Request::default(),
                res_one: res.clone(),
                res_other: res,
            },
            &|index, name, event| logs.borrow_mut().push((index, name.into(), event)),
        )
        .unwrap();
    assert!(result.stored);
    assert_eq!(
        *logs.borrow(),
        vec![(
            0,
            "free".into(),
            AddonLog::Stored {
                expression: "True".into()
            }
        )]
    );
}

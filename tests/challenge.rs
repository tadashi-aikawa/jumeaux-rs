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
                    let path = text.split_whitespace().nth(1).unwrap().to_owned();
                    let cookie = text.to_ascii_lowercase().contains("cookie: token=ok");
                    records.lock().unwrap().push((path.clone(), cookie));
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    let delay = if path.contains("slow") { 80 } else { 10 };
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    let body = if path.contains("invalid") { "{" } else { "{\"a\":1}" };
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
async fn cookie_scope_is_explicit_and_clients_can_be_shared() {
    use std::sync::Arc;
    let temp = Temp::new();
    let server = Server::start();
    for (key, processes, scope, expected_cookie) in [
        ("threads", None, None, true),
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
    let mut config = server.config(&temp.0);
    config.processes = Some(1);
    let result = Arc::new(Pipeline::build(&config).unwrap())
        .exec(server.transport(&config), Vec::new(), "undecided", None)
        .await;
    assert_eq!(result.err().unwrap().kind, "ValueError");
    assert!(!temp.0.join("undecided").exists());
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
    assert_eq!(result.err().unwrap().kind, "JSONDecodeError");
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

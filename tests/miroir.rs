//! Python版がHTTPで送った項目・オブジェクトをRustの実HTTPと比較する。
#![cfg(feature = "miroir")]
use jumeaux::{
    addons::{
        Layer,
        r#final::Pipeline,
        parse_config,
        payload::{FinalPayload, FinalReference},
    },
    models::{Addon, Report},
    notification::Context,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("../compat/fixtures/miroir.json")).unwrap()
}
fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

const STS: &str = r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/"><AssumeRoleResult><Credentials><AccessKeyId>ASIAEXAMPLE0000000000</AccessKeyId><SecretAccessKey>test-secret</SecretAccessKey><SessionToken>test-session</SessionToken><Expiration>2099-01-01T00:00:00Z</Expiration></Credentials><AssumedRoleUser><AssumedRoleId>id:session</AssumedRoleId><Arn>arn:aws:sts::123456789012:assumed-role/test/session</Arn></AssumedRoleUser></AssumeRoleResult></AssumeRoleResponse>"#;

fn receive(mut stream: TcpStream, records: &Mutex<Vec<Value>>, mode: &str) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut input = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    input.read_line(&mut first).unwrap();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        input.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim().split_once(':').unwrap();
        headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
    }
    if headers
        .get("expect")
        .is_some_and(|s| s.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
    }
    let mut body = vec![0; headers["content-length"].parse().unwrap()];
    input.read_exact(&mut body).unwrap();
    let token = headers.get("x-amz-security-token");
    let mut response = "{}";
    let mut content_type = "application/x-amz-json-1.0";
    let record = if headers.contains_key("x-amz-target") {
        assert!(headers["authorization"].contains("/ap-northeast-1/dynamodb/"));
        let mut value: Value = serde_json::from_slice(&body).unwrap();
        if let Some(tags) = value
            .get_mut("Item")
            .and_then(|v| v.get_mut("tags"))
            .and_then(|v| v.get_mut("SS"))
            .and_then(Value::as_array_mut)
        {
            tags.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        }
        json!({"service":"dynamodb", "body":value, "token":token})
    } else if first.starts_with("POST") {
        let text = String::from_utf8(body).unwrap();
        let mut values = BTreeMap::<String, Vec<String>>::new();
        for pair in text.split('&') {
            let (key, value) = pair.split_once('=').unwrap();
            values.entry(decode(key)).or_default().push(decode(value));
        }
        response = STS;
        content_type = "text/xml";
        json!({"service":"sts", "body":values})
    } else {
        assert!(headers["authorization"].contains("/ap-northeast-1/s3/"));
        assert_eq!(headers["x-amz-content-sha256"], "UNSIGNED-PAYLOAD");
        for header in [
            "content-encoding",
            "transfer-encoding",
            "x-amz-trailer",
            "x-amz-decoded-content-length",
            "x-amz-checksum-crc32",
            "x-amz-sdk-checksum-algorithm",
        ] {
            assert!(
                !headers.contains_key(header),
                "unexpected {header}: {headers:?}"
            );
        }
        assert!(!headers["host"].starts_with("test-bucket."));
        let uri = first.split_whitespace().nth(1).unwrap();
        let path = decode(uri.split('?').next().unwrap());
        let mut record = json!({"service":"s3", "path":path,
            "content_type":headers.get("content-type"), "cache_control":headers.get("cache-control"), "token":token});
        if path.ends_with(".zip") {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body)).unwrap();
            let mut members = BTreeMap::new();
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).unwrap();
                let mut bytes = vec![];
                file.read_to_end(&mut bytes).unwrap();
                members.insert(file.name().unwrap().into_owned(), hex(&bytes));
            }
            record["members"] = json!(members);
        } else {
            record["hex"] = json!(hex(&body));
        }
        record
    };
    let fail = mode == "failure"
        || mode == "s3-failure" && record["service"] == "s3"
        || mode == "zip-failure" && record.get("members").is_some();
    records.lock().unwrap().push(record);
    if fail {
        let body = if headers.contains_key("x-amz-target") {
            r#"{"__type":"com.amazonaws.dynamodb.v20120810#ResourceNotFoundException","message":"Requested table does not exist"}"#
        } else {
            "<Error><Code>AccessDenied</Code><Message>Upload denied</Message></Error>"
        };
        write!(
            stream,
            "HTTP/1.1 400 Bad Request\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()
        )
        .unwrap();
    } else {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
    }
}

#[test]
fn python_http_objects_items_and_assume_role() {
    let mut object_count = 0;
    let mut item_count = 0;
    for (index, case) in cases().iter().enumerate() {
        for mode in if index == 0 {
            vec![
                "env",
                "profile",
                "sdk-endpoints",
                "disabled-local-stack",
                "partial-endpoints",
                "failure",
                "s3-failure",
                "zip-failure",
                "localhost-env",
                "localhost-global-env",
                "localhost-profile",
                "localhost-global-profile",
                "explicit-overrides-env",
                "no-region",
                "large-zip",
            ]
        } else {
            vec!["env"]
        } {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let stopped = Arc::new(AtomicBool::new(false));
            let records = Arc::new(Mutex::new(vec![]));
            let stop = Arc::clone(&stopped);
            let captured = Arc::clone(&records);
            let worker = std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => receive(stream, &captured, mode),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("{e}"),
                    }
                }
            });
            let root =
                std::env::temp_dir().join(format!("miroir-{}-{index}-{mode}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let config = root.join("config");
            let credentials = root.join("credentials");
            let profile = match mode {
                "localhost-profile" => format!(
                    "[profile miroir-test]\nregion = ap-northeast-1\nservices = local\n[services local]\ns3 =\n  endpoint_url = http://localhost:{port}\n"
                ),
                "localhost-global-profile" => format!(
                    "[profile miroir-test]\nregion = ap-northeast-1\nendpoint_url = http://localhost:{port}\n"
                ),
                _ => "[profile miroir-test]\nregion = ap-northeast-1\n".into(),
            };
            std::fs::write(&config, profile).unwrap();
            std::fs::write(
                &credentials,
                "[miroir-test]\naws_access_key_id = test\naws_secret_access_key = test\n",
            )
            .unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "miroir_child", "--nocapture"]);
            for (key, _) in std::env::vars() {
                if key.starts_with("AWS_") || key.to_ascii_lowercase().ends_with("_proxy") {
                    command.env_remove(key);
                }
            }
            command
                .env("MIROIR_CASE", index.to_string())
                .env("MIROIR_ROOT", &root)
                .env("MIROIR_PORT", port.to_string())
                .env("MIROIR_MODE", mode)
                .env("AWS_EC2_METADATA_DISABLED", "true")
                .env("AWS_CONFIG_FILE", &config)
                .env("AWS_SHARED_CREDENTIALS_FILE", &credentials)
                .env("AWS_ENDPOINT_URL_STS", format!("http://127.0.0.1:{port}"))
                .env("AWS_ENDPOINT_URL_S3", format!("http://127.0.0.1:{port}"))
                .env(
                    "AWS_ENDPOINT_URL_DYNAMODB",
                    format!("http://127.0.0.1:{port}"),
                )
                .env("AWS_MAX_ATTEMPTS", "1")
                .env("NO_PROXY", "*");
            if mode == "profile" || mode.ends_with("profile") {
                command.env("AWS_PROFILE", "miroir-test");
            } else {
                command
                    .env("AWS_ACCESS_KEY_ID", "test")
                    .env("AWS_SECRET_ACCESS_KEY", "test")
                    .env("AWS_REGION", "ap-northeast-1");
            }
            match mode {
                "localhost-env" => {
                    command.env("AWS_ENDPOINT_URL_S3", format!("http://localhost:{port}"));
                }
                "localhost-global-env" => {
                    command
                        .env_remove("AWS_ENDPOINT_URL_S3")
                        .env("AWS_ENDPOINT_URL", format!("http://localhost:{port}"));
                }
                "localhost-profile" | "localhost-global-profile" => {
                    command.env_remove("AWS_ENDPOINT_URL_S3");
                }
                "explicit-overrides-env" => {
                    command.env("AWS_ENDPOINT_URL_S3", "http://localhost:1");
                }
                "no-region" => {
                    command.env_remove("AWS_REGION");
                }
                _ => (),
            }
            let mut child = command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let start = Instant::now();
            while child.try_wait().unwrap().is_none() {
                if start.elapsed() > Duration::from_secs(40) {
                    child.kill().unwrap();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().unwrap();
            stopped.store(true, Ordering::Relaxed);
            worker.join().unwrap();
            std::fs::remove_dir_all(&root).unwrap();
            assert!(
                output.status.success(),
                "{} {mode}: {} {}",
                case["name"],
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = records.lock().unwrap();
            if mode == "no-region" {
                assert!(actual.is_empty());
                continue;
            }
            if mode.ends_with("failure") {
                let count = match mode {
                    "failure" => 1,
                    "s3-failure" => 2,
                    _ => 8,
                };
                assert_eq!(actual.len(), count);
                continue;
            }
            let expected = case["expected"].as_array().unwrap();
            assert_eq!(actual.len(), expected.len(), "{} {mode}", case["name"]);
            // 大きなZIPもHTTP本文が展開不要であることをreceive内で検査する。
            if mode == "large-zip" {
                continue;
            }
            for want in expected {
                let got = actual
                    .iter()
                    .find(|r| r["service"] == want["service"] && r["path"] == want["path"])
                    .unwrap_or_else(|| {
                        panic!(
                            "missing {} {} in {:?}",
                            want["service"],
                            want["path"],
                            actual.iter().map(|v| &v["path"]).collect::<Vec<_>>()
                        )
                    });
                assert_eq!(
                    got, want,
                    "{} {mode}: {} {}",
                    case["name"], want["service"], want["path"]
                );
            }
            if mode == "env" {
                object_count += actual.iter().filter(|r| r["service"] == "s3").count();
                item_count += actual.iter().filter(|r| r["service"] == "dynamodb").count();
            }
        }
    }
    eprintln!(
        "miroir: {object_count} S3 objects, {item_count} DynamoDB items match Python HTTP fixtures"
    );
}

#[test]
fn miroir_child() {
    let Ok(index) = std::env::var("MIROIR_CASE") else {
        return;
    };
    let case = cases().remove(index.parse::<usize>().unwrap());
    let root = PathBuf::from(std::env::var("MIROIR_ROOT").unwrap());
    let report: Report = serde_json::from_value(case["report"].clone()).unwrap();
    let destination = root.join(&report.key);
    for directory in ["one", "other", "one-props", "other-props", "empty"] {
        std::fs::create_dir_all(destination.join(directory)).unwrap();
    }
    for (path, body) in case["files"].as_object().unwrap() {
        let path = destination.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, unhex(body.as_str().unwrap())).unwrap();
    }
    if std::env::var("MIROIR_MODE").unwrap() == "large-zip" {
        let mut bytes = vec![0; 2 * 1024 * 1024];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes).unwrap();
        std::fs::write(destination.join("nested/large.bin"), bytes).unwrap();
    }
    let mut config: Value = serde_json::from_str(
        &case["config"]
            .to_string()
            .replace("PORT", &std::env::var("MIROIR_PORT").unwrap()),
    )
    .unwrap();
    match std::env::var("MIROIR_MODE").unwrap().as_str() {
        "sdk-endpoints"
        | "localhost-env"
        | "localhost-global-env"
        | "localhost-profile"
        | "localhost-global-profile" => {
            config.as_object_mut().unwrap().shift_remove("endpoints");
        }
        "disabled-local-stack" => {
            config.as_object_mut().unwrap().shift_remove("endpoints");
            config["local_stack"] = json!({"use":false});
        }
        "partial-endpoints" => {
            config["endpoints"]
                .as_object_mut()
                .unwrap()
                .shift_remove("s3");
        }
        _ => (),
    }
    let addons: Vec<Addon> = serde_json::from_value(json!([
        {"name":"miroir", "config":config}, {"name":"summary", "config":{"sysout":true}}
    ]))
    .unwrap();
    let pipeline = Pipeline::build(&addons).unwrap();
    let mut output = report.summary.output.clone();
    output.response_dir = root.to_str().unwrap().into();
    let fail = std::env::var("MIROIR_MODE").unwrap().ends_with("failure");
    let mut stdout = String::new();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(pipeline.exec(
            FinalPayload {
                report,
                output_summary: output,
            },
            &FinalReference { notifiers: None },
            &Context::default(),
            |text| {
                stdout.push_str(text);
                Ok(())
            },
        ));
    if std::env::var("MIROIR_MODE").unwrap() == "no-region" {
        let error = result.err().unwrap().error;
        assert_eq!(error.kind, "NoRegionError");
        assert_eq!(error.message, "You must specify a region.");
        assert!(stdout.is_empty());
        let mut cli_config: Value =
            serde_yaml_ng::from_str(include_str!("../compat/python-baseline/simple/config.yml"))
                .unwrap();
        cli_config["addons"]["final"] = json!([
            {"name":"miroir","config":{"table":"t","bucket":"b"}},
            {"name":"json"}
        ]);
        std::fs::write(root.join("config.yml"), cli_config.to_string()).unwrap();
        std::fs::write(root.join("requests"), "").unwrap();
        let cli = Command::new(env!("CARGO_BIN_EXE_jumeaux"))
            .current_dir(&root)
            .args(["run", "requests"])
            .output()
            .unwrap();
        assert_eq!(cli.status.code(), Some(1));
        let stderr = String::from_utf8(cli.stderr).unwrap();
        assert!(
            stderr.contains("NoRegionError: You must specify a region."),
            "{stderr}"
        );
        assert!(!root.join("responses/latest/report.json").exists());
        return;
    }
    if fail {
        let error = result.err().unwrap().error;
        assert!(error.message.starts_with("service error:"), "{error}");
        let reason = if std::env::var("MIROIR_MODE").unwrap() == "failure" {
            "Requested table does not exist"
        } else {
            "Upload denied"
        };
        assert!(error.message.contains(reason), "{error}");
        assert!(stdout.is_empty());
        assert!(
            !destination
                .with_file_name(format!("{}.zip", case["report"]["key"].as_str().unwrap()))
                .exists()
        );
        return;
    }
    result.unwrap();
    assert!(!stdout.is_empty());
    if let Some(want) = case["local_report_hex"].as_str() {
        assert_eq!(
            std::fs::read(destination.join("report.json")).unwrap(),
            unhex(want)
        );
    } else {
        assert!(!destination.join("report.json").exists());
    }
    assert!(
        !destination
            .with_file_name(format!("{}.zip", case["report"]["key"].as_str().unwrap()))
            .exists()
    );
}

#[test]
fn exclusive_endpoints_rejected_during_config_loading() {
    for enabled in [true, false] {
        let addon: Addon = serde_json::from_value(json!({"name":"miroir", "config":{
            "table":"t", "bucket":"b", "local_stack":{"use":enabled}, "endpoints":{}
        }}))
        .unwrap();
        let error = parse_config(Layer::Final, &addon, "addons.final[0]").unwrap_err();
        assert_eq!(error.kind, "ValueError");
        assert_eq!(
            error.message,
            "miroir config: endpoints and local_stack cannot be used together"
        );
    }
}

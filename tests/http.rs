use jumeaux::{
    compat::template::TemplateEngine,
    http::{ChallengeOutcome, HttpTransport, TransportOptions, prepare},
    models::{AccessPoint, Request},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

#[test]
fn python_request_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/http-request.json")).unwrap();
    for case in &cases {
        let endpoint = serde_json::from_value(case["input"]["endpoint"].clone()).unwrap();
        let request = serde_json::from_value(case["input"]["request"].clone()).unwrap();
        let actual = match prepare(&endpoint, &request) {
            Ok(mut p) => {
                if p.headers
                    .get("user-agent")
                    .is_some_and(|v| v.starts_with("jumeaux/"))
                {
                    p.headers
                        .insert("user-agent".into(), "jumeaux/<version>".into());
                }
                json!({"value": {"original_url": p.original_url, "url": p.url.to_string(), "headers": p.headers, "body_hex": p.body.iter().map(|b| format!("{b:02x}")).collect::<String>()}})
            }
            Err(error) => json!({"error": error.kind}),
        };
        assert_eq!(actual, case["expected"], "{}", case["name"]);
    }
    println!("HTTP request: {}/{} Python一致", cases.len(), cases.len());
}

fn endpoint(host: &str) -> AccessPoint {
    serde_json::from_value(json!({"name": "test", "host": host})).unwrap()
}

fn server(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut records = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                assert_eq!(stream.read(&mut b).unwrap(), 1);
                bytes.push(b[0]);
            }
            records.push(String::from_utf8(bytes).unwrap());
            stream.write_all(&response).unwrap();
        }
        records
    });
    (host, handle)
}

fn options() -> TransportOptions {
    TransportOptions {
        trust_env: false,
        read_timeout: Some(Duration::from_secs(3)),
        ..TransportOptions::default()
    }
}

#[cfg(unix)]
#[tokio::test]
async fn ca_file_errors_propagate_but_invalid_contents_become_failure() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("ca-errors-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/http-review2.json")).unwrap();
    for case in cases
        .iter()
        .filter(|case| case["input"].get("bundle").is_some())
    {
        let label = case["input"]["bundle"].as_str().unwrap();
        let bundle = directory.join(label);
        let body: &[u8] = match label {
            "empty" => b"",
            "bad-certificate" => {
                b"-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----\n"
            }
            _ => b"not pem",
        };
        if label != "missing" {
            std::fs::write(&bundle, body).unwrap();
        }
        if label == "unreadable" {
            std::fs::set_permissions(&bundle, std::fs::Permissions::from_mode(0o0)).unwrap();
        }
        let access = endpoint("https://localhost:1");
        let transport = HttpTransport::with_options(
            &access,
            &access,
            0,
            TransportOptions {
                ca_bundle: Some(bundle.clone()),
                ..options()
            },
        )
        .unwrap();
        let request = Request {
            path: "/".into(),
            ..Request::default()
        };
        let sent = transport.send_one(&request).await;
        let challenged = transport.challenge(1, &request).await;
        if label == "unreadable" {
            std::fs::set_permissions(&bundle, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let expected = case.get("rust_expected").unwrap_or(&case["expected"]);
        assert_eq!(
            sent.unwrap_err().kind,
            expected["send_error"].as_str().unwrap()
        );
        let actual = match challenged {
            Err(error) => error.kind,
            Ok(ChallengeOutcome::Failure(_)) => "failure",
            _ => panic!("CA error expected"),
        };
        assert_eq!(actual, expected["challenge"].as_str().unwrap(), "{label}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn pair_runs_both_endpoints_and_headers_are_case_insensitive_in_templates() {
    let reply = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Test: A\r\nX-Test: B\r\nContent-Length: 2\r\n\r\n{}".to_vec();
    let (host, worker) = server(vec![reply.clone(), reply]);
    let access = endpoint(&host);
    let transport = HttpTransport::with_options(&access, &access, 0, options()).unwrap();
    let request = Request {
        path: "/".into(),
        ..Request::default()
    };
    let (one, other) = transport.concurrent_request(&request).await.unwrap();
    assert_eq!(one.body, other.body);
    assert_eq!(one.headers["x-test"], "A, B");
    assert!(one.headers.keys().all(|k| *k == k.to_ascii_lowercase()));
    assert_eq!(one.encoding.as_deref(), Some("utf-8"));
    assert_eq!(
        TemplateEngine::new()
            .when(
                "res.headers['X-TeSt'] == 'A, B'",
                minijinja::context!(res => one.to_template_value())
            )
            .unwrap()
            .to_json()
            .unwrap(),
        json!(true)
    );
    assert_eq!(worker.join().unwrap().len(), 2);
}

#[tokio::test]
async fn failure_trial_omits_both_status_codes() {
    let (host, worker) = server(vec![Vec::new(), Vec::new()]);
    let access = endpoint(&host);
    let transport = HttpTransport::with_options(&access, &access, 0, options()).unwrap();
    let request = Request {
        path: "/lost".into(),
        ..Request::default()
    };
    let ChallengeOutcome::Failure(trial) = transport.challenge(3, &request).await.unwrap() else {
        panic!("failure expected")
    };
    let value = serde_json::to_value(trial).unwrap();
    assert_eq!(value["status"], "failure");
    assert_eq!(value["name"], "3");
    assert_eq!(
        value["one"],
        json!({"url": format!("{host}/lost?"), "type": "unknown"})
    );
    assert_eq!(value["one"], value["other"]);
    assert_eq!(worker.join().unwrap().len(), 2);
}

#[tokio::test]
async fn connection_failure_precedes_response_encoding_and_type_conversion() {
    let (one, worker_one) = server(vec![
        b"HTTP/1.1 200 OK\r\nContent-Type: malformed\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let (other, worker_other) = server(vec![Vec::new()]);
    let transport =
        HttpTransport::with_options(&endpoint(&one), &endpoint(&other), 0, options()).unwrap();
    let request = Request {
        path: "/".into(),
        ..Request::default()
    };
    assert!(matches!(
        transport.challenge(1, &request).await.unwrap(),
        ChallengeOutcome::Failure(_)
    ));
    worker_one.join().unwrap();
    worker_other.join().unwrap();
}

#[tokio::test]
async fn post_read_timeout_propagates_instead_of_becoming_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (one, _) = listener.accept().unwrap();
        let (other, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(200));
        drop((one, other));
    });
    let transport = HttpTransport::with_options(
        &endpoint(&host),
        &endpoint(&host),
        2,
        TransportOptions {
            read_timeout: Some(Duration::from_millis(50)),
            ..options()
        },
    )
    .unwrap();
    let request = Request {
        path: "/".into(),
        method: jumeaux::models::HttpMethod::Post,
        ..Request::default()
    };
    assert_eq!(
        transport.challenge(1, &request).await.unwrap_err().kind,
        "ReadTimeout"
    );
    worker.join().unwrap();
}

#[tokio::test]
async fn cookies_are_shared_between_one_and_other_and_gzip_headers_are_preserved() {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(b"expanded").unwrap();
    let gzip = gzip.finish().unwrap();
    let mut compressed = format!(
        "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
        gzip.len()
    )
    .into_bytes();
    compressed.extend(&gzip);
    let (host, worker) = server(vec![
        b"HTTP/1.1 200 OK\r\nSet-Cookie: shared=yes; Path=/\r\nContent-Length: 0\r\n\r\n".to_vec(),
        compressed,
    ]);
    let access = endpoint(&host);
    let transport = HttpTransport::with_options(&access, &access, 0, options()).unwrap();
    let request = Request {
        path: "/".into(),
        ..Request::default()
    };
    transport.send_one(&request).await.unwrap();
    let response = transport.send_other(&request).await.unwrap();
    assert_eq!(&*response.body, b"expanded");
    assert_eq!(response.headers["content-encoding"], "gzip");
    assert_eq!(response.headers["content-length"], gzip.len().to_string());
    assert!(worker.join().unwrap()[1].contains("cookie: shared=yes"));
}

#[tokio::test]
async fn reqwest_retry_can_limit_count_but_classifier_has_no_response_headers() {
    let (host, worker) = server(vec![Vec::new(), Vec::new(), Vec::new()]);
    let client = reqwest::Client::builder()
        .no_proxy()
        .retry(
            reqwest::retry::for_host("127.0.0.1")
                .no_budget()
                .max_retries_per_request(2)
                .classify_fn(|r| {
                    if r.error().is_some() {
                        r.retryable()
                    } else {
                        r.success()
                    }
                }),
        )
        .build()
        .unwrap();
    assert!(client.get(host).send().await.is_err());
    assert_eq!(worker.join().unwrap().len(), 3);
}

#[tokio::test]
async fn retry_after_waits_before_repeating_get() {
    let (host, worker) = server(vec![
        b"HTTP/1.1 503 Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let access = endpoint(&host);
    let transport = HttpTransport::with_options(&access, &access, 1, options()).unwrap();
    let start = std::time::Instant::now();
    let response = transport
        .send_one(&Request {
            path: "/".into(),
            ..Request::default()
        })
        .await
        .unwrap();
    assert_eq!(response.status_code, 200);
    assert!(start.elapsed() >= Duration::from_secs(1));
    assert_eq!(worker.join().unwrap().len(), 2);
}

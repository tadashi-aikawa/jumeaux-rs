use jumeaux::{
    addons::{
        r#final::{self as final_addons, Pipeline},
        payload::{FinalPayload, FinalReference},
    },
    models::Report,
    notification::{Context, Verbosity},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

fn report() -> Report {
    serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn report_numbers_use_python_repr_without_rounding_big_integers() {
    let mut report = report();
    report.trials[0].json = Some(
        serde_json::from_str(
            r#"{"values":[1.5e-7,1e+2,0.00001,1.10,-0.0,18446744073709551617],"nested":{"x":1E2}}"#,
        )
        .unwrap(),
    );
    let expected: Value = serde_json::from_str(
        r#"{"values":[1.5e-07,100.0,1e-05,1.1,-0.0,18446744073709551617],"nested":{"x":100.0}}"#,
    )
    .unwrap();
    let input = report.trials[0].json.clone();
    for indent in [None, Some(-1), Some(0), Some(2), Some(4)] {
        let rendered = final_addons::json::Config {
            sysout: false,
            indent,
        }
        .render(&report)
        .unwrap();
        let parsed: Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed["trials"][0]["json"], expected);
        assert!(rendered.contains("1.5e-07"));
        assert!(rendered.contains("1e-05"));
    }
    let mut bytes = Vec::new();
    report
        .summary
        .output
        .write_report(&mut bytes, &report)
        .unwrap();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["trials"][0]["json"], expected);
    assert_eq!(report.trials[0].json, input);
}

#[test]
fn python_final_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/final-addons.json")).unwrap();
    assert_eq!(cases.len(), 41);
    for case in cases {
        let report: Report = serde_json::from_value(case["report"].clone()).unwrap();
        if case["name"] == "notify/context" {
            assert_eq!(
                jumeaux::report::template_value(&report).unwrap(),
                case["expected"]
            );
            continue;
        }
        let text = match case["addon"].as_str().unwrap() {
            "summary" => final_addons::summary::render(&report),
            "json" => serde_json::from_value::<final_addons::json::Config>(case["config"].clone())
                .unwrap()
                .render(&report)
                .unwrap(),
            "csv" => serde_json::from_value::<final_addons::csv::Config>(case["config"].clone())
                .unwrap()
                .render(&report)
                .unwrap(),
            _ => unreachable!(),
        };
        if case["config"]["sysout"] == true {
            assert_eq!(
                format!("{text}\n"),
                case["expected"]["stdout"],
                "{}",
                case["name"]
            );
        } else {
            let mut output = report.summary.output.clone();
            if case["addon"] == "csv" {
                output.encoding = "utf8".into();
            }
            let mut bytes = Vec::new();
            output.write_summary(&mut bytes, &text).unwrap();
            assert_eq!(
                hex(&bytes),
                case["expected"]["file_hex"],
                "{}",
                case["name"]
            );
        }
    }
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "jumeaux-final-{}-{}",
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

#[tokio::test]
async fn viewer_log_paths_keep_trailing_response_dir_slashes() {
    let temp = Temp::new();
    for suffix in ["", "/", "//"] {
        let mut report = report();
        report.key = "path-test".into();
        report.summary.output.response_dir = format!("{}{suffix}", temp.0.display());
        std::fs::create_dir_all(temp.0.join(&report.key)).unwrap();
        let expected_root = format!("{}/{}", report.summary.output.response_dir, report.key);
        let pipeline =
            Pipeline::build(&serde_json::from_value::<Vec<_>>(json!([{"name":"viewer"}])).unwrap())
                .unwrap();
        let result = pipeline
            .exec(
                FinalPayload {
                    output_summary: report.summary.output.clone(),
                    report,
                },
                &FinalReference { notifiers: None },
                &Context::default(),
                |_| Ok(()),
            )
            .await
            .unwrap();
        assert_eq!(
            result.logs[0].message,
            format!("[final/viewer] Create {expected_root}/index.html")
        );
        assert_eq!(
            result.logs[1].message,
            format!("[final/viewer] report.json doesn't exist in {expected_root}.")
        );
        assert_eq!(
            std::fs::read(temp.0.join("path-test/index.html")).unwrap(),
            final_addons::viewer::HTML
        );
    }
}

#[tokio::test]
async fn final_stream_delivers_logs_before_stdout_and_preserves_them_on_failure() {
    let temp = Temp::new();
    let mut report = report();
    report.summary.output.response_dir = temp.0.to_str().unwrap().into();
    std::fs::create_dir(temp.0.join(&report.key)).unwrap();
    let addons = serde_json::from_value::<Vec<_>>(json!([
        {"name":"viewer"}, {"name":"summary", "config":{"sysout":true}},
        {"name":"viewer"}, {"name":"csv", "config":{"column_names":["seq"],"output_path":temp.0.join("missing/out.csv")}},
        {"name":"json"}
    ])).unwrap();
    let events = std::cell::RefCell::new(Vec::new());
    let key = report.key.clone();
    let failure = Pipeline::build(&addons)
        .unwrap()
        .exec_with_log(
            FinalPayload {
                output_summary: report.summary.output.clone(),
                report,
            },
            &FinalReference { notifiers: None },
            &Context::default(),
            |_| {
                events.borrow_mut().push("stdout".to_owned());
                Ok(())
            },
            |log| events.borrow_mut().push(log.message.clone()),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(failure.error.kind, "IOError");
    assert!(failure.logs.is_empty());
    let events = events.into_inner();
    assert_eq!(events.len(), 7);
    assert!(events[0].starts_with("[final/viewer] Create "));
    assert_eq!(events[3], "stdout");
    assert_eq!(&events[..3], &events[4..]);
    assert!(!temp.0.join(key).join("report.json").exists());
}

#[tokio::test]
async fn notify_stream_delivers_start_log_before_waiting_for_webhook() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::Duration,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/webhook", listener.local_addr().unwrap());
    let (sent, received) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        assert_eq!(
            received.recv_timeout(Duration::from_secs(2)).unwrap(),
            "Notify using slack@v2"
        );
        let header = String::from_utf8(header).unwrap();
        let length = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        stream.read_exact(&mut vec![0; length]).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
            .unwrap();
    });
    let report = report();
    let pipeline = Pipeline::build(&serde_json::from_value::<Vec<_>>(json!([{"name":"notify","config":{"notifies":[{"notifier":"hook","message":"hello"}]}}])).unwrap()).unwrap();
    let result = pipeline
        .exec_with_log(
            FinalPayload {
                output_summary: report.summary.output.clone(),
                report,
            },
            &FinalReference {
                notifiers: Some(
                    serde_json::from_value(json!({"hook":{"type":"slack","version":2}})).unwrap(),
                ),
            },
            &Context {
                webhook_url: Some(url),
                ..Context::default()
            },
            |_| Ok(()),
            |log| {
                if log.message == "Notify using slack@v2" {
                    sent.send(log.message.clone()).unwrap();
                }
            },
        )
        .await
        .unwrap();
    assert!(result.logs.is_empty());
    assert_eq!(result.notifications.len(), 1);
    server.join().unwrap();
}

#[tokio::test]
async fn final_order_output_encoding_and_viewer_bytes() {
    let temp = Temp::new();
    let mut report = report();
    let output = &mut report.summary.output;
    output.response_dir = temp.0.to_str().unwrap().into();
    output.encoding = "utf-16".into();
    let root = temp.0.join(&report.key);
    std::fs::create_dir(&root).unwrap();
    let addons: Vec<jumeaux::models::Addon> = serde_json::from_value(json!([
        {"name":"viewer"}, {"name":"json", "config":{"indent":2}},
        {"name":"viewer"}, {"name":"summary"},
        {"name":"summary", "config":{"sysout":true}},
        {"name":"json", "config":{"sysout":true}},
        {"name":"csv", "config":{"column_names":["name"],"output_path":temp.0.join("result.csv")}}
    ]))
    .unwrap();
    let pipeline = Pipeline::build(&addons).unwrap();
    let mut stdout = String::new();
    let outcome = pipeline
        .exec(
            FinalPayload {
                output_summary: report.summary.output.clone(),
                report: report.clone(),
            },
            &FinalReference { notifiers: None },
            &Context::default(),
            |s| {
                stdout.push_str(s);
                Ok(())
            },
        )
        .await
        .unwrap();
    assert!(outcome.exit_code.is_none());
    assert_eq!(
        outcome
            .logs
            .iter()
            .filter(|l| l.level == Verbosity::Warning)
            .count(),
        2
    );
    assert_eq!(
        std::fs::read(root.join("index.html")).unwrap(),
        final_addons::viewer::HTML
    );
    for name in ["summary.txt", "report.json"] {
        assert!(
            std::fs::read(root.join(name))
                .unwrap()
                .starts_with(&[0xff, 0xfe])
        );
    }
    assert_eq!(
        std::fs::read_to_string(temp.0.join("result.csv")).unwrap(),
        "1\n2\n"
    );
    assert_eq!(
        stdout,
        format!(
            "{}\n{}\n",
            final_addons::summary::render(&report),
            final_addons::json::Config {
                sysout: true,
                indent: None
            }
            .render(&report)
            .unwrap()
        )
    );
}

#[tokio::test]
async fn notify_conditions_nulls_and_errors_stop_later_addons() {
    let temp = Temp::new();
    let mut report = report();
    report.notifiers =
        Some(serde_json::from_value(json!({"hook":{"type":"slack","version":2}})).unwrap());
    let addons: Vec<jumeaux::models::Addon> = serde_json::from_value(json!([{"name":"notify","config":{"notifies":[
        {"notifier":"missing","message":"skip", "when":"False"},
        {"notifier":"hook","message":"{{ title }} / {{ description }}", "when":"description is none"}
    ]}}])).unwrap();
    let pipeline = Pipeline::build(&addons).unwrap();
    let payload = FinalPayload {
        output_summary: report.summary.output.clone(),
        report: report.clone(),
    };
    let outcome = pipeline
        .exec(
            payload,
            &FinalReference {
                notifiers: report.notifiers.clone(),
            },
            &Context::default(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(outcome.notifications.len(), 1);
    assert!(outcome.notifications[0].skipped);
    assert_eq!(
        outcome
            .logs
            .iter()
            .filter(|l| l.level == Verbosity::Warning)
            .count(),
        3
    );

    let addons: Vec<jumeaux::models::Addon> = serde_json::from_value(json!([
        {"name":"notify"}, {"name":"csv", "config":{"column_names":[], "output_path":temp.0.join("should-not-exist")}}
    ])).unwrap();
    let pipeline = Pipeline::build(&addons).unwrap();
    let result = pipeline
        .exec(
            FinalPayload {
                output_summary: report.summary.output.clone(),
                report,
            },
            &FinalReference { notifiers: None },
            &Context::default(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.logs.len(), 2);
    assert!(!temp.0.join("should-not-exist").exists());
}

#[test]
fn final_validates_even_unreached_templates_and_miroir() {
    for addon in [
        json!({"name":"notify","config":{"notifies":[{"notifier":"x","message":"{{ broken", "when":"False"}]}}),
        json!({"name":"miroir", "config":{"table":"a","bucket":"b", "local_stack":{"use":false}, "endpoints":{}}}),
    ] {
        assert!(Pipeline::build(&[serde_json::from_value(addon).unwrap()]).is_err());
    }
}

#[tokio::test]
async fn notify_sends_all_selected_messages_before_reporting_http_errors() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let receiver = std::thread::spawn(move || {
        let mut bodies = vec![];
        for status in [400, 200] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = vec![];
            let mut byte = [0];
            while !bytes.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
            }
            let header = String::from_utf8(bytes).unwrap();
            let size: usize = header
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .unwrap()
                .parse()
                .unwrap();
            assert!(!header.to_ascii_lowercase().contains("content-type:"));
            let mut body = vec![0; size];
            stream.read_exact(&mut body).unwrap();
            bodies.push(serde_json::from_slice::<Value>(&body).unwrap());
            write!(
                stream,
                "HTTP/1.1 {status} Result\r\nContent-Length: 4\r\nConnection: close\r\n\r\noops"
            )
            .unwrap();
        }
        bodies
    });
    let mut report = report();
    report.notifiers =
        Some(serde_json::from_value(json!({"hook":{"type":"slack","version":2}})).unwrap());
    let addons: Vec<jumeaux::models::Addon> = serde_json::from_value(json!([
        {"name":"notify", "config":{"notifies":[
            {"notifier":"hook","message":"{{ title }}: {{ summary.status.different }}"},
            {"notifier":"missing","message":"never", "when":"False"},
            {"notifier":"hook","message":"{{ description }}", "when":"description is none"}
        ]}}, {"name":"json","config":{"sysout":true}}
    ]))
    .unwrap();
    let pipeline = Pipeline::build(&addons).unwrap();
    let result = pipeline
        .exec(
            FinalPayload {
                output_summary: report.summary.output.clone(),
                report: report.clone(),
            },
            &FinalReference {
                notifiers: report.notifiers,
            },
            &Context {
                webhook_url: Some(url),
                thread_ts: Some("123".into()),
            },
            |_| panic!("later final must not execute"),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, Some(1));
    assert_eq!(result.notifications.len(), 2);
    assert_eq!(result.logs.last().unwrap().message, "oops");
    assert_eq!(
        receiver.join().unwrap(),
        vec![
            json!({"text":"Simple: 1","link_names":1,"thread_ts":"123"}),
            json!({"text":"None","link_names":1,"thread_ts":"123"})
        ]
    );
}

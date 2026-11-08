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

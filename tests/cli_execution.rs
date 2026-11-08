use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "jumeaux-cli-execution-{}-{}",
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
fn command(temp: &Temp) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_jumeaux"));
    c.current_dir(&temp.0)
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env_remove("SLACK_INCOMING_WEBHOOKS_URL");
    c
}

#[test]
fn connection_retry_warnings_match_urllib3_at_every_verbosity() {
    let temp = Temp::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let os_error = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap_err();
    let code = os_error.raw_os_error().unwrap();
    let detail = os_error.to_string();
    let detail = detail.strip_suffix(&format!(" (os error {code})")).unwrap();
    std::fs::write(temp.0.join("requests"), "/path?q=1\n").unwrap();
    let config = json!({"one":{"name":"One","host":format!("http://127.0.0.1:{port}")},"other":{"name":"Other","host":format!("http://127.0.0.1:{port}")},"output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"},"final":[{"name":"json"}]}});
    std::fs::write(
        temp.0.join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    for verbosity in [None, Some("-v"), Some("-vv"), Some("-vvv")] {
        let mut cmd = command(&temp);
        cmd.args(["run", "requests"]);
        if let Some(flag) = verbosity {
            cmd.arg(flag);
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        let warnings: Vec<_> = stderr
            .lines()
            .filter(|l| l.contains("Retrying ("))
            .collect();
        assert_eq!(warnings.len(), 6, "{stderr}");
        for remaining in [2, 1, 0] {
            let expected = format!(
                "Retrying (Retry(total={remaining}, connect=None, read=None, redirect=None, status=None)) after connection broken by 'NewConnectionError(\"HTTPConnection(host='127.0.0.1', port={port}): Failed to establish a new connection: [Errno {code}] {detail}\")': /path?q=1"
            );
            assert_eq!(
                warnings.iter().filter(|line| **line == expected).count(),
                2,
                "{warnings:#?}"
            );
        }
    }
    let output = command(&temp)
        .args(["run", "requests", "--max-retries", "0"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Retrying ("));
}

#[test]
fn retry_restores_raw_comparison_and_emits_both_warning_sources() {
    let temp = Temp::new();
    // LISTENを持たないローカルportでfailure Trialを作る。外部APIへ送らない。
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut source: Value =
        serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap();
    source["summary"]["output"]["logger"] = json!({"old":true});
    source["unknown"] = json!(true);
    source["summary"]["tags"] = json!(["old-tag"]);
    source["addons"]["final"] = json!([{"name":"json"}]);
    for side in ["one", "other"] {
        source["summary"][side]["host"] = json!(format!("http://127.0.0.1:{port}"));
    }
    source["trials"][0]["method"] = json!("POST");
    source["trials"][0]["raw"] = json!("raw body 日本語");
    for with_comparison in [false, true] {
        if with_comparison {
            source["summary"]["comparison"] =
                json!({"judge_response_header":true,"ignore_response_header_keys":["x-custom"]});
        }
        std::fs::write(
            temp.0.join("source.json"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        let result = command(&temp)
            .args([
                "retry",
                "source.json",
                "--max-retries",
                "0",
                "--title",
                "Retry",
                "--threads",
                "2",
            ])
            .output()
            .unwrap();
        assert_eq!(
            result.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stderr = String::from_utf8(result.stderr).unwrap();
        assert!(stderr.contains("report.unknown"));
        assert!(stderr.contains("summary.output.logger"));
        assert_eq!(stderr.contains("comparison: missing"), !with_comparison);
        let report: Value = serde_json::from_slice(
            &std::fs::read(temp.0.join("responses/latest/report.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["retry_hash"], source["key"]);
        assert_ne!(report["key"], source["key"]);
        assert_eq!(
            report["summary"]["status"],
            json!({"same":0,"different":0,"failure":2})
        );
        assert_eq!(report["trials"][0]["raw"], source["trials"][0]["raw"]);
        assert_eq!(report["summary"]["tags"], json!([]));
        assert_eq!(report["title"], "Retry");
        assert_eq!(
            report["summary"]["concurrency"],
            json!({"threads":2,"processes":1})
        );
        assert_eq!(
            report["summary"]["comparison"],
            if with_comparison {
                source["summary"]["comparison"].clone()
            } else {
                json!({"judge_response_header":false,"ignore_response_header_keys":["Content-Length","Date"]})
            }
        );
    }
}

#[test]
fn verbose_yaml_does_not_expose_number_representation_and_empty_run_succeeds() {
    let temp = Temp::new();
    std::fs::write(temp.0.join("requests"), "").unwrap();
    let config = json!({"one":{"name":"One","host":"http://127.0.0.1:1"},"other":{"name":"Other","host":"http://127.0.0.1:1"},
        "output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"},"reqs2reqs":[{"name":"head","config":{"size":2}}],"final":[{"name":"json"}]}});
    std::fs::write(
        temp.0.join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let output = command(&temp)
        .args(["run", "requests", "-vv"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("$serde_json"));
    let yaml=stderr.split("Merge with yaml files or report, and args\n----\n").nth(1).unwrap().split("\n\n\n--------------------------------------------------------------------------------").next().unwrap();
    let value: Value = serde_yaml_ng::from_str(yaml).unwrap();
    assert_eq!(value["addons"]["reqs2reqs"][0]["config"]["size"], 2);
    let report: Value = serde_json::from_slice(
        &std::fs::read(temp.0.join("responses/latest/report.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        report["summary"]["status"],
        json!({"same":0,"different":0,"failure":0})
    );
    assert_eq!(report["trials"], json!([]));
}

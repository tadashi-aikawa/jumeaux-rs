#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "jumeaux-exec-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_jumeaux"));
        c.current_dir(&self.0);
        c
    }
    fn write(&self, name: &str, value: &Value) {
        std::fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn addon(options: Value) -> Value {
    json!({"name":"exec","config":{"command":["python3",format!("{}/tests/fixtures/exec.py",env!("CARGO_MANIFEST_DIR"))],"options":options}})
}
fn report(temp: &Temp) -> Value {
    let mut r: Value =
        serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap();
    r["trials"] = json!([]);
    r["summary"]["output"]["response_dir"] = json!(temp.0.join("responses"));
    r["addons"] = json!({"log2reqs":addon(json!({"marker":"marker"})),"final":[addon(json!({"marker":"marker"})),{"name":"json"}]});
    r
}
#[test]
fn retry_requires_permission_before_validate_even_for_unused_log2reqs() {
    let temp = Temp::new();
    temp.write("report.json", &report(&temp));
    let refused = temp
        .command()
        .args(["retry", "report.json"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--allow-exec"));
    assert!(!temp.0.join("marker").exists());
    assert!(!temp.0.join("responses").exists());
    let accepted = temp
        .command()
        .args(["retry", "report.json", "--allow-exec"])
        .output()
        .unwrap();
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(temp.0.join("marker")).unwrap(),
        "log2reqs validate\nfinal validate\nfinal payload\n"
    );
    let saved: Value = serde_json::from_slice(
        &std::fs::read(temp.0.join("responses/latest/report.json")).unwrap(),
    )
    .unwrap();
    assert!(saved.get("allow_exec").is_none());
    let again = temp
        .command()
        .args(["retry", "responses/latest/report.json"])
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(temp.0.join("marker"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}
#[test]
fn validate_failure_precedes_input_reads_and_artifact_creation() {
    let temp = Temp::new();
    temp.write("config.yml",&json!({"one":{"name":"one","host":"http://127.0.0.1:1"},"other":{"name":"other","host":"http://127.0.0.1:1"},"output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"},"final":[addon(json!({"action":"invalid_validate"}))]}}));
    let out = temp
        .command()
        .args(["run", "missing", "--config", "config.yml"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("config.yml: addons.final[0]"), "{error}");
    assert!(error.contains("valid"), "{error}");
    assert!(!temp.0.join("responses").exists());
}
#[test]
fn sigint_and_sigterm_stop_validate_and_final_with_conventional_exit_codes() {
    use std::io::{BufRead, BufReader};
    for mode in ["validate", "payload"] {
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let temp = Temp::new();
            let mut r = report(&temp);
            r["addons"] = json!({"log2reqs":{"name":"plain"},"final":[addon(json!({"hang_mode":mode})),{"name":"json"}]});
            temp.write("report.json", &r);
            let mut child = temp
                .command()
                .args(["retry", "report.json", "--allow-exec"])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut reader = BufReader::new(child.stderr.take().unwrap());
            let mut line = String::new();
            loop {
                line.clear();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line.contains("ready") {
                    break;
                }
            }
            let start = Instant::now();
            // SAFETY: このテストがspawnしたCLIだけへ送信する。
            assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
            let status = child.wait().unwrap();
            assert_eq!(status.code(), Some(128 + signal));
            assert!(start.elapsed() < Duration::from_secs(3));
            assert!(!temp.0.join("responses/latest/report.json").exists());
        }
    }
}

//! ローカル記録用受信先とPython保存済み基準による統合比較。
//! 実行: cargo test --test challenge_e2e -- --ignored --nocapture
#[path = "support/progress.rs"]
mod progress;
use jumeaux::{
    addons::log2reqs,
    config,
    executor::Pipeline,
    http::{HttpTransport, TransportOptions},
    notification::Context,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

struct Receiver {
    child: Child,
    temp: PathBuf,
}
impl Drop for Receiver {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.temp);
    }
}
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()));
        } else {
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }
}
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), out);
            } else if entry.file_name() != ".gitkeep" {
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
    for dir in ["one", "other", "one-props", "other-props"] {
        visit(root, &root.join(dir), &mut out);
    }
    out
}
fn normalize(trials: &mut Value, port: u16) {
    for trial in trials.as_array_mut().unwrap() {
        chrono::DateTime::parse_from_rfc3339(trial["request_time"].as_str().unwrap()).unwrap();
        trial.as_object_mut().unwrap().remove("request_time");
        for side in ["one", "other"] {
            if let Some(seconds) = trial[side].get("response_sec") {
                assert!(seconds.as_f64().unwrap() >= 0.0);
            }
            let summary = trial[side].as_object_mut().unwrap();
            summary.remove("response_sec");
            if let Some(headers) = summary.get_mut("response_header") {
                *headers = Value::Object(
                    headers
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                        .collect(),
                );
            }
            let url = summary["url"]
                .as_str()
                .unwrap()
                .replace(&format!("localhost:{port}"), "localhost:8000");
            summary.insert("url".into(), json!(url));
        }
    }
}
fn wait_ready(receiver: &mut Receiver, path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(
            receiver.child.try_wait().unwrap().is_none(),
            "receiver exited"
        );
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "receiver readiness timeout"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[tokio::test]
#[ignore = "Python基準の.venvとローカル受信先を使う受入試験"]
async fn templates_match_python_saved_trials_and_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = root.join("compat/python-baseline");
    let source = std::env::var_os("JUMEAUX_PYTHON_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.parent().unwrap().join("jumeaux"));
    let temp = std::env::temp_dir().join(format!("jumeaux-challenge-e2e-{}", std::process::id()));
    std::fs::create_dir(&temp).unwrap();
    let child = Command::new(source.join(".venv/bin/python"))
        .arg("-B")
        .arg(root.join("compat/harness/receivers.py"))
        .arg(&temp)
        .args(["--api-port", "0", "--notification-port", "0"])
        .env("PYTHONPATH", &source)
        .env("PYTHONWARNINGS", "ignore")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut receiver = Receiver {
        child,
        temp: temp.clone(),
    };
    wait_ready(&mut receiver, &temp.join("receiver.ready"));
    let port: u16 = std::fs::read_to_string(temp.join("receiver.ports"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut totals = (0, 0, 0);
    for name in [
        "simple",
        "ignore",
        "xml",
        "html",
        "post",
        "threads_2",
        "processes_2",
        "threads_and_processes",
        "all_same",
        "force_json",
        "ignore_order",
        "judge_response_headers",
        "path_custom",
        "path_empty",
        "query_custom",
        "request_headers",
        "root_array",
        "notifier",
        "encoding_fixed",
        "output_encoding",
        "header_ignore_case",
        "header_condition_case",
        "verbose_2",
        "verbose_3",
    ] {
        let case = baseline.join(name);
        let work = temp.join(name);
        std::fs::create_dir(&work).unwrap();
        copy_dir(&case.join("api"), &work.join("api"));
        writeln!(receiver.child.stdin.as_mut().unwrap(), "{}", work.display()).unwrap();
        receiver.child.stdin.as_mut().unwrap().flush().unwrap();
        wait_ready(&mut receiver, &work.join("receiver.ready"));
        let migrated = work.join("config.yml");
        std::fs::write(
            &migrated,
            std::fs::read_to_string(case.join("config.yml"))
                .unwrap()
                .replace("    - name: viewer\n", ""),
        )
        .unwrap();
        let mut config = config::load(&[migrated], &[]).unwrap();
        config.one.host = config
            .one
            .host
            .replace("localhost:8000", &format!("localhost:{port}"));
        config.other.host = config
            .other
            .host
            .replace("localhost:8000", &format!("localhost:{port}"));
        config.output.response_dir = work.join("responses").to_str().unwrap().into();
        config.input_files = Some(vec![case.join("requests").to_str().unwrap().into()]);
        let expected_case = baseline.join(if name == "header_ignore_case" {
            "header_ignore_control"
        } else {
            name
        });
        let expected_dir = expected_case.join("responses/latest");
        let report_bytes = std::fs::read(expected_dir.join("report.json")).unwrap();
        let report_text = jumeaux::compat::encoding::decode(
            &report_bytes,
            &config.output.encoding,
            jumeaux::compat::encoding::Errors::Strict,
        )
        .unwrap();
        let expected: Value = serde_json::from_str(&report_text).unwrap();
        let concurrency = &expected["summary"]["concurrency"];
        config.threads = concurrency["threads"].as_i64().unwrap();
        if concurrency["processes"].as_i64().unwrap() != 1 {
            config.processes = concurrency["processes"].as_i64();
        }
        if name == "threads_and_processes" {
            config.threads = 4;
        }
        let input = log2reqs::prepare(&config, &Context::default())
            .await
            .unwrap();
        assert!(input.exit_code.is_none());
        let transport = Arc::new(
            HttpTransport::with_options(
                &config.one,
                &config.other,
                config.max_retries,
                TransportOptions {
                    trust_env: false,
                    ..Default::default()
                },
            )
            .unwrap(),
        );
        let result = Arc::new(Pipeline::build(&config).unwrap())
            .exec(transport, input.requests, "test-key", None)
            .await
            .unwrap();
        assert_eq!(
            json!(result.concurrency),
            *concurrency,
            "{name} concurrency"
        );
        assert_eq!(result.comparison, config.comparison());
        if name.starts_with("verbose_") {
            progress::assert_verbose_materials(
                &result.logs,
                &std::fs::read_to_string(case.join("run.stderr")).unwrap(),
                if name == "verbose_2" { 2 } else { 3 },
                port,
            );
        }
        let mut actual_trials = json!(result.trials);
        let mut expected_trials = expected["trials"].clone();
        normalize(&mut actual_trials, port);
        normalize(&mut expected_trials, 8000);
        assert_eq!(actual_trials, expected_trials, "{name} trials");
        let actual_files = files(&work.join("responses/test-key"));
        let expected_files = files(&expected_dir);
        assert_eq!(
            actual_files.keys().collect::<Vec<_>>(),
            expected_files.keys().collect::<Vec<_>>(),
            "{name} file set"
        );
        for (path, body) in &actual_files {
            assert_eq!(body, &expected_files[path], "{name}: {path}");
        }
        assert_eq!(
            std::fs::read_link(work.join("responses/latest")).unwrap(),
            Path::new("test-key")
        );
        let records = std::fs::read_to_string(work.join("api.jsonl")).unwrap();
        assert_eq!(
            records.lines().count(),
            result.trials.len() * 2,
            "{name} receiver count"
        );
        println!(
            "{name}: {} trials matched, {}/{} saved files matched",
            result.trials.len(),
            actual_files.len(),
            expected_files.len()
        );
        totals.0 += 1;
        totals.1 += result.trials.len();
        totals.2 += actual_files.len();
    }
    println!(
        "TOTAL: {} cases, {} trials, {} saved files matched",
        totals.0, totals.1, totals.2
    );
}

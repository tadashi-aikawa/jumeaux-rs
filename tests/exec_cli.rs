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
    let mut source = report(&temp);
    source["addons"]["final"][0]["config"]["env"] = json!({"JUMEAUX_REMOVE":null});
    source["addons"]["final"][0]["config"]["options"]["nested"] =
        json!({"null":null,"list":[null]});
    temp.write("report.json", &source);
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
    assert_eq!(
        saved["addons"]["final"][0]["config"]["env"],
        json!({"JUMEAUX_REMOVE":null})
    );
    assert_eq!(
        saved["addons"]["final"][0]["config"]["options"]["nested"],
        json!({"null":null,"list":[null]})
    );
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
fn retry_rejects_a_transformer_even_when_its_when_is_false() {
    let temp = Temp::new();
    let mut r = report(&temp);
    r["addons"] = json!({"log2reqs":{"name":"plain"},"res2res":[{"name":"json","config":{"when":"False","transformer":addon(json!({"marker":"marker"}))["config"]}}],"final":[{"name":"json"}]});
    temp.write("report.json", &r);
    let refused = temp
        .command()
        .args(["retry", "report.json"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--allow-exec"));
    assert!(!temp.0.join("marker").exists());
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
        "res2res validate\n"
    );
}

#[test]
fn all_host_checks_precede_validate_and_skipped_tags_are_not_checked() {
    let temp = Temp::new();
    let mut config = json!({"one":{"name":"one","host":"http://127.0.0.1:1"},"other":{"name":"other","host":"http://127.0.0.1:1"},"output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"},"res2res":[addon(json!({"marker":"marker"}))],"final":[{"name":"exec","tags":["skip"],"config":{"command":["/no/such/executable"]}}]}});
    temp.write("config.yml", &config);
    let failed = temp
        .command()
        .args(["run", "missing", "--config", "config.yml"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("executable not found"));
    assert!(!temp.0.join("marker").exists());
    let skipped = temp
        .command()
        .args([
            "run",
            "missing",
            "--config",
            "config.yml",
            "--skip-addon-tag",
            "skip",
        ])
        .output()
        .unwrap();
    assert_eq!(skipped.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&skipped.stderr).contains("missing"));
    assert_eq!(
        std::fs::read_to_string(temp.0.join("marker")).unwrap(),
        "res2res validate\n"
    );
    config["addons"]["final"] = json!([]);
    config["addons"]["res2res"][0]["config"]["env"] = json!({"INVALID=NAME":"value"});
    temp.write("config.yml", &config);
    let invalid = temp
        .command()
        .args(["run", "missing", "--config", "config.yml"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid environment"));
    assert_eq!(
        std::fs::read_to_string(temp.0.join("marker")).unwrap(),
        "res2res validate\n"
    );
    assert!(!temp.0.join("responses").exists());
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

#[test]
fn signals_stop_all_running_trial_commands_and_skip_final() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    for signal in [libc::SIGINT, libc::SIGTERM] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let receiver = std::thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            }
        });
        let temp = Temp::new();
        temp.write("config.yml", &json!({"one":{"name":"one","host":host},"other":{"name":"other","host":host},"output":{"response_dir":"responses"},"addons":{"log2reqs":{"name":"plain"},"res2res":[addon(json!({"hang_mode":"payload","pid_file":"pids"}))],"final":[{"name":"json"}]}}));
        std::fs::write(temp.0.join("requests"), "/first\n/second\n").unwrap();
        let mut child = temp
            .command()
            .args([
                "run",
                "requests",
                "--config",
                "config.yml",
                "--threads",
                "2",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stderr.take().unwrap());
        let mut ready = 0;
        let mut line = String::new();
        while ready < 2 {
            line.clear();
            assert_ne!(
                reader.read_line(&mut line).unwrap(),
                0,
                "CLI ended before both commands started"
            );
            if line.contains("ready") {
                ready += 1;
            }
        }
        let pids: Vec<i32> = std::fs::read_to_string(temp.0.join("pids"))
            .unwrap()
            .lines()
            .map(|p| p.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 2);
        let start = Instant::now();
        // SAFETY: このテストが所有するCLIだけを中断する。
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        assert_eq!(child.wait().unwrap().code(), Some(128 + signal));
        assert!(start.elapsed() < Duration::from_secs(3));
        for pid in pids {
            // SAFETY: signal 0は生存確認だけで、プロセスを変更しない。
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "external process {pid} survived"
            );
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );
        }
        assert!(!temp.0.join("responses/latest/report.json").exists());
        receiver.join().unwrap();
    }
}

#[test]
fn nine_layer_cli_preserves_order_mutations_context_and_call_counts() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    for stored in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let receiver = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                loop {
                    line.clear();
                    assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            }
        });
        let temp = Temp::new();
        let mut addons = json!({});
        for layer in jumeaux::addons::Layer::ALL {
            let changes = match layer {
                jumeaux::addons::Layer::Res2Res => json!({"tags":["external"]}),
                jumeaux::addons::Layer::Res2Dict => json!({"result":{"null":null,"external":true}}),
                jumeaux::addons::Layer::StoreCriterion => json!({"stored":stored}),
                jumeaux::addons::Layer::Dump => json!({"body":"AGFi/w=="}),
                _ => json!({}),
            };
            let a = addon(json!({"trace":"trace","set":changes}));
            addons[layer.name()] = if layer == jumeaux::addons::Layer::Log2Reqs {
                a
            } else {
                json!([a])
            };
        }
        // resultがあってもexecを経由し、後続builtinはforceなしでその値を維持する。
        addons["res2dict"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"name":"json"}));
        addons["res2dict"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"json"}));
        addons["final"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"json"}));
        temp.write("config.yml", &json!({"one":{"name":"one","host":host},"other":{"name":"other","host":host},"output":{"response_dir":"responses"},"addons":addons}));
        let output = temp
            .command()
            .args(["run", "missing-is-owned-by-exec", "--config", "config.yml"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        receiver.join().unwrap();
        let trace: Vec<Value> = std::fs::read_to_string(temp.0.join("trace"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(trace.len(), if stored { 21 } else { 19 });
        for (item, layer) in trace[..9].iter().zip(jumeaux::addons::Layer::ALL) {
            assert_eq!(item["mode"], "validate");
            assert_eq!(item["layer"], layer.name());
            assert!(item["context"]["trial_seq"].is_null());
            assert!(item["context"]["side"].is_null());
        }
        let mut expected = vec![
            ("log2reqs", None),
            ("reqs2reqs", None),
            ("res2res", Some("one")),
            ("res2res", Some("other")),
            ("res2dict", Some("one")),
            ("res2dict", Some("other")),
            ("judgement", None),
            ("store_criterion", None),
        ];
        if stored {
            expected.extend([("dump", Some("one")), ("dump", Some("other"))]);
        }
        expected.extend([("did_challenge", None), ("final", None)]);
        for (item, (layer, side)) in trace[9..].iter().zip(expected) {
            assert_eq!(item["mode"], "payload");
            assert_eq!(item["layer"], layer);
            assert_eq!(item["context"]["side"], json!(side));
            assert_eq!(
                item["context"]["trial_seq"],
                if ["log2reqs", "reqs2reqs", "final"].contains(&layer) {
                    Value::Null
                } else {
                    json!(1)
                }
            );
        }
        let report: Value = serde_json::from_slice(
            &std::fs::read(temp.0.join("responses/latest/report.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["trials"][0]["tags"], json!(["external"]));
        assert_eq!(report["trials"][0]["status"], "same");
        for side in ["one", "other"] {
            let summary = &report["trials"][0][side];
            if stored {
                assert_eq!(
                    std::fs::read(
                        temp.0
                            .join("responses/latest")
                            .join(summary["file"].as_str().unwrap())
                    )
                    .unwrap(),
                    b"\0ab\xff"
                );
                let props: Value = serde_json::from_slice(
                    &std::fs::read(
                        temp.0
                            .join("responses/latest")
                            .join(summary["prop_file"].as_str().unwrap()),
                    )
                    .unwrap(),
                )
                .unwrap();
                // 通信時はnullを保持するが、保存時は従来のOwlMixin互換で除く。
                assert_eq!(props, json!({"external":true}));
            } else {
                assert!(summary.get("file").is_none());
            }
        }
    }
}

fn script(temp: &Temp, path: &str, message: &Value) -> std::process::Output {
    script_bytes(temp, path, &serde_json::to_vec(message).unwrap())
}
fn script_bytes(temp: &Temp, path: &str, message: &[u8]) -> std::process::Output {
    use std::io::Write;
    let mut child = Command::new("python3")
        .arg(path)
        .current_dir(&temp.0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(message).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn generated_samples_handle_special_diff_paths_large_tables_and_nonfinite_output() {
    let temp = Temp::new();
    assert!(
        temp.command()
            .args(["init", "addon"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let paths = json!([
        "root<'a>b'>",
        "root<'quote\\\'key'>",
        "root<'back\\\\slash'>",
        "root<'line\\nkey'>",
        "root<'array'><0>",
        "root<'missing'>",
        "root<'null'>"
    ]);
    let mut message = json!({"protocol_version":1,"layer":"judgement","mode":"payload","options":{"values":["apple","None"]},"payload":{"diffs_by_cognition":{"known":{"added":["root<'keep'>"],"changed":[],"removed":[]},"ignore_values":{"added":[],"changed":["root<'earlier'>"],"removed":[]},"unknown":{"added":[],"changed":paths,"removed":[]}},"regard_as_same_body":false,"regard_as_same_header":false},"reference":{"dict_one":{"a>b":"apple","quote'key":"apple","back\\slash":"apple","line\nkey":"apple","array":["apple"],"null":null},"dict_other":{}}});
    let path = "addon_sample/judgement/ignore_values.py";
    let output = script(&temp, path, &message);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        actual["diffs_by_cognition"]["known"],
        message["payload"]["diffs_by_cognition"]["known"]
    );
    let moved = actual["diffs_by_cognition"]["ignore_values"]["changed"]
        .as_array()
        .unwrap();
    assert_eq!(moved[0], "root<'earlier'>");
    assert_eq!(&moved[1..], paths.as_array().unwrap());
    assert!(actual["diffs_by_cognition"].get("unknown").is_none());
    assert_eq!(actual["regard_as_same_body"], true);
    assert_eq!(actual["regard_as_same_header"], false);
    message["payload"]["diffs_by_cognition"]["unknown"]["changed"] =
        json!(["root<'missing'><invalid>"]);
    assert!(!script(&temp, path, &message).status.success());
    message["options"]["values"] = json!([]);
    assert_eq!(
        serde_json::from_slice::<Value>(&script(&temp, path, &message).stdout).unwrap(),
        message["payload"]
    );

    let rows: Vec<_> = (0..3000)
        .map(|seq| json!({"seq":seq,"name":"wide table row","path":"/some/path","status":"same"}))
        .collect();
    let table = json!({"protocol_version":1,"layer":"final","mode":"payload","options":{"columns":["seq","name","path","status"]},"payload":{"report":{"trials":rows},"output_summary":{}},"reference":null});
    let output = script(&temp, "addon_sample/final/table.py", &table);
    assert!(output.status.success());
    assert!(output.stderr.len() > 65536);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).lines().count(),
        3001
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        table["payload"]
    );
    let invalid = br#"{"protocol_version":1,"layer":"reqs2reqs","mode":"payload","options":{"path":"/added"},"payload":{"requests":[],"number":NaN},"reference":null}"#;
    let output = script_bytes(&temp, "addon_sample/reqs2reqs/add_path.py", invalid);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
#[test]
fn init_addon_generates_working_stdlib_templates_without_overwrite() {
    let temp = Temp::new();
    let out = temp.command().args(["init", "addon"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!temp.0.join("config.yml").exists());
    assert!(
        !temp
            .command()
            .args(["init", "addon"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let cases = [
        (
            "reqs2reqs",
            "add_path",
            json!({"path":"/added"}),
            json!({"requests":[]}),
        ),
        (
            "judgement",
            "ignore_values",
            json!({"values":["apple"]}),
            json!({"diffs_by_cognition":{"unknown":{"added":[],"changed":["root<'fruit'>"],"removed":[]}},"regard_as_same_body":false,"regard_as_same_header":false}),
        ),
        (
            "final",
            "table",
            json!({"columns":["seq","name","path","status"]}),
            json!({"report":{"trials":[{"seq":1,"name":"name\ntab\t","path":"/a","status":"same"}]},"output_summary":{}}),
        ),
    ];
    for (layer, name, options, payload) in cases {
        let path = format!("addon_sample/{layer}/{name}.py");
        let mut message = json!({"protocol_version":1,"layer":layer,"mode":"validate","options":options,"payload":null,"reference":null});
        let valid = script(&temp, &path, &message);
        assert!(valid.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&valid.stdout).unwrap(),
            json!({"valid":true})
        );
        assert!(valid.stderr.is_empty());
        message["mode"] = json!("payload");
        message["payload"] = payload.clone();
        message["reference"] =
            json!({"dict_one":{"fruit":"apple"},"dict_other":{"fruit":"orange"}});
        let out = script(&temp, &path, &message);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let result: Value = serde_json::from_slice(&out.stdout).unwrap();
        match layer {
            "reqs2reqs" => assert_eq!(result["requests"][0]["path"], "/added"),
            "judgement" => {
                assert_eq!(result["regard_as_same_body"], true);
                assert_eq!(result["regard_as_same_header"], false);
                assert!(result["diffs_by_cognition"].get("unknown").is_none());
            }
            _ => {
                assert_eq!(result, payload);
                assert!(String::from_utf8_lossy(&out.stderr).contains("name\\ntab\\t"));
            }
        }
        message["options"] = json!({"unknown":true});
        assert!(!script(&temp, &path, &message).status.success());
    }
    let readme = std::fs::read_to_string(temp.0.join("addon_sample/README.md")).unwrap();
    let source = readme
        .split("```python\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    std::fs::write(temp.0.join("wrap.py"), source).unwrap();
    let message = json!({"protocol_version":1,"layer":"res2res","mode":"json_transformer","options":{},"payload":{"body":"eyJhIjoxfQ==","encoding":null},"reference":null});
    let out = script(&temp, "wrap.py", &message);
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap(),
        json!("{\"wrap\": {\"a\": 1}}")
    );
}

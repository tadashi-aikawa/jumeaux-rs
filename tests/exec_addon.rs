#![cfg(unix)]
use jumeaux::{
    addons::{
        self, Layer,
        exec::{Invocation, Runtime, wire::Wire},
        payload::*,
    },
    config::Config,
    models::*,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

fn addon(action: &str) -> Addon {
    serde_json::from_value(json!({"name":"exec", "config":{
        "command":["python3",format!("{}/tests/fixtures/exec.py",env!("CARGO_MANIFEST_DIR"))],
        "options":{"action":action}}}))
    .unwrap()
}
fn runtime(layer: Layer, action: &str) -> Arc<Runtime> {
    Runtime::prepare_layer(layer, &[addon(action)]).unwrap()
}
fn response() -> Response {
    Response {
        body: (0..=255).collect::<Vec<_>>().into(),
        encoding: None,
        headers: Default::default(),
        url: "http://localhost/".into(),
        status_code: 200,
        elapsed: 0.125678,
        elapsed_sec: 0.125,
        response_type: "json".into(),
    }
}
fn roundtrip<S: addons::Stage>(payload: S::Payload, reference: S::Reference)
where
    S::Payload: Wire,
    S::Reference: Wire,
{
    let runtime = runtime(S::LAYER, "echo");
    runtime.validate().unwrap();
    let before = payload.wire().unwrap();
    let after = runtime
        .payload::<S>(
            0,
            payload,
            &reference,
            Invocation {
                trial_seq: Some(3),
                side: Some("one"),
            },
        )
        .unwrap()
        .wire()
        .unwrap();
    assert_eq!(before, after, "{}", S::LAYER.name());
}
#[test]
fn all_nine_layers_preserve_models_binary_nulls_and_large_integers() {
    let report: Report =
        serde_json::from_str(include_str!("../compat/python-baseline/simple/report.json")).unwrap();
    let (config, _) = Config::from_report(&report).unwrap();
    let request: Request = serde_json::from_value(json!({"path":"/x","json":{"null":null,"list":[null,1],"large":123456789012345678901234567890u128}})).unwrap();
    let response = response();
    let loaded = runtime(Layer::Log2Reqs, "echo")
        .payload::<addons::Log2Reqs>(
            0,
            Log2ReqsPayload {
                file: "missing-is-allowed".into(),
                requests: vec![],
            },
            &(),
            Invocation::default(),
        )
        .unwrap();
    assert_eq!(loaded.requests[0].path, "/fixture");
    roundtrip::<addons::Reqs2Reqs>(
        Reqs2ReqsPayload {
            requests: vec![request.clone()],
        },
        config,
    );
    roundtrip::<addons::Res2Res>(
        Res2ResPayload {
            response: response.clone(),
            req: request.clone(),
            tags: vec!["tag".into()],
        },
        (),
    );
    roundtrip::<addons::Res2Dict>(
        Res2DictPayload {
            response: response.clone(),
            result: Some(json!({"null":null,"array":[null]})),
        },
        (),
    );
    roundtrip::<addons::Judgement>(
        JudgementPayload {
            diffs_by_cognition: None,
            regard_as_same_body: true,
            regard_as_same_header: false,
        },
        JudgementReference {
            name: "name".into(),
            path: "/x".into(),
            qs: Default::default(),
            headers: Default::default(),
            res_one: response.clone(),
            res_other: response.clone(),
            dict_one: None,
            dict_other: None,
        },
    );
    roundtrip::<addons::StoreCriterion>(
        StoreCriterionPayload { stored: true },
        StoreCriterionReference {
            status: Status::Same,
            req: request,
            res_one: response.clone(),
            res_other: response.clone(),
        },
    );
    roundtrip::<addons::Dump>(
        DumpPayload {
            response: response.clone(),
            body: (0..=255).collect(),
            encoding: None,
        },
        (),
    );
    roundtrip::<addons::DidChallenge>(
        DidChallengePayload {
            trial: report.trials[0].clone(),
        },
        DidChallengeReference {
            res_one: response.clone(),
            res_other: response,
            res_one_props: None,
            res_other_props: None,
        },
    );
    roundtrip::<addons::Final>(
        FinalPayload {
            output_summary: report.summary.output.clone(),
            report,
        },
        FinalReference { notifiers: None },
    );
}
#[test]
fn each_layer_rejects_exit_invalid_json_timeout_missing_command_and_validate_failure() {
    for layer in Layer::ALL {
        for (action, kind) in [
            ("exit", "ExecAddonError"),
            ("invalid", "ExecAddonProtocolError"),
            ("timeout", "ExecAddonTimeout"),
        ] {
            let mut a = addon(action);
            a.config
                .as_mut()
                .unwrap()
                .insert("timeout_sec".into(), json!(0.1));
            let rt = Runtime::prepare_layer(layer, &[a]).unwrap();
            let error = rt
                .invoke(
                    layer,
                    0,
                    "payload",
                    json!({}),
                    Value::Null,
                    Default::default(),
                    Ok,
                )
                .unwrap_err();
            assert_eq!(error.kind, kind, "{layer:?}: {error}");
        }
        assert_eq!(
            runtime(layer, "invalid_validate")
                .validate()
                .unwrap_err()
                .kind,
            "ConfigError"
        );
        let mut a = addon("echo");
        a.config
            .as_mut()
            .unwrap()
            .insert("command".into(), json!(["/no/such/exec"]));
        assert!(Runtime::prepare_layer(layer, &[a]).is_err());
    }
}
#[test]
fn stderr_stream_is_complete_bounded_and_not_retained_as_execution_logs() {
    for (action, expected) in [
        ("stderr", format!("{}終端\n", "x".repeat(100000))),
        ("mixed_stderr", format!("�{}あ\n次\n", "x".repeat(8189))),
    ] {
        let mut addons: Addons =
            serde_json::from_value(json!({"log2reqs":{"name":"plain"},"dump":[]})).unwrap();
        addons.dump.push(addon(action));
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let rt = Runtime::prepare(&addons, 3, move |e| {
            captured.lock().unwrap().push(e.clone())
        })
        .unwrap();
        rt.invoke(
            Layer::Dump,
            0,
            "payload",
            json!({}),
            Value::Null,
            Default::default(),
            Ok,
        )
        .unwrap();
        let events = events.lock().unwrap();
        let text: String = events.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(text, expected);
        assert!(!events[0].continued);
        assert!(events[1].continued);
        assert!(events.iter().all(|e| e.text.len() <= 8195));
    }
}

#[test]
fn stderr_tail_and_stdout_limit_are_independent_of_streaming() {
    for limit in [0, 7, 65536] {
        let mut a = addon("stderr_failure");
        a.config
            .as_mut()
            .unwrap()
            .insert("max_stderr_bytes".into(), json!(limit));
        let rt = Runtime::prepare_layer(Layer::Dump, &[a]).unwrap();
        let error = rt
            .invoke(
                Layer::Dump,
                0,
                "payload",
                json!({}),
                Value::Null,
                Default::default(),
                Ok,
            )
            .unwrap_err();
        if limit == 0 {
            assert!(!error.message.contains("stderr末尾"));
        } else {
            let tail = error
                .message
                .split("重複する場合があります）:\n")
                .nth(1)
                .unwrap();
            assert_eq!(tail.len(), limit);
            assert!(tail.ends_with("tail"));
            assert!(!tail.contains("prefix"));
        }
    }
    let mut a = addon("output");
    let config = a.config.as_mut().unwrap();
    config.insert("max_stdout_bytes".into(), json!(16));
    config.insert(
        "options".into(),
        json!({"action":"output","text":"x".repeat(17)}),
    );
    let error = Runtime::prepare_layer(Layer::Dump, &[a])
        .unwrap()
        .invoke(
            Layer::Dump,
            0,
            "payload",
            json!({}),
            Value::Null,
            Default::default(),
            Ok,
        )
        .unwrap_err();
    assert!(error.message.contains("max_stdout_bytes"));
}

#[test]
fn environment_cwd_relative_path_and_literal_arguments_follow_config() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!("jumeaux-exec-env-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let python = std::process::Command::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    let executable = String::from_utf8(python.stdout).unwrap();
    symlink(executable.trim(), dir.join("bin/python-fixture")).unwrap();
    let mut a = addon("environment");
    let cfg = a.config.as_mut().unwrap();
    cfg.insert("cwd".into(), json!(dir));
    cfg.insert(
        "command".into(),
        json!([
            "python-fixture",
            format!("{}/tests/fixtures/exec.py", env!("CARGO_MANIFEST_DIR")),
            "",
            "a b",
            "$HOME",
            "$(echo no)"
        ]),
    );
    cfg.insert(
        "env".into(),
        json!({"PATH":"bin", "HOME":null, "JUMEAUX_TEST":"value"}),
    );
    for inherit in [true, false] {
        a.config
            .as_mut()
            .unwrap()
            .insert("inherit_env".into(), json!(inherit));
        let rt = Runtime::prepare_layer(Layer::Dump, &[a.clone()]).unwrap();
        rt.validate().unwrap();
        let result = rt
            .invoke(
                Layer::Dump,
                0,
                "payload",
                json!({}),
                Value::Null,
                Default::default(),
                Ok,
            )
            .unwrap();
        assert_eq!(
            std::fs::canonicalize(result["cwd"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(&dir).unwrap()
        );
        assert_eq!(result["args"], json!(["", "a b", "$HOME", "$(echo no)"]));
        assert_eq!(result["env"]["PATH"], "bin");
        assert_eq!(result["env"]["JUMEAUX_TEST"], "value");
        assert!(result["env"].get("HOME").is_none());
        if !inherit {
            assert!(result["env"].get("USER").is_none());
        }
    }
    a.config
        .as_mut()
        .unwrap()
        .insert("env".into(), json!({"PATH":null}));
    assert!(
        Runtime::prepare_layer(Layer::Dump, &[a])
            .err()
            .unwrap()
            .message
            .contains("PATH is unset")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn validate_accepts_only_the_exact_success_object() {
    for output in [
        "true",
        "{}",
        "{\"valid\":false}",
        "{\"valid\":true,\"extra\":1}",
        "{\"valid\":1}",
    ] {
        let mut a = addon("output");
        a.config
            .as_mut()
            .unwrap()
            .insert("options".into(), json!({"action":"output","text":output}));
        assert_eq!(
            Runtime::prepare_layer(Layer::Dump, &[a])
                .unwrap()
                .validate()
                .unwrap_err()
                .kind,
            "ConfigError"
        );
    }
}
#[test]
fn protocol_rejects_invalid_documents_and_preserves_number_lexemes() {
    for bytes in [
        b"".as_slice(),
        b"{} {}",
        b"\xef\xbb\xbf{}",
        b"NaN",
        b"Infinity",
        b"1e999",
        b"{\"a\":1,\"\\u0061\":2}",
        b"\xff",
    ] {
        assert!(addons::exec::json::parse(bytes).is_err(), "{bytes:?}");
    }
    assert_eq!(
        addons::exec::json::parse(b"[123456789012345678901234567890,1.2300,null]")
            .unwrap()
            .to_string(),
        "[123456789012345678901234567890,1.2300,null]"
    );
}
#[test]
fn typed_protocol_rejects_unknown_fields_invalid_base64_and_nonfinite_inputs() {
    let mut payload = DumpPayload {
        response: response(),
        body: vec![0, 255],
        encoding: None,
    }
    .wire()
    .unwrap();
    payload["body"] = json!("%%%=");
    assert!(DumpPayload::from_wire(payload).is_err());
    assert!(StoreCriterionPayload::from_wire(json!({"stored":true,"extra":1})).is_err());
    assert!(StoreCriterionPayload::from_wire(json!({})).is_err());
    let error = StoreCriterionPayload::from_wire(json!({"stored":"secret-body"})).unwrap_err();
    assert!(error.message.contains("$.stored"));
    assert!(error.message.contains("expected a boolean"));
    assert!(!error.message.contains("secret-body"));
    let mut r = response();
    r.elapsed = f64::NAN;
    assert!(r.wire().is_err());
}
#[test]
fn descendant_holding_pipes_does_not_defeat_timeout() {
    let mut a = addon("descendant");
    a.config
        .as_mut()
        .unwrap()
        .insert("timeout_sec".into(), json!(0.1));
    let rt = Runtime::prepare_layer(Layer::Dump, &[a]).unwrap();
    let start = std::time::Instant::now();
    assert_eq!(
        rt.invoke(
            Layer::Dump,
            0,
            "payload",
            json!({}),
            Value::Null,
            Default::default(),
            Ok
        )
        .unwrap_err()
        .kind,
        "ExecAddonTimeout"
    );
    assert!(start.elapsed().as_secs_f64() < 3.0);
}

#[test]
fn transformer_preserves_when_encoding_and_response_metadata() {
    let transformer = addon("echo").config.unwrap();
    for (when, encoding, expected) in [
        ("True", None, b"?".as_slice()),
        ("True", Some("utf8"), "あ".as_bytes()),
        ("False", None, &[0, 255]),
        (
            "req.raw.is_none() and (missing|length == 0)",
            None,
            b"?".as_slice(),
        ),
        ("req.raw.is_some()", None, &[0, 255]),
    ] {
        let mut a: Addon = serde_json::from_value(json!({"name":"json","config":{"transformer":transformer,"when":when,"default_encoding":"ascii"}})).unwrap();
        a.config.as_mut().unwrap()["transformer"]["options"] =
            json!({"text":"あ","expect_null_encoding":encoding.is_none()});
        let rt = Runtime::prepare_layer(Layer::Res2Res, &[a]).unwrap();
        rt.validate().unwrap();
        let mut response = response();
        response.body = vec![0, 255].into();
        response.encoding = encoding.map(str::to_owned);
        response.headers.insert("Content-Length".into(), "2".into());
        let before = response.clone();
        let result = rt
            .payload::<addons::Res2Res>(
                0,
                Res2ResPayload {
                    response,
                    req: Request {
                        path: "/sample".into(),
                        ..Default::default()
                    },
                    tags: vec!["tag".into()],
                },
                &(),
                Invocation::default(),
            )
            .unwrap();
        assert_eq!(&*result.response.body, expected);
        assert_eq!(result.response.headers, before.headers);
        assert_eq!(result.response.elapsed, before.elapsed);
        assert_eq!(result.tags, ["tag"]);
        if when == "True" {
            assert_eq!(result.response.response_type, "json");
        }
    }
}

#[test]
fn misspelled_names_have_candidates_without_python_migration_advice() {
    for cls in ["Executor", "OldClass"] {
        let a: Addon = serde_json::from_value(json!({"name":"shufle","cls_name":cls})).unwrap();
        let error = addons::parse_config(Layer::Reqs2Reqs, &a, "addons.reqs2reqs[0]").unwrap_err();
        assert!(error.message.contains("候補: shuffle"));
        for unwanted in ["cls_name", "Python", "旧.py", "https://"] {
            assert!(!error.message.contains(unwanted), "{}", error.message);
        }
        let a: Addon =
            serde_json::from_value(json!({"name":"custom.add_path","cls_name":cls})).unwrap();
        let error = addons::parse_config(Layer::Reqs2Reqs, &a, "addons.reqs2reqs[0]").unwrap_err();
        assert!(error.message.contains("登録済みの名前:"));
        assert!(error.message.contains("Python modules cannot be loaded"));
        assert!(error.message.contains("旧.py"));
        assert_eq!(error.message.contains("cls_nameは削除"), cls != "Executor");
    }
}

#[test]
fn stderr_unterminated_line_gets_one_newline_even_at_the_chunk_boundary() {
    for text in [
        String::new(),
        "tail".into(),
        "tail\n".into(),
        "x".repeat(8192),
        format!("{}あ", "x".repeat(8191)),
    ] {
        let mut addons: Addons =
            serde_json::from_value(json!({"log2reqs":{"name":"plain"}})).unwrap();
        let mut a = addon("stderr_unterminated");
        a.config.as_mut().unwrap()["options"]["text"] = json!(text);
        addons.dump.push(a);
        let captured = Arc::new(Mutex::new(String::new()));
        let events = Arc::clone(&captured);
        let runtime = Runtime::prepare(&addons, 0, move |e| {
            events.lock().unwrap().push_str(&e.text)
        })
        .unwrap();
        runtime
            .invoke(
                Layer::Dump,
                0,
                "payload",
                json!({}),
                Value::Null,
                Default::default(),
                Ok,
            )
            .unwrap();
        let expected = if text.is_empty() || text.ends_with('\n') {
            text
        } else {
            format!("{text}\n")
        };
        assert_eq!(*captured.lock().unwrap(), expected);
    }
}

#[test]
fn nonzero_exit_returns_promptly_and_diagnostics_have_plain_context() {
    let runtime = runtime(Layer::Dump, "exit");
    let start = std::time::Instant::now();
    for _ in 0..5 {
        let error = runtime
            .invoke(
                Layer::Dump,
                0,
                "payload",
                json!({}),
                Value::Null,
                Invocation {
                    trial_seq: Some(1),
                    side: Some("one"),
                },
                Ok,
            )
            .unwrap_err();
        assert!(
            error.message.contains("trial=1 side=one"),
            "{}",
            error.message
        );
        assert!(!error.message.contains("Some("));
        assert!(error.message.contains("diagnostic"));
    }
    assert!(
        start.elapsed() < std::time::Duration::from_millis(2500),
        "five failures took {:?}",
        start.elapsed()
    );
    let error = runtime.validate().unwrap_err();
    assert_eq!(error.kind, "ConfigError");
    assert_eq!(error.message.matches("addons.dump[0].config").count(), 1);
    assert!(!error.message.contains("trial="));
    assert!(!error.message.contains("side="));
}

#[test]
fn argv_zero_uses_the_configured_name_without_resolved_path_prefixes() {
    for command in ["sh", "/bin/sh"] {
        let a: Addon = serde_json::from_value(json!({"name":"exec", "config":{"command":[command, "-c", "cat >/dev/null; printf '\"%s\"' \"$0\""]}})).unwrap();
        let result = Runtime::prepare_layer(Layer::Dump, &[a])
            .unwrap()
            .invoke(
                Layer::Dump,
                0,
                "payload",
                json!({}),
                Value::Null,
                Default::default(),
                Ok,
            )
            .unwrap();
        assert_eq!(result, command);
    }
}

#[test]
fn transformer_old_keys_and_syntax_errors_are_rejected_before_start() {
    for old in [
        json!({"module":"old"}),
        json!({"function":"transform"}),
        json!({"command":["python3"],"module":"old"}),
    ] {
        let a: Addon =
            serde_json::from_value(json!({"name":"json","config":{"transformer":old}})).unwrap();
        let err = addons::parse_config(Layer::Res2Res, &a, "addons.res2res[0]").unwrap_err();
        assert!(err.message.contains("旧.py"));
        assert!(err.message.contains("https://"));
    }
    let a: Addon = serde_json::from_value(
        json!({"name":"json","config":{"transformer":addon("echo").config,"when":"("}}),
    )
    .unwrap();
    assert!(Runtime::prepare_layer(Layer::Res2Res, &[a]).is_err());
}

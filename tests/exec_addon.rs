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
    let mut addons: Addons =
        serde_json::from_value(json!({"log2reqs":{"name":"plain"},"dump":[]})).unwrap();
    addons.dump.push(addon("stderr"));
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
    assert_eq!(text, format!("{}終端\n", "x".repeat(100000)));
    assert!(!events[0].continued);
    assert!(events[1].continued);
    assert!(events.iter().all(|e| e.text.len() <= 8195));
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

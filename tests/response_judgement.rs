use jumeaux::{
    addons::{self, Chain, Factory, Judgement, Res2Dict, Res2Res, payload::*},
    compat::Result,
    models::{Addon, Addons, Request, Response},
};
use serde_json::{Value, json};

fn response(value: &Value) -> Response {
    let mut value = value.clone();
    let hex = value.as_object_mut().unwrap().remove("body_hex").unwrap();
    let hex = hex.as_str().unwrap();
    let body: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    value["body"] = json!(body);
    serde_json::from_value(value).unwrap()
}

fn response_value(response: Response) -> Value {
    let hex: String = response.body.iter().map(|b| format!("{b:02x}")).collect();
    let mut value = serde_json::to_value(response).unwrap();
    value.as_object_mut().unwrap().remove("body");
    value["body_hex"] = json!(hex);
    value
}

fn reference(value: &Value) -> JudgementReference {
    JudgementReference {
        name: value["name"].as_str().unwrap().into(),
        path: value["path"].as_str().unwrap().into(),
        qs: serde_json::from_value(value["qs"].clone()).unwrap(),
        headers: serde_json::from_value(value["headers"].clone()).unwrap(),
        res_one: response(&value["res_one"]),
        res_other: response(&value["res_other"]),
        dict_one: value.get("dict_one").filter(|v| !v.is_null()).cloned(),
        dict_other: value.get("dict_other").filter(|v| !v.is_null()).cloned(),
    }
}

fn judgement_value(payload: JudgementPayload) -> Value {
    let mut result = json!({"regard_as_same_body": payload.regard_as_same_body, "regard_as_same_header": payload.regard_as_same_header});
    if let Some(diffs) = payload.diffs_by_cognition {
        result["diffs_by_cognition"] = json!(diffs);
    }
    result
}

fn execute(input: &Value) -> Result<Value> {
    let addon: Addon =
        serde_json::from_value(json!({"name":input["addon"], "config":input["config"]})).unwrap();
    let payload = &input["payload"];
    Ok(match input["layer"].as_str().unwrap() {
        "when" => {
            let value = jumeaux::compat::json::parse(payload["text"].as_str().unwrap())?;
            jumeaux::compat::template::TemplateEngine::new()
                .when(
                    input["config"]["expression"].as_str().unwrap(),
                    jumeaux::compat::values::from_json_for_when(&json!({"x":value}))?,
                )?
                .to_json()?
        }
        "diff" => {
            let value = jumeaux::compat::json::parse(payload["text"].as_str().unwrap())?;
            json!(jumeaux::diff::diff(&value, &json!(0))?)
        }
        "res2res" => {
            let executor = addons::res2res::Factory.create(&addon)?;
            let payload = executor.exec(
                Res2ResPayload {
                    response: response(&payload["response"]),
                    req: serde_json::from_value(payload["req"].clone()).unwrap(),
                    tags: serde_json::from_value(payload["tags"].clone()).unwrap(),
                },
                &(),
            )?;
            json!({"response":response_value(payload.response), "req":payload.req, "tags":payload.tags})
        }
        "res2dict" => {
            let executor = addons::res2dict::Factory.create(&addon)?;
            let payload = executor.exec(
                Res2DictPayload {
                    response: response(&payload["response"]),
                    result: payload.get("result").filter(|v| !v.is_null()).cloned(),
                },
                &(),
            )?;
            let mut result = json!({"response":response_value(payload.response)});
            if let Some(value) = payload.result {
                if input["summary"] == "depth" {
                    let (mut leaf, mut depth) = (&value, 0);
                    while let Some(items) = leaf.as_array() {
                        depth += 1;
                        leaf = &items[0];
                    }
                    result["result"] = json!({"depth":depth,"leaf":leaf});
                } else {
                    result["result"] = value;
                }
            }
            result
        }
        "judgement" => {
            let executor = addons::judgement::Factory.create(&addon)?;
            let payload = executor.exec(
                JudgementPayload {
                    diffs_by_cognition: serde_json::from_value(
                        payload["diffs_by_cognition"].clone(),
                    )
                    .unwrap(),
                    regard_as_same_body: payload["regard_as_same_body"].as_bool().unwrap(),
                    regard_as_same_header: payload["regard_as_same_header"].as_bool().unwrap(),
                },
                &reference(&input["reference"]),
            )?;
            judgement_value(payload)
        }
        layer => panic!("unknown layer: {layer}"),
    })
}

#[test]
fn python_response_judgement_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/response-judgement.json")).unwrap();
    let mut failures = Vec::new();
    let mut differences = 0;
    let mut pending = 0;
    for case in &cases {
        let actual = match execute(&case["input"]) {
            Ok(value) => json!({"value":value}),
            Err(error) => json!({"error":error.kind}),
        };
        let expected = if let Some(expected) = case.get("rust_expected") {
            differences += 1;
            assert_ne!(
                expected, &case["expected"],
                "resolved difference: {}",
                case["name"]
            );
            assert!(case["difference"].as_str().is_some_and(|s| !s.is_empty()));
            expected
        } else if let Some(expected) = case.get("pending_rust_expected") {
            pending += 1;
            assert_ne!(expected, &case["expected"]);
            expected
        } else {
            &case["expected"]
        };
        if &actual != expected {
            failures.push(format!(
                "{}: expected {expected}, actual {actual}",
                case["name"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!(
        "response/judgement fixtures: {} checked; {} Python matches, {differences} approved differences, {pending} pending",
        cases.len(),
        cases.len() - differences - pending
    );
}

fn text_response(text: &str) -> Response {
    Response {
        body: text.as_bytes().into(),
        encoding: Some("utf-8".into()),
        headers: Default::default(),
        url: "http://example.test/".into(),
        status_code: 200,
        elapsed: 0.0,
        elapsed_sec: 0.0,
        response_type: "json".into(),
    }
}

fn test_reference(one: &str, other: &str) -> JudgementReference {
    JudgementReference {
        name: "test".into(),
        path: "/".into(),
        qs: Default::default(),
        headers: Default::default(),
        res_one: text_response(one),
        res_other: text_response(other),
        dict_one: None,
        dict_other: None,
    }
}

#[test]
fn initialization_distinguishes_missing_empty_and_bytes() {
    let mut reference = test_reference("{}", "{}");
    let initial =
        |r: &JudgementReference| addons::judgement::initial_payload(r, false, &[]).unwrap();
    assert!(initial(&reference).regard_as_same());
    assert!(initial(&reference).diffs_by_cognition.is_none());
    reference.res_other = text_response("{ }");
    assert!(!initial(&reference).regard_as_same_body);
    reference.dict_one = Some(json!({}));
    reference.dict_other = Some(json!({}));
    assert_eq!(
        initial(&reference).diffs_by_cognition,
        Some(Default::default())
    );
    assert!(initial(&reference).regard_as_same());
    reference.dict_other = Some(json!({"a":1}));
    assert!(!initial(&reference).regard_as_same_body);
    assert_eq!(
        initial(&reference).diffs_by_cognition.unwrap()["unknown"].added,
        ["root<'a'>"]
    );
    reference.res_other = reference.res_one.clone();
    assert!(initial(&reference).regard_as_same_body);
}

#[test]
fn header_names_and_exclusions_ignore_case_but_values_do_not() {
    let mut reference = test_reference("{}", "{}");
    reference.res_one.headers =
        serde_json::from_value(json!({"Content-Type":"application/json","Last-Modified":"one"}))
            .unwrap();
    reference.res_other.headers =
        serde_json::from_value(json!({"content-type":"application/json","last-modified":"other"}))
            .unwrap();
    assert!(
        !addons::judgement::initial_payload(&reference, true, &[])
            .unwrap()
            .regard_as_same_header
    );
    assert!(
        addons::judgement::initial_payload(&reference, true, &["lAsT-mOdIfIeD".into()])
            .unwrap()
            .regard_as_same_header
    );
    assert!(
        addons::judgement::initial_payload(&reference, false, &[])
            .unwrap()
            .regard_as_same_header
    );
    reference
        .res_other
        .headers
        .insert("content-type".into(), "APPLICATION/JSON".into());
    assert!(
        !addons::judgement::initial_payload(&reference, true, &["last-modified".into()])
            .unwrap()
            .regard_as_same_header
    );
}

#[test]
fn chains_pass_converted_types_and_stop_judgement_after_same() {
    let addons: Addons = serde_json::from_value(json!({
        "log2reqs":{"name":"plain"},
        "res2res":[{"name":"type","config":{"conditions":[{"type":"json"}]}},{"name":"json_sort","config":{"items":[{"when":"True","targets":[{"path":"root"}]}]}}],
        "res2dict":[{"name":"json"},{"name":"xml"}],
        "judgement":[{"name":"same","config":{"when_any":["True"]}},{"name":"ignore","config":{"ignores":[{"title":"bad","conditions":[{"when":"missing.value == 1"}]}]}},{"name":"same","config":{"when_any":["missing.value == 1"]}}]
    })).unwrap();
    let mut response = text_response("[2,1]");
    response.response_type = "plain".into();
    let converted = Chain::<Res2Res>::build(&addons, &addons::res2res::Factory)
        .unwrap()
        .exec(
            Res2ResPayload {
                response,
                req: Request::default(),
                tags: vec![],
            },
            &(),
        )
        .unwrap();
    assert_eq!(converted.response.text().unwrap(), "[1, 2]");
    let dict = Chain::<Res2Dict>::build(&addons, &addons::res2dict::Factory)
        .unwrap()
        .exec(
            Res2DictPayload {
                response: converted.response,
                result: None,
            },
            &(),
        )
        .unwrap();
    assert_eq!(dict.result, Some(json!([1, 2])));
    let reference = test_reference("one", "other");
    let payload = addons::judgement::initial_payload(&reference, false, &[]).unwrap();
    let result = Chain::<Judgement>::build(&addons, &addons::judgement::Factory)
        .unwrap()
        .exec(payload, &reference)
        .unwrap();
    assert!(result.regard_as_same());
}

#[test]
fn template_reports_match_python_cognition_and_status() {
    let mut trials = 0;
    for name in [
        "ignore",
        "ignore_order",
        "judge_response_headers",
        "force_json",
        "xml",
        "html",
        "all_same",
        "root_array",
        "header_ignore_case",
        "header_ignore_control",
        "header_condition_case",
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("compat/python-baseline")
            .join(name);
        let text = std::fs::read_to_string(root.join("config.yml")).unwrap();
        let config: jumeaux::config::Config =
            serde_yaml_ng::from_str(&text.replace("    - name: viewer\n", "")).unwrap();
        let report: Value =
            serde_json::from_slice(&std::fs::read(root.join("report.json")).unwrap()).unwrap();
        let res2res = Chain::<Res2Res>::build(&config.addons, &addons::res2res::Factory).unwrap();
        let res2dict =
            Chain::<Res2Dict>::build(&config.addons, &addons::res2dict::Factory).unwrap();
        let judgement =
            Chain::<Judgement>::build(&config.addons, &addons::judgement::Factory).unwrap();
        for trial in report["trials"].as_array().unwrap() {
            let req: Request = serde_json::from_value(json!({
                "name":trial["name"], "method":trial["method"], "path":trial["path"], "qs":trial["queries"], "headers":trial["headers"]
            })).unwrap();
            let mut responses = Vec::new();
            let mut dicts = Vec::new();
            for side in ["one", "other"] {
                let summary = &trial[side];
                let file = root
                    .join("api")
                    .join(side)
                    .join(req.path.trim_start_matches('/'));
                let body = std::fs::read(file).unwrap();
                let mut response = text_response("");
                response.body = body.into();
                response.url = summary["url"].as_str().unwrap().into();
                response.encoding = summary["encoding"].as_str().map(str::to_owned);
                // Content-Type is captured before res2res/type changes the reported type.
                let content_type = summary["content_type"].as_str().unwrap();
                response.response_type = content_type
                    .split(';')
                    .next()
                    .unwrap()
                    .split('/')
                    .nth(1)
                    .unwrap()
                    .into();
                response.headers = summary
                    .get("response_header")
                    .map(|v| serde_json::from_value(v.clone()).unwrap())
                    .unwrap_or_default();
                if response.headers.is_empty() {
                    response
                        .headers
                        .insert("Content-Type".into(), content_type.into());
                    response.headers.insert(
                        "Last-Modified".into(),
                        if side == "one" {
                            "Sat, 01 Jan 2000 00:00:00 GMT"
                        } else {
                            "Sun, 02 Jan 2000 00:00:00 GMT"
                        }
                        .into(),
                    );
                }
                let response = res2res
                    .exec(
                        Res2ResPayload {
                            response,
                            req: req.clone(),
                            tags: vec![],
                        },
                        &(),
                    )
                    .unwrap()
                    .response;
                let dict = res2dict
                    .exec(
                        Res2DictPayload {
                            response: response.clone(),
                            result: None,
                        },
                        &(),
                    )
                    .unwrap()
                    .result;
                responses.push(response);
                dicts.push(dict);
            }
            let reference = JudgementReference {
                name: req.name.clone().unwrap_or_default(),
                path: req.path.clone(),
                qs: req.qs,
                headers: req.headers,
                res_one: responses.remove(0),
                res_other: responses.remove(0),
                dict_one: dicts.remove(0),
                dict_other: dicts.remove(0),
            };
            let initial = addons::judgement::initial_payload(
                &reference,
                config.judge_response_header,
                &config.ignore_response_header_keys,
            )
            .unwrap();
            let result = judgement.exec(initial, &reference).unwrap();
            let status = if result.regard_as_same() {
                "same"
            } else {
                "different"
            };
            // 小文字の除外設定は承認済みの差異。Python対照ケースのsameが期待値。
            let expected_status = if name == "header_ignore_case" {
                "same"
            } else {
                trial["status"].as_str().unwrap()
            };
            assert_eq!(status, expected_status, "{name}: {}", req.path);
            assert_eq!(
                judgement_value(result).get("diffs_by_cognition"),
                trial.get("diffs_by_cognition"),
                "{name}: {}",
                req.path
            );
            trials += 1;
        }
    }
    println!(
        "template report trials: {trials}/{trials} cognition/status matched (11 templates/cases)"
    );
}

use jumeaux::{
    addons::{self, AddonExecutor, payload::Res2ResPayload},
    compat::{self, template::TemplateEngine, values},
    diff,
    models::{Request, Response},
};
use serde_json::{Value, json};

fn response(text: &str) -> Response {
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

fn sorter() -> addons::res2res::json_sort::Executor {
    addons::res2res::json_sort::Executor::new(
        serde_json::from_value(json!({"items":[{"when":"True","targets":[{"path":".*"}]}]}))
            .unwrap(),
    )
    .unwrap()
}

fn sort(text: &str) -> compat::Result<Res2ResPayload> {
    sorter().exec(
        Res2ResPayload {
            response: response(text),
            req: Request::default(),
            tags: vec![],
        },
        &(),
    )
}

fn assert_nonfinite<T>(result: compat::Result<T>) {
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("non-finite input was accepted"),
    };
    assert_eq!(error.kind, "ValueError");
    assert_eq!(error.message, "non-finite numbers are not supported");
}

#[test]
fn nonfinite_errors_are_identical_at_all_boundaries() {
    let engine = TemplateEngine::new();
    let converter = addons::res2dict::json::Config { force: false };
    for token in ["NaN", "Infinity", "-Infinity", "1e400", "-1e400"] {
        for text in [token.to_owned(), format!("{{\"x\":[{token}]}}")] {
            assert_nonfinite(compat::json::parse(&text));
            assert_nonfinite(sort(&text));
            assert_nonfinite(converter.exec(
                addons::payload::Res2DictPayload {
                    response: response(&text),
                    result: None,
                },
                &(),
            ));
        }
        let expression = if token.contains('e') {
            token.to_owned()
        } else {
            format!("'{token}'|float")
        };
        assert_nonfinite(engine.when(&expression, json!({})));
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_nonfinite(values::checked_context(minijinja::context!(x => value)));
    }
    for text in ["1e400", "-1e400", "{\"a\":[1e400]}"] {
        // serdeのarbitrary_precision経由で文字列入口を迂回した場合も拒否する。
        let value: Value = serde_json::from_str(text).unwrap();
        assert_nonfinite(diff::diff(&value, &json!(0)));
        assert_nonfinite(diff::diff(&json!(0), &value));
        assert_nonfinite(engine.when("True", &value));
        assert_nonfinite(values::optional(Some(&value)));
    }
    assert_nonfinite(engine.when("'1e400'", json!({})));
    let huge_integer: Value = serde_json::from_str(&format!("1{}", "0".repeat(400))).unwrap();
    assert_nonfinite(engine.when("True", &huge_integer));
    assert_nonfinite(values::optional(Some(&huge_integer)));
    assert!(diff::diff(&huge_integer, &json!(0)).is_ok());
    assert!(compat::json::parse(r#"["NaN", "Infinity", "1e400"]"#).is_ok());
    assert!(
        engine
            .when("x == 'Infinity'", json!({"x":"Infinity"}))
            .unwrap()
            .truthy()
    );
    assert!(
        engine
            .when("inf == 1 and x.NaN == 2", json!({"inf":1,"x":{"NaN":2}}))
            .unwrap()
            .truthy()
    );
    assert!(compat::json::parse("[1e308,-1e308,1e-400]").is_ok());
}

#[test]
fn sort_output_can_always_enter_json_converter() {
    let sorted = sort("[1e308,1e-400,-0,2]").unwrap();
    assert!(!sorted.response.text().unwrap().contains("Infinity"));
    let converted = addons::res2dict::json::Config { force: false }
        .exec(
            addons::payload::Res2DictPayload {
                response: sorted.response,
                result: None,
            },
            &(),
        )
        .unwrap();
    assert!(converted.result.is_some());
    assert_nonfinite(sort("[1e400]"));
}

// スタックオーバーフローはabortになるため、親プロセスが終了状態まで検査する。
#[test]
fn deep_json_survives_on_two_megabyte_stack() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "deep_json_worker", "--nocapture"])
        .env("JUMEAUX_DEEP_JSON_WORKER", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn deep_json_worker() {
    if std::env::var_os("JUMEAUX_DEEP_JSON_WORKER").is_none() {
        return;
    }
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for depth in [129, 200, 1000] {
                for shape in ["array", "object", "mixed"] {
                    let mut one = "[2,1]".to_owned();
                    let mut other = "[2,3]".to_owned();
                    for index in 1..depth {
                        let (open, close) =
                            if shape == "array" || (shape == "mixed" && index % 2 == 0) {
                                ("[", "]")
                            } else {
                                ("{\"a\":", "}")
                            };
                        one = format!("{open}{one}{close}");
                        other = format!("{open}{other}{close}");
                    }
                    let a = compat::json::parse(&one).unwrap();
                    let b = compat::json::parse(&other).unwrap();
                    assert_eq!(compat::json::validate(&a).unwrap(), depth);
                    let changes = diff::diff(&a, &b).unwrap();
                    assert_eq!(changes.changed.len(), 1);
                    assert_eq!(
                        diff::get_by_diff_key(&b, &changes.changed[0]),
                        Some(&json!(3))
                    );
                    let sorted = sort(&one).unwrap();
                    let text = sorted.response.text().unwrap();
                    assert!(text.contains("[1, 2]"));
                    assert_eq!(
                        compat::json::validate(&compat::json::parse(&text).unwrap()).unwrap(),
                        depth
                    );
                    // 深い不正JSONも途中で構築したValueを安全に破棄する。
                    assert_eq!(
                        compat::json::parse(&one[..one.len() - 1]).unwrap_err().kind,
                        "JSONDecodeError"
                    );
                    let reference = addons::payload::JudgementReference {
                        name: "deep".into(),
                        path: "/".into(),
                        qs: Default::default(),
                        headers: Default::default(),
                        res_one: response(&one),
                        res_other: response(&other),
                        dict_one: Some(a),
                        dict_other: Some(b),
                    };
                    let initial =
                        addons::judgement::initial_payload(&reference, false, &[]).unwrap();
                    let same =
                        addons::judgement::same::Executor::new(addons::judgement::same::Config {
                            when_any: vec!["False".into(), "dict_one.value is none".into()],
                        })
                        .unwrap();
                    assert!(!same.exec(initial, &reference).unwrap().regard_as_same_body);
                }
            }
            let too_deep = format!("{}0{}", "[".repeat(1001), "]".repeat(1001));
            assert_eq!(
                compat::json::parse(&too_deep).unwrap_err().kind,
                "LimitExceeded"
            );
            assert_eq!(sort(&too_deep).unwrap_err().kind, "LimitExceeded");
            let mut value = json!(0);
            for _ in 0..1001 {
                value = Value::Array(vec![value]);
            }
            assert_eq!(
                diff::diff(&value, &json!(0)).unwrap_err().kind,
                "LimitExceeded"
            );
            let reference = addons::payload::JudgementReference {
                name: "deep".into(),
                path: "/".into(),
                qs: Default::default(),
                headers: Default::default(),
                res_one: response("{}"),
                res_other: response("{}"),
                dict_one: Some(value),
                dict_other: Some(json!({})),
            };
            assert_eq!(
                addons::judgement::initial_payload(&reference, false, &[])
                    .unwrap_err()
                    .kind,
                "LimitExceeded"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

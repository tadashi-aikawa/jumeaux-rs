use jumeaux::compat::{
    template::{self, TemplateEngine},
    values::{Request, Response, TemplateInput, checked_context, from_json_for_when},
};
use serde_json::{Value, json};

#[test]
fn python_option_method_fixtures() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../compat/fixtures/template/option-methods.json"
    ))
    .unwrap();
    let engine = TemplateEngine::new();
    let mut failures = Vec::new();
    for case in &cases {
        let input = &case["input"];
        let context = checked_context(minijinja::context!(
            one => jumeaux::compat::values::optional(Some(&input["data"]["one"])).unwrap().into_when_value(),
            other => jumeaux::compat::values::optional(Some(&input["data"]["other"])).unwrap().into_when_value(),
            empty => jumeaux::compat::values::optional(None).unwrap().into_when_value(),
        )).unwrap();
        let expression = input["expression"].as_str().unwrap();
        let actual = if input["render"] == true {
            let template = if expression.starts_with("{%") {
                expression.to_owned()
            } else {
                format!("{{{{{expression}}}}}")
            };
            engine
                .render(&template, context.into_when_value())
                .map(Value::from)
        } else {
            engine
                .when(expression, context)
                .and_then(|value| value.to_json())
        };
        let actual = match actual {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error.kind}),
        };
        if actual != case["expected"] {
            failures.push(format!(
                "{}: actual={actual}, expected={}",
                case["name"], case["expected"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("Optionメソッド: {}/{} Python一致", cases.len(), cases.len());
}

#[test]
fn python_template_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/template/cases.json")).unwrap();
    let review: Vec<Value> = serde_json::from_str(include_str!(
        "../compat/fixtures/template/option-review.json"
    ))
    .unwrap();
    let cases: Vec<_> = cases.into_iter().chain(review).collect();
    let engine = TemplateEngine::new();
    let mut failures = Vec::new();
    let mut differences = 0;
    for case in &cases {
        let input = &case["input"];
        let string = |key: &str| input[key].as_str().unwrap();
        let actual = match string("operation") {
            "when" => from_json_for_when(&input["data"])
                .and_then(|context| engine.when(string("expression"), context))
                .and_then(|v| v.to_json()),
            "when_type" => engine
                .when(
                    string("expression"),
                    from_json_for_when(&json!({})).unwrap(),
                )
                .map(|v| json!({"type": v.type_name(), "truthy": v.truthy()})),
            "render" => engine
                .render(string("template"), &input["data"])
                .map(Value::from),
            "option_when" | "option_render" => {
                let context = checked_context(minijinja::context!(
                    one => jumeaux::compat::values::optional(Some(&input["data"]["one"]))
                        .unwrap().into_when_value()
                ))
                .unwrap();
                if string("operation") == "option_render" {
                    engine
                        .render(string("template"), context.into_when_value())
                        .map(Value::from)
                } else {
                    engine
                        .when(string("expression"), context)
                        .and_then(|value| value.to_json())
                }
            }
            "response_when" => {
                let response = Response {
                    body: string("text").as_bytes().into(),
                    encoding: Some("utf-8".into()),
                    headers: [("content-type".into(), "text/plain".into())].into(),
                    url: "http://localhost/test".into(),
                    status_code: input["status_code"].as_u64().unwrap() as u16,
                    elapsed: 0.0,
                    elapsed_sec: 0.0,
                    response_type: "plain".into(),
                };
                engine
                    .when(
                        string("expression"),
                        checked_context(
                            minijinja::context!(response => response.to_template_value()),
                        )
                        .unwrap(),
                    )
                    .and_then(|v| v.to_json())
            }
            "request_when" => {
                let request: Request = serde_json::from_value(input["request"].clone()).unwrap();
                engine
                    .when(
                        string("expression"),
                        checked_context(
                            minijinja::context!(request => request.to_template_value()),
                        )
                        .unwrap(),
                    )
                    .and_then(|v| v.to_json())
            }
            "datetime" => template::parse_datetime_at(
                string("value"),
                chrono::DateTime::parse_from_rfc3339(string("now")).unwrap(),
            )
            .map(Value::from),
            op => panic!("unhandled operation {op}"),
        };
        let actual = match actual {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error.kind}),
        };
        let expected = if let Some(expected) = case.get("rust_expected") {
            assert_ne!(expected, &case["expected"], "{}", case["name"]);
            assert!(case["difference"].as_str().is_some_and(|s| !s.is_empty()));
            differences += 1;
            expected
        } else {
            &case["expected"]
        };
        if &actual != expected {
            failures.push(format!(
                "{}: actual={actual}, expected={}",
                case["name"], expected
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!(
        "template: {}/{} Python一致、{}件の差異を明示検証",
        cases.len() - differences,
        cases.len(),
        differences
    );
}

#[test]
fn literal_eval_does_not_execute_expressions() {
    for expression in [
        "__import__('os')",
        "1 + 2",
        "[x for x in []]",
        "True or False",
    ] {
        assert_eq!(
            template::literal_eval(expression).unwrap_err().kind,
            "ValueError"
        );
    }
    assert_eq!(
        template::literal_eval("'False'")
            .unwrap()
            .to_json()
            .unwrap(),
        json!("False")
    );
    assert_eq!(
        template::literal_eval("False").unwrap().to_json().unwrap(),
        json!(false)
    );
}

#[test]
fn invalid_template_is_rejected_before_render() {
    assert_eq!(
        TemplateEngine::new()
            .validate("{{ broken")
            .unwrap_err()
            .kind,
        "TemplateSyntaxError"
    );
    let engine = TemplateEngine::new();
    for source in [
        "{% if false %}{{ x|absent }}{% endif %}",
        "{% macro m() %}{{ x is absent }}{% endmacro %}",
        "{% filter absent %}text{% endfilter %}",
    ] {
        assert_eq!(
            engine.validate(source).unwrap_err().kind,
            "TemplateAssertionError"
        );
    }
    engine
        .validate("{# {{ x|absent }} #}{{ 'x'|upper }}{{ 2 is even }}")
        .unwrap();
}

#[test]
fn request_contexts_preserve_order_and_option_contracts() {
    let request: Request = serde_json::from_value(json!({
        "path": "/search", "qs": {"z": ["1"], "a": ["2"]},
        "headers": {"Z-Header": "first", "A-Header": "second"}
    }))
    .unwrap();
    let engine = TemplateEngine::new();
    assert_eq!(
        engine
            .render(
                "{{ qs|list }} {{ headers|list }} {{ raw is defined }}",
                request.to_dict_value()
            )
            .unwrap(),
        "['z', 'a'] ['Z-Header', 'A-Header'] False"
    );
    assert!(
        engine
            .when(
                "request.raw.is_none()",
                checked_context(minijinja::context!(request => request.to_template_value()))
                    .unwrap()
            )
            .unwrap()
            .truthy()
    );
}

#[test]
fn literal_collections_keep_python_key_equality() {
    use template::{LiteralValue, literal_eval};
    let LiteralValue::Set(values) = literal_eval("{True, 1, 1.0, 1+0j}").unwrap() else {
        panic!("set expected")
    };
    assert_eq!(values.len(), 1);
    let LiteralValue::Dict(values) = literal_eval("{True: 'first', 1.0: 'last'}").unwrap() else {
        panic!("dict expected")
    };
    assert_eq!(
        values,
        vec![(
            LiteralValue::Bool(true),
            LiteralValue::String("last".into())
        )]
    );
    assert_eq!(literal_eval("{[]}").unwrap_err().kind, "TypeError");
    assert!(
        literal_eval("123456789012345678901234567890")
            .unwrap()
            .to_json()
            .is_err()
    );
}

#[test]
fn response_property_errors_are_lazy_and_propagated() {
    let response = Response {
        body: vec![255].into(),
        encoding: Some("unknown-codec".into()),
        headers: Default::default(),
        url: String::new(),
        status_code: 200,
        elapsed: 0.0,
        elapsed_sec: 0.0,
        response_type: "plain".into(),
    };
    let engine = TemplateEngine::new();
    assert!(std::sync::Arc::ptr_eq(
        &response.body,
        &response.clone().body
    ));
    assert_eq!(engine.render("{{ response.unknown }} {{ response.status_code }} {{ response.url }} {{ response.elapsed_sec }} {{ response.type }}", minijinja::context!(response => response.to_template_value())).unwrap(), " 200  0.0 plain");
    assert!(
        engine
            .when(
                "response.ok",
                checked_context(minijinja::context!(response => response.to_template_value()))
                    .unwrap()
            )
            .unwrap()
            .truthy()
    );
    assert_eq!(
        engine
            .render(
                "{{ response.text }}",
                minijinja::context!(response => response.to_template_value())
            )
            .unwrap_err()
            .kind,
        "LookupError"
    );
}

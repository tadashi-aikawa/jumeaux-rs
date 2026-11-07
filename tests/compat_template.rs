use jumeaux::compat::{
    template::{self, TemplateEngine},
    values::{Request, Response},
};
use serde_json::{Value, json};

#[test]
fn python_template_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/template/cases.json")).unwrap();
    let engine = TemplateEngine::new();
    let mut failures = Vec::new();
    for case in &cases {
        let input = &case["input"];
        let string = |key: &str| input[key].as_str().unwrap();
        let actual = match string("operation") {
            "when" => engine
                .when(string("expression"), &input["data"])
                .and_then(|v| v.to_json()),
            "when_type" => engine
                .when(string("expression"), json!({}))
                .map(|v| json!({"type": v.type_name(), "truthy": v.truthy()})),
            "render" => engine
                .render(string("template"), &input["data"])
                .map(Value::from),
            "response_when" => {
                let response = Response {
                    body: string("text").as_bytes().to_vec(),
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
                        minijinja::context!(response => response.to_template_value()),
                    )
                    .and_then(|v| v.to_json())
            }
            "request_when" => {
                let request: Request = serde_json::from_value(input["request"].clone()).unwrap();
                engine
                    .when(
                        string("expression"),
                        minijinja::context!(request => request.to_template_value()),
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
        if actual != case["expected"] {
            failures.push(format!(
                "{}: actual={actual}, expected={}",
                case["name"], case["expected"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("template: {}/{} Python一致", cases.len(), cases.len());
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
        body: vec![255],
        encoding: Some("unknown-codec".into()),
        headers: Default::default(),
        url: String::new(),
        status_code: 200,
        elapsed: 0.0,
        elapsed_sec: 0.0,
        response_type: "plain".into(),
    };
    let engine = TemplateEngine::new();
    assert!(
        engine
            .when(
                "response.ok",
                minijinja::context!(response => response.to_template_value())
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

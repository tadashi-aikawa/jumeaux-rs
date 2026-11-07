use jumeaux::markup::{Error, html, xml};
use serde_json::{Value, json};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("../compat/fixtures/markup/cases.json")).unwrap()
}

fn differences() -> Vec<Value> {
    serde_json::from_str(include_str!("markup_differences.json")).unwrap()
}

fn key(case: &Value) -> (&str, &str, &str) {
    let input = case.get("input").unwrap_or(case);
    (
        input["format"].as_str().unwrap(),
        input["operation"].as_str().unwrap_or("dict"),
        case["name"].as_str().unwrap(),
    )
}

fn check_fixtures(target_format: &str, target_operation: &str) {
    let cases = fixtures();
    let differences = differences();
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut mismatches = 0;
    for case in &cases {
        let input = &case["input"];
        let body = input["body"].as_str().unwrap();
        let format = input["format"].as_str().unwrap();
        let operation = input["operation"].as_str().unwrap_or("dict");
        if (format, operation) != (target_format, target_operation) {
            continue;
        }
        checked += 1;
        let result = match (format, operation) {
            ("html", "pretty") => html::pretty(body).map(Value::String),
            ("xml", "pretty") => xml::pretty(body).map(Value::String),
            ("html", "dict") => html::to_dict(body),
            ("xml", "dict") => xml::to_dict(body),
            _ => panic!("unknown operation: {input}"),
        };
        let result = result.map(|dict| {
            // 条件式エンジンの統合は別タスク。既存fixtureの2式は同じ参照と比較を行う。
            if let Some(expression) = input["when"].as_str() {
                let condition = match expression {
                    "dict_one.html.body.p.b['##value'] == 'b'" => {
                        dict["html"]["body"]["p"]["b"]["##value"] == "b"
                    }
                    "dict_one.r.c is none and dict_one.r['#text'] == 'beforeafter'" => {
                        dict["r"].get("c") == Some(&Value::Null)
                            && dict["r"]["#text"] == "beforeafter"
                    }
                    _ => panic!("unhandled condition: {expression}"),
                };
                json!({"dict": dict, "condition": condition})
            } else {
                dict
            }
        });
        let actual = match result {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error.python_exception()}),
        };
        let expected = if let Some(difference) = differences
            .iter()
            .find(|difference| key(difference) == key(case))
        {
            mismatches += 1;
            assert_ne!(
                difference["expected"], case["expected"],
                "解消済みの差異: {}",
                case["name"]
            );
            &difference["expected"]
        } else {
            &case["expected"]
        };
        if &actual != expected {
            failures.push(format!(
                "{format}/{operation}/{}\nexpected: {}\nactual:   {actual}",
                case["name"], expected
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} fixtures failed:\n{}",
        failures.len(),
        checked,
        failures.join("\n")
    );
    assert!(checked > 0);
    println!(
        "{target_format}/{target_operation}: {checked}件検証、Python完全一致{}件、未決差異{mismatches}件",
        checked - mismatches
    );
}

#[test]
fn html_dict_fixtures() {
    check_fixtures("html", "dict");
}

#[test]
fn html_pretty_fixtures() {
    check_fixtures("html", "pretty");
}

#[test]
fn xml_dict_fixtures() {
    check_fixtures("xml", "dict");
}

#[test]
fn xml_pretty_fixtures() {
    check_fixtures("xml", "pretty");
}

#[test]
fn fixture_manifest_has_no_duplicates_or_unused_differences() {
    let cases = fixtures();
    let differences = differences();
    assert_eq!(cases.len(), 110);
    assert_eq!(differences.len(), 7);
    let mut keys = std::collections::HashSet::new();
    for case in &cases {
        let (format, operation, _) = key(case);
        assert!(
            matches!((format, operation), ("html" | "xml", "dict" | "pretty")),
            "unhandled fixture: {case}"
        );
        assert!(keys.insert(key(case)), "duplicate: {case}");
    }
    let mut difference_keys = std::collections::HashSet::new();
    for difference in &differences {
        assert!(
            difference_keys.insert(key(difference)),
            "duplicate: {difference}"
        );
        assert!(keys.contains(&key(difference)), "unused: {difference}");
        assert!(!difference["reason"].as_str().unwrap().is_empty());
    }
}

#[test]
fn deeply_nested_markup_returns_an_error() {
    let html = format!("{}x{}", "<div>".repeat(270), "</div>".repeat(270));
    let xml = format!("{}x{}", "<r>".repeat(270), "</r>".repeat(270));
    assert_eq!(html::to_dict(&html), Err(Error::LimitExceeded));
    assert_eq!(html::pretty(&html), Err(Error::LimitExceeded));
    assert_eq!(xml::to_dict(&xml), Err(Error::LimitExceeded));
    assert_eq!(xml::pretty(&xml), Err(Error::LimitExceeded));
}

#[test]
fn recursive_entities_are_bounded_and_disabled_in_dict_text() {
    let input = "<!DOCTYPE r [<!ENTITY x '&x;'>]><r>&x;</r>";
    assert_eq!(xml::to_dict(input).unwrap(), json!({"r": null}));
    assert_eq!(xml::pretty(input), Err(Error::LimitExceeded));
    let attribute = "<!DOCTYPE r [<!ENTITY x '&x;'>]><r a='&x;'/>";
    assert_eq!(xml::to_dict(attribute), Err(Error::LimitExceeded));
}

use jumeaux::diff::{PathSegment, diff, format_path, get_by_diff_key};
use serde_json::{Value, json};

#[test]
fn all_diff_fixtures() {
    let fixture = std::env::var_os("JUMEAUX_DIFF_FIXTURES")
        .map(|path| std::fs::read_to_string(path).unwrap())
        .unwrap_or_else(|| include_str!("../compat/fixtures/diff/cases.json").to_owned());
    let cases: Vec<Value> = serde_json::from_str(&fixture).unwrap();
    assert!(!cases.is_empty());
    let mut failures = Vec::new();
    for case in &cases {
        let input = &case["input"];
        let actual = match input["operation"].as_str().unwrap() {
            "diff" => {
                let result = diff(&input["one"], &input["other"]);
                json!({"value": {"added": result.added, "changed": result.changed, "removed": result.removed}})
            }
            "get_by_diff_key" => json!({"value": get_by_diff_key(
                &input["dict"], input["path"].as_str().unwrap()
            )}),
            operation => panic!("unknown operation: {operation}"),
        };
        // python_mismatchでも常に修正後のexpectedを検証する。
        if actual != case["expected"] {
            failures.push(format!(
                "{}: expected {}, actual {actual}",
                case["name"], case["expected"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("diff fixtures: {}/{} matched", cases.len(), cases.len());
}

#[test]
fn generated_paths_round_trip_special_keys() {
    for key in [
        "",
        "root",
        "myroot",
        "a[0]",
        "a<b>",
        "a.b",
        "a'b",
        "a\"b",
        "a'\"b",
        "'><0><'",
        "a\\nb",
        "a\nb",
        "\0",
        "日本語🦉",
        "0",
    ] {
        let before = json!({"items": [{key: 1}]});
        let after = json!({"items": [{key: 2}]});
        let path = format_path(&[
            PathSegment::Key("items".into()),
            PathSegment::Index(0),
            PathSegment::Key(key.into()),
        ]);
        assert_eq!(diff(&before, &after).changed, vec![path.clone()]);
        assert_eq!(get_by_diff_key(&before, &path), Some(&json!(1)), "{path}");
        assert_eq!(get_by_diff_key(&after, &path), Some(&json!(2)), "{path}");
    }
}

#[test]
fn root_missing_null_and_wrong_container_types() {
    let value = json!({"items": [null], "0": 42});
    assert_eq!(format_path(&[]), "root");
    assert_eq!(get_by_diff_key(&value, "root"), Some(&value));
    assert_eq!(
        get_by_diff_key(&value, "root<'items'><0>"),
        Some(&Value::Null)
    );
    for path in [
        "root<'missing'>",
        "root<'items'><1>",
        "root<0>",
        "root<'items'><'0'>",
        "root<'items'><0><'a'>",
    ] {
        assert_eq!(get_by_diff_key(&value, path), None, "{path}");
    }
}

#[test]
fn malformed_paths_are_rejected() {
    let value = json!({"a": [1]});
    for path in [
        "",
        "Root",
        "rootroot",
        "root<",
        "root<>",
        "root<'a'",
        "root<'a>",
        "root<'a'>garbage",
        "root<'a'><-1>",
        "root<'a'><+0>",
        "root<'a'>< 0>",
        "root<'a'><9999999999999999999999999999999>",
        "root<'\\q'>",
        "root<'\\uD800'>",
    ] {
        assert_eq!(get_by_diff_key(&value, path), None, "{path}");
    }
}

#[test]
fn python_literal_escapes_are_supported() {
    let value = json!({"\0": 1, "🦉": 2, "a'b": 3});
    assert_eq!(get_by_diff_key(&value, "root<'\\x00'>"), Some(&json!(1)));
    assert_eq!(
        get_by_diff_key(&value, "root<'\\U0001f989'>"),
        Some(&json!(2))
    );
    assert_eq!(get_by_diff_key(&value, "root<'a\\'b'>"), Some(&json!(3)));
}

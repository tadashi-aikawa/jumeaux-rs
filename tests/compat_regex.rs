use jumeaux::compat::regex::Regex;
use serde_json::{Value, json};

#[test]
fn python_regex_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/regex/cases.json")).unwrap();
    let mut failures = Vec::new();
    let mut differences = 0;
    for case in &cases {
        let input = &case["input"];
        let string = |key: &str| input[key].as_str().unwrap();
        let compiled = match string("operation") {
            "exact_match" | "reg" => Regex::new_exact(string("pattern")),
            _ => Regex::new(string("pattern")),
        };
        let actual = compiled.and_then(|regex| match string("operation") {
            "exact_match" | "reg" => regex.exact_match(string("target")).map(Value::from),
            "search" => regex.search(string("target")).map(Value::from),
            "sub" => regex
                .sub(string("replacement"), string("target"))
                .map(Value::from),
            op => panic!("unhandled operation {op}"),
        });
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
        "regex: {}/{} Python一致、{}件の差異を明示検証",
        cases.len() - differences,
        cases.len(),
        differences
    );
}

#[test]
fn path_custom_access_point_replacement() {
    let pattern = Regex::new(r"([^-]+)-(\d).json").unwrap();
    pattern.validate_replacement(r"\1/case-\2.json").unwrap();
    assert_eq!(
        pattern.sub(r"\1/case-\2.json", "same-1.json").unwrap(),
        "same/case-1.json"
    );
    assert_eq!(
        pattern.sub(r"\1/case-\2.json", "different-2.json").unwrap(),
        "different/case-2.json"
    );
}

#[test]
fn config_errors_include_original_pattern() {
    assert!(Regex::new("[").unwrap_err().message.contains('['));
    let pattern = Regex::new("(a)").unwrap();
    assert!(
        pattern
            .validate_replacement(r"\2")
            .unwrap_err()
            .message
            .contains("(a)")
    );
}

#[test]
fn exact_match_and_full_match_have_distinct_newline_contracts() {
    let pattern = Regex::new(r"\d+").unwrap();
    assert!(pattern.exact_match("123\n").unwrap());
    assert!(!pattern.full_match("123\n").unwrap());
    assert!(!pattern.exact_match("123\n\n").unwrap());
}

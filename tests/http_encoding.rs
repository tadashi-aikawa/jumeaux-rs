use jumeaux::compat::encoding::http_encoding;
use serde_json::Value;

#[test]
fn python_http_encoding_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/encoding/http.json")).unwrap();
    for case in &cases {
        let input = &case["input"];
        let body: Vec<u8> = input["body_hex"]
            .as_str()
            .unwrap()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let actual = http_encoding(
            input["content_type"].as_str(),
            &body,
            input["default"].as_str(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            case["expected"]["value"],
            "{}",
            case["name"]
        );
    }
    println!(
        "HTTP encoding: {} / {} Python一致",
        cases.len(),
        cases.len()
    );
}

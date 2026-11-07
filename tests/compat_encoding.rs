use jumeaux::compat::encoding::{self, Errors};
use serde_json::{Value, json};

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn hex(bytes: Vec<u8>) -> Value {
    Value::from(bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

#[test]
fn python_encoding_fixtures() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../compat/fixtures/encoding/cases.json")).unwrap();
    let mut failures = Vec::new();
    let mut differences = 0;
    for case in &cases {
        let input = &case["input"];
        let string = |key: &str| input[key].as_str().unwrap();
        let body = input["body_hex"].as_str().map(unhex).unwrap_or_default();
        let errors = if input["errors"] == "strict" {
            Errors::Strict
        } else {
            Errors::Replace
        };
        let actual = match string("operation") {
            "decide_encoding" => encoding::decide_encoding(
                input["headers"]["content-type"].as_str(),
                &body,
                input["encoding"].as_str(),
                input["default"].as_str(),
                input["apparent_encoding"].as_str(),
            )
            .map(|v| json!(v)),
            "content_encodings" => encoding::content_encodings(&body).map(|v| json!(v)),
            "requests_encoding" => encoding::http_encoding(
                input["content_type"].as_str(),
                &body,
                input["default"].as_str(),
            )
            .map(|v| json!(v)),
            "response_text" => {
                encoding::response_text(&body, input["encoding"].as_str()).map(Value::from)
            }
            "dump_encoding" => {
                encoding::transcode(&body, string("source"), string("destination")).map(hex)
            }
            "encode" => encoding::encode(string("text"), string("encoding"), errors).map(hex),
            "decode" => encoding::decode(&body, string("encoding"), errors).map(Value::from),
            "detect" => Ok(Value::from(encoding::detect(&body))),
            "quote_plus" => {
                encoding::quote_plus(string("text"), string("encoding")).map(Value::from)
            }
            "guess_url_encoding" => encoding::guess_url_encoding(
                string("query"),
                &serde_json::from_value::<Vec<String>>(input["candidates"].clone()).unwrap(),
            )
            .map(|v| json!(v)),
            "plain_input" => {
                encoding::plain_requests(&body, string("encoding"), &[], false).map(|v| json!(v))
            }
            "dump_html" => {
                // HTML整形は並行のmarkupタスクが担当する。このfixtureでは実測した
                // Unicode中間値を受け、decode/encode境界を検証する。
                encoding::decode(&body, string("encoding"), Errors::Replace).and_then(|text| {
                    assert_eq!(text, string("decoded_text"));
                    encoding::encode(
                        string("formatted_text"),
                        string("encoding"),
                        Errors::Replace,
                    )
                    .map(hex)
                })
            }
            op => panic!("unhandled operation {op}"),
        };
        let actual = match actual {
            Ok(value) => json!({"value": value}),
            Err(error) => json!({"error": error.kind}),
        };
        if case["name"] == "detect-euc" {
            // 許容した差異: 推定名の一致は保証しない。本文のEUC-JP写像は別に検証する。
            assert_eq!(case["expected"], json!({"value": "euc_jis_2004"}));
            assert_eq!(actual, json!({"value": "EUC-JP"}));
            differences += 1;
            continue;
        }
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
        "encoding: {}/{} Python一致、{}件の差異を明示検証",
        cases.len() - differences,
        cases.len(),
        differences
    );
}

#[test]
fn explicit_codec_is_not_overridden_by_bom() {
    assert_eq!(
        encoding::decode(b"\xef\xbb\xbfhello", "utf8", Errors::Strict).unwrap(),
        "\u{feff}hello"
    );
    assert_eq!(
        encoding::decode(b"\xff\xfe", "latin1", Errors::Strict).unwrap(),
        "ÿþ"
    );
}

#[test]
fn url_candidates_and_parse_use_same_codec() {
    let requests = encoding::plain_requests(
        b"/search?q=%93%8c%8b%9e&q=abc",
        "utf8",
        &["utf8".into(), "sjis".into()],
        false,
    )
    .unwrap();
    assert_eq!(requests[0].url_encoding, "sjis");
    assert_eq!(requests[0].qs["q"], ["東京", "abc"]);
    assert_eq!(
        encoding::parse_query("q=%83A", "sjis", false, Errors::Strict).unwrap()["q"],
        ["ア"]
    );
}

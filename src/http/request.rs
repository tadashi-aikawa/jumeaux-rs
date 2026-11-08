//! requestsへ渡す直前のURL・ヘッダ・本文を組み立てる。
use crate::{
    compat::{Error, Result, encoding::quote_plus, regex::Regex, template::parse_datetime},
    models::{AccessPoint, Headers, HttpMethod, Queries, QueryCustomization, Request},
};
use reqwest::Url;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    /// failureのTrialはrequestsによる正規化前のURLを保存する。
    pub original_url: String,
    pub url: Url,
    pub method: HttpMethod,
    pub headers: Headers,
    pub body: Vec<u8>,
}

fn select_key(pattern: &str, original: &Queries) -> String {
    if let Some(key) = pattern.strip_suffix("/i") {
        original
            .keys()
            .find(|k| k.to_lowercase() == key.to_lowercase())
            .cloned()
            .unwrap_or_else(|| key.into())
    } else {
        pattern.into()
    }
}

pub fn query_string(
    qs: &Queries,
    custom: Option<&QueryCustomization>,
    encoding: &str,
) -> Result<String> {
    let mut result = qs.clone();
    if let Some(custom) = custom {
        if let Some(overwrite) = &custom.overwrite {
            for (key, values) in overwrite {
                result.insert(
                    select_key(key, qs),
                    values
                        .iter()
                        .map(|v| parse_datetime(v))
                        .collect::<Result<_>>()?,
                );
            }
        }
        if let Some(remove) = &custom.remove {
            for key in remove {
                // Pythonはoverwrite後でもキーの検索には元のqsを使う。
                result.shift_remove(&select_key(key, qs));
            }
        }
    }
    result
        .iter()
        .flat_map(|(key, values)| values.iter().map(move |value| (key, value)))
        .map(|(key, value)| {
            Ok(format!(
                "{}={}",
                quote_plus(key, encoding)?,
                quote_plus(value, encoding)?
            ))
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("&"))
}

pub fn merge_headers(endpoint: &Headers, request: &Headers) -> Headers {
    merge_headers_preserving_case(endpoint, request)
        .into_iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v))
        .collect()
}

pub(super) fn merge_headers_preserving_case(endpoint: &Headers, request: &Headers) -> Headers {
    let mut headers = Headers::from([(
        "User-Agent".into(),
        format!("jumeaux/{}", env!("CARGO_PKG_VERSION")),
    )]);
    headers.extend(endpoint.clone());
    headers.extend(request.clone());
    // Pythonの通常dictによるassignの後、requestsが大小無視dictへ変換する。
    headers
}

fn python_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(v) => if *v { "True" } else { "False" }.into(),
        Value::Null => "None".into(),
        Value::Array(vs) => format!(
            "[{}]",
            vs.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(vs) => format!(
            "{{{}}}",
            vs.iter()
                .map(|(k, v)| format!(
                    "{}: {}",
                    python_repr(&Value::String(k.clone())),
                    python_repr(v)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Number(n) => number_string(n),
    }
}

fn number_string(number: &serde_json::Number) -> String {
    let text = number.to_string();
    if !text.contains(['.', 'e', 'E']) {
        return text;
    }
    let value: f64 = text.parse().expect("JSON number");
    if !value.is_finite() {
        return value.to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if !(-4..16).contains(&exponent) {
        format!("{mantissa}e{exponent:+03}")
    } else {
        let text = value.to_string();
        if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        }
    }
}

fn validate_json_numbers(value: &Value) -> Result<()> {
    match value {
        Value::Number(n)
            if n.to_string().contains(['.', 'e', 'E'])
                && !n
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(f64::INFINITY)
                    .is_finite() =>
        {
            Err(Error::new(
                "InvalidJSONError",
                "Out of range float values are not JSON compliant",
            ))
        }
        Value::Array(vs) => vs.iter().try_for_each(validate_json_numbers),
        Value::Object(vs) => vs.values().try_for_each(validate_json_numbers),
        _ => Ok(()),
    }
}

fn python_repr(value: &Value) -> String {
    if let Value::String(s) = value {
        let quote = if s.contains('\'') && !s.contains('"') {
            '"'
        } else {
            '\''
        };
        format!(
            "{quote}{}{quote}",
            s.replace('\\', "\\\\")
                .replace(quote, &format!("\\{quote}"))
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t")
        )
    } else {
        python_str(value)
    }
}

/// requestsのjson.dumps(ensure_ascii=True)の区切りと文字エスケープ。
pub fn json_body(value: &Value) -> String {
    match value {
        Value::Array(vs) => format!(
            "[{}]",
            vs.iter().map(json_body).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(vs) => format!(
            "{{{}}}",
            vs.iter()
                .map(|(k, v)| format!("{}: {}", json_body(&Value::String(k.clone())), json_body(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::String(s) => {
            let quoted = serde_json::to_string(s).expect("string");
            let mut result = String::new();
            for c in quoted.chars() {
                if c.is_ascii() && c != '\u{7f}' {
                    result.push(c);
                } else {
                    use std::fmt::Write;
                    for unit in c.encode_utf16(&mut [0; 2]) {
                        write!(result, "\\u{unit:04x}").unwrap();
                    }
                }
            }
            result
        }
        Value::Number(n) => number_string(n),
        _ => value.to_string(),
    }
}

fn body(request: &Request) -> Result<(Vec<u8>, Option<&'static str>)> {
    if request.method == HttpMethod::Get {
        return Ok((Vec::new(), None));
    }
    if let Some(raw) = request.raw.as_ref().filter(|v| !v.is_empty()) {
        return Ok((raw.as_bytes().to_vec(), None));
    }
    if let Some(form) = request.form.as_ref().filter(|v| !v.is_empty()) {
        let mut pairs = Vec::new();
        for (key, value) in form {
            let values: Vec<Value> = match value {
                Value::Array(vs) => vs.clone(),
                Value::Object(vs) => vs.keys().cloned().map(Value::String).collect(),
                _ => vec![value.clone()],
            };
            for value in values.into_iter().filter(|v| !v.is_null()) {
                // requestsの展開後、urllib.urlencode(doseq=True)がもう1段展開する。
                let values = match value {
                    Value::Array(vs) => vs,
                    Value::Object(vs) => vs.keys().cloned().map(Value::String).collect(),
                    value => vec![value],
                };
                for value in values {
                    pairs.push(format!(
                        "{}={}",
                        quote_plus(key, "utf-8")?,
                        quote_plus(&python_str(&value), "utf-8")?
                    ));
                }
            }
        }
        return Ok((
            pairs.join("&").into_bytes(),
            Some("application/x-www-form-urlencoded"),
        ));
    }
    if let Some(json) = &request.json {
        validate_json_numbers(&Value::Object(json.clone()))?;
        return Ok((
            json_body(&Value::Object(json.clone())).into_bytes(),
            Some("application/json"),
        ));
    }
    Ok((Vec::new(), None))
}

/// requests.utils.requote_uri: unreservedの%HHは復元し、不正な%はescapeする。
pub fn requote_url(value: &str) -> String {
    let bytes = value.as_bytes();
    let invalid = bytes.iter().enumerate().any(|(i, &b)| {
        b == b'%'
            && (i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit())
    });
    let mut result = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && !invalid {
            let decoded = u8::from_str_radix(&value[i + 1..i + 3], 16).expect("hex");
            if decoded.is_ascii_alphanumeric() || b"-._~".contains(&decoded) {
                result.push(decoded as char);
            } else {
                result.push_str(&value[i..i + 3].to_ascii_uppercase());
            }
            i += 3;
        } else {
            if b.is_ascii_alphanumeric() || b"!#$&'()*+,-./:;=?@[]_~".contains(&b) {
                result.push(b as char);
            } else {
                use std::fmt::Write;
                write!(result, "%{b:02X}").unwrap();
            }
            i += 1;
        }
    }
    result
}

pub fn prepare(endpoint: &AccessPoint, request: &Request) -> Result<PreparedRequest> {
    let path = if let Some(path) = &endpoint.path {
        Regex::new(&path.before)?.sub(&path.after, &request.path)?
    } else {
        request.path.clone()
    };
    let original_url = format!(
        "{}{}?{}",
        endpoint.host,
        path,
        query_string(&request.qs, endpoint.query.as_ref(), &request.url_encoding)?
    );
    let mut url = Url::parse(&requote_url(&original_url))
        .map_err(|e| Error::new("InvalidURL", e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::new("InvalidSchema", url.scheme()));
    }
    if url.query() == Some("") {
        url.set_query(None);
    }
    let (body, content_type) = body(request)?;
    let mut headers = merge_headers(&endpoint.headers, &request.headers);
    if request.method == HttpMethod::Post
        && !body.is_empty()
        && headers.contains_key("content-length")
    {
        headers.insert("content-length".into(), body.len().to_string());
    }
    if let Some(content_type) = content_type {
        headers
            .entry("content-type".into())
            .or_insert_with(|| content_type.into());
    }
    Ok(PreparedRequest {
        original_url,
        url,
        method: request.method,
        headers,
        body,
    })
}

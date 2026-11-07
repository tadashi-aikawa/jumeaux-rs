//! JSON入力の共通境界。非有限数を拒否し、コンテナの深さを1000までに制限する。
use super::{Error, Result};
use serde::Deserialize;
use serde_json::Value;

pub const MAX_DEPTH: usize = 1000;

pub fn nonfinite_error() -> Error {
    Error::new("ValueError", "non-finite numbers are not supported")
}

fn depth_error() -> Error {
    Error::new("LimitExceeded", "JSON nesting exceeds 1000 containers")
}

/// 引用文字列を飛ばして数値字句を検査する。whenの描画結果にも使う。
pub fn scan(text: &str) -> Result<usize> {
    scan_tokens(text, true)
}

/// 条件式内の識別子は数値字句と区別する。値は式入力・描画結果で検査する。
pub(crate) fn scan_expression(text: &str) -> Result<usize> {
    scan_tokens(text, false)
}

fn scan_tokens(text: &str, nonfinite_names: bool) -> Result<usize> {
    let bytes = text.as_bytes();
    let (mut i, mut depth, mut max_depth) = (0, 0usize, 0);
    while i < bytes.len() {
        match bytes[i] {
            quote @ (b'"' | b'\'') => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == quote {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'[' | b'{' => {
                depth += 1;
                max_depth = max_depth.max(depth);
                if depth > MAX_DEPTH {
                    return Err(depth_error());
                }
                i += 1;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_digit()
                        || matches!(bytes[i], b'.' | b'e' | b'E')
                        || (matches!(bytes[i], b'+' | b'-') && matches!(bytes[i - 1], b'e' | b'E')))
                {
                    i += 1;
                }
                let token = &text[start..i];
                if token.contains(['.', 'e', 'E'])
                    && token.parse::<f64>().is_ok_and(|n| !n.is_finite())
                {
                    return Err(nonfinite_error());
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if nonfinite_names
                    && matches!(
                        &text[start..i],
                        "NaN" | "nan" | "Infinity" | "infinity" | "inf"
                    )
                {
                    return Err(nonfinite_error());
                }
            }
            _ => i += 1,
        }
    }
    Ok(max_depth)
}

/// serde_json::Valueから直接渡されたオーバーフローも、文字列入口と同じエラーにする。
pub fn validate(value: &Value) -> Result<usize> {
    let mut pending = vec![(value, 0)];
    let mut maximum = 0;
    while let Some((value, parent_depth)) = pending.pop() {
        match value {
            Value::Array(_) | Value::Object(_) => {
                let depth = parent_depth + 1;
                if depth > MAX_DEPTH {
                    return Err(depth_error());
                }
                maximum = maximum.max(depth);
                match value {
                    Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth))),
                    Value::Object(values) => pending.extend(values.values().map(|v| (v, depth))),
                    _ => unreachable!(),
                }
            }
            Value::Number(number)
                if number.as_str().contains(['.', 'e', 'E'])
                    && !number
                        .as_str()
                        .parse::<f64>()
                        .map_err(|_| nonfinite_error())?
                        .is_finite() =>
            {
                return Err(nonfinite_error());
            }
            _ => {}
        }
    }
    Ok(maximum)
}

/// 深い入力だけ専用スタックで処理する。通常の応答ではスレッドを作らない。
pub(crate) fn with_stack<T: Send>(depth: usize, f: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    if depth <= 64 {
        return f();
    }
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("bounded-json".into())
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, f)
            .map_err(|e| Error::new("RuntimeError", e.to_string()))?;
        match worker.join() {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

pub fn parse(text: &str) -> Result<Value> {
    let depth = scan(text)?;
    with_stack(depth, || {
        let mut deserializer = serde_json::Deserializer::from_str(text);
        deserializer.disable_recursion_limit();
        let value = Value::deserialize(&mut deserializer)
            .map_err(|e| Error::new("JSONDecodeError", e.to_string()))?;
        deserializer
            .end()
            .map_err(|e| Error::new("JSONDecodeError", e.to_string()))?;
        validate(&value)?;
        Ok(value)
    })
}

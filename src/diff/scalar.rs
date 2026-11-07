use num_bigint::BigInt;
use num_traits::FromPrimitive;
use serde_json::{Number, Value};

#[derive(PartialEq)]
enum PythonNumber {
    Integer(BigInt),
    Float(f64),
}

impl From<&Number> for PythonNumber {
    fn from(value: &Number) -> Self {
        let literal = value.as_str();
        // is_f64/as_i64では範囲外の整数を分類できないため、JSONの構文で分ける。
        if literal.contains(['.', 'e', 'E']) {
            Self::Float(
                literal
                    .parse()
                    .expect("JSON number is a valid float literal"),
            )
        } else {
            Self::Integer(
                literal
                    .parse()
                    .expect("JSON integer is a valid BigInt literal"),
            )
        }
    }
}

// 通常の比較は型変更を検出する。小数の表記差はPythonと同じくf64で吸収する。
pub(super) fn number_equal(a: &Number, b: &Number) -> bool {
    PythonNumber::from(a) == PythonNumber::from(b)
}

/// SequenceMatcher用のPythonの等値クラス。True == 1 == 1.0、-0.0 == 0。
#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) enum ScalarKey {
    Null,
    Integer(BigInt),
    Float(u64),
    String(String),
}

impl From<&Value> for ScalarKey {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Integer(BigInt::from(u8::from(*b))),
            Value::String(s) => Self::String(s.clone()),
            Value::Number(n) => match PythonNumber::from(n) {
                PythonNumber::Integer(n) => Self::Integer(n),
                PythonNumber::Float(f) if f.fract() == 0.0 => {
                    // 整数側をf64へ丸めず、floatの実際の整数値へ合わせる。
                    Self::Integer(BigInt::from_f64(f).expect("integral finite float"))
                }
                PythonNumber::Float(f) => Self::Float(f.to_bits()),
            },
            Value::Array(_) | Value::Object(_) => unreachable!("scalar arrays only"),
        }
    }
}

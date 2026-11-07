//! Jinja2描画とast.literal_evalの二段階を維持する。
use std::fmt::Write;

use chrono::{DateTime, Duration, FixedOffset, Local};
use minijinja::{Environment, Value, value::ValueKind};
use rustpython_parser::{Parse, ast};
use serde::Serialize;
use serde_json::Value as Json;

use super::{Error, Result, regex};

pub struct TemplateEngine {
    env: Environment<'static>,
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TemplateEngine {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
        env.set_formatter(|out, _state, value| {
            if value.kind() == ValueKind::Invalid {
                value.try_iter()?;
            }
            write!(out, "{}", python_display(value, false)).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::WriteFailure, e.to_string())
            })
        });
        env.add_filter("reg", |target: &str, pattern: &str| {
            regex::exact_match(target, pattern).map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
                    .with_source(e)
            })
        });
        env.add_function("calc_distance_km", calc_distance_km);
        env.add_function("equals_without_host", equals_without_host);
        Self { env }
    }

    pub fn validate(&self, template: &str) -> Result<()> {
        self.env
            .template_from_str(template)
            .map(|_| ())
            .map_err(template_error)
    }

    pub fn render(&self, template: &str, data: impl Serialize) -> Result<String> {
        self.env
            .render_str(template, super::values::serialized(data))
            .map_err(template_error)
    }

    /// boolに即時変換しない。数値・リストなどは後続の呼び出し元がtruthinessを判定する。
    pub fn when(&self, expression: &str, data: impl Serialize) -> Result<LiteralValue> {
        let rendered = self.render(&format!("{{{{{expression}}}}}"), data)?;
        literal_eval(&rendered)
    }
}

fn template_error(error: minijinja::Error) -> Error {
    use std::error::Error as _;
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(error) = cause.downcast_ref::<Error>() {
            return error.clone();
        }
        source = cause.source();
    }
    let kind = match error.kind() {
        minijinja::ErrorKind::UndefinedError => "UndefinedError",
        minijinja::ErrorKind::SyntaxError => "TemplateSyntaxError",
        minijinja::ErrorKind::UnknownMethod | minijinja::ErrorKind::InvalidOperation => "TypeError",
        _ => "TemplateError",
    };
    Error::new(kind, error.to_string())
}

fn python_display(value: &Value, nested: bool) -> String {
    match value.kind() {
        ValueKind::Undefined => String::new(),
        ValueKind::None => "None".into(),
        ValueKind::Bool => if value.is_true() { "True" } else { "False" }.into(),
        ValueKind::String if nested => python_repr(value.as_str().unwrap_or("")),
        ValueKind::Bytes => {
            let mut output = String::from("b'");
            for b in value.as_bytes().unwrap_or_default() {
                match *b {
                    b'\'' => output.push_str("\\'"),
                    b'\\' => output.push_str("\\\\"),
                    b'\n' => output.push_str("\\n"),
                    b'\r' => output.push_str("\\r"),
                    b'\t' => output.push_str("\\t"),
                    32..=126 => output.push(*b as char),
                    b => {
                        write!(output, "\\x{b:02x}").unwrap();
                    }
                }
            }
            output.push('\'');
            output
        }
        ValueKind::Seq => format!(
            "[{}]",
            value
                .try_iter()
                .unwrap()
                .map(|v| python_display(&v, true))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ValueKind::Map => format!(
            "{{{}}}",
            value
                .try_iter()
                .unwrap()
                .map(|k| format!(
                    "{}: {}",
                    python_display(&k, true),
                    python_display(&value.get_item(&k).unwrap_or_default(), true)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}

fn python_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut result = quote.to_string();
    for c in text.chars() {
        match c {
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\\' => result.push_str("\\\\"),
            c if c == quote => {
                result.push('\\');
                result.push(c);
            }
            c if c.is_control() => {
                write!(result, "\\x{:02x}", u32::from(c)).unwrap();
            }
            c => result.push(c),
        }
    }
    result.push(quote);
    result
}

/// ast.literal_evalの型を保つ。JSONに投影するとtuple等の型は消えるため、
/// 条件判定はtruthy()、型が要る呼び出し元はこのenumを使う。
#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    None,
    Bool(bool),
    Int(ast::bigint::BigInt),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Self>),
    Tuple(Vec<Self>),
    Set(Vec<Self>),
    Dict(Vec<(Self, Self)>),
    Complex { real: f64, imag: f64 },
    Ellipsis,
}

impl LiteralValue {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::None => "NoneType",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "str",
            Self::Bytes(_) => "bytes",
            Self::List(_) => "list",
            Self::Tuple(_) => "tuple",
            Self::Set(_) => "set",
            Self::Dict(_) => "dict",
            Self::Complex { .. } => "complex",
            Self::Ellipsis => "ellipsis",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Self::None => false,
            Self::Bool(v) => *v,
            Self::Int(v) => *v != ast::bigint::BigInt::from(0),
            Self::Float(v) => *v != 0.0,
            Self::String(v) => !v.is_empty(),
            Self::Bytes(v) => !v.is_empty(),
            Self::List(v) | Self::Tuple(v) | Self::Set(v) => !v.is_empty(),
            Self::Dict(v) => !v.is_empty(),
            Self::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
            Self::Ellipsis => true,
        }
    }

    pub fn to_json(&self) -> Result<Json> {
        let invalid = || {
            Error::new(
                "TypeError",
                format!("{} is not JSON representable", self.type_name()),
            )
        };
        match self {
            Self::None => Ok(Json::Null),
            Self::Bool(v) => Ok(Json::Bool(*v)),
            Self::String(v) => Ok(Json::String(v.clone())),
            Self::Int(v) => v
                .to_string()
                .parse::<i64>()
                .map(Json::from)
                .or_else(|_| v.to_string().parse::<u64>().map(Json::from))
                .map_err(|_| invalid()),
            Self::Float(v) => serde_json::Number::from_f64(*v)
                .map(Json::Number)
                .ok_or_else(invalid),
            Self::List(v) | Self::Tuple(v) => v
                .iter()
                .map(Self::to_json)
                .collect::<Result<Vec<_>>>()
                .map(Json::Array),
            Self::Dict(v) => {
                let mut map = serde_json::Map::new();
                for (key, value) in v {
                    let Self::String(key) = key else {
                        return Err(invalid());
                    };
                    map.insert(key.clone(), value.to_json()?);
                }
                Ok(Json::Object(map))
            }
            _ => Err(invalid()),
        }
    }

    fn hashable(&self) -> bool {
        match self {
            Self::List(_) | Self::Set(_) | Self::Dict(_) => false,
            Self::Tuple(v) => v.iter().all(Self::hashable),
            _ => true,
        }
    }

    fn python_equal(&self, other: &Self) -> bool {
        // Pythonのdict/setではTrue、1、1.0が同じキーになる。
        match (self, other) {
            (Self::Bool(v), other) => {
                Self::Int(ast::bigint::BigInt::from(u8::from(*v))).python_equal(other)
            }
            (this, Self::Bool(v)) => {
                this.python_equal(&Self::Int(ast::bigint::BigInt::from(u8::from(*v))))
            }
            (Self::Int(i), Self::Float(f)) | (Self::Float(f), Self::Int(i)) => {
                f.is_finite()
                    && f.fract() == 0.0
                    && format!("{f:.0}")
                        .parse::<ast::bigint::BigInt>()
                        .is_ok_and(|v| v == *i)
            }
            (Self::Complex { real, imag }, other) if *imag == 0.0 => {
                Self::Float(*real).python_equal(other)
            }
            (this, Self::Complex { real, imag }) if *imag == 0.0 => {
                this.python_equal(&Self::Float(*real))
            }
            (Self::Tuple(a), Self::Tuple(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.python_equal(b))
            }
            _ => self == other,
        }
    }
}

pub fn literal_eval(text: &str) -> Result<LiteralValue> {
    let expr = ast::Expr::parse(text.trim_start_matches([' ', '\t']), "<when>")
        .map_err(|e| Error::new("SyntaxError", e.to_string()))?;
    literal(&expr)
}

fn literal(expr: &ast::Expr) -> Result<LiteralValue> {
    let invalid = || {
        Error::new(
            "ValueError",
            "when result is not a supported Python literal",
        )
    };
    match expr {
        ast::Expr::Constant(expr) => match &expr.value {
            ast::Constant::None => Ok(LiteralValue::None),
            ast::Constant::Bool(value) => Ok(LiteralValue::Bool(*value)),
            ast::Constant::Str(value) => Ok(LiteralValue::String(value.clone())),
            ast::Constant::Int(value) => Ok(LiteralValue::Int(value.clone())),
            ast::Constant::Float(value) => Ok(LiteralValue::Float(*value)),
            ast::Constant::Bytes(value) => Ok(LiteralValue::Bytes(value.clone())),
            ast::Constant::Complex { real, imag } => Ok(LiteralValue::Complex {
                real: *real,
                imag: *imag,
            }),
            ast::Constant::Ellipsis => Ok(LiteralValue::Ellipsis),
            ast::Constant::Tuple(value) => value
                .iter()
                .map(|value| {
                    literal(&ast::Expr::Constant(ast::ExprConstant {
                        range: expr.range,
                        value: value.clone(),
                        kind: None,
                    }))
                })
                .collect::<Result<Vec<_>>>()
                .map(LiteralValue::Tuple),
        },
        ast::Expr::List(expr) => expr
            .elts
            .iter()
            .map(literal)
            .collect::<Result<Vec<_>>>()
            .map(LiteralValue::List),
        ast::Expr::Tuple(expr) => expr
            .elts
            .iter()
            .map(literal)
            .collect::<Result<Vec<_>>>()
            .map(LiteralValue::Tuple),
        ast::Expr::Set(expr) => {
            let mut values = Vec::new();
            for expr in &expr.elts {
                let value = literal(expr)?;
                if !value.hashable() {
                    return Err(Error::new("TypeError", "unhashable set member"));
                }
                if !values.iter().any(|v: &LiteralValue| v.python_equal(&value)) {
                    values.push(value);
                }
            }
            Ok(LiteralValue::Set(values))
        }
        ast::Expr::Call(expr)
            if expr.args.is_empty()
                && expr.keywords.is_empty()
                && matches!(&*expr.func, ast::Expr::Name(name) if name.id.as_str() == "set") =>
        {
            Ok(LiteralValue::Set(Vec::new()))
        }
        ast::Expr::Dict(expr) => {
            let mut result: Vec<(LiteralValue, LiteralValue)> = Vec::new();
            for (key, value) in expr.keys.iter().zip(&expr.values) {
                let key = literal(key.as_ref().ok_or_else(invalid)?)?;
                if !key.hashable() {
                    return Err(Error::new("TypeError", "unhashable dict key"));
                }
                let value = literal(value)?;
                if let Some((_, old)) = result.iter_mut().find(|(k, _)| k.python_equal(&key)) {
                    *old = value;
                } else {
                    result.push((key, value));
                }
            }
            Ok(LiteralValue::Dict(result))
        }
        ast::Expr::UnaryOp(expr) if matches!(expr.op, ast::UnaryOp::UAdd | ast::UnaryOp::USub) => {
            if !matches!(*expr.operand, ast::Expr::Constant(_)) {
                return Err(invalid());
            }
            let value = literal(&expr.operand)?;
            let minus = expr.op == ast::UnaryOp::USub;
            match value {
                LiteralValue::Int(value) => {
                    Ok(LiteralValue::Int(if minus { -value } else { value }))
                }
                LiteralValue::Float(value) => {
                    Ok(LiteralValue::Float(if minus { -value } else { value }))
                }
                LiteralValue::Complex { real, imag } => Ok(LiteralValue::Complex {
                    real: if minus { -real } else { real },
                    imag: if minus { -imag } else { imag },
                }),
                _ => Err(invalid()),
            }
        }
        ast::Expr::BinOp(expr) if matches!(expr.op, ast::Operator::Add | ast::Operator::Sub) => {
            let real = match literal(&expr.left)? {
                LiteralValue::Int(v) => v.to_string().parse::<f64>().map_err(|_| invalid())?,
                LiteralValue::Float(v) => v,
                _ => return Err(invalid()),
            };
            if !matches!(*expr.right, ast::Expr::Constant(_)) {
                return Err(invalid());
            }
            let LiteralValue::Complex { imag, .. } = literal(&expr.right)? else {
                return Err(invalid());
            };
            Ok(LiteralValue::Complex {
                real,
                imag: if expr.op == ast::Operator::Sub {
                    -imag
                } else {
                    imag
                },
            })
        }
        _ => Err(invalid()),
    }
}

pub fn calc_distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let rad1 = lat1.to_radians();
    let rad2 = lat2.to_radians();
    let lat = (lat2 - lat1).to_radians();
    let lon = (lon2 - lon1).to_radians();
    let a = (lat / 2.0).sin() * (lat / 2.0).sin()
        + rad1.cos() * rad2.cos() * (lon / 2.0).sin() * (lon / 2.0).sin();
    6371.0 * (2.0 * a.sqrt().atan2((1.0 - a).sqrt()))
}

pub fn equals_without_host(one: &str, other: &str) -> bool {
    let pattern = regex::Regex::new("https?://[^$/]+").expect("constant regex");
    pattern.sub("", one).unwrap() == pattern.sub("", other).unwrap()
}

pub fn parse_datetime(value: &str) -> Result<String> {
    let now = Local::now();
    parse_datetime_with_zone(value, now.fixed_offset(), &now.format("%Z").to_string())
}

pub fn parse_datetime_at(value: &str, now: DateTime<FixedOffset>) -> Result<String> {
    let zone = if now.offset().local_minus_utc() == 0 {
        "UTC".into()
    } else {
        format!("UTC{}", now.offset())
    };
    parse_datetime_with_zone(value, now, &zone)
}

fn parse_datetime_with_zone(value: &str, now: DateTime<FixedOffset>, zone: &str) -> Result<String> {
    let re = ::regex::Regex::new(r"^\$DATETIME\((.+)\)\((.+)\)(?:\n)?$").expect("constant regex");
    let Some(captures) = re.captures(value) else {
        return Ok(value.into());
    };
    let seconds: i64 = captures[2]
        .trim()
        .parse()
        .map_err(|e| Error::new("ValueError", format!("{value}: {e}")))?;
    let offset =
        Duration::try_seconds(seconds).ok_or_else(|| Error::new("OverflowError", value))?;
    let time = now
        .checked_add_signed(offset)
        .ok_or_else(|| Error::new("OverflowError", value))?;
    let mut result = String::new();
    let mut format = String::new();
    let mut chars = captures[1].chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            format.push(c);
            continue;
        }
        match chars.next() {
            Some('f') => format.push_str("%6f"),
            Some('Z') => format.push_str(zone),
            Some(c) => {
                format.push('%');
                format.push(c);
            }
            None => format.push('%'),
        }
    }
    write!(result, "{}", time.format(&format)).map_err(|_| {
        Error::new(
            "ValueError",
            format!("unsupported datetime format: {}", &captures[1]),
        )
    })?;
    Ok(result)
}

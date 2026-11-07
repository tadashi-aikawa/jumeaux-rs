//! Jinja2描画とast.literal_evalの二段階を維持する。
use std::{
    fmt::Write,
    sync::{LazyLock, RwLock},
};

use chrono::{DateTime, Duration, FixedOffset, Local};
use minijinja::{
    Environment, State, Value,
    value::{Kwargs, Rest, ValueKind, ValueOrKwargs, from_args},
};
use rustpython_parser::{Parse, ast};
use serde_json::Value as Json;

use super::{
    Error, Result, regex,
    values::{RenderInput, TemplateInput},
};

pub struct TemplateEngine {
    env: RwLock<Environment<'static>>,
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
        env.add_filter("int", python_int);
        env.add_filter("float", python_float);
        env.add_filter("tojson", python_tojson);
        env.add_filter("string", |value: &Value| {
            if value.is_safe() {
                value.clone()
            } else {
                Value::from(python_display(value, false))
            }
        });
        env.add_filter("join", python_join);
        env.add_filter("replace", python_replace);
        env.add_filter("urlencode", python_urlencode);
        use minijinja_contrib::{filters, globals};
        env.add_filter(
            "truncate",
            |state: &mut State, value: &Value, args: Rest<ValueOrKwargs>| {
                filters::truncate(
                    state,
                    value,
                    positional_kwargs(args, &["length", "killwords", "end", "leeway"])?,
                )
            },
        );
        env.add_filter(
            "filesizeformat",
            |value: f64, args: Rest<ValueOrKwargs>| -> MiniResult<String> {
                let kwargs = positional_kwargs(args, &["binary"])?;
                let binary = kwargs.get("binary")?;
                kwargs.assert_all_used()?;
                Ok(filters::filesizeformat(value, binary))
            },
        );
        env.add_filter("striptags", filters::striptags);
        env.add_filter("wordcount", filters::wordcount);
        env.add_filter("wordwrap", |value: &Value, args: Rest<ValueOrKwargs>| {
            filters::wordwrap(
                value,
                positional_kwargs(
                    args,
                    &[
                        "width",
                        "break_long_words",
                        "wrapstring",
                        "break_on_hyphens",
                    ],
                )?,
            )
        });
        env.add_filter(
            "sort",
            |state: &State, value: Value, args: Rest<ValueOrKwargs>| {
                minijinja::filters::sort(
                    state,
                    value,
                    positional_kwargs(args, &["reverse", "case_sensitive", "attribute"])?,
                )
            },
        );
        env.add_filter("dictsort", |value: &Value, args: Rest<ValueOrKwargs>| {
            minijinja::filters::dictsort(
                value,
                positional_kwargs(args, &["case_sensitive", "by", "reverse"])?,
            )
        });
        env.add_filter(
            "unique",
            |state: &State, value: Value, args: Rest<ValueOrKwargs>| {
                minijinja::filters::unique(
                    state,
                    value,
                    positional_kwargs(args, &["case_sensitive", "attribute"])?,
                )
            },
        );
        env.add_filter("groupby", |value: Value, args: Rest<ValueOrKwargs>| {
            minijinja::filters::groupby(
                value,
                None,
                positional_kwargs(args, &["attribute", "default", "case_sensitive"])?,
            )
        });
        env.add_filter("random", filters::random);
        env.add_function("cycler", |items: Rest<Value>| globals::cycler(items.0));
        env.add_function("joiner", globals::joiner);
        env.add_function("lipsum", globals::lipsum);
        for name in ["bool", "chain", "lines", "split", "zip"] {
            env.remove_filter(name);
        }
        for name in ["startingwith", "endingwith", "safe"] {
            env.remove_test(name);
        }
        Self {
            env: RwLock::new(env),
        }
    }

    pub fn validate(&self, template: &str) -> Result<()> {
        self.load(template)
    }

    /// 設定読み込み時に構文・登録名を検査し、コンパイル済みテンプレートを保持する。
    pub fn load(&self, template: &str) -> Result<()> {
        if self
            .env
            .read()
            .expect("template environment")
            .get_template(template)
            .is_ok()
        {
            return Ok(());
        }
        let mut env = self.env.write().expect("template environment");
        if env.get_template(template).is_ok() {
            return Ok(());
        }
        // 文字列検索ではコメントや未実行分岐を正しく扱えないため、ASTを走査する。
        let ast = minijinja::machinery::parse(template, "<template>", Default::default())
            .map_err(template_error)?;
        let ast = serde_json::to_value(ast)
            .map_err(|e| Error::new("TemplateSyntaxError", e.to_string()))?;
        validate_names(&ast, &env.empty_state())?;
        env.add_template_owned(template.to_owned(), template.to_owned())
            .map_err(template_error)
    }

    pub fn render(&self, template: &str, data: impl RenderInput) -> Result<String> {
        self.load(template)?;
        self.env
            .read()
            .expect("template environment")
            .get_template(template)
            .map_err(template_error)?
            .render(data.into_value())
            .map_err(template_error)
    }

    /// boolに即時変換しない。数値・リストなどは後続の呼び出し元がtruthinessを判定する。
    pub fn when(&self, expression: &str, data: impl TemplateInput) -> Result<LiteralValue> {
        super::json::scan_expression(expression)?;
        let data = data.into_when_value();
        let rendered = self.render(&format!("{{{{{expression}}}}}"), data)?;
        literal_eval(&rendered)
    }
}

fn validate_names(node: &Json, state: &State) -> Result<()> {
    match node {
        Json::Object(fields) => {
            if let Some(kind @ ("Filter" | "Test")) = node["expr"].as_str() {
                let name = node["inner"][0]["name"].as_str().ok_or_else(|| {
                    Error::new("TemplateSyntaxError", "unsupported MiniJinja AST schema")
                })?;
                let exists = if kind == "Filter" {
                    minijinja::tests::is_filter(state, name)
                } else {
                    minijinja::tests::is_test(state, name)
                };
                if !exists {
                    return Err(Error::new(
                        "TemplateAssertionError",
                        format!("unknown {kind}: {name}"),
                    ));
                }
            }
            for child in fields.values() {
                validate_names(child, state)?;
            }
        }
        Json::Array(items) => {
            for child in items {
                validate_names(child, state)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn numeric_string(value: &Value, base: Option<u32>) -> Value {
    let Some(s) = value.as_str() else {
        return value.clone();
    };
    let normalized: String = s
        .trim()
        .chars()
        .map(|c| {
            decimal_digit(c)
                .and_then(|n| char::from_digit(n, 10))
                .unwrap_or(c)
        })
        .collect();
    let s = normalized.as_str();
    let digits = s.trim_start_matches(['+', '-']);
    let prefix = digits.get(..2).unwrap_or("").to_ascii_lowercase();
    let detected = match prefix.as_str() {
        "0x" => 16,
        "0o" => 8,
        "0b" => 2,
        _ => 10,
    };
    let radix = match base {
        Some(0) => detected,
        Some(2..=36) => base.unwrap(),
        _ => 10,
    };
    let prefix_end =
        if base.is_some() && detected == radix && prefix.starts_with('0') && detected != 10 {
            Some(s.len() - digits.len() + 2)
        } else {
            None
        };
    // 数字間と、intの基数prefix直後だけunderscoreを許す。
    let chars: Vec<_> = s.chars().collect();
    if chars.iter().enumerate().any(|(i, c)| {
        *c == '_'
            && (i == 0
                || i + 1 == chars.len()
                || (!chars[i - 1].is_digit(radix) && prefix_end != Some(i))
                || !chars[i + 1].is_digit(radix))
    }) {
        return Value::from(s);
    }
    Value::from(s.replace('_', ""))
}

fn python_int(
    state: &State,
    value: &Value,
    args: Rest<ValueOrKwargs>,
) -> std::result::Result<Value, minijinja::Error> {
    if value.is_undefined() {
        return Err(minijinja::Error::from(minijinja::ErrorKind::UndefinedError));
    }
    let args = args.into_values();
    let (args, kwargs): (&[Value], Kwargs) = from_args(&args)?;
    if args.len() > 2 {
        return Err(minijinja::Error::from(
            minijinja::ErrorKind::TooManyArguments,
        ));
    }
    if (!args.is_empty() && kwargs.has("default")) || (args.len() > 1 && kwargs.has("base")) {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "duplicate filter argument",
        ));
    }
    let default = args
        .first()
        .cloned()
        .or(if kwargs.has("default") {
            Some(kwargs.get::<Value>("default")?)
        } else {
            None
        })
        .unwrap_or(Value::from(0));
    let base_value = if let Some(base) = args.get(1) {
        base.clone()
    } else if kwargs.has("base") {
        kwargs.get::<Value>("base")?
    } else {
        Value::from(10)
    };
    // 不正なbaseもJinja2はfloat変換へfallbackする。未対応の基数1で同じ経路へ渡す。
    let mut base = u32::try_from(base_value).unwrap_or(1);
    kwargs.assert_all_used()?;
    let value = numeric_string(value, Some(base));
    if value.is_none() {
        return Ok(default);
    }
    if value
        .as_str()
        .filter(|s| !(2..=36).contains(&base) || i128::from_str_radix(s, base).is_err())
        .and_then(|s| s.parse::<f64>().ok())
        .is_some_and(|f| !f.is_finite())
        || (value.kind() == ValueKind::Number
            && !value.is_integer()
            && f64::try_from(value.clone()).is_ok_and(|f| !f.is_finite()))
    {
        return Ok(default);
    }
    if let Some(s) = value.as_str() {
        let digits = s.trim_start_matches(['+', '-']);
        if base == 0 {
            let prefix = digits.get(..2).unwrap_or("").to_ascii_lowercase();
            base = match prefix.as_str() {
                "0x" => 16,
                "0o" => 8,
                "0b" => 2,
                _ => 10,
            };
        }
    }
    let converted = minijinja::filters::int(
        state,
        &value,
        None,
        Some(base),
        Kwargs::from_iter(std::iter::empty::<(&str, Value)>()),
    );
    converted.or_else(|_| {
        let fallback = numeric_string(&value, None);
        Ok(fallback
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|f| f.is_finite())
            .map(|f| Value::from(f as i128))
            .unwrap_or(default))
    })
}

fn decimal_digit(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        return c.to_digit(10);
    }
    static DECIMAL: LazyLock<::regex::Regex> =
        LazyLock::new(|| ::regex::Regex::new(r"^\p{Nd}$").unwrap());
    let mut buffer = [0; 4];
    if !DECIMAL.is_match(c.encode_utf8(&mut buffer)) {
        return None;
    }
    // Unicode Ndの連続区間は0〜9の組。数学用数字では複数組が隣接する。
    let mut start = u32::from(c);
    while let Some(previous) = char::from_u32(start - 1) {
        if !DECIMAL.is_match(previous.encode_utf8(&mut buffer)) {
            break;
        }
        start -= 1;
    }
    Some((u32::from(c) - start) % 10)
}

fn python_float(
    state: &State,
    value: &Value,
    args: Rest<ValueOrKwargs>,
) -> std::result::Result<Value, minijinja::Error> {
    if value.is_undefined() {
        return Err(minijinja::Error::from(minijinja::ErrorKind::UndefinedError));
    }
    let args = args.into_values();
    let (args, kwargs): (&[Value], Kwargs) = from_args(&args)?;
    if args.len() > 1 {
        return Err(minijinja::Error::from(
            minijinja::ErrorKind::TooManyArguments,
        ));
    }
    if !args.is_empty() && kwargs.has("default") {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "duplicate filter argument",
        ));
    }
    let default = args
        .first()
        .cloned()
        .or(if kwargs.has("default") {
            Some(kwargs.get::<Value>("default")?)
        } else {
            None
        })
        .unwrap_or(Value::from(0.0));
    kwargs.assert_all_used()?;
    minijinja::filters::float(
        state,
        &numeric_string(value, None),
        Some(default),
        Kwargs::from_iter(std::iter::empty::<(&str, Value)>()),
    )
}

type MiniResult<T> = std::result::Result<T, minijinja::Error>;

fn positional_kwargs(args: Rest<ValueOrKwargs>, names: &[&str]) -> MiniResult<Kwargs> {
    let values = args.into_values();
    let (args, kwargs): (&[Value], Kwargs) = from_args(&values)?;
    if args.len() > names.len() {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "too many positional arguments",
        ));
    }
    let mut mapped = indexmap::IndexMap::new();
    for name in kwargs.args() {
        mapped.insert(name.to_owned(), kwargs.get::<Value>(name)?);
    }
    for (name, value) in names.iter().zip(args) {
        if mapped.insert((*name).into(), value.clone()).is_some() {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "duplicate argument",
            ));
        }
    }
    Ok(mapped.into_iter().collect())
}

fn python_replace(value: &Value, args: Rest<ValueOrKwargs>) -> MiniResult<String> {
    let kwargs = positional_kwargs(args, &["old", "new", "count"])?;
    if !kwargs.has("old") || !kwargs.has("new") {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "replace requires old and new",
        ));
    }
    let old = kwargs.get::<Value>("old")?;
    let new = kwargs.get::<Value>("new")?;
    let count = kwargs.get::<Option<i64>>("count")?.unwrap_or(-1);
    kwargs.assert_all_used()?;
    let input = python_display(value, false);
    let old = python_display(&old, false);
    let new = python_display(&new, false);
    Ok(if count < 0 {
        input.replace(&old, &new)
    } else {
        input.replacen(&old, &new, count as usize)
    })
}

fn python_join(value: &Value, args: Rest<ValueOrKwargs>) -> MiniResult<String> {
    let kwargs = positional_kwargs(args, &["d", "attribute"])?;
    let delimiter = if kwargs.has("d") {
        python_display(&kwargs.get::<Value>("d")?, false)
    } else {
        String::new()
    };
    let attribute = kwargs.get::<Option<Value>>("attribute")?;
    kwargs.assert_all_used()?;
    let mut items = Vec::new();
    for mut item in value.try_iter()? {
        if let Some(attribute) = &attribute {
            for key in attribute.to_string().split('.') {
                item = if let Ok(index) = key.parse::<usize>() {
                    item.get_item(&Value::from(index))?
                } else {
                    item.get_attr(key)?
                };
            }
        }
        if item.kind() == ValueKind::Invalid {
            item.try_iter()?;
        }
        items.push(python_display(&item, false));
    }
    Ok(items.join(&delimiter))
}

fn python_urlencode(value: &Value) -> MiniResult<String> {
    if !matches!(
        value.kind(),
        ValueKind::Map | ValueKind::Seq | ValueKind::Iterable
    ) {
        return minijinja::filters::urlencode(&Value::from(python_display(value, false)));
    }
    let mut pairs = Vec::new();
    for item in value.try_iter()? {
        let (key, value) = if value.kind() == ValueKind::Map {
            (item.clone(), value.get_item(&item)?)
        } else {
            let pair: Vec<_> = item.try_iter()?.collect();
            if pair.len() != 2 {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "urlencode expects pairs",
                ));
            }
            (pair[0].clone(), pair[1].clone())
        };
        let quote = |value| {
            super::encoding::quote_plus(&python_display(value, false), "utf-8").map_err(|e| {
                minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, e.to_string())
            })
        };
        pairs.push(format!("{}={}", quote(&key)?, quote(&value)?));
    }
    Ok(pairs.join("&"))
}

fn python_tojson(
    value: &Value,
    indent: Option<Value>,
    kwargs: Kwargs,
) -> std::result::Result<Value, minijinja::Error> {
    let indent = indent
        .or(kwargs.get::<Option<Value>>("indent")?)
        .map(|v| i64::try_from(v).map(|v| v.max(0) as usize))
        .transpose()?;
    kwargs.assert_all_used()?;
    let rendered = json_display(value, indent, 0)?;
    let mut output = String::new();
    for c in rendered.chars() {
        if matches!(c, '<' | '>' | '&' | '\'') {
            write!(output, "\\u{:04x}", u32::from(c)).unwrap();
        } else if c.is_ascii() {
            output.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0; 2]) {
                write!(output, "\\u{unit:04x}").unwrap();
            }
        }
    }
    Ok(Value::from_safe_string(output))
}

fn json_display(value: &Value, indent: Option<usize>, depth: usize) -> MiniResult<String> {
    let quoted = |s: &str| serde_json::to_string(s).expect("string JSON");
    let (open, close, parts) = match value.kind() {
        ValueKind::None => return Ok("null".into()),
        ValueKind::Bool => return Ok(if value.is_true() { "true" } else { "false" }.into()),
        ValueKind::String => return Ok(quoted(value.as_str().unwrap())),
        ValueKind::Number if value.is_integer() => return Ok(value.to_string()),
        ValueKind::Number => {
            let number = f64::try_from(value.clone())?;
            return Ok(if number.is_nan() {
                "NaN".into()
            } else if number == f64::INFINITY {
                "Infinity".into()
            } else if number == f64::NEG_INFINITY {
                "-Infinity".into()
            } else {
                python_float_repr(number)
            });
        }
        ValueKind::Seq => (
            '[',
            ']',
            value
                .try_iter()?
                .map(|v| json_display(&v, indent, depth + 1))
                .collect::<MiniResult<Vec<_>>>()?,
        ),
        ValueKind::Map => {
            let mut keys: Vec<_> = value.try_iter()?.collect();
            let comparable =
                |key: &Value| matches!(key.kind(), ValueKind::Number | ValueKind::Bool);
            if keys.len() > 1
                && !(keys.iter().all(|k| k.kind() == ValueKind::String)
                    || keys.iter().all(comparable))
            {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    "JSON keys cannot be sorted together",
                ));
            }
            let sort_key = |key: &Value| {
                if key.kind() == ValueKind::Bool {
                    Value::from(i64::from(key.is_true()))
                } else {
                    key.clone()
                }
            };
            keys.sort_by_key(sort_key);
            let parts = keys
                .iter()
                .map(|k| {
                    Ok(format!(
                        "{}: {}",
                        quoted(&if k.kind() == ValueKind::String {
                            k.to_string()
                        } else {
                            json_display(k, None, 0)?
                        }),
                        json_display(&value.get_item(k)?, indent, depth + 1)?
                    ))
                })
                .collect::<MiniResult<Vec<_>>>()?;
            ('{', '}', parts)
        }
        _ => {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "value is not JSON serializable",
            ));
        }
    };
    if parts.is_empty() {
        return Ok(format!("{open}{close}"));
    }
    Ok(if let Some(width) = indent {
        let padding = " ".repeat(width * (depth + 1));
        format!(
            "{open}\n{padding}{}\n{}{close}",
            parts.join(&format!(",\n{padding}")),
            " ".repeat(width * depth)
        )
    } else {
        format!("{open}{}{close}", parts.join(", "))
    })
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
        ValueKind::Number if !value.is_integer() => {
            python_float_repr(f64::try_from(value.clone()).unwrap())
        }
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

pub(crate) fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if !value.is_finite() {
        return value.to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if !(-4..16).contains(&exponent) {
        format!("{mantissa}e{exponent:+03}")
    } else {
        let mut result = value.to_string();
        if !result.contains('.') {
            result.push_str(".0");
        }
        result
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
    super::json::scan_literal(text)?;
    let expr = ast::Expr::parse(text.trim_start_matches([' ', '\t']), "<when>")
        .map_err(|e| Error::new("SyntaxError", e.to_string()))?;
    let value = literal(&expr)?;
    validate_literal(&value)?;
    Ok(value)
}

fn validate_literal(value: &LiteralValue) -> Result<()> {
    match value {
        LiteralValue::Float(n) if !n.is_finite() => return Err(super::json::nonfinite_error()),
        LiteralValue::Complex { real, imag } if !real.is_finite() || !imag.is_finite() => {
            return Err(super::json::nonfinite_error());
        }
        LiteralValue::List(v) | LiteralValue::Tuple(v) | LiteralValue::Set(v) => {
            for value in v {
                validate_literal(value)?;
            }
        }
        LiteralValue::Dict(v) => {
            for (key, value) in v {
                validate_literal(key)?;
                validate_literal(value)?;
            }
        }
        _ => {}
    }
    Ok(())
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
    static PATTERN: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new("https?://[^$/]+").expect("constant regex"));
    PATTERN.sub("", one).unwrap() == PATTERN.sub("", other).unwrap()
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
    static PATTERN: LazyLock<::regex::Regex> = LazyLock::new(|| {
        ::regex::Regex::new(r"^\$DATETIME\((.+)\)\((.+)\)(?:\n)?$").expect("constant regex")
    });
    let Some(captures) = PATTERN.captures(value) else {
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

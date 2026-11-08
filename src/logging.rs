//! CLIだけがstderrを所有する。executorからのイベントは実行中に整形する。
use jumeaux::{
    addons::log::{AddonLog, ConversionReason},
    compat::{Error, Result},
    executor::{Event, Progress, Stage},
    http::{HttpEvent, Side},
    models::Status,
    notification::{Level, Log, Verbosity},
};
use serde_json::{Value, json};
use std::io::Write;

#[derive(Clone, Copy)]
pub struct Logger(pub u8);
impl Logger {
    fn enabled(&self, level: Verbosity) -> bool {
        let required = match level {
            Verbosity::Verbose => 1,
            Verbosity::VeryVerbose => 2,
            Verbosity::Debug => 3,
            _ => 0,
        };
        self.0 >= required
    }
    pub fn log(&self, level: Verbosity, message: impl AsRef<str>) {
        if !self.enabled(level) {
            return;
        }
        let message = message.as_ref();
        let text = match level {
            Verbosity::Debug => format!("\x1b[32m[ DEBUG ] {message}\x1b[0m\n"),
            Verbosity::Warning => format!("\x1b[33m[WARNING] {message}\x1b[0m\n"),
            Verbosity::Error => format!("\x1b[31m[ ERROR ] {message}\x1b[0m\n"),
            _ => format!("{message}\n"),
        };
        let _ = std::io::stderr().lock().write_all(text.as_bytes());
    }
    pub fn logs(&self, logs: &[Log]) {
        for log in logs {
            self.log(
                match log.level {
                    Level::Info => Verbosity::Normal,
                    Level::Warning => Verbosity::Warning,
                    Level::Error => Verbosity::Error,
                },
                &log.message,
            );
        }
    }
    pub fn progress(&self, p: &Progress) {
        if !self.enabled(p.level) {
            return;
        }
        for line in progress_lines(p) {
            // urllib3のWARNINGはJumeaux Loggerの色・接頭辞を持たない。
            let level = if matches!(p.event, Event::Http(HttpEvent::Retrying { .. })) {
                Verbosity::Normal
            } else {
                p.level
            };
            self.log(level, line);
        }
    }
}

fn side(s: Side) -> &'static str {
    match s {
        Side::One => "One",
        Side::Other => "Other",
    }
}
pub fn repr(value: &Value) -> String {
    static ENGINE: std::sync::LazyLock<jumeaux::compat::template::TemplateEngine> =
        std::sync::LazyLock::new(jumeaux::compat::template::TemplateEngine::new);
    ENGINE
        .render("{{ value }}", json!({"value":value}))
        .unwrap_or_else(|_| value.to_string())
}
fn progress_lines(p: &Progress) -> Vec<String> {
    let prefix = format!("[{} / {}]", p.seq, p.total);
    match &p.event {
        Event::Started(r) => vec![
            format!("{prefix} {}", "-".repeat(80)),
            format!(
                "{prefix}  {}. {}",
                p.seq,
                r.name.as_deref().unwrap_or(&r.path)
            ),
            format!("{prefix} {}", "-".repeat(80)),
        ],
        Event::Http(HttpEvent::Url { side: s, url }) => {
            vec![format!("{prefix} {:5} URL:   {url}", side(*s))]
        }
        Event::Http(HttpEvent::Retrying {
            remaining,
            reason,
            path,
        }) => vec![format!(
            "Retrying (Retry(total={remaining}, connect=None, read=None, redirect=None, status=None)) after connection broken by '{}({})': {path}",
            reason.kind,
            {
                let list = repr(&json!([reason.message]));
                list[1..list.len() - 1].to_owned()
            }
        )],
        Event::Http(HttpEvent::Proxy { side: s, host }) => vec![format!(
            "{prefix} {:5} PROXY: {}",
            side(*s),
            host.as_ref()
                .map(|h| repr(
                    &json!({"http":format!("http://{h}"),"https":format!("https://{h}")})
                ))
                .unwrap_or_else(|| "None".into())
        )],
        Event::Http(HttpEvent::RequestDetails(r)) => {
            let mut lines = vec![];
            if !r.headers.is_empty() {
                lines.push(format!(
                    "{prefix} Additional headers:   {}",
                    repr(&json!(r.headers))
                ));
            }
            for (key, value) in [
                ("raw", json!(r.raw)),
                ("form", json!(r.form)),
                ("json", json!(r.json)),
            ] {
                if !value.is_null() {
                    lines.push(format!(
                        "{prefix} {key}:   {}",
                        match &value {
                            Value::String(s) => s.clone(),
                            v => repr(v),
                        }
                    ));
                }
            }
            lines
        }
        Event::Http(HttpEvent::RequestHeaders { side: s, headers }) => vec![format!(
            "{:5} Request headers: {}",
            side(*s),
            repr(&json!(headers))
        )],
        Event::Http(HttpEvent::Response {
            side: s,
            status_code,
            response_sec,
            bytes,
            content_type,
        }) => vec![format!(
            "{prefix} {:6} {status_code} / {response_sec:?}s / {bytes}b / {}",
            format!("{}:", side(*s)),
            content_type.as_deref().unwrap_or("None")
        )],
        Event::StageFinished { stage, elapsed } => {
            let label = match stage {
                Stage::Res2Res(s) => format!("{}   res2res", side(*s)),
                Stage::Res2Dict(s) => format!("{}   res2dict", side(*s)),
                Stage::DiffDiagnosis => "Diff diagnosis".into(),
                Stage::Judgement => "Judgement".into(),
                Stage::StoreCriterion => "Store criterion".into(),
                Stage::DidChallenge => "Did challenge".into(),
                Stage::Dump(_) => return vec![],
            };
            vec![format!(
                "{prefix} ⏰ {label}:   {:?}ms",
                elapsed.as_micros() as f64 / 1000.0
            )]
        }
        Event::Judged(r) => vec![format!(
            "{prefix} {} ({} - {}) <{:?}s - {:?}s> {{HttpMethod.{}}} {}",
            if r.status == Status::Same { "O" } else { "X" },
            r.one.status_code.unwrap(),
            r.other.status_code.unwrap(),
            r.one.response_sec.unwrap(),
            r.other.response_sec.unwrap(),
            match r.method {
                jumeaux::models::HttpMethod::Get => "GET",
                jumeaux::models::HttpMethod::Post => "POST",
            },
            r.name.as_deref().unwrap_or(&r.path)
        )],
        Event::ConnectionFailed(r) => vec![format!(
            "{prefix} 💀 {}",
            r.name.as_deref().unwrap_or("None")
        )],
        Event::Addon { log, .. } => addon_lines(log),
        _ => vec![],
    }
}
fn addon_lines(log: &AddonLog) -> Vec<String> {
    match log {
        AddonLog::Stored { expression } => {
            vec![format!("[store_criterion/free] Stored for `{expression}`.")]
        }
        AddonLog::RegardAsSame { expression } => vec![format!(
            "[judgement/same] Regard as same by `{expression}`."
        )],
        AddonLog::NoMatchingTags => {
            vec!["[did_challenge/tag] There are no matched conditions".into()]
        }
        AddonLog::Sleep { seconds } => vec![format!("Sleep:  {seconds:.2} sec")],
        AddonLog::JsonSortSkipped => {
            vec!["[res2res/json_sort] Skipped because this response is not json.".into()]
        }
        AddonLog::CognitionDiffs(diffs) => vec![
            "[judgement/ignore] ----- [START] diffs by cognition".into(),
            jumeaux::output::json_text(&json!(diffs), Some(4)).unwrap_or_default(),
            "[judgement/ignore] ----- [END] diffs by cognition".into(),
        ],
        AddonLog::XmlParsed { success } => vec![
            if *success {
                "Success to parsing xml"
            } else {
                "Fail to parsing xml"
            }
            .into(),
        ],
        AddonLog::Conversion { format, reason } => vec![format!(
            "[res2dict/{format}] {}",
            match reason {
                ConversionReason::AlreadyExists =>
                    "Skipped because result is already existed.".into(),
                ConversionReason::Forced => format!("Force to convert to dict as {format}"),
                ConversionReason::ResponseType =>
                    format!("Convert to dict as {format} because this response is {format}."),
                ConversionReason::DifferentType =>
                    format!("Skipped because this response is not {format}."),
                ConversionReason::Block => "Convert to dict as block".into(),
            }
        )],
    }
}

/// JSON Numberを直接出力し、arbitrary_precisionの内部mapをYAMLへ漏らさない。
pub fn config_yaml(config: &jumeaux::config::Config) -> Result<String> {
    fn scalar(v: &Value) -> Result<String> {
        match v {
            Value::String(s) if s.contains(['\n', '\r']) => {
                Ok(serde_json::to_string(s).expect("string JSON"))
            }
            Value::String(s) => yaml_string(s),
            _ => Ok(v.to_string()),
        }
    }
    fn block(v: &Value) -> bool {
        match v {
            Value::Object(m) => !m.is_empty(),
            Value::Array(a) => !a.is_empty(),
            _ => false,
        }
    }
    fn emit(v: &Value, indent: usize, out: &mut String) -> Result<()> {
        let pad = " ".repeat(indent);
        match v {
            Value::Object(m) => {
                let mut keys: Vec<_> = m.keys().collect();
                keys.sort();
                for key in keys {
                    let item = &m[key];
                    out.push_str(&format!("{pad}{}:", scalar(&json!(key))?));
                    if block(item) {
                        out.push('\n');
                        emit(item, indent + 2, out)?;
                    } else {
                        out.push_str(&format!(" {}\n", scalar(item)?));
                    }
                }
            }
            Value::Array(a) => {
                for item in a {
                    if block(item) {
                        let mut nested = String::new();
                        emit(item, indent + 2, &mut nested)?;
                        out.push_str(&format!("{pad}- {}", nested.trim_start()));
                    } else {
                        out.push_str(&format!("{pad}- {}\n", scalar(item)?));
                    }
                }
            }
            _ => out.push_str(&format!("{pad}{}\n", scalar(v)?)),
        }
        Ok(())
    }
    let value =
        serde_json::to_value(config).map_err(|e| Error::new("ValueError", e.to_string()))?;
    let mut out = String::new();
    emit(&value, 0, &mut out)?;
    Ok(out)
}

fn yaml_string(s: &str) -> Result<String> {
    // PyYAML resolver.pyのYAML 1.1 implicit resolvers。日付は範囲検証ではなく構文で判定する。
    static IMPLICIT: std::sync::LazyLock<regex::RegexSet> = std::sync::LazyLock::new(|| {
        regex::RegexSet::new([
            r"\A(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)\z",
            r"\A(?:~|null|Null|NULL|)\z",
            r"\A(?:[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))\z",
            r"\A(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)\z",
            r"\A(?:[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?)\z",
            r"\A(?:<<|=)\z",
        ]).expect("PyYAML implicit resolvers")
    });
    if IMPLICIT.is_match(s) {
        return Ok(format!("'{}'", s.replace('\'', "''")));
    }
    // serde_yamlの構文上のクォートは使うが、YAML 1.2だけが数値とみなす文字列は外す。
    if (s.parse::<f64>().is_ok()
        || matches!(
            serde_yaml_ng::from_str::<Value>(s),
            Ok(Value::Number(_) | Value::Bool(_) | Value::Null)
        ))
        && !s.contains(char::is_whitespace)
        && !s.starts_with(['!', '&', '*', '\'', '"'])
    {
        return Ok(s.into());
    }
    serde_yaml_ng::to_string(s)
        .map(|s| s.trim_end_matches('\n').to_string())
        .map_err(|e| Error::new("ValueError", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_strings_match_pyyaml_implicit_resolution() {
        // Python基準環境のyaml.safe_dumpで観測。キーと値で共通のスカラー規則。
        for s in [
            "yes",
            "NO",
            "2001-01-01",
            "1.5e-7",
            "00",
            "0b10",
            "0x10",
            "1_000",
            "1:20",
            "12:00:00",
            ".5",
            "+12",
            "~",
            "null",
            "Null",
            "NULL",
            "true",
            "True",
            "TRUE",
            "on",
            "Off",
            "OFF",
            "1.0",
            ".inf",
            "-.Inf",
            ".NaN",
            "2001-1-1T1:02:03Z",
            "2001-13-40",
            "2001-01-01t01:02:03.1+01:00",
            "<<",
            "=",
        ] {
            assert_eq!(yaml_string(s).unwrap(), format!("'{s}'"), "{s}");
        }
        for s in [
            "1e3",
            "1.5e7",
            "0o77",
            "08",
            "01:20",
            "-.5",
            "nUlL",
            "TrUe",
            "Y",
            "N",
            "2001-1-1",
            "a:b",
            "a,b",
            "日本語",
        ] {
            assert_eq!(yaml_string(s).unwrap(), s, "{s}");
        }
        for s in ["a: b", "a # b", " leading", "trailing ", "[x]"] {
            assert_eq!(yaml_string(s).unwrap(), format!("'{s}'"), "{s}");
        }
    }

    #[test]
    fn stage_elapsed_truncates_to_microseconds_and_keeps_float_notation() {
        for (nanos, expected) in [
            (0, "0.0"),
            (999, "0.0"),
            (57_999, "0.057"),
            (1_000_999, "1.0"),
            (12_345_678, "12.345"),
        ] {
            let progress = Progress {
                seq: 1,
                total: 2,
                level: Verbosity::VeryVerbose,
                event: Event::StageFinished {
                    stage: Stage::Judgement,
                    elapsed: std::time::Duration::from_nanos(nanos),
                },
            };
            assert_eq!(
                progress_lines(&progress),
                [format!("[1 / 2] ⏰ Judgement:   {expected}ms")]
            );
        }
    }
}

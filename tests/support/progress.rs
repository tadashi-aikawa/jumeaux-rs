//! CLI実装ではない。保存済みstderrの各trial行をイベントの素の値だけで復元できることを固定する。
use jumeaux::{
    addons::log::{AddonLog, ConversionReason},
    executor::{Event, Progress, Stage},
    http::{HttpEvent, Side},
    models::{HttpMethod, Status},
    notification::Verbosity,
};

fn side(side: Side) -> &'static str {
    match side {
        Side::One => "One",
        Side::Other => "Other",
    }
}
fn lines(progress: &Progress) -> Vec<String> {
    let prefix = format!("[{} / {}]", progress.seq, progress.total);
    match &progress.event {
        Event::Started(request) => vec![
            format!("{prefix} {}", "-".repeat(80)),
            format!(
                "{prefix}  {}. {}",
                progress.seq,
                request.name.as_deref().unwrap_or(&request.path)
            ),
            format!("{prefix} {}", "-".repeat(80)),
        ],
        Event::Http(HttpEvent::Url {
            side: endpoint,
            url,
        }) => vec![format!("{prefix} {:5} URL:   {url}", side(*endpoint))],
        Event::Http(HttpEvent::Proxy {
            side: endpoint,
            host,
        }) => {
            assert!(host.is_none(), "verbose baseline has no explicit proxy");
            vec![format!("{prefix} {:5} PROXY: None", side(*endpoint))]
        }
        Event::Http(HttpEvent::RequestDetails(request)) => {
            assert!(request.headers.is_empty());
            assert!(request.raw.is_none() && request.form.is_none() && request.json.is_none());
            vec![]
        }
        Event::Http(HttpEvent::RequestHeaders {
            side: endpoint,
            headers,
        }) => {
            let headers = headers
                .iter()
                .map(|(key, value)| format!("'{key}': '{value}'"))
                .collect::<Vec<_>>()
                .join(", ");
            vec![format!(
                "{:5} Request headers: {{{headers}}}",
                side(*endpoint)
            )]
        }
        Event::Http(HttpEvent::Response {
            side: endpoint,
            status_code,
            response_sec,
            bytes,
            content_type,
        }) => {
            assert!(response_sec.is_finite() && *response_sec >= 0.0);
            vec![format!(
                "{prefix} {:6} {status_code} / {response_sec:?}s / {bytes}b / {}",
                format!("{}:", side(*endpoint)),
                content_type.as_deref().unwrap_or("None")
            )]
        }
        Event::StageFinished { stage, elapsed } => {
            assert!(!elapsed.is_zero());
            let label = match stage {
                Stage::Res2Res(endpoint) => format!("{}   res2res", side(*endpoint)),
                Stage::Res2Dict(endpoint) => format!("{}   res2dict", side(*endpoint)),
                Stage::DiffDiagnosis => "Diff diagnosis".into(),
                Stage::Judgement => "Judgement".into(),
                Stage::StoreCriterion => "Store criterion".into(),
                Stage::DidChallenge => "Did challenge".into(),
                Stage::Dump(_) => panic!("Python does not time dump separately"),
            };
            vec![format!(
                "{prefix} ⏰ {label}:   {}ms",
                elapsed.as_secs_f64() * 1000.0
            )]
        }
        Event::Judged(result) => {
            let symbol = if result.status == Status::Same {
                "O"
            } else {
                "X"
            };
            let method = match result.method {
                HttpMethod::Get => "GET",
                HttpMethod::Post => "POST",
            };
            vec![format!(
                "{prefix} {symbol} ({} - {}) <{:?}s - {:?}s> {{HttpMethod.{method}}} {}",
                result.one.status_code.unwrap(),
                result.other.status_code.unwrap(),
                result.one.response_sec.unwrap(),
                result.other.response_sec.unwrap(),
                result.name.as_deref().unwrap_or(&result.path)
            )]
        }
        Event::Addon {
            stage,
            index,
            name,
            log,
        } => {
            assert_eq!(*index, 0);
            match log {
                AddonLog::Stored { expression } => {
                    assert_eq!(*stage, Stage::StoreCriterion);
                    assert_eq!(name, "free");
                    vec![format!("[store_criterion/free] Stored for `{expression}`.")]
                }
                AddonLog::Conversion {
                    format,
                    reason: ConversionReason::ResponseType,
                } => {
                    assert!(matches!(stage, Stage::Res2Dict(_)));
                    assert_eq!(name, format);
                    vec![format!(
                        "[res2dict/{format}] Convert to dict as {format} because this response is {format}."
                    )]
                }
                _ => panic!("unexpected baseline addon: {log:?}"),
            }
        }
        Event::Saved { .. } | Event::StorageDecision { .. } => vec![],
        other => panic!("unexpected baseline event: {other:?}"),
    }
}

pub fn assert_verbose_materials(events: &[Progress], stderr: &str, verbosity: u8, port: u16) {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let prefix = regex::Regex::new(r"^\[\d+ / \d+\]").unwrap();
    let time = regex::Regex::new(r"\d+(?:\.\d+)?(?:ms|s)\b").unwrap();
    let version = regex::Regex::new(r"jumeaux/[0-9.]+").unwrap();
    let normalize = |line: &str| {
        let line = line.replace(&format!("localhost:{port}"), "localhost:8000");
        let line = time.replace_all(&line, "<time>");
        version.replace_all(&line, "jumeaux/<version>").into_owned()
    };
    let expected: Vec<_> = stderr
        .lines()
        .filter_map(|line| {
            let line = ansi.replace_all(line, "");
            let (debug, line) = line
                .strip_prefix("[ DEBUG ] ")
                .map_or((false, line.as_ref()), |s| (true, s));
            if !(prefix.is_match(line)
                || line.starts_with("[store_criterion/")
                || line.starts_with("[res2dict/")
                || line.starts_with("One   Request headers:")
                || line.starts_with("Other Request headers:"))
            {
                return None;
            }
            let level = if debug {
                Verbosity::Debug
            } else if line.contains("] O (") {
                Verbosity::Verbose
            } else if line.contains("] X (") {
                Verbosity::Normal
            } else {
                Verbosity::VeryVerbose
            };
            Some((level, normalize(line)))
        })
        .collect();
    let actual: Vec<_> = events
        .iter()
        .filter(|event| verbosity == 3 || event.level != Verbosity::Debug)
        .flat_map(|event| {
            lines(event)
                .into_iter()
                .map(move |line| (event.level, line))
        })
        .map(|(level, line)| (level, normalize(&line)))
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(
        actual, expected,
        "verbose_{verbosity}: every application-owned trial line must have matching event data and level"
    );
    println!(
        "verbose_{verbosity}: {}/{} trial log lines have matching event data",
        actual.len(),
        expected.len()
    );
}

//! reportのモデル境界だけを1回走査する。任意JSONやヘッダ名は刈り込まない。
//! モデルにフィールドを足したときは対応する表も更新する。
use serde_json::Value;

enum Shape {
    Object(&'static [(&'static str, Shape)]),
    List(&'static Shape),
    Values(&'static Shape),
    Any,
    Obsolete,
}
use Shape::*;

const ACCESS_POINT: Shape = Object(&[
    ("name", Any),
    ("host", Any),
    ("path", Object(&[("before", Any), ("after", Any)])),
    ("query", Object(&[("overwrite", Any), ("remove", Any)])),
    ("proxy", Any),
    ("default_response_encoding", Any),
    ("headers", Any),
]);
const NOTIFIER: Shape = Object(&[
    ("type", Any),
    ("version", Any),
    ("channel", Any),
    ("username", Any),
    ("icon_emoji", Any),
    ("icon_url", Any),
    ("use_blocks", Any),
]);
const ADDON: Shape = Object(&[
    ("name", Any),
    ("cls_name", Any),
    ("config", Any),
    ("include", Any),
    ("tags", Any),
]);
const ADDONS: Shape = Object(&[
    ("log2reqs", ADDON),
    ("reqs2reqs", List(&ADDON)),
    ("res2res", List(&ADDON)),
    ("res2dict", List(&ADDON)),
    ("judgement", List(&ADDON)),
    ("store_criterion", List(&ADDON)),
    ("dump", List(&ADDON)),
    ("did_challenge", List(&ADDON)),
    ("final", List(&ADDON)),
]);
const RESPONSE: Shape = Object(&[
    ("url", Any),
    ("type", Any),
    ("status_code", Any),
    ("byte", Any),
    ("response_sec", Any),
    ("content_type", Any),
    ("mime_type", Any),
    ("encoding", Any),
    ("file", Any),
    ("prop_file", Any),
    ("response_header", Any),
]);
const DIFF: Shape = Object(&[("added", Any), ("changed", Any), ("removed", Any)]);
const TRIAL: Shape = Object(&[
    ("seq", Any),
    ("name", Any),
    ("tags", Any),
    ("headers", Any),
    ("queries", Any),
    ("raw", Any),
    ("form", Any),
    ("json", Any),
    ("one", RESPONSE),
    ("other", RESPONSE),
    ("method", Any),
    ("path", Any),
    ("request_time", Any),
    ("status", Any),
    ("diffs_by_cognition", Values(&DIFF)),
]);
const SUMMARY: Shape = Object(&[
    ("one", ACCESS_POINT),
    ("other", ACCESS_POINT),
    (
        "status",
        Object(&[("same", Any), ("different", Any), ("failure", Any)]),
    ),
    ("tags", Any),
    (
        "time",
        Object(&[("start", Any), ("end", Any), ("elapsed_sec", Any)]),
    ),
    (
        "concurrency",
        Object(&[("threads", Any), ("processes", Any)]),
    ),
    (
        "output",
        Object(&[
            ("response_dir", Any),
            ("encoding", Any),
            ("logger", Obsolete),
        ]),
    ),
    (
        "comparison",
        Object(&[
            ("judge_response_header", Any),
            ("ignore_response_header_keys", Any),
        ]),
    ),
    ("default_encoding", Any),
]);
const REPORT: Shape = Object(&[
    ("version", Any),
    ("key", Any),
    ("title", Any),
    ("description", Any),
    ("notifiers", Values(&NOTIFIER)),
    ("summary", SUMMARY),
    ("trials", List(&TRIAL)),
    ("addons", ADDONS),
    ("retry_hash", Any),
]);

pub(super) fn prune(value: &mut Value) -> Vec<String> {
    let mut warnings = Vec::new();
    visit(value, &REPORT, "report", &mut warnings);
    warnings
}

fn visit(value: &mut Value, shape: &Shape, path: &str, warnings: &mut Vec<String>) {
    match (shape, value) {
        (Object(fields), Value::Object(map)) => {
            map.retain(|key, value| {
                let child_path = format!("{path}.{key}");
                match fields.iter().find(|(name, _)| name == key) {
                    Some((_, Obsolete)) => {
                        if !value.is_null() {
                            warnings.push(format!("{child_path}: ignored obsolete setting"));
                        }
                        false
                    }
                    Some((_, shape)) => {
                        visit(value, shape, &child_path, warnings);
                        true
                    }
                    None => {
                        warnings.push(format!("{child_path}: ignored unknown key"));
                        false
                    }
                }
            });
        }
        (List(shape), Value::Array(values)) => {
            for (index, value) in values.iter_mut().enumerate() {
                visit(value, shape, &format!("{path}[{index}]"), warnings);
            }
        }
        (Values(shape), Value::Object(map)) => {
            for (key, value) in map {
                visit(value, shape, &format!("{path}.{key}"), warnings);
            }
        }
        // 型の不一致は変更しない。後続の厳格なデシリアライズでエラーにする。
        _ => {}
    }
}

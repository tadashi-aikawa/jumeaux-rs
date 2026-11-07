//! 組み込み設定のキー・型を検証する。任意JSON本文のキーは検査しない。
use super::{Layer, validate_registration};
use crate::{compat::Result, config::invalid, models::Addon};
use serde_json::Value;

#[derive(Clone, Copy)]
enum Rule {
    String,
    Bool,
    Integer,
    Number,
    Strings,
    Mapping,
    Object(&'static str),
    List(&'static str),
}
use Rule::*;

fn fields(schema: &str) -> &'static [(&'static str, Rule)] {
    match schema {
        "log2reqs/plain" => &[
            ("encoding", String),
            ("keep_blank", Bool),
            ("candidate_for_url_encodings", Strings),
        ],
        "log2reqs/csv" => &[
            ("encoding", String),
            ("keep_blank", Bool),
            ("dialect", String),
        ],
        "log2reqs/json" | "log2reqs/yaml" | "dump/encoding" => &[("encoding", String)],
        "reqs2reqs/add" => &[("location", String), ("reqs", List("request"))],
        "request" => &[
            ("name", String),
            ("method", String),
            ("path", String),
            ("qs", Mapping),
            ("raw", String),
            ("form", Mapping),
            ("json", Mapping),
            ("headers", Mapping),
            ("url_encoding", String),
        ],
        "reqs2reqs/empty_guard" => &[("notifies", List("empty_notify"))],
        "empty_notify" => &[("notifier", String), ("message", String)],
        "reqs2reqs/filter" => &[("when", String)],
        "reqs2reqs/head" => &[("size", Integer)],
        "reqs2reqs/repeat" => &[("times", Integer)],
        "reqs2reqs/rename" => &[("conditions", List("rename"))],
        "rename" => &[("name", String), ("when", String)],
        "reqs2reqs/replace" => &[("items", List("replace"))],
        "replace" => &[("when", String), ("queries", Mapping), ("headers", Mapping)],
        "res2res/json" => &[
            ("transformer", Object("transformer")),
            ("default_encoding", String),
            ("when", String),
        ],
        "transformer" => &[("module", String), ("function", String)],
        "res2res/json_sort" => &[("items", List("sorter")), ("footprints_tag", String)],
        "sorter" => &[("when", String), ("targets", List("target"))],
        "target" => &[("path", String), ("sort_keys", Strings)],
        "res2res/type" => &[("conditions", List("type"))],
        "type" => &[("type", String), ("when", String)],
        "res2dict/block" => &[
            ("force", Bool),
            ("header_regexp", String),
            ("record_regexp", String),
        ],
        "res2dict/json" | "res2dict/html" | "res2dict/xml" => &[("force", Bool)],
        "judgement/same" | "store_criterion/free" => &[("when_any", Strings)],
        "judgement/ignore" => &[("ignores", List("ignore"))],
        "ignore" => &[("title", String), ("conditions", List("ignore_condition"))],
        "ignore_condition" => &[
            ("when", String),
            ("added", List("case")),
            ("changed", List("case")),
            ("removed", List("case")),
        ],
        "case" => &[("path", String), ("when", String)],
        "dump/json" | "dump/html" | "dump/xml" => &[("default_encoding", String), ("force", Bool)],
        "did_challenge/sleep" => &[("min", Number), ("max", Number)],
        "did_challenge/tag" => &[("conditions", List("tag"))],
        "tag" => &[("tag", String), ("when", String)],
        "final/csv" => &[
            ("column_names", Strings),
            ("output_path", String),
            ("with_header", Bool),
        ],
        "final/json" => &[("sysout", Bool), ("indent", Integer)],
        "final/summary" => &[("sysout", Bool)],
        "final/notify" => &[("notifies", List("notify"))],
        "notify" => &[("notifier", String), ("message", String), ("when", String)],
        "final/miroir" => &[
            ("table", String),
            ("bucket", String),
            ("prefix", String),
            ("cache_max_age", Integer),
            ("with_zip", Bool),
            ("assumed_role_arn", String),
            ("checklist", String),
            ("local_stack", Object("local_stack")),
            ("endpoints", Object("endpoints")),
            ("when", Strings),
        ],
        "local_stack" => &[("use", Bool), ("endpoint", String)],
        "endpoints" => &[("dynamodb", String), ("s3", String)],
        "reqs2reqs/shuffle" | "final/viewer" => &[],
        _ => unreachable!("registered schema: {schema}"),
    }
}

fn validate(value: &Value, schema: &str, path: &str) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "expected mapping"))?;
    let fields = fields(schema);
    for (key, value) in object {
        let p = format!("{path}.{key}");
        let rule = fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, rule)| rule)
            .ok_or_else(|| invalid(&p, "unknown key"))?;
        // nullの既定値・必須項目は各アドオン本体で扱う。ここでは形とキーを検証する。
        if value.is_null() {
            continue;
        }
        let valid = match rule {
            String => value.is_string(),
            Bool => value.is_boolean(),
            Integer => value.as_i64().is_some(),
            Number => value.is_number(),
            Strings => value
                .as_array()
                .is_some_and(|v| v.iter().all(Value::is_string)),
            Mapping => value.is_object(),
            Object(child) => {
                validate(value, child, &p)?;
                true
            }
            List(child) => {
                let list = value
                    .as_array()
                    .ok_or_else(|| invalid(&p, "expected list"))?;
                for (i, v) in list.iter().enumerate() {
                    validate(v, child, &format!("{p}[{i}]"))?;
                }
                true
            }
        };
        if !valid {
            return Err(invalid(&p, "invalid value type"));
        }
    }
    Ok(())
}

pub fn validate_addon(layer: Layer, addon: &Addon, path: &str) -> Result<()> {
    validate_registration(layer, addon, path)?;
    if let Some(config) = &addon.config {
        validate(
            &Value::Object(config.clone()),
            &format!("{}/{}", layer.name(), addon.name),
            &format!("{path}.config"),
        )?;
        if layer == Layer::Res2Res && addon.name == "json" && config.contains_key("transformer") {
            return Err(invalid(
                &format!("{path}.config.transformer"),
                "Python transformer is unsupported; external command contract is undecided",
            ));
        }
    }
    Ok(())
}

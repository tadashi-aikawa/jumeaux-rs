//! YAMLの読み込み、レイヤー単位のマージ、CLI上書き。
use crate::{
    addons::{Layer, validate_addon},
    cli::{ReportArgs, RunArgs},
    compat::{Error, Result},
    models::*,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub one: AccessPoint,
    pub other: AccessPoint,
    pub output: OutputSummary,
    #[serde(default = "crate::models::one")]
    pub threads: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<i64>,
    #[serde(default = "max_retries")]
    pub max_retries: i64,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifiers: Option<IndexMap<String, Notifier>>,
    pub addons: Addons,
    #[serde(default)]
    pub judge_response_header: bool,
    #[serde(default = "ignored_headers")]
    pub ignore_response_header_keys: Vec<String>,
}
fn max_retries() -> i64 {
    3
}
fn ignored_headers() -> Vec<String> {
    vec!["Content-Length".into(), "Date".into()]
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MergedArgs {
    pub files: Option<Vec<String>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub tag: Option<Vec<String>>,
    pub threads: Option<i64>,
    pub processes: Option<i64>,
    pub max_retries: Option<i64>,
}

/// 保存場所を決める型ではない。retry呼出元が元設定などから明示的に供給する。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderJudgementSettings {
    pub judge_response_header: bool,
    pub ignore_response_header_keys: Vec<String>,
}
impl MergedArgs {
    pub fn from_cli(args: &ReportArgs) -> Self {
        Self {
            title: args.title.clone(),
            description: args.description.clone(),
            threads: args.threads,
            processes: args.processes,
            max_retries: args.max_retries,
            tag: (!args.tag.is_empty()).then(|| args.tag.clone()),
            files: None,
        }
    }
    /// Pythonのretryコマンドはタグ未指定でもSome([])を渡す。
    pub fn from_retry_args(args: &ReportArgs) -> Self {
        Self {
            tag: Some(args.tag.clone()),
            ..Self::from_cli(args)
        }
    }
}

impl Config {
    /// 旧reportだけではヘッダ判定設定を復元できないため、必須引数にする。
    pub fn from_report(report: &Report, headers: HeaderJudgementSettings) -> Result<Self> {
        validate_output(&report.summary.output)?;
        let addons = report
            .addons
            .clone()
            .ok_or_else(|| invalid("report.addons", "required for retry"))?;
        for layer in Layer::ALL {
            for (index, addon) in layer.configured(&addons).iter().enumerate() {
                validate_addon(
                    layer,
                    addon,
                    &format!("report.addons.{}[{index}]", layer.name()),
                )?;
            }
        }
        Ok(Self {
            one: report.summary.one.clone(),
            other: report.summary.other.clone(),
            output: report.summary.output.clone(),
            threads: 1,
            processes: None,
            max_retries: max_retries(),
            title: Some(report.title.clone()),
            description: report.description.clone(),
            tags: None,
            input_files: None,
            notifiers: report.notifiers.clone(),
            addons,
            judge_response_header: headers.judge_response_header,
            ignore_response_header_keys: headers.ignore_response_header_keys,
        })
    }

    /// CLIのtruthy判定は項目ごとに異なる。threads/max_retriesは0も上書きする。
    pub fn merge_args(&mut self, args: &MergedArgs) {
        if let Some(v) = args.threads {
            self.threads = v;
        }
        if let Some(v) = args.processes.filter(|v| *v != 0) {
            self.processes = Some(v);
        }
        if let Some(v) = args.max_retries {
            self.max_retries = v;
        }
        if let Some(v) = args.title.as_ref().filter(|v| !v.is_empty()) {
            self.title = Some(v.clone());
        }
        if let Some(v) = args.description.as_ref().filter(|v| !v.is_empty()) {
            self.description = Some(v.clone());
        }
        if let Some(tags) = &args.tag {
            self.tags = Some(tags.clone());
        }
        if let Some(files) = &args.files {
            self.input_files = Some(files.clone());
        }
    }

    pub fn concurrency(&self) -> Result<Concurrency> {
        if let Some(processes) = self.processes.filter(|v| *v != 0) {
            if processes < 1 {
                return Err(invalid("processes", "must be positive"));
            }
            Ok(Concurrency {
                threads: 1,
                processes,
            })
        } else {
            if self.threads < 1 {
                return Err(invalid("threads", "must be positive"));
            }
            Ok(Concurrency {
                threads: self.threads,
                processes: 1,
            })
        }
    }

    pub fn from_run_args(args: &RunArgs) -> Result<Self> {
        let paths = if args.config.is_empty() {
            vec!["config.yml".into()]
        } else {
            args.config.clone()
        };
        let mut config = load(&paths, &args.skip_addon_tag)?;
        config.merge_args(&MergedArgs {
            files: Some(args.files.clone()),
            ..MergedArgs::from_cli(&args.options)
        });
        config.concurrency()?;
        Ok(config)
    }
}

pub(crate) fn invalid(path: &str, message: impl std::fmt::Display) -> Error {
    Error::new("ConfigError", format!("{path}: {message}"))
}

pub(crate) fn check_keys<'a>(
    value: &'a Value,
    keys: &[&str],
    path: &str,
) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid(path, "expected mapping"))?;
    for key in object.keys() {
        if !keys.contains(&key.as_str()) {
            return Err(invalid(&format!("{path}.{key}"), "unknown key"));
        }
    }
    Ok(object)
}

fn read_yaml(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|e| invalid(&path.display().to_string(), e))?;
    // YAMLから直接JSON Valueへ読む。JSON ValueをYAML serializerへ渡すと
    // arbitrary_precisionの内部map表現が漏れるため、往復させない。
    serde_yaml_ng::from_str(&text).map_err(|e| invalid(&path.display().to_string(), e))
}

fn validate_top(value: &Value, path: &str) -> Result<()> {
    let object = check_keys(
        value,
        &[
            "one",
            "other",
            "output",
            "threads",
            "processes",
            "max_retries",
            "title",
            "description",
            "tags",
            "input_files",
            "notifiers",
            "addons",
            "judge_response_header",
            "ignore_response_header_keys",
        ],
        path,
    )?;
    for endpoint in ["one", "other"] {
        if let Some(v) = object.get(endpoint) {
            let p = format!("{path}.{endpoint}");
            let e = check_keys(
                v,
                &[
                    "name",
                    "host",
                    "path",
                    "query",
                    "proxy",
                    "default_response_encoding",
                    "headers",
                ],
                &p,
            )?;
            for (key, keys) in [
                ("path", &["before", "after"][..]),
                ("query", &["overwrite", "remove"][..]),
            ] {
                if let Some(v) = e.get(key).filter(|v| !v.is_null()) {
                    check_keys(v, keys, &format!("{p}.{key}"))?;
                }
            }
        }
    }
    if let Some(v) = object.get("output") {
        check_keys(
            v,
            &["response_dir", "encoding", "logger"],
            &format!("{path}.output"),
        )?;
    }
    if let Some(v) = object.get("notifiers").filter(|v| !v.is_null()) {
        let notifiers = v
            .as_object()
            .ok_or_else(|| invalid("notifiers", "expected mapping"))?;
        for (name, notifier) in notifiers {
            check_keys(
                notifier,
                &[
                    "type",
                    "version",
                    "channel",
                    "username",
                    "icon_emoji",
                    "icon_url",
                    "use_blocks",
                ],
                &format!("{path}.notifiers.{name}"),
            )?;
        }
    }
    Ok(())
}

/// Python同様、タグ判定はinclude元だけを見る。includeは1段で全体を置き換える。
fn resolve_addon(value: &Value, file: &Path, path: &str, layer: Layer) -> Result<Value> {
    let object = check_keys(
        value,
        &["name", "cls_name", "config", "include", "tags"],
        path,
    )?;
    let resolved = if let Some(include) = object.get("include") {
        let include = include
            .as_str()
            .ok_or_else(|| invalid(&format!("{path}.include"), "expected path string"))?;
        read_yaml(&file.parent().unwrap_or(Path::new(".")).join(include))
            .map_err(|e| invalid(&format!("{path}.include"), e))?
    } else {
        value.clone()
    };
    let addon: Addon = deserialize(&resolved, path)?;
    validate_addon(layer, &addon, path)?;
    Ok(resolved)
}

pub(crate) fn deserialize<T: serde::de::DeserializeOwned>(value: &Value, path: &str) -> Result<T> {
    serde_path_to_error::deserialize(value)
        .map_err(|e| invalid(&format!("{path}.{}", e.path()), e.inner()))
}

pub fn load<P: AsRef<Path>>(paths: &[P], skip_tags: &[String]) -> Result<Config> {
    let mut merged = Map::new();
    let mut merged_addons = Map::new();
    for file in paths {
        let file = file.as_ref();
        let mut document = read_yaml(file)?;
        validate_top(&document, &file.display().to_string())?;
        let object = document.as_object_mut().expect("validated mapping");
        if let Some(addons) = object.remove("addons") {
            let layers = check_keys(&addons, &Layer::NAMES, "addons")?;
            for (name, value) in layers {
                let layer = Layer::from_name(name).expect("validated layer");
                let path = format!("addons.{name}");
                if layer == Layer::Log2Reqs {
                    if !value.is_null() {
                        merged_addons
                            .insert(name.clone(), resolve_addon(value, file, &path, layer)?);
                    }
                } else {
                    let list = value
                        .as_array()
                        .ok_or_else(|| invalid(&path, "expected list"))?;
                    let mut resolved = Vec::new();
                    for (i, value) in list.iter().enumerate() {
                        let p = format!("{path}[{i}]");
                        let object = check_keys(
                            value,
                            &["name", "cls_name", "config", "include", "tags"],
                            &p,
                        )?;
                        let tags = object
                            .get("tags")
                            .map(|v| deserialize::<Vec<String>>(v, &format!("{p}.tags")))
                            .transpose()?
                            .unwrap_or_default();
                        if tags.iter().any(|tag| skip_tags.contains(tag)) {
                            continue;
                        }
                        resolved.push(resolve_addon(value, file, &p, layer)?);
                    }
                    // Python create_configのif vに一致。空/全件skipは前レイヤーを消さない。
                    if !resolved.is_empty() {
                        merged_addons.insert(name.clone(), Value::Array(resolved));
                    }
                }
            }
        }
        merged.extend(object.clone());
    }
    merged.insert("addons".into(), Value::Object(merged_addons));
    let config: Config = deserialize(&Value::Object(merged), "config")?;
    validate_output(&config.output)?;
    Ok(config)
}

fn validate_output(output: &OutputSummary) -> Result<()> {
    if output.logger.as_ref().is_some_and(|v| !v.is_null()) {
        return Err(invalid(
            "output.logger",
            "obsolete Python setting; remove this key",
        ));
    }
    crate::compat::encoding::encode(
        "",
        &output.encoding,
        crate::compat::encoding::Errors::Strict,
    )
    .map_err(|e| invalid("output.encoding", e))?;
    Ok(())
}

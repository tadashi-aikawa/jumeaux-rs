//! YAMLの読み込み、レイヤー単位のマージ、CLI上書き。
use crate::models::defaults::{null_default, null_default_with};
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
    #[serde(default = "crate::models::one", deserialize_with = "null_one")]
    pub threads: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<i64>,
    #[serde(default = "max_retries", deserialize_with = "null_max_retries")]
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
    #[serde(default, deserialize_with = "null_default")]
    pub judge_response_header: bool,
    #[serde(default = "ignored_headers", deserialize_with = "null_ignored_headers")]
    pub ignore_response_header_keys: Vec<String>,
}
null_default_with!(null_one, i64, one());
null_default_with!(null_max_retries, i64, max_retries());
null_default_with!(null_ignored_headers, Vec<String>, ignored_headers());

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

/// reportのsummary.comparisonに保存するヘッダ判定設定。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HeaderJudgementSettings {
    #[serde(default, deserialize_with = "null_default")]
    pub judge_response_header: bool,
    #[serde(default = "ignored_headers", deserialize_with = "null_ignored_headers")]
    pub ignore_response_header_keys: Vec<String>,
}
impl Default for HeaderJudgementSettings {
    fn default() -> Self {
        Self {
            judge_response_header: false,
            ignore_response_header_keys: ignored_headers(),
        }
    }
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
    pub fn comparison(&self) -> HeaderJudgementSettings {
        HeaderJudgementSettings {
            judge_response_header: self.judge_response_header,
            ignore_response_header_keys: self.ignore_response_header_keys.clone(),
        }
    }

    /// 警告の表示は呼出元の責務とする。
    pub fn from_report(report: &Report) -> Result<(Self, Vec<String>)> {
        let headers = report.summary.comparison.clone().unwrap_or_default();
        let (config, mut warnings) = Self::from_report_with_headers(report, headers)?;
        if report.summary.comparison.is_none() {
            warnings.insert(0, "report.summary.comparison: missing; using Python defaults (false, Content-Length/Date)".into());
        }
        Ok((config, warnings))
    }

    /// 元configを明示指定する呼出元向け。通常のretryは保存済みcomparisonを使う。
    pub fn from_report_with_headers(
        report: &Report,
        headers: HeaderJudgementSettings,
    ) -> Result<(Self, Vec<String>)> {
        let mut warnings = Vec::new();
        let mut output = report.summary.output.clone();
        if output.logger.take().is_some_and(|value| !value.is_null()) {
            warnings.push("report.summary.output.logger: ignored obsolete setting".into());
        }
        validate_output(&output)?;
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
        Ok((
            Self {
                one: report.summary.one.clone(),
                other: report.summary.other.clone(),
                output,
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
            },
            warnings,
        ))
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

fn read_yaml(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).map_err(|e| invalid(&path.display().to_string(), e))?;
    let mut yaml: Value =
        serde_yaml_ng::from_str(&text).map_err(|e| invalid(&path.display().to_string(), e))?;
    expand_yaml_merges(&mut yaml, &path.display().to_string())?;
    Ok(yaml)
}

/// JSON Valueへの直接読込は重複キーを後勝ちにする。YAMLのマージだけを追加する。
pub(crate) fn expand_yaml_merges(value: &mut Value, path: &str) -> Result<()> {
    match value {
        Value::Object(map) => {
            let merge = map.remove("<<");
            for (key, value) in map.iter_mut() {
                expand_yaml_merges(value, &error_path(path, key))?;
            }
            if let Some(mut merge) = merge {
                let merge_path = error_path(path, "<<");
                expand_yaml_merges(&mut merge, &merge_path)?;
                let sources = match merge {
                    Value::Object(source) => vec![Value::Object(source)],
                    Value::Array(sources) => sources,
                    _ => return Err(invalid(&merge_path, "expected mapping or list of mappings")),
                };
                for source in sources {
                    let Value::Object(source) = source else {
                        return Err(invalid(&merge_path, "expected mapping in merge list"));
                    };
                    for (key, value) in source {
                        // 明示キーが最優先。マージリストは先頭のマッピングを優先する。
                        map.entry(key).or_insert(value);
                    }
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                expand_yaml_merges(value, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// includeだけをファイル単位で解決する。キー・名前・型の検証はマージ後。
fn resolve_addon(value: &Value, file: &Path, path: &str) -> Result<(Value, String)> {
    if let Some(include) = value.get("include").filter(|v| !v.is_null()) {
        let include = include.as_str().ok_or_else(|| {
            invalid(
                &format!("{}.{}.include", file.display(), path),
                "expected path string",
            )
        })?;
        let source = file.parent().unwrap_or(Path::new(".")).join(include);
        Ok((read_yaml(&source)?, source.display().to_string()))
    } else {
        Ok((value.clone(), file.display().to_string()))
    }
}

pub(crate) fn error_path(path: &str, child: &str) -> String {
    match child {
        "" | "." => path.to_string(),
        _ if path.is_empty() => child.to_string(),
        _ if child.starts_with('[') => format!("{path}{child}"),
        _ => format!("{path}.{child}"),
    }
}

pub(crate) fn deserialize<T: serde::de::DeserializeOwned>(value: &Value, path: &str) -> Result<T> {
    serde_path_to_error::deserialize(value)
        .map_err(|e| invalid(&error_path(path, &e.path().to_string()), e.inner()))
}

pub fn load<P: AsRef<Path>>(paths: &[P], skip_tags: &[String]) -> Result<Config> {
    let mut merged = Map::new();
    let mut merged_addons = Map::new();
    let mut sources = IndexMap::<String, String>::new();
    for file in paths {
        let file = file.as_ref();
        let mut document = read_yaml(file)?;
        let object = document
            .as_object_mut()
            .ok_or_else(|| invalid(&file.display().to_string(), "expected mapping"))?;
        if let Some(addons) = object.remove("addons") {
            let layers = addons.as_object().ok_or_else(|| {
                invalid(&format!("{}.addons", file.display()), "expected mapping")
            })?;
            for (name, value) in layers {
                let path = format!("addons.{name}");
                let layer = Layer::from_name(name);
                if layer == Some(Layer::Log2Reqs) {
                    if !value.is_null() {
                        let (resolved, source) = resolve_addon(value, file, &path)?;
                        sources.insert(path, source);
                        merged_addons.insert(name.clone(), resolved);
                    }
                } else if layer.is_some() && value.is_array() {
                    let mut resolved = Vec::new();
                    let mut origins = Vec::new();
                    for value in value.as_array().expect("array") {
                        let p = format!("{path}[{}]", resolved.len());
                        let tags = value
                            .get("tags")
                            .filter(|v| !v.is_null())
                            .map(|v| {
                                deserialize::<Vec<String>>(
                                    v,
                                    &format!("{}.{}.tags", file.display(), p),
                                )
                            })
                            .transpose()?
                            .unwrap_or_default();
                        if tags.iter().any(|tag| skip_tags.contains(tag)) {
                            continue;
                        }
                        let (value, source) = resolve_addon(value, file, &p)?;
                        origins.push((p, source));
                        resolved.push(value);
                    }
                    // Python互換: 空/全件skipは前レイヤーを維持する。
                    if !resolved.is_empty() {
                        sources.retain(|key, _| !key.starts_with(&format!("{path}[")));
                        sources.extend(origins);
                        merged_addons.insert(name.clone(), Value::Array(resolved));
                    }
                } else if !value.is_null() || layer.is_none() {
                    sources.insert(path, file.display().to_string());
                    merged_addons.insert(name.clone(), value.clone());
                }
            }
        }
        for (key, value) in object {
            sources.insert(key.clone(), file.display().to_string());
            merged.insert(key.clone(), value.clone());
        }
    }
    merged.insert("addons".into(), Value::Object(merged_addons));
    let result = (|| {
        if merged
            .get("output")
            .and_then(Value::as_object)
            .is_some_and(|o| o.get("logger").is_some_and(|v| !v.is_null()))
        {
            return Err(invalid(
                "output.logger",
                "obsolete Python setting; remove this key",
            ));
        }
        let config: Config = deserialize(&Value::Object(merged), "")?;
        validate_output(&config.output)?;
        for layer in Layer::ALL {
            for (i, addon) in layer.configured(&config.addons).iter().enumerate() {
                let path = if layer == Layer::Log2Reqs {
                    "addons.log2reqs".into()
                } else {
                    format!("addons.{}[{i}]", layer.name())
                };
                validate_addon(layer, addon, &path)?;
            }
        }
        Ok(config)
    })();
    result.map_err(|mut error: Error| {
        if let Some((_, source)) = sources
            .iter()
            .filter(|(path, _)| {
                error.message.starts_with(&format!("{path}:"))
                    || error.message.starts_with(&format!("{path}."))
                    || error.message.starts_with(&format!("{path}["))
            })
            .max_by_key(|(path, _)| path.len())
        {
            error.message = format!("{source}: {}", error.message);
        }
        error
    })
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

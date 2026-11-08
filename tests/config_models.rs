use clap::Parser;
use jumeaux::{
    addons::{self, AddonExecutor, Chain, Factory, Layer, Stage},
    cli::{Cli, Command},
    compat::{
        self,
        encoding::{self, Errors},
    },
    config::{self, Config},
    models::*,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn fixture(path: &str) -> Value {
    serde_json::from_slice(&fs::read(root().join(path)).unwrap()).unwrap()
}
fn report(path: &Path) -> (Value, Report) {
    let bytes = fs::read(path).unwrap();
    let codec = if bytes.starts_with(&[0xff, 0xfe]) {
        "utf-16"
    } else {
        "utf8"
    };
    let text = encoding::decode(&bytes, codec, Errors::Strict).unwrap();
    (
        serde_json::from_str(&text).unwrap(),
        serde_json::from_str(&text).unwrap(),
    )
}
fn args(value: &Value) -> (jumeaux::cli::ReportArgs, Vec<String>) {
    let mut argv = vec!["jumeaux".to_string(), "retry".into(), "report.json".into()];
    for key in [
        "title",
        "description",
        "threads",
        "processes",
        "max_retries",
    ] {
        if let Some(value) = value.get(key).filter(|v| !v.is_null()) {
            argv.push(format!(
                "--{}={}",
                key.replace('_', "-"),
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            ));
        }
    }
    if let Some(tags) = value.get("tag").and_then(Value::as_array) {
        for tag in tags {
            argv.push(format!("--tag={}", tag.as_str().unwrap()));
        }
    }
    let Command::Retry(args) = Cli::parse_from(argv).command else {
        unreachable!()
    };
    (
        args.options,
        serde_json::from_value(value.get("files").cloned().unwrap_or(json!([]))).unwrap(),
    )
}

#[test]
fn python_config_and_model_fixtures() {
    let cases = fixture("compat/fixtures/config-models.json");
    let mut count = 0;
    for case in cases.as_array().unwrap() {
        let input = &case["input"];
        let actual = match case["operation"].as_str().unwrap() {
            "config" => {
                let paths: Vec<_> = input["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        root()
                            .join("compat/fixtures/config")
                            .join(p.as_str().unwrap())
                    })
                    .collect();
                let skip =
                    serde_json::from_value::<Vec<String>>(input["skip_tags"].clone()).unwrap();
                let mut config = config::load(&paths, &skip).unwrap();
                let args = serde_json::from_value(input["args"].clone()).unwrap();
                config.merge_args(&args);
                serde_json::to_value(config).unwrap()
            }
            "Request" => {
                serde_json::to_value(serde_json::from_value::<Request>(input.clone()).unwrap())
                    .unwrap()
            }
            "Trial" => {
                serde_json::to_value(serde_json::from_value::<Trial>(input.clone()).unwrap())
                    .unwrap()
            }
            "Report" => {
                serde_json::to_value(serde_json::from_value::<Report>(input.clone()).unwrap())
                    .unwrap()
            }
            "number-spelling" | "report-number-spelling" | "output" => continue,
            operation => panic!("unknown fixture operation {operation}"),
        };
        assert_eq!(actual, case["expected"]["value"], "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 18);
    eprintln!("Python config/model fixtures: {count}/{count}");
}

#[test]
fn all_baseline_reports_round_trip_without_exclusions() {
    let baseline = root().join("compat/python-baseline");
    let templates = [
        "all_same",
        "force_json",
        "html",
        "ignore",
        "ignore_order",
        "judge_response_headers",
        "notifier",
        "path_custom",
        "path_empty",
        "post",
        "query_custom",
        "request_headers",
        "root_array",
        "simple",
        "xml",
    ];
    for template in templates {
        assert!(baseline.join(template).join("report.json").is_file());
    }
    // 16番目のencoding_hellは基準側で失敗し、reportを生成しない。
    assert!(!baseline.join("encoding_hell/report.json").exists());
    assert_eq!(
        fs::read_to_string(baseline.join("encoding_hell/run.exit-code"))
            .unwrap()
            .trim(),
        "1"
    );
    let mut count = 0;
    for entry in fs::read_dir(&baseline).unwrap() {
        let path = entry.unwrap().path().join("report.json");
        if !path.is_file() {
            continue;
        }
        let (expected, model) = report(&path);
        assert_eq!(
            serde_json::to_value(&model).unwrap(),
            expected,
            "{}",
            path.display()
        );
        let mut bytes = Vec::new();
        model
            .summary
            .output
            .write_report(&mut bytes, &model)
            .unwrap();
        assert_eq!(
            bytes,
            fs::read(&path).unwrap(),
            "report bytes: {}",
            path.display()
        );
        let text =
            encoding::decode(&bytes, &model.summary.output.encoding, Errors::Strict).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            expected,
            "{}",
            path.display()
        );
        count += 1;
    }
    assert_eq!(count, 31);
    eprintln!(
        "Python baseline report round trips: {count}/{count}; templates 15/15 + encoding_hell expected failure"
    );
}

#[test]
fn optional_report_fields_and_arbitrary_json_nulls_follow_owlmixin() {
    let (_, mut report) = report(&root().join("compat/python-baseline/simple/report.json"));
    report.description = Some(String::new());
    report.notifiers = Some(Default::default());
    report.retry_hash = Some(String::new());
    report.trials[0].diffs_by_cognition = None;
    report.trials[0].json = Some(
        serde_json::from_value(json!({"none":null,"empty":{},"array":[null,false,0,""]})).unwrap(),
    );
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["description"], "");
    assert_eq!(value["notifiers"], json!({}));
    assert_eq!(value["retry_hash"], "");
    assert!(
        !value["trials"][0]
            .as_object()
            .unwrap()
            .contains_key("diffs_by_cognition")
    );
    assert_eq!(
        value["trials"][0]["json"],
        json!({"empty":{},"array":[false,0,""]})
    );
    assert!(report.trials[0].json.as_ref().unwrap().contains_key("none"));
    report.trials[0].diffs_by_cognition = Some(Default::default());
    report.trials[0].raw = Some("body".into());
    assert_eq!(report.trials[0].to_request().raw.as_deref(), Some("body"));
    assert_eq!(
        serde_json::to_value(report).unwrap()["trials"][0]["diffs_by_cognition"],
        json!({})
    );
}

#[test]
fn number_spelling_difference_is_observed_without_normalizing() {
    let cases = fixture("compat/fixtures/config-models.json");
    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "number-spelling")
        .unwrap();
    let model: Request = serde_json::from_str(case["input"].as_str().unwrap()).unwrap();
    let actual = serde_json::to_value(model).unwrap();
    let python: Value = serde_json::from_str(case["expected"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(actual["json"]["decimal"].to_string(), "1.2300");
    assert_eq!(python["json"]["decimal"].to_string(), "1.23");
    assert_eq!(actual["json"]["exponent"].to_string(), "1e+2");
    assert_eq!(python["json"]["exponent"].to_string(), "100.0");
    assert_eq!(actual["json"]["big"].to_string(), "18446744073709551617");
    assert_eq!(actual["json"]["big"], python["json"]["big"]);
    assert_ne!(actual, python);

    let case = cases
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["operation"] == "report-number-spelling")
        .unwrap();
    let model: Report = serde_json::from_str(case["input"].as_str().unwrap()).unwrap();
    let mut bytes = Vec::new();
    model
        .summary
        .output
        .write_report(&mut bytes, &model)
        .unwrap();
    let actual: Value = serde_json::from_slice(&bytes).unwrap();
    let python: Value = serde_json::from_str(case["expected"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(actual["trials"][0]["json"]["decimal"].to_string(), "1.2300");
    assert_eq!(python["trials"][0]["json"]["decimal"].to_string(), "1.23");
    assert_eq!(actual["trials"][0]["json"]["exponent"].to_string(), "1e+2");
    assert_eq!(python["trials"][0]["json"]["exponent"].to_string(), "100.0");
    assert_eq!(
        actual["trials"][0]["json"]["big"],
        python["trials"][0]["json"]["big"]
    );
    assert_ne!(
        String::from_utf8(bytes).unwrap(),
        case["expected"]["value"].as_str().unwrap()
    );
}

#[test]
fn python_text_stream_fixtures_match_each_flush() {
    let cases = fixture("compat/fixtures/config-models.json");
    let mut count = 0;
    for case in cases
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["operation"] == "output")
    {
        let input = &case["input"];
        let output = OutputSummary {
            response_dir: "out".into(),
            encoding: input["encoding"].as_str().unwrap().into(),
            logger: None,
        };
        let mut bytes = Vec::new();
        let mut expected_chunks = Vec::new();
        let result = (|| -> compat::Result<()> {
            let mut writer = output.writer(&mut bytes)?;
            for chunk in input["chunks"].as_array().unwrap() {
                writer.write_text(chunk.as_str().unwrap())?;
                writer.flush()?;
                expected_chunks.push(
                    writer
                        .get_ref()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                );
            }
            Ok(())
        })();
        let actual = match result {
            Ok(()) => json!({"value":expected_chunks}),
            Err(e) => json!({"error":e.kind}),
        };
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 12);
    eprintln!("Python text stream fixtures: {count}/{count}");
}

fn base_config() -> Config {
    config::load(&[root().join("compat/fixtures/config/base.yml")], &[]).unwrap()
}

#[test]
fn concurrency_uses_processes_first_and_rejects_only_active_nonpositive_count() {
    let mut config = base_config();
    for (threads, processes, expected) in [
        (4, Some(2), Some((1, 2))),
        (0, Some(2), Some((1, 2))),
        (4, Some(0), Some((4, 1))),
        (4, None, Some((4, 1))),
        (0, None, None),
        (-1, None, None),
        (4, Some(-1), None),
    ] {
        config.threads = threads;
        config.processes = processes;
        assert_eq!(
            config.concurrency().ok().map(|c| (c.threads, c.processes)),
            expected
        );
    }
}

#[test]
fn run_and_retry_normalize_empty_cli_tags_differently() {
    let (options, _) = args(&json!({}));
    let mut config = base_config();
    config.merge_args(&config::MergedArgs::from_cli(&options));
    assert_eq!(config.tags, Some(vec!["base".into()]));
    config.merge_args(&config::MergedArgs::from_retry_args(&options));
    assert_eq!(config.tags, Some(vec![]));
}

#[test]
fn retry_fixture_restores_raw_and_requires_explicit_header_settings() {
    let cases = fixture("compat/fixtures/retry.json");
    for case in cases.as_array().unwrap() {
        let report: Report = serde_json::from_value(case["input"]["report"].clone()).unwrap();
        let headers = serde_json::from_value(case["input"]["original_config"].clone()).unwrap();
        let (config, warnings) = Config::from_report_with_headers(&report, headers).unwrap();
        assert!(warnings.is_empty());
        let trials: Vec<_> = report
            .trials
            .iter()
            .map(|t| {
                let req = t.to_request();
                json!({"name":req.name,"raw":req.raw,"form":req.form,"json":req.json})
            })
            .collect();
        let actual = json!({"trials":trials,"header_settings":{
            "judge_response_header":config.judge_response_header,
            "ignore_response_header_keys":config.ignore_response_header_keys
        }});
        assert_eq!(actual, case["expected"]["value"]);
        assert_ne!(actual, case["python_observed"]["value"]);
    }
    assert_eq!(cases.as_array().unwrap().len(), 1);
}

static TEMP_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jumeaux-config-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, text).unwrap();
        p
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn unsupported_settings_report_the_precise_key() {
    let base = root().join("compat/fixtures/config/base.yml");
    let temp = Temp::new();
    for (yaml, key) in [
        ("unknown: 1", "unknown"),
        (
            "one: {name: one, host: http://localhost, typo: x}",
            "one.typo",
        ),
        (
            "one: {name: one, host: http://localhost, path: {before: x, after: y, typo: z}}",
            "one.path.typo",
        ),
        ("output: {response_dir: out, logger: true}", "output.logger"),
        (
            "output: {response_dir: out, encoding: no-such-codec}",
            "output.encoding",
        ),
        ("addons: {typo: []}", "addons.typo"),
        (
            "addons: {log2reqs: {name: package.module}}",
            "addons.log2reqs.name",
        ),
        (
            "addons: {final: [{name: not_registered}]}",
            "addons.final[0].name",
        ),
        (
            "addons: {final: [{name: json, cls_name: Custom}]}",
            "addons.final[0].cls_name",
        ),
        (
            "addons: {final: [{name: json, config: {typo: 1}}]}",
            "addons.final[0].config.typo",
        ),
        (
            "addons: {res2res: [{name: type, config: {conditions: [{type: json, typo: x}]}}]}",
            "addons.res2res[0].config.conditions[0].typo",
        ),
        (
            "addons: {res2res: [{name: json, config: {transformer: {module: python_module}}}]}",
            "addons.res2res[0].config.transformer",
        ),
        (
            "addons: {reqs2reqs: [{name: head, config: {size: wrong}}]}",
            "addons.reqs2reqs[0].config.size",
        ),
        ("threads: wrong", "threads"),
    ] {
        let override_path = temp.write("override.yml", yaml);
        let error = config::load(&[&base, &override_path], &[]).unwrap_err();
        assert!(error.message.contains(key), "{yaml}: {error}");
    }
}

#[test]
fn includes_resolve_against_each_source_file_before_later_merges() {
    let temp = Temp::new();
    fs::create_dir(temp.0.join("first")).unwrap();
    fs::create_dir(temp.0.join("second")).unwrap();
    temp.write("first/addon.yml", "name: head\nconfig: {size: 2}");
    temp.write("second/addon.yml", "name: repeat\nconfig: {times: 3}");
    let a = temp.write(
        "first/config.yml",
        "addons:\n  reqs2reqs: [{include: addon.yml}]",
    );
    let b = temp.write(
        "second/config.yml",
        "addons:\n  reqs2reqs: [{include: addon.yml}]",
    );
    let config =
        config::load(&[root().join("compat/fixtures/config/base.yml"), a, b], &[]).unwrap();
    assert_eq!(config.addons.reqs2reqs.len(), 1);
    assert_eq!(config.addons.reqs2reqs[0].name, "repeat");
    assert_eq!(
        config.addons.reqs2reqs[0].config.as_ref().unwrap()["times"],
        3
    );
}

#[test]
fn every_baseline_config_loads_except_the_known_broken_template() {
    let mut count = 0;
    for entry in fs::read_dir(root().join("compat/python-baseline")).unwrap() {
        let dir = entry.unwrap().path();
        let path = dir.join("config.yml");
        if !path.is_file() {
            continue;
        }
        if dir.file_name().unwrap() == "encoding_hell" {
            assert!(
                config::load(&[path], &[])
                    .unwrap_err()
                    .message
                    .contains("items[0].conditions")
            );
        } else {
            let config =
                config::load(&[&path], &[]).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            addons::Executor::skeleton(&config.addons).unwrap();
            count += 1;
        }
    }
    assert!(count >= 31);
    eprintln!("Baseline configurations: {count}/{count} + encoding_hell expected failure");
}

struct TestStage<const N: usize>;
impl<const N: usize> Stage for TestStage<N> {
    const LAYER: Layer = Layer::ALL[N];
    type Payload = Vec<String>;
    type Reference = String;
}
struct TraceFactory(Arc<Mutex<Vec<Addon>>>);
struct TraceAddon {
    addon: Addon,
}
impl<const N: usize> Factory<TestStage<N>> for TraceFactory {
    fn create(&self, addon: &Addon) -> compat::Result<Box<dyn AddonExecutor<TestStage<N>>>> {
        self.0.lock().unwrap().push(addon.clone());
        Ok(Box::new(TraceAddon {
            addon: addon.clone(),
        }))
    }
}
impl<const N: usize> AddonExecutor<TestStage<N>> for TraceAddon {
    fn exec(&self, mut payload: Vec<String>, reference: &String) -> compat::Result<Vec<String>> {
        assert_eq!(reference, "reference");
        payload.push(self.addon.name.clone());
        Ok(payload)
    }
}
fn check_layer<const N: usize>() {
    let layer = Layer::ALL[N];
    let mut value = serde_json::to_value(base_config().addons).unwrap();
    let names = layer.names();
    let entries: Vec<_> = names
        .iter()
        .rev()
        .filter(|name| !(layer == Layer::Res2Res && **name == "json"))
        .filter(|name| cfg!(feature = "miroir") || !(layer == Layer::Final && **name == "miroir"))
        .map(|name| {
            let cases = fixture("compat/fixtures/review-fixes.json");
            let case = cases
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == format!("addon-valid-{}/{name}", layer.name()))
                .unwrap();
            json!({"name":name,"config":case["input"]["config"],"tags":["metadata"]})
        })
        .collect();
    value[layer.name()] = if layer == Layer::Log2Reqs {
        entries[0].clone()
    } else {
        json!(entries)
    };
    let addons: Addons = serde_json::from_value(value).unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let chain = Chain::<TestStage<N>>::build(&addons, &TraceFactory(captured.clone())).unwrap();
    let expected: Vec<_> = layer
        .configured(&addons)
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(chain.exec(vec![], &"reference".into()).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(&*captured.lock().unwrap()).unwrap(),
        serde_json::to_value(layer.configured(&addons)).unwrap()
    );
}
#[test]
fn all_nine_layers_preserve_config_reference_and_list_order() {
    check_layer::<0>();
    check_layer::<1>();
    check_layer::<2>();
    check_layer::<3>();
    check_layer::<4>();
    check_layer::<5>();
    check_layer::<6>();
    check_layer::<7>();
    check_layer::<8>();
    assert_eq!(
        Layer::ALL.iter().map(|l| l.name()).collect::<Vec<_>>(),
        Layer::NAMES
    );
}

#[test]
fn output_encoding_handles_multiple_writes_and_strict_errors() {
    for codec in [
        "utf8",
        "utf-8-sig",
        "utf-16",
        "utf-32",
        "cp932",
        "iso2022_jp",
    ] {
        let output = OutputSummary {
            response_dir: "out".into(),
            encoding: codec.into(),
            logger: None,
        };
        let mut writer = output.writer(Vec::new()).unwrap();
        writer.write_text("日本").unwrap();
        writer.write_text("語\n").unwrap();
        writer.flush().unwrap();
        let bytes = writer.into_inner();
        assert_eq!(
            encoding::decode(&bytes, codec, Errors::Strict).unwrap(),
            "日本語\n",
            "{codec}"
        );
        if codec != "iso2022_jp" {
            assert_eq!(
                bytes,
                encoding::encode("日本語\n", codec, Errors::Strict).unwrap()
            );
        }
    }
    let output = OutputSummary {
        response_dir: "out".into(),
        encoding: "ascii".into(),
        logger: None,
    };
    let mut writer = output.writer(Vec::new()).unwrap();
    assert_eq!(
        writer.write_text("日本").unwrap_err().kind,
        "UnicodeEncodeError"
    );
    assert!(writer.into_inner().is_empty());
}

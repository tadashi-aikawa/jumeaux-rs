//! 外部コマンドのホスト検証・通信・所有プロセスの中断。出力先は呼出元が所有する。
mod config;
pub mod json;
mod process;
pub mod wire;
use super::{AddonConfig, Layer, Stage, parse_config};
use crate::{
    compat::{Error, Result},
    config::invalid,
    models::{Addon, Addons},
};
pub use config::Config;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
};
use wire::Wire;

pub const MIGRATION: &str = "旧.pyをcommandに指定するだけでは動きません。stdin/stdoutのJSON契約へ書き換えてください。\n雛形: jumeaux init addon\n移行ガイド: https://tadashi-aikawa.github.io/jumeaux-rs/addons/exec/";

// retryの許可はプロセス生成前に呼出元で判定する。設定へ保存しない。
pub fn check_retry(addons: &Addons, allow_exec: bool) -> Result<()> {
    if allow_exec {
        return Ok(());
    }
    for layer in Layer::ALL {
        for (index, addon) in layer.configured(addons).iter().enumerate() {
            if addon.name == "exec" || (layer == Layer::Res2Res && addon.name == "json") {
                return Err(invalid(
                    &format!("report.addons.{}[{index}]", layer.name()),
                    format!(
                        "外部コマンドを含むReportのretryを拒否しました。Report内のcommandと実行先を確認し、信頼できる場合だけ --allow-exec を指定してください。validateも外部コマンド実行に含まれます。\n{MIGRATION}"
                    ),
                ));
            }
        }
    }
    Ok(())
}
#[derive(Debug, Default, Clone, Copy)]
pub struct Invocation {
    pub trial_seq: Option<u64>,
    pub side: Option<&'static str>,
}
#[derive(Debug, Clone)]
pub struct Stderr {
    pub layer: Layer,
    pub index: usize,
    pub trial_seq: Option<u64>,
    pub side: Option<&'static str>,
    pub continued: bool,
    pub text: String,
}
struct Prepared {
    layer: Layer,
    index: usize,
    path: String,
    config: Config,
    executable: PathBuf,
    cwd: PathBuf,
    env: BTreeMap<OsString, OsString>,
    transformer: Option<super::res2res::json::Compiled>,
}
pub struct Runtime {
    commands: Vec<Prepared>,
    invocation_cwd: PathBuf,
    verbosity: u8,
    observer: Box<dyn Fn(&Stderr) + Send + Sync>,
    cancelled: Arc<AtomicI32>,
}
impl Runtime {
    pub fn prepare(
        addons: &Addons,
        verbosity: u8,
        observer: impl Fn(&Stderr) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        Self::prepare_with_sources(addons, verbosity, observer, &Default::default())
    }
    pub fn prepare_with_sources(
        addons: &Addons,
        verbosity: u8,
        observer: impl Fn(&Stderr) + Send + Sync + 'static,
        sources: &indexmap::IndexMap<String, String>,
    ) -> Result<Arc<Self>> {
        Self::prepare_layers(
            Layer::ALL
                .into_iter()
                .map(|layer| (layer, layer.configured(addons))),
            verbosity,
            observer,
            sources,
        )
    }
    pub fn prepare_layer(layer: Layer, addons: &[Addon]) -> Result<Arc<Self>> {
        Self::prepare_layers([(layer, addons)], 0, |_| {}, &Default::default())
    }
    fn prepare_layers<'a>(
        layers: impl IntoIterator<Item = (Layer, &'a [Addon])>,
        verbosity: u8,
        observer: impl Fn(&Stderr) + Send + Sync + 'static,
        sources: &indexmap::IndexMap<String, String>,
    ) -> Result<Arc<Self>> {
        let invocation_cwd = std::env::current_dir().map_err(|e| invalid("cwd", e))?;
        let environment: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        let mut commands = Vec::new();
        for (layer, addons) in layers {
            for (index, addon) in addons.iter().enumerate() {
                let logical = if layer == Layer::Log2Reqs {
                    "addons.log2reqs".into()
                } else {
                    format!("addons.{}[{index}]", layer.name())
                };
                let path = sources
                    .get(&logical)
                    .map(|s| format!("{s}: {logical}"))
                    .unwrap_or(logical);
                let parsed = parse_config(layer, addon, &path)?;
                let (config, transformer, command_path) = match parsed {
                    AddonConfig::Exec(config) => (config, None, format!("{path}.config")),
                    AddonConfig::Res2ResJson(config) => {
                        let compiled = super::res2res::json::Compiled::new(
                            config.clone(),
                            format!("{path}.config"),
                        )?;
                        (
                            config.transformer,
                            Some(compiled),
                            format!("{path}.config.transformer"),
                        )
                    }
                    _ => continue,
                };
                let mut command = Prepared::new(
                    layer,
                    index,
                    command_path,
                    config,
                    &invocation_cwd,
                    &environment,
                )?;
                command.transformer = transformer;
                commands.push(command);
            }
        }
        Ok(Arc::new(Self {
            commands,
            invocation_cwd,
            verbosity,
            observer: Box::new(observer),
            cancelled: Arc::new(AtomicI32::new(0)),
        }))
    }
    pub fn contains(&self, layer: Layer, index: usize) -> bool {
        self.commands
            .iter()
            .any(|c| c.layer == layer && c.index == index)
    }
    pub fn check_cancelled(&self) -> Result<()> {
        match self.cancelled.load(Ordering::SeqCst) {
            0 => Ok(()),
            signal => Err(Error::new(
                "ExecAddonInterrupted",
                format!("interrupted by signal {signal}"),
            )),
        }
    }
    pub async fn interrupted(&self) -> Error {
        loop {
            if let Err(error) = self.check_cancelled() {
                return error;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    pub fn exit_code(&self) -> Option<u8> {
        match self.cancelled.load(Ordering::SeqCst) {
            0 => None,
            n => Some((128 + n) as u8),
        }
    }
    pub fn cancel(&self, signal: i32) {
        let _ = self
            .cancelled
            .compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
    }
    /// シグナルハンドラはCLIが明示登録する。ライブラリ呼出しは親のハンドラを変更しない。
    #[cfg(unix)]
    pub fn signals(&self) -> Result<Signals> {
        let mut guard = Signals(Vec::new());
        for signal in [libc::SIGINT, libc::SIGTERM] {
            let cancelled = Arc::clone(&self.cancelled);
            // SAFETY: ハンドラはlock-free atomic操作のみ。allocation・I/O・ロックなし。
            let id = unsafe {
                signal_hook_registry::register(signal, move || {
                    let _ =
                        cancelled.compare_exchange(0, signal, Ordering::SeqCst, Ordering::SeqCst);
                })
            }
            .map_err(|e| invalid("signals", e))?;
            guard.0.push(id);
        }
        Ok(guard)
    }
    pub fn validate(&self) -> Result<()> {
        for command in &self.commands {
            self.invoke(
                command.layer,
                command.index,
                "validate",
                Value::Null,
                Value::Null,
                Invocation::default(),
                |value| {
                    if value == json!({"valid": true}) {
                        Ok(())
                    } else {
                        Err(json::protocol("$: expected exactly {\"valid\":true}"))
                    }
                },
            )
            .map_err(|e| {
                if e.kind == "ExecAddonInterrupted" {
                    e
                } else {
                    invalid(&command.path, e)
                }
            })?;
        }
        Ok(())
    }
    pub fn payload<S: Stage>(
        &self,
        index: usize,
        payload: S::Payload,
        reference: &S::Reference,
        invocation: Invocation,
    ) -> Result<S::Payload>
    where
        S::Payload: Wire,
        S::Reference: Wire,
    {
        if let Some(transformer) = self
            .commands
            .iter()
            .find(|c| c.layer == S::LAYER && c.index == index)
            .and_then(|c| c.transformer.as_ref())
        {
            let payload = super::payload::Res2ResPayload::from_wire(payload.wire()?)?;
            return S::Payload::from_wire(
                transformer
                    .apply(self, index, payload, invocation)?
                    .wire()?,
            );
        }
        self.invoke(
            S::LAYER,
            index,
            "payload",
            payload.wire()?,
            reference.wire()?,
            invocation,
            S::Payload::from_wire,
        )
    }
    // 通信envelopeの各フィールドと、診断保持中に行う型復元を明示する。
    #[allow(clippy::too_many_arguments)]
    pub fn invoke<T>(
        &self,
        layer: Layer,
        index: usize,
        mode: &str,
        payload: Value,
        reference: Value,
        invocation: Invocation,
        decode: impl FnOnce(Value) -> Result<T>,
    ) -> Result<T> {
        let command = self
            .commands
            .iter()
            .find(|c| c.layer == layer && c.index == index)
            .ok_or_else(|| invalid(layer.name(), "external command was not prepared"))?;
        let envelope = json!({"protocol_version": 1, "layer": layer.name(), "mode": mode,
            "options": command.config.options,
            "context": {"invocation_cwd": self.invocation_cwd, "verbosity": self.verbosity, "trial_seq": invocation.trial_seq, "side": invocation.side},
            "payload": payload, "reference": reference});
        crate::compat::json::validate(&envelope)
            .map_err(|_| json::protocol("input: non-finite number or excessive JSON depth"))?;
        let input = serde_json::to_vec(&envelope)
            .map_err(|_| json::protocol("input: cannot encode JSON"))?;
        let result = (|| {
            let output = process::run(self, command, invocation, &input)?;
            self.check_cancelled()?;
            json::parse(&output.bytes)
                .and_then(decode)
                .map_err(|e| with_tail(e, &output.tail))
        })();
        result.map_err(|mut e| {
            e.message = format!(
                "{} ({}; trial={:?}; side={:?}): {}",
                command.path,
                command.config.command[0],
                invocation.trial_seq,
                invocation.side,
                e.message
            );
            e
        })
    }
}
#[cfg(unix)]
pub struct Signals(Vec<signal_hook_registry::SigId>);
#[cfg(unix)]
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook_registry::unregister(id);
        }
    }
}

impl Prepared {
    fn new(
        layer: Layer,
        index: usize,
        path: String,
        config: Config,
        invocation_cwd: &Path,
        environment: &BTreeMap<OsString, OsString>,
    ) -> Result<Self> {
        config.validate(&path)?;
        let cwd = invocation_cwd.join(config.cwd.as_deref().unwrap_or("."));
        if !cwd.is_dir() {
            return Err(invalid(
                &format!("{path}.cwd"),
                "expected existing directory",
            ));
        }
        let mut env = if config.inherit_env {
            environment.clone()
        } else {
            BTreeMap::new()
        };
        for (key, value) in &config.env {
            if let Some(value) = value {
                env.insert(key.into(), value.into());
            } else {
                env.remove(&OsString::from(key));
            }
        }
        let name = &config.command[0];
        let candidates = if name.contains('/') {
            vec![cwd.join(name)]
        } else {
            let search = env.get(&OsString::from("PATH")).ok_or_else(|| {
                invalid(
                    &format!("{path}.command[0]"),
                    "PATH is unset; specify a path containing /",
                )
            })?;
            std::env::split_paths(search)
                .map(|p| cwd.join(p).join(name))
                .collect()
        };
        let executable = candidates
            .into_iter()
            .find(|p| executable(p))
            .ok_or_else(|| {
                invalid(
                    &format!("{path}.command[0]"),
                    format!("executable not found or not executable: {name}"),
                )
            })?;
        Ok(Self {
            layer,
            index,
            path,
            config,
            executable,
            cwd,
            env,
            transformer: None,
        })
    }
}
fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
fn with_tail(mut error: Error, tail: &[u8]) -> Error {
    if !tail.is_empty() {
        error.message.push_str(&format!(
            "\nstderr末尾（表示済み部分と重複する場合があります）:\n{}",
            String::from_utf8_lossy(tail)
        ));
    }
    error
}

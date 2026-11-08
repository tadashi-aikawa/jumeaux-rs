//! 組み込みアドオンの静的登録。現段階のexecutorは副作用を持たない骨組み。
pub mod did_challenge;
pub mod dump;
pub mod exec;
pub mod r#final;
pub mod judgement;
pub mod log;
pub mod log2reqs;
pub mod payload;
mod registry;
pub mod reqs2reqs;
pub mod res2dict;
pub mod res2res;
pub mod store_criterion;
use crate::{
    compat::Result,
    config::{Config, invalid},
    models::*,
};
use payload::*;
pub use registry::{AddonConfig, parse_config, validate_addon};
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Log2Reqs,
    Reqs2Reqs,
    Res2Res,
    Res2Dict,
    Judgement,
    StoreCriterion,
    Dump,
    DidChallenge,
    Final,
}
impl Layer {
    pub const ALL: [Self; 9] = [
        Self::Log2Reqs,
        Self::Reqs2Reqs,
        Self::Res2Res,
        Self::Res2Dict,
        Self::Judgement,
        Self::StoreCriterion,
        Self::Dump,
        Self::DidChallenge,
        Self::Final,
    ];
    pub const NAMES: [&'static str; 9] = [
        "log2reqs",
        "reqs2reqs",
        "res2res",
        "res2dict",
        "judgement",
        "store_criterion",
        "dump",
        "did_challenge",
        "final",
    ];
    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.name() == name)
    }
    pub fn configured(self, addons: &Addons) -> &[Addon] {
        match self {
            Self::Log2Reqs => std::slice::from_ref(&addons.log2reqs),
            Self::Reqs2Reqs => &addons.reqs2reqs,
            Self::Res2Res => &addons.res2res,
            Self::Res2Dict => &addons.res2dict,
            Self::Judgement => &addons.judgement,
            Self::StoreCriterion => &addons.store_criterion,
            Self::Dump => &addons.dump,
            Self::DidChallenge => &addons.did_challenge,
            Self::Final => &addons.final_addons,
        }
    }
}

/// レイヤーごとにpayloadと参照型を固定する。異なるレイヤーは接続できない。
pub trait Stage: Send + Sync + 'static {
    const LAYER: Layer;
    type Payload;
    type Reference;
    fn external(
        _runtime: &exec::Runtime,
        _index: usize,
        _payload: Self::Payload,
        _reference: &Self::Reference,
        _invocation: exec::Invocation,
    ) -> Result<Self::Payload> {
        Err(invalid(
            Self::LAYER.name(),
            "custom Stage has no external wire contract",
        ))
    }
}
pub trait AddonExecutor<S: Stage>: Send + Sync {
    fn exec(&self, payload: S::Payload, reference: &S::Reference) -> Result<S::Payload>;
    fn exec_with_log(
        &self,
        payload: S::Payload,
        reference: &S::Reference,
        _log: &dyn Fn(log::AddonLog),
    ) -> Result<S::Payload> {
        self.exec(payload, reference)
    }
}
macro_rules! stages {
    ($($stage:ident: $payload:ty, $reference:ty);+ $(;)?) => {$(
        pub struct $stage;
        impl Stage for $stage {
            const LAYER: Layer = Layer::$stage;
            type Payload = $payload;
            type Reference = $reference;
            fn external(runtime: &exec::Runtime, index: usize, payload: Self::Payload, reference: &Self::Reference, invocation: exec::Invocation) -> Result<Self::Payload> {
                runtime.payload::<Self>(index, payload, reference, invocation)
            }
        }
    )+};
}
stages! {
    Log2Reqs: Log2ReqsPayload, ();
    Reqs2Reqs: Reqs2ReqsPayload, Config;
    Res2Res: Res2ResPayload, ();
    Res2Dict: Res2DictPayload, ();
    Judgement: JudgementPayload, JudgementReference;
    StoreCriterion: StoreCriterionPayload, StoreCriterionReference;
    Dump: DumpPayload, ();
    DidChallenge: DidChallengePayload, DidChallengeReference;
    Final: FinalPayload, FinalReference;
}
pub trait Factory<S: Stage> {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<S>>>;
}
pub struct SkeletonFactory;
struct Skeleton<S: Stage> {
    _config: AddonConfig,
    stage: PhantomData<S>,
}
impl<S: Stage> AddonExecutor<S> for Skeleton<S> {
    fn exec(&self, payload: S::Payload, _: &S::Reference) -> Result<S::Payload> {
        Ok(payload)
    }
}
impl<S: Stage> Factory<S> for SkeletonFactory {
    fn create(&self, addon: &Addon) -> Result<Box<dyn AddonExecutor<S>>> {
        Ok(Box::new(Skeleton::<S> {
            _config: parse_config(S::LAYER, addon, &format!("addons.{}", S::LAYER.name()))?,
            stage: PhantomData,
        }))
    }
}
pub struct Chain<S: Stage> {
    executors: Vec<Option<Box<dyn AddonExecutor<S>>>>,
    names: Vec<String>,
    runtime: std::sync::Arc<exec::Runtime>,
}
impl<S: Stage> Chain<S> {
    pub fn build(addons: &Addons, factory: &impl Factory<S>) -> Result<Self> {
        let executors = S::LAYER
            .configured(addons)
            .iter()
            .enumerate()
            .map(|(index, addon)| {
                validate_addon(
                    S::LAYER,
                    addon,
                    &format!("addons.{}[{index}]", S::LAYER.name()),
                )?;
                if addon.name == "exec" || (S::LAYER == Layer::Res2Res && addon.name == "json") {
                    Ok(None)
                } else {
                    factory.create(addon).map(Some)
                }
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            executors,
            runtime: exec::Runtime::prepare_layer(S::LAYER, S::LAYER.configured(addons))?,
            names: S::LAYER
                .configured(addons)
                .iter()
                .map(|a| a.name.clone())
                .collect(),
        })
    }
    pub fn with_runtime(mut self, runtime: std::sync::Arc<exec::Runtime>) -> Self {
        self.runtime = runtime;
        self
    }
    /// Pythonのreduceと同じ順序。直前の戻り値を次へ渡し、最初のエラーで停止する。
    pub fn exec(&self, payload: S::Payload, reference: &S::Reference) -> Result<S::Payload> {
        self.exec_with_log(payload, reference, &|_, _, _| {})
    }
    pub fn exec_with_log(
        &self,
        payload: S::Payload,
        reference: &S::Reference,
        log: &dyn Fn(usize, &str, log::AddonLog),
    ) -> Result<S::Payload> {
        self.exec_at(payload, reference, exec::Invocation::default(), log)
    }
    pub fn exec_at(
        &self,
        mut payload: S::Payload,
        reference: &S::Reference,
        invocation: exec::Invocation,
        log: &dyn Fn(usize, &str, log::AddonLog),
    ) -> Result<S::Payload> {
        self.runtime.check_cancelled()?;
        for (index, executor) in self.executors.iter().enumerate() {
            self.runtime.check_cancelled()?;
            payload = if let Some(executor) = executor {
                executor.exec_with_log(payload, reference, &|event| {
                    log(index, &self.names[index], event)
                })?
            } else {
                S::external(&self.runtime, index, payload, reference, invocation)?
            };
        }
        self.runtime.check_cancelled()?;
        Ok(payload)
    }
}
pub struct Executor {
    pub log2reqs: Chain<Log2Reqs>,
    pub reqs2reqs: Chain<Reqs2Reqs>,
    pub res2res: Chain<Res2Res>,
    pub res2dict: Chain<Res2Dict>,
    pub judgement: Chain<Judgement>,
    pub store_criterion: Chain<StoreCriterion>,
    pub dump: Chain<Dump>,
    pub did_challenge: Chain<DidChallenge>,
    pub final_addons: Chain<Final>,
}
impl Executor {
    /// HTTP/保存/通知は実行しない。後続の実装でfactoryを実アドオンへ置き換える。
    pub fn skeleton(addons: &Addons) -> Result<Self> {
        Ok(Self {
            log2reqs: Chain::build(addons, &SkeletonFactory)?,
            reqs2reqs: Chain::build(addons, &SkeletonFactory)?,
            res2res: Chain::build(addons, &SkeletonFactory)?,
            res2dict: Chain::build(addons, &SkeletonFactory)?,
            judgement: Chain::build(addons, &SkeletonFactory)?,
            store_criterion: Chain::build(addons, &SkeletonFactory)?,
            dump: Chain::build(addons, &SkeletonFactory)?,
            did_challenge: Chain::build(addons, &SkeletonFactory)?,
            final_addons: Chain::build(addons, &SkeletonFactory)?,
        })
    }
}
pub(crate) fn validate_registration(layer: Layer, addon: &Addon, path: &str) -> Result<()> {
    if !layer.names().contains(&addon.name.as_str()) {
        let removed_viewer = layer == Layer::Final && addon.name == "viewer";
        let candidate = layer
            .names()
            .iter()
            .map(|name| (edit_distance(&addon.name, name), *name))
            .filter(|(distance, _)| *distance <= 2)
            .min();
        let mut message = format!("unsupported addon {:?}; ", addon.name);
        if let Some((_, name)) = candidate {
            message.push_str(&format!("候補: {name}"));
        } else {
            message.push_str(&format!("登録済みの名前: {}", layer.names().join(", ")));
            if !removed_viewer {
                message.push_str("\nPythonのカスタムアドオンを使う場合は name: exec と config.command へ移行し、従来のconfigをconfig.optionsへ移してください。");
                if addon.name.contains('.') {
                    message.push_str("\nPython modules cannot be loaded.");
                }
                if addon.cls_name != "Executor" {
                    message.push_str("\ncls_nameは削除してください。");
                }
                message.push_str(&format!("\n{}", exec::MIGRATION));
            }
        }
        if removed_viewer {
            let source = if path.starts_with("report.") {
                "レポート"
            } else {
                "設定"
            };
            message.push_str(&format!("\nfinal/viewer はRust版にはありません。{source}の addons.final から name: viewer の項目を削除してください。"));
        }
        return Err(invalid(&format!("{path}.name"), message));
    }
    if addon.cls_name != "Executor" {
        return Err(invalid(
            &format!("{path}.cls_name"),
            format!(
                "only Executor is supported; cls_nameを削除し、name: exec のconfig.commandと引数で処理を指定してください。\n{}",
                exec::MIGRATION
            ),
        ));
    }
    if addon.include.is_some() {
        return Err(invalid(
            &format!("{path}.include"),
            "unresolved/nested include",
        ));
    }
    Ok(())
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<_> = right.chars().collect();
    let mut row: Vec<_> = (0..=right.len()).collect();
    for (i, a) in left.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (row[j] + 1)
                .min(previous + 1)
                .min(diagonal + usize::from(a != *b));
            diagonal = previous;
        }
    }
    row[right.len()]
}

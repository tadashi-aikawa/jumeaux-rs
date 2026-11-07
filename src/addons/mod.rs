//! 組み込みアドオンの静的登録。現段階のexecutorは副作用を持たない骨組み。
pub mod log2reqs;
pub mod payload;
mod schema;
use crate::{
    compat::Result,
    config::{Config, invalid},
    models::*,
};
use payload::*;
pub use schema::validate_addon;
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
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Self::Log2Reqs => &["plain", "csv", "json", "yaml"],
            Self::Reqs2Reqs => &[
                "add",
                "empty_guard",
                "filter",
                "head",
                "rename",
                "repeat",
                "replace",
                "shuffle",
            ],
            Self::Res2Res => &["json", "json_sort", "type"],
            Self::Res2Dict => &["block", "html", "json", "xml"],
            Self::Judgement => &["ignore", "same"],
            Self::StoreCriterion => &["free"],
            Self::Dump => &["encoding", "html", "json", "xml"],
            Self::DidChallenge => &["sleep", "tag"],
            Self::Final => &["csv", "json", "miroir", "notify", "summary", "viewer"],
        }
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
}
pub trait AddonExecutor<S: Stage>: Send + Sync {
    fn exec(&self, payload: S::Payload, reference: &S::Reference) -> Result<S::Payload>;
}
macro_rules! stages {
    ($($stage:ident: $payload:ty, $reference:ty);+ $(;)?) => {$(
        pub struct $stage;
        impl Stage for $stage {
            const LAYER: Layer = Layer::$stage;
            type Payload = $payload;
            type Reference = $reference;
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
    _addon: Addon,
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
            _addon: addon.clone(),
            stage: PhantomData,
        }))
    }
}
pub struct Chain<S: Stage> {
    executors: Vec<Box<dyn AddonExecutor<S>>>,
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
                factory.create(addon)
            })
            .collect::<Result<_>>()?;
        Ok(Self { executors })
    }
    /// Pythonのreduceと同じ順序。直前の戻り値を次へ渡し、最初のエラーで停止する。
    pub fn exec(&self, mut payload: S::Payload, reference: &S::Reference) -> Result<S::Payload> {
        for executor in &self.executors {
            payload = executor.exec(payload, reference)?;
        }
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
        return Err(invalid(
            &format!("{path}.name"),
            format!(
                "unsupported addon {:?}; Python modules cannot be loaded",
                addon.name
            ),
        ));
    }
    if addon.cls_name != "Executor" {
        return Err(invalid(
            &format!("{path}.cls_name"),
            "only Executor is supported",
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

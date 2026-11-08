//! 実行中の構造化イベントと保存用ログを同じ経路へ流す。
use super::TrialProgress;
use crate::{
    addons::{self, Chain, log::AddonLog},
    compat::{Error, Result},
    http::{HttpEvent, Side},
    models::Request,
    notification::Verbosity,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub seq: u64,
    pub total: usize,
    pub level: Verbosity,
    pub event: Event,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Started(Box<Request>),
    Http(HttpEvent),
    StageFinished {
        stage: Stage,
        elapsed: Duration,
    },
    Addon {
        stage: Stage,
        index: usize,
        name: String,
        log: AddonLog,
    },
    Judged(Box<TrialProgress>),
    ConnectionFailed(Box<TrialProgress>),
    StorageDecision {
        stored: bool,
    },
    Saved {
        side: Side,
        kind: SavedKind,
        path: PathBuf,
        bytes: usize,
        encoding: Option<String>,
    },
    Failed(Error),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Res2Res(Side),
    Res2Dict(Side),
    DiffDiagnosis,
    Judgement,
    StoreCriterion,
    Dump(Side),
    DidChallenge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedKind {
    Body,
    Props,
}
impl Progress {
    /// 既定・-vの結果行だけを取り出す。
    pub fn summary(&self) -> Option<&TrialProgress> {
        match &self.event {
            Event::Judged(result) | Event::ConnectionFailed(result) => Some(result),
            _ => None,
        }
    }
}
impl Event {
    fn level(&self) -> Verbosity {
        match self {
            Self::Http(event) => event.level(),
            Self::Judged(result) if result.status == crate::models::Status::Same => {
                Verbosity::Verbose
            }
            Self::Judged(_) | Self::ConnectionFailed(_) => Verbosity::Normal,
            Self::Addon { log, .. } => log.level(),
            Self::Failed(_) => Verbosity::Error,
            _ => Verbosity::VeryVerbose,
        }
    }
}
pub(super) type Observer = Box<dyn FnMut(&Progress) + Send>;
struct State {
    logs: Vec<Progress>,
    observer: Option<Observer>,
}
pub(super) struct Stream(Mutex<State>);
impl Stream {
    pub fn new(observer: Option<Observer>) -> Arc<Self> {
        Arc::new(Self(Mutex::new(State {
            logs: Vec::new(),
            observer,
        })))
    }
    pub fn take(&self) -> Vec<Progress> {
        std::mem::take(&mut self.0.lock().unwrap().logs)
    }
}
#[derive(Clone)]
pub(super) struct TrialEvents {
    seq: u64,
    total: usize,
    stream: Arc<Stream>,
}
impl TrialEvents {
    pub fn outcome(
        &self,
        request: &Request,
        status: crate::models::Status,
        one: &crate::models::ResponseSummary,
        other: &crate::models::ResponseSummary,
    ) {
        let summary = Box::new(TrialProgress {
            seq: self.seq,
            total: self.total,
            name: request.name.clone(),
            path: request.path.clone(),
            method: request.method,
            status,
            one: one.into(),
            other: other.into(),
        });
        self.emit(if status == crate::models::Status::Failure {
            Event::ConnectionFailed(summary)
        } else {
            Event::Judged(summary)
        });
    }
    pub fn new(seq: u64, total: usize, stream: Arc<Stream>) -> Self {
        Self { seq, total, stream }
    }
    pub fn silent(seq: u64) -> Self {
        Self::new(seq, 1, Stream::new(Some(Box::new(|_| {}))))
    }
    pub fn emit(&self, event: Event) {
        let event = Progress {
            seq: self.seq,
            total: self.total,
            level: event.level(),
            event,
        };
        // コールバックの並行呼び出しはしない。通知する場合は複製・保持をしない。
        let mut state = self.stream.0.lock().unwrap();
        if let Some(observer) = &mut state.observer {
            observer(&event);
        } else {
            state.logs.push(event);
        }
    }
    pub fn timed<T>(&self, stage: Stage, run: impl FnOnce() -> Result<T>) -> Result<T> {
        let start = Instant::now();
        let result = run()?;
        self.elapsed(stage, start);
        Ok(result)
    }
    pub fn elapsed(&self, stage: Stage, start: Instant) {
        self.emit(Event::StageFinished {
            stage,
            elapsed: start.elapsed(),
        });
    }
    pub fn chain<S: addons::Stage>(
        &self,
        stage: Stage,
        chain: &Chain<S>,
        payload: S::Payload,
        reference: &S::Reference,
    ) -> Result<S::Payload> {
        let side = match stage {
            Stage::Res2Res(side) | Stage::Res2Dict(side) | Stage::Dump(side) => Some(match side {
                Side::One => "one",
                Side::Other => "other",
            }),
            _ => None,
        };
        chain.exec_at(
            payload,
            reference,
            addons::exec::Invocation {
                trial_seq: Some(self.seq),
                side,
            },
            &|index, name, log| {
                self.emit(Event::Addon {
                    stage,
                    index,
                    name: name.into(),
                    log,
                });
            },
        )
    }
}

//! HTTPからTrial・レスポンス保存までの統合。Report集計とfinalは呼出元の責務。
mod progress;
use crate::{
    addons::{
        self, Chain, DidChallenge, Dump, Judgement, Res2Dict, Res2Res, StoreCriterion, payload::*,
    },
    compat::{Error, Result},
    config::{Config, HeaderJudgementSettings},
    http::{ChallengeOutcome, HttpTransport, ResponsePair, Side},
    models::{
        Concurrency, HttpMethod, OutputSummary, Request, Response, ResponseSummary, Status, Trial,
    },
    notification::{Failure, LoggedResult},
};
pub use progress::{Event, Progress, SavedKind, Stage};
use progress::{Stream, TrialEvents};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

/// processesは既定でtrialごとにCookieを分離する。threadsは常に共有する。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCookieScope {
    Shared,
    #[default]
    PerTrial,
}

/// CLIの進捗行の材料。出力・書式・verbosityの選択は呼出側が担う。
#[derive(Debug, Clone, PartialEq)]
pub struct TrialProgress {
    pub seq: u64,
    pub total: usize,
    /// Trial.nameのseq補完前の値。通信失敗の名前なしはNoneとして表示する。
    pub name: Option<String>,
    pub path: String,
    pub method: HttpMethod,
    pub status: Status,
    pub one: ProgressResponse,
    pub other: ProgressResponse,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressResponse {
    pub status_code: Option<u16>,
    pub response_sec: Option<f64>,
}
impl From<&ResponseSummary> for ProgressResponse {
    fn from(summary: &ResponseSummary) -> Self {
        Self {
            status_code: summary.status_code,
            response_sec: summary.response_sec,
        }
    }
}

#[derive(Debug)]
pub struct Execution {
    pub trials: Vec<Trial>,
    /// execでは全イベントを保持する。exec_with_progressでは保持せず空を返す。
    pub logs: Vec<Progress>,
    pub started_at: chrono::DateTime<chrono::FixedOffset>,
    pub ended_at: chrono::DateTime<chrono::FixedOffset>,
    pub concurrency: Concurrency,
    /// 後続のReport組み立てでsummary.comparisonへ保存する。
    pub comparison: HeaderJudgementSettings,
}

pub struct Pipeline {
    config: Config,
    runtime: Arc<addons::exec::Runtime>,
    res2res: Chain<Res2Res>,
    res2dict: Chain<Res2Dict>,
    judgement: Chain<Judgement>,
    store: Chain<StoreCriterion>,
    dump: Chain<Dump>,
    did: Chain<DidChallenge>,
}
impl Pipeline {
    pub fn with_runtime(mut self, runtime: Arc<addons::exec::Runtime>) -> Self {
        self.runtime = Arc::clone(&runtime);
        self.res2res = self.res2res.with_runtime(Arc::clone(&runtime));
        self.res2dict = self.res2dict.with_runtime(Arc::clone(&runtime));
        self.judgement = self.judgement.with_runtime(Arc::clone(&runtime));
        self.store = self.store.with_runtime(Arc::clone(&runtime));
        self.dump = self.dump.with_runtime(Arc::clone(&runtime));
        self.did = self.did.with_runtime(runtime);
        self
    }
    pub fn build(config: &Config) -> Result<Self> {
        config.concurrency()?;
        Ok(Self {
            config: config.clone(),
            runtime: addons::exec::Runtime::prepare(&config.addons, 0, |_| {})?,
            res2res: Chain::build(&config.addons, &addons::res2res::Factory)?,
            res2dict: Chain::build(&config.addons, &addons::res2dict::Factory)?,
            judgement: Chain::build(&config.addons, &addons::judgement::Factory)?,
            store: Chain::build(&config.addons, &addons::store_criterion::Factory)?,
            dump: Chain::build(&config.addons, &addons::dump::Factory)?,
            did: Chain::build(&config.addons, &addons::did_challenge::Factory)?,
        })
    }

    /// prepare済みRequestを入力順で返す。HTTPと同期アドオンを含めて指定数に制限する。
    pub async fn exec(
        self: &Arc<Self>,
        transport: Arc<HttpTransport>,
        requests: Vec<Request>,
        key: &str,
        process_cookies: Option<ProcessCookieScope>,
    ) -> LoggedResult<Execution, Progress> {
        self.exec_to(transport, requests, key, process_cookies, Stream::new(None))
            .await
    }

    /// イベントは発生時に通知する。コールバックはworker上で直列に呼び、logsには保持しない。
    /// コールバックは短時間で戻り、panicしないこと。channelへ送ってCLI側で整形してもよい。
    pub async fn exec_with_progress(
        self: &Arc<Self>,
        transport: Arc<HttpTransport>,
        requests: Vec<Request>,
        key: &str,
        process_cookies: Option<ProcessCookieScope>,
        observer: impl FnMut(&Progress) + Send + 'static,
    ) -> LoggedResult<Execution, Progress> {
        self.exec_to(
            transport,
            requests,
            key,
            process_cookies,
            Stream::new(Some(Box::new(observer))),
        )
        .await
    }

    async fn exec_to(
        self: &Arc<Self>,
        transport: Arc<HttpTransport>,
        requests: Vec<Request>,
        key: &str,
        process_cookies: Option<ProcessCookieScope>,
        logs: Arc<Stream>,
    ) -> LoggedResult<Execution, Progress> {
        self.runtime.check_cancelled()?;
        let concurrency = self.config.concurrency()?;
        let processes = self.config.processes.is_some_and(|n| n != 0);
        let isolated =
            processes && process_cookies.unwrap_or_default() == ProcessCookieScope::PerTrial;
        let limit = if processes {
            concurrency.processes
        } else {
            concurrency.threads
        } as usize;
        let output = self.config.output.clone();
        let key_owned = key.to_owned();
        let storage = Arc::new(
            tokio::task::spawn_blocking(move || Storage::create(&output, &key_owned))
                .await
                .map_err(join_error)??,
        );
        let started_at = chrono::Local::now().fixed_offset();
        let total = requests.len();
        let mut pending = requests.into_iter().enumerate();
        let mut running = tokio::task::JoinSet::new();
        let mut indices = HashMap::new();
        let mut completed = Vec::new();
        let mut stopped = false;
        loop {
            stopped |= self.runtime.check_cancelled().is_err();
            while !stopped && self.runtime.check_cancelled().is_ok() && running.len() < limit {
                let Some((index, request)) = pending.next() else {
                    break;
                };
                let pipeline = Arc::clone(self);
                let storage = Arc::clone(&storage);
                let transport = if isolated {
                    Arc::new(transport.isolated_cookies())
                } else {
                    Arc::clone(&transport)
                };
                let logs = Arc::clone(&logs);
                let seq = index as u64 + 1;
                let handle = running.spawn(async move {
                    let events = TrialEvents::new(seq, total, logs);
                    let result = pipeline
                        .challenge_with_log(&transport, seq, request, storage, events.clone())
                        .await;
                    if let Err(error) = &result {
                        events.emit(Event::Failed(error.clone()));
                    }
                    result
                });
                indices.insert(handle.id(), index);
            }
            let Some(result) = running.join_next_with_id().await else {
                break;
            };
            let (id, result) = match result {
                Ok(pair) => pair,
                Err(error) => (error.id(), Err(join_error(error))),
            };
            // abortしない。spawn_blocking内の保存とsleepまで終えてからエラーを返す。
            stopped |= result.is_err();
            completed.push((indices.remove(&id).unwrap(), result));
        }
        let ended_at = chrono::Local::now().fixed_offset();
        self.runtime.check_cancelled()?;
        let logs = logs.take();
        completed.sort_by_key(|(index, _)| *index);
        let trials = completed
            .into_iter()
            .map(|(_, result)| result)
            .collect::<Result<Vec<_>>>()
            .map_err(|error| Failure {
                error,
                logs: logs.clone(),
            })?;
        tokio::task::spawn_blocking(move || storage.update_latest())
            .await
            .map_err(join_error)
            .and_then(|result| result)
            .map_err(|error| Failure {
                error,
                logs: logs.clone(),
            })?;
        Ok(Execution {
            trials,
            logs,
            started_at,
            ended_at,
            concurrency,
            comparison: self.config.comparison(),
        })
    }

    pub async fn challenge(
        self: &Arc<Self>,
        transport: &HttpTransport,
        seq: u64,
        request: Request,
        storage: Arc<Storage>,
    ) -> Result<Trial> {
        self.challenge_with_log(transport, seq, request, storage, TrialEvents::silent(seq))
            .await
    }

    async fn challenge_with_log(
        self: &Arc<Self>,
        transport: &HttpTransport,
        seq: u64,
        request: Request,
        storage: Arc<Storage>,
        events: TrialEvents,
    ) -> Result<Trial> {
        events.emit(Event::Started(Box::new(request.clone())));
        self.runtime.check_cancelled()?;
        let observe = |event| events.emit(Event::Http(event));
        let outcome = tokio::select! {
            outcome = transport.challenge_with_events(seq,&request,&observe) => outcome?,
            error = self.runtime.interrupted() => return Err(error),
        };
        match outcome {
            ChallengeOutcome::Failure(trial) => {
                events.outcome(&request, trial.status, &trial.one, &trial.other);
                Ok(*trial)
            }
            ChallengeOutcome::Responses(pair) => {
                let pipeline = Arc::clone(self);
                tokio::task::spawn_blocking(move || {
                    pipeline.finish_with_log(seq, request, *pair, &storage, &events)
                })
                .await
                .map_err(join_error)?
            }
        }
    }

    /// HTTP実測を注入できる同期入口。ファイルI/O・sleepを含むためasyncからはspawn_blockingで呼ぶ。
    pub fn finish(
        &self,
        seq: u64,
        request: Request,
        pair: ResponsePair,
        storage: &Storage,
    ) -> Result<Trial> {
        self.finish_with_log(seq, request, pair, storage, &TrialEvents::silent(seq))
    }

    fn finish_with_log(
        &self,
        seq: u64,
        request: Request,
        pair: ResponsePair,
        storage: &Storage,
        events: &TrialEvents,
    ) -> Result<Trial> {
        let name = request.name.clone().unwrap_or_else(|| seq.to_string());
        let start = Instant::now();
        let one = events.chain(
            Stage::Res2Res(Side::One),
            &self.res2res,
            Res2ResPayload {
                response: pair.one,
                req: request.clone(),
                tags: Vec::new(),
            },
            &(),
        )?;
        events.elapsed(Stage::Res2Res(Side::One), start);
        let start = Instant::now();
        let other = events.chain(
            Stage::Res2Res(Side::Other),
            &self.res2res,
            Res2ResPayload {
                response: pair.other,
                req: request.clone(),
                tags: Vec::new(),
            },
            &(),
        )?;
        events.elapsed(Stage::Res2Res(Side::Other), start);
        let start = Instant::now();
        let dict_one = events
            .chain(
                Stage::Res2Dict(Side::One),
                &self.res2dict,
                Res2DictPayload {
                    response: one.response.clone(),
                    result: None,
                },
                &(),
            )?
            .result;
        events.elapsed(Stage::Res2Dict(Side::One), start);
        let start = Instant::now();
        let dict_other = events
            .chain(
                Stage::Res2Dict(Side::Other),
                &self.res2dict,
                Res2DictPayload {
                    response: other.response.clone(),
                    result: None,
                },
                &(),
            )?
            .result;
        events.elapsed(Stage::Res2Dict(Side::Other), start);
        let reference = JudgementReference {
            name: name.clone(),
            path: request.path.clone(),
            qs: request.qs.clone(),
            headers: request.headers.clone(),
            res_one: one.response.clone(),
            res_other: other.response.clone(),
            dict_one: dict_one.clone(),
            dict_other: dict_other.clone(),
        };
        let diffs = events.timed(Stage::DiffDiagnosis, || {
            addons::judgement::diagnose_diff(&reference)
        })?;
        let judged = events.timed(Stage::Judgement, || {
            let initial = addons::judgement::payload_from_diff(
                &reference,
                self.config.judge_response_header,
                &self.config.ignore_response_header_keys,
                diffs,
            );
            events.chain(Stage::Judgement, &self.judgement, initial, &reference)
        })?;
        let status = if judged.regard_as_same() {
            Status::Same
        } else {
            Status::Different
        };
        let mut summary_one = summary(&one.response, self.config.judge_response_header);
        let mut summary_other = summary(&other.response, self.config.judge_response_header);
        events.outcome(&request, status, &summary_one, &summary_other);
        let store_start = Instant::now();
        let store_req = Request {
            name: Some(name.clone()),
            path: request.path.clone(),
            qs: request.qs.clone(),
            headers: request.headers.clone(),
            ..Request::default()
        };
        let stored = events
            .chain(
                Stage::StoreCriterion,
                &self.store,
                StoreCriterionPayload { stored: false },
                &StoreCriterionReference {
                    status,
                    req: store_req,
                    res_one: one.response.clone(),
                    res_other: other.response.clone(),
                },
            )?
            .stored;
        events.emit(Event::StorageDecision { stored });
        if stored {
            summary_one.file = Some(response_file("one", seq, &name));
            summary_other.file = Some(response_file("other", seq, &name));
            // Pythonと同じくoneのdump・書込を終えてからotherへ進む。
            self.save_response(
                storage,
                summary_one.file.as_deref().unwrap(),
                &one.response,
                Side::One,
                events,
            )?;
            self.save_response(
                storage,
                summary_other.file.as_deref().unwrap(),
                &other.response,
                Side::Other,
                events,
            )?;
            for (side, label, props, summary) in [
                (Side::One, "one", &dict_one, &mut summary_one),
                (Side::Other, "other", &dict_other, &mut summary_other),
            ] {
                if let Some(props) = props {
                    let file = prop_file(label, seq, &name);
                    let body = prop_bytes(props)?;
                    storage.write(&file, &body)?;
                    events.emit(Event::Saved {
                        side,
                        kind: SavedKind::Props,
                        path: storage.root.join(&file),
                        bytes: body.len(),
                        encoding: Some("utf-8".into()),
                    });
                    summary.prop_file = Some(file);
                }
            }
        }
        events.elapsed(Stage::StoreCriterion, store_start);
        let mut tags = one.tags;
        tags.extend(other.tags);
        let mut unique = Vec::new();
        for tag in tags {
            if !unique.contains(&tag) {
                unique.push(tag);
            }
        }
        let trial = Trial {
            seq,
            name,
            tags: unique,
            headers: request.headers,
            queries: request.qs,
            raw: request.raw,
            form: request.form,
            json: request.json,
            method: request.method,
            path: request.path,
            one: summary_one,
            other: summary_other,
            request_time: pair.request_time,
            status,
            diffs_by_cognition: judged.diffs_by_cognition,
        };
        Ok(events
            .timed(Stage::DidChallenge, || {
                events.chain(
                    Stage::DidChallenge,
                    &self.did,
                    DidChallengePayload { trial },
                    &DidChallengeReference {
                        res_one: one.response,
                        res_other: other.response,
                        res_one_props: dict_one,
                        res_other_props: dict_other,
                    },
                )
            })?
            .trial)
    }

    fn save_response(
        &self,
        storage: &Storage,
        file: &str,
        response: &Response,
        side: Side,
        events: &TrialEvents,
    ) -> Result<()> {
        let dumped = events.chain(
            Stage::Dump(side),
            &self.dump,
            DumpPayload {
                response: response.clone(),
                body: response.body.to_vec(),
                encoding: response.encoding.clone(),
            },
            &(),
        )?;
        storage.write(file, &dumped.body)?;
        events.emit(Event::Saved {
            side,
            kind: SavedKind::Body,
            path: storage.root.join(file),
            bytes: dumped.body.len(),
            encoding: dumped.encoding,
        });
        Ok(())
    }
}

fn join_error(error: tokio::task::JoinError) -> Error {
    Error::new("RuntimeError", error.to_string())
}
fn io_error(error: std::io::Error, path: &Path) -> Error {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::AlreadyExists => "FileExistsError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        std::io::ErrorKind::NotADirectory => "NotADirectoryError",
        std::io::ErrorKind::IsADirectory => "IsADirectoryError",
        _ => "IOError",
    };
    Error::new(kind, format!("{error}: '{}'", path.display()))
}

fn summary(response: &Response, headers: bool) -> ResponseSummary {
    let content_type = response.content_type().map(str::to_owned);
    ResponseSummary {
        url: response.url.clone(),
        response_type: response.response_type.clone(),
        status_code: Some(response.status_code),
        byte: Some(response.body.len() as u64),
        response_sec: Some(response.elapsed_sec),
        mime_type: content_type
            .as_ref()
            .map(|s| s.split(';').next().unwrap().to_owned()),
        content_type,
        encoding: response.encoding.clone(),
        file: None,
        prop_file: None,
        response_header: headers.then(|| {
            response
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect()
        }),
    }
}

pub fn response_file(side: &str, seq: u64, name: &str) -> String {
    format!("{side}/({seq}){name}")
}
pub fn prop_file(side: &str, seq: u64, name: &str) -> String {
    format!("{side}-props/({seq}){name}.json")
}

/// Python to_jsonのnull除外・ソート・区切り・数値表記。propはoutput.encodingに関係なくUTF-8。
pub fn prop_bytes(value: &serde_json::Value) -> Result<Vec<u8>> {
    if !value.is_object() && !value.is_array() {
        return Err(Error::new("TypeError", "A argument must be dict or list"));
    }
    let depth = crate::compat::json::validate(value)?;
    crate::compat::json::with_stack(depth, || {
        fn remove_null(value: &serde_json::Value) -> serde_json::Value {
            match value {
                serde_json::Value::Object(map) => serde_json::Value::Object(
                    map.iter()
                        .filter(|(_, v)| !v.is_null())
                        .map(|(k, v)| (k.clone(), remove_null(v)))
                        .collect(),
                ),
                serde_json::Value::Array(list) => serde_json::Value::Array(
                    list.iter()
                        .filter(|v| !v.is_null())
                        .map(remove_null)
                        .collect(),
                ),
                value => value.clone(),
            }
        }
        let text = crate::compat::json::dumps(&remove_null(value), None, true)?;
        // output.rsと同じstrict codec経路。propだけは固定UTF-8。
        let output = OutputSummary {
            response_dir: String::new(),
            encoding: "utf-8".into(),
            logger: None,
        };
        let mut writer = output.writer(Vec::new())?;
        writer.write_text(&text)?;
        Ok(writer.into_inner())
    })
}

pub fn hash_from_args(args: &str) -> String {
    hash_from_args_at(args, chrono::Local::now().fixed_offset())
}
pub fn hash_from_args_at(args: &str, now: chrono::DateTime<chrono::FixedOffset>) -> String {
    let timestamp = if now.timestamp_subsec_micros() == 0 {
        now.format("%Y-%m-%d %H:%M:%S%:z").to_string()
    } else {
        now.format("%Y-%m-%d %H:%M:%S%.6f%:z").to_string()
    };
    ring::digest::digest(
        &ring::digest::SHA256,
        format!("{timestamp}{args}").as_bytes(),
    )
    .as_ref()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect()
}

pub struct Storage {
    root: PathBuf,
    key: String,
}
impl Storage {
    /// keyディレクトリは再使用できる。既存の保存用サブディレクトリはエラーとする。
    pub fn create(output: &OutputSummary, key: &str) -> Result<Self> {
        if key.is_empty()
            || Path::new(key).components().count() != 1
            || !matches!(
                Path::new(key).components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(Error::new("ValueError", "key must be one directory name"));
        }
        let root = Path::new(&output.response_dir).join(key);
        for side in ["one", "other", "one-props", "other-props"] {
            let path = root.join(side);
            match std::fs::create_dir(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(&root).map_err(|error| io_error(error, &path))?;
                    std::fs::create_dir(&path).map_err(|error| io_error(error, &path))?;
                }
                result => result.map_err(|error| io_error(error, &path))?,
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o777))
                    .map_err(|error| io_error(error, &path))?;
            }
        }
        Ok(Self {
            root,
            key: key.into(),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn write(&self, file: &str, body: &[u8]) -> Result<()> {
        let path = self.root.join(file);
        std::fs::write(&path, body).map_err(|error| io_error(error, &path))
    }
    pub fn update_latest(&self) -> Result<()> {
        let latest = self.root.parent().unwrap().join("latest");
        match std::fs::symlink_metadata(&latest) {
            Ok(_) => std::fs::remove_file(&latest).map_err(|error| io_error(error, &latest))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error, &latest)),
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&self.key, &latest).map_err(|error| io_error(error, &latest))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&self.key, &latest)
            .map_err(|error| io_error(error, &latest))?;
        Ok(())
    }
}

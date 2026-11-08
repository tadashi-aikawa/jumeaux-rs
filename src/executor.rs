//! HTTPからTrial・レスポンス保存までの統合。Report集計とfinalは呼出元の責務。
use crate::{
    addons::{
        self, Chain, DidChallenge, Dump, Judgement, Res2Dict, Res2Res, StoreCriterion, payload::*,
    },
    compat::{Error, Result},
    config::{Config, HeaderJudgementSettings},
    http::{ChallengeOutcome, HttpTransport, ResponsePair},
    models::{Concurrency, OutputSummary, Request, Response, ResponseSummary, Status, Trial},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// 未決のprocesses時Cookie契約を呼出元が明示する。暗黙の既定値は設けない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCookieScope {
    Shared,
    PerTrial,
}

pub struct Execution {
    pub trials: Vec<Trial>,
    pub concurrency: Concurrency,
    /// 後続のReport組み立てでsummary.comparisonへ保存する。
    pub comparison: HeaderJudgementSettings,
}

pub struct Pipeline {
    config: Config,
    res2res: Chain<Res2Res>,
    res2dict: Chain<Res2Dict>,
    judgement: Chain<Judgement>,
    store: Chain<StoreCriterion>,
    dump: Chain<Dump>,
    did: Chain<DidChallenge>,
}
impl Pipeline {
    pub fn build(config: &Config) -> Result<Self> {
        config.concurrency()?;
        Ok(Self {
            config: config.clone(),
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
    ) -> Result<Execution> {
        let concurrency = self.config.concurrency()?;
        let processes = self.config.processes.is_some_and(|n| n != 0);
        let isolated = if processes {
            process_cookies.ok_or_else(|| {
                Error::new(
                    "ValueError",
                    "processes: Cookie scope is undecided; select Shared or PerTrial explicitly",
                )
            })? == ProcessCookieScope::PerTrial
        } else {
            false
        };
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
        let mut pending = requests.into_iter().enumerate();
        let mut running = tokio::task::JoinSet::new();
        let mut completed = Vec::new();
        loop {
            while running.len() < limit {
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
                running.spawn(async move {
                    (
                        index,
                        pipeline
                            .challenge(&transport, index as u64 + 1, request, storage)
                            .await,
                    )
                });
            }
            let Some(result) = running.join_next().await else {
                break;
            };
            completed.push(result.map_err(join_error)?);
        }
        completed.sort_by_key(|(index, _)| *index);
        let trials = completed
            .into_iter()
            .map(|(_, result)| result)
            .collect::<Result<Vec<_>>>()?;
        tokio::task::spawn_blocking(move || storage.update_latest())
            .await
            .map_err(join_error)??;
        Ok(Execution {
            trials,
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
        match transport.challenge(seq, &request).await? {
            ChallengeOutcome::Failure(trial) => Ok(*trial),
            ChallengeOutcome::Responses(pair) => {
                let pipeline = Arc::clone(self);
                tokio::task::spawn_blocking(move || pipeline.finish(seq, request, *pair, &storage))
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
        let name = request.name.clone().unwrap_or_else(|| seq.to_string());
        let one = self.res2res.exec(
            Res2ResPayload {
                response: pair.one,
                req: request.clone(),
                tags: Vec::new(),
            },
            &(),
        )?;
        let other = self.res2res.exec(
            Res2ResPayload {
                response: pair.other,
                req: request.clone(),
                tags: Vec::new(),
            },
            &(),
        )?;
        let dict_one = self
            .res2dict
            .exec(
                Res2DictPayload {
                    response: one.response.clone(),
                    result: None,
                },
                &(),
            )?
            .result;
        let dict_other = self
            .res2dict
            .exec(
                Res2DictPayload {
                    response: other.response.clone(),
                    result: None,
                },
                &(),
            )?
            .result;
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
        let initial = addons::judgement::initial_payload(
            &reference,
            self.config.judge_response_header,
            &self.config.ignore_response_header_keys,
        )?;
        let judged = self.judgement.exec(initial, &reference)?;
        let status = if judged.regard_as_same() {
            Status::Same
        } else {
            Status::Different
        };
        let store_req = Request {
            name: Some(name.clone()),
            path: request.path.clone(),
            qs: request.qs.clone(),
            headers: request.headers.clone(),
            ..Request::default()
        };
        let stored = self
            .store
            .exec(
                StoreCriterionPayload { stored: false },
                &StoreCriterionReference {
                    status,
                    req: store_req,
                    res_one: one.response.clone(),
                    res_other: other.response.clone(),
                },
            )?
            .stored;
        let mut summary_one = summary(&one.response, self.config.judge_response_header);
        let mut summary_other = summary(&other.response, self.config.judge_response_header);
        if stored {
            summary_one.file = Some(response_file("one", seq, &name));
            summary_other.file = Some(response_file("other", seq, &name));
            // Pythonと同じくoneのdump・書込を終えてからotherへ進む。
            self.save_response(storage, summary_one.file.as_deref().unwrap(), &one.response)?;
            self.save_response(
                storage,
                summary_other.file.as_deref().unwrap(),
                &other.response,
            )?;
            for (side, props, summary) in [
                ("one", &dict_one, &mut summary_one),
                ("other", &dict_other, &mut summary_other),
            ] {
                if let Some(props) = props {
                    let file = prop_file(side, seq, &name);
                    storage.write(&file, &prop_bytes(props)?)?;
                    summary.prop_file = Some(file);
                }
            }
        }
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
        Ok(self
            .did
            .exec(
                DidChallengePayload { trial },
                &DidChallengeReference {
                    res_one: one.response,
                    res_other: other.response,
                    res_one_props: dict_one,
                    res_other_props: dict_other,
                },
            )?
            .trial)
    }

    fn save_response(&self, storage: &Storage, file: &str, response: &Response) -> Result<()> {
        let dumped = self.dump.exec(
            DumpPayload {
                response: response.clone(),
                body: response.body.to_vec(),
                encoding: response.encoding.clone(),
            },
            &(),
        )?;
        storage.write(file, &dumped.body)
    }
}

fn join_error(error: tokio::task::JoinError) -> Error {
    Error::new("RuntimeError", error.to_string())
}
fn io_error(error: std::io::Error) -> Error {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => "FileNotFoundError",
        std::io::ErrorKind::AlreadyExists => "FileExistsError",
        std::io::ErrorKind::PermissionDenied => "PermissionError",
        _ => "IOError",
    };
    Error::new(kind, error.to_string())
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
    /// Python同様、既存実行ディレクトリの再使用はエラーとする。
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
        std::fs::create_dir_all(&output.response_dir).map_err(io_error)?;
        std::fs::create_dir(&root).map_err(io_error)?;
        for side in ["one", "other", "one-props", "other-props"] {
            let path = root.join(side);
            std::fs::create_dir(&path).map_err(io_error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o777))
                    .map_err(io_error)?;
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
        std::fs::write(self.root.join(file), body).map_err(io_error)
    }
    pub fn update_latest(&self) -> Result<()> {
        let latest = self.root.parent().unwrap().join("latest");
        match std::fs::symlink_metadata(&latest) {
            Ok(_) => std::fs::remove_file(&latest).map_err(io_error)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error)),
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&self.key, latest).map_err(io_error)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&self.key, latest).map_err(io_error)?;
        Ok(())
    }
}

//! Python requestsとのHTTP互換境界。アドオン・レポート保存は呼出元の責務。
mod request;
mod transport;
pub use request::{PreparedRequest, json_body, merge_headers, prepare, query_string};
pub use transport::{HttpTransport, TransportOptions};

use crate::{
    compat::{Error, Result},
    models::{Request, Response, ResponseSummary, Status, Trial},
};

#[derive(Debug)]
pub enum ChallengeOutcome {
    Responses(Box<ResponsePair>),
    Failure(Box<Trial>),
}

#[derive(Debug)]
pub struct ResponsePair {
    pub one: Response,
    pub other: Response,
    pub request_time: String,
}

pub fn is_connection_error(error: &Error) -> bool {
    matches!(
        error.kind,
        "ConnectionError" | "ConnectTimeout" | "ProxyError" | "SSLError"
    )
}

fn failure_summary(url: String) -> ResponseSummary {
    ResponseSummary {
        url,
        response_type: "unknown".into(),
        status_code: None,
        byte: None,
        response_sec: None,
        content_type: None,
        mime_type: None,
        encoding: None,
        file: None,
        prop_file: None,
        response_header: None,
    }
}

impl HttpTransport {
    /// 両側の通信完了を待つ。片側が成功してもConnectionErrorなら両側summaryを省略する。
    pub async fn challenge(&self, seq: u64, request: &Request) -> Result<ChallengeOutcome> {
        let one = prepare(&self.one.access, request)?;
        let other = prepare(&self.other.access, request)?;
        let url_one = one.original_url.clone();
        let url_other = other.original_url.clone();
        let request_time = chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%.6f")
            .to_string();
        let (r_one, r_other) = tokio::join!(
            self.send_prepared(&self.one, one),
            self.send_prepared(&self.other, other)
        );
        match r_one.and_then(|a| r_other.map(|b| (a, b))) {
            Ok((one, other)) => Ok(ChallengeOutcome::Responses(Box::new(ResponsePair {
                one: one.into_response(self.one.access.default_response_encoding.as_deref())?,
                other: other
                    .into_response(self.other.access.default_response_encoding.as_deref())?,
                request_time,
            }))),
            Err(error) if is_connection_error(&error) => {
                Ok(ChallengeOutcome::Failure(Box::new(Trial {
                    seq,
                    name: request.name.clone().unwrap_or_else(|| seq.to_string()),
                    tags: Vec::new(),
                    headers: request.headers.clone(),
                    queries: request.qs.clone(),
                    raw: request.raw.clone(),
                    form: request.form.clone(),
                    json: request.json.clone(),
                    one: failure_summary(url_one),
                    other: failure_summary(url_other),
                    method: request.method,
                    path: request.path.clone(),
                    request_time,
                    status: Status::Failure,
                    diffs_by_cognition: None,
                })))
            }
            Err(error) => Err(error),
        }
    }

    pub async fn concurrent_request(&self, request: &Request) -> Result<(Response, Response)> {
        let one = prepare(&self.one.access, request)?;
        let other = prepare(&self.other.access, request)?;
        let (one, other) = tokio::join!(
            self.send_prepared(&self.one, one),
            self.send_prepared(&self.other, other)
        );
        let (one, other) = (one?, other?);
        Ok((
            one.into_response(self.one.access.default_response_encoding.as_deref())?,
            other.into_response(self.other.access.default_response_encoding.as_deref())?,
        ))
    }
}

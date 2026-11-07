//! Pythonの9レイヤーに対応するpayloadと読み取り専用reference。
use crate::models::*;
use indexmap::IndexMap;
use serde_json::Value;
#[derive(Debug, Clone)]
pub struct Log2ReqsPayload {
    pub file: String,
    pub requests: Vec<Request>,
}
#[derive(Debug, Clone)]
pub struct Reqs2ReqsPayload {
    pub requests: Vec<Request>,
}
#[derive(Debug, Clone)]
pub struct Res2ResPayload {
    pub response: Response,
    pub req: Request,
    pub tags: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct Res2DictPayload {
    pub response: Response,
    pub result: Option<Value>,
}
#[derive(Debug, Clone)]
pub struct DumpPayload {
    pub response: Response,
    pub body: Vec<u8>,
    pub encoding: Option<String>,
}
#[derive(Debug, Clone)]
pub struct JudgementPayload {
    pub diffs_by_cognition: Option<CognitionDiffs>,
    pub regard_as_same_body: bool,
    pub regard_as_same_header: bool,
}
impl JudgementPayload {
    pub fn regard_as_same(&self) -> bool {
        self.regard_as_same_body && self.regard_as_same_header
    }
}
#[derive(Debug, Clone)]
pub struct JudgementReference {
    pub name: String,
    pub path: String,
    pub qs: Queries,
    pub headers: Headers,
    pub res_one: Response,
    pub res_other: Response,
    pub dict_one: Option<Value>,
    pub dict_other: Option<Value>,
}
#[derive(Debug, Clone)]
pub struct StoreCriterionPayload {
    pub stored: bool,
}
#[derive(Debug, Clone)]
pub struct StoreCriterionReference {
    pub status: Status,
    pub req: Request,
    pub res_one: Response,
    pub res_other: Response,
}
#[derive(Debug, Clone)]
pub struct DidChallengePayload {
    pub trial: Trial,
}
#[derive(Debug, Clone)]
pub struct DidChallengeReference {
    pub res_one: Response,
    pub res_other: Response,
    pub res_one_props: Option<Value>,
    pub res_other_props: Option<Value>,
}
#[derive(Debug, Clone)]
pub struct FinalPayload {
    pub report: Report,
    pub output_summary: OutputSummary,
}
#[derive(Debug, Clone)]
pub struct FinalReference {
    pub notifiers: Option<IndexMap<String, Notifier>>,
}

//! Python addons/log2reqs/plain.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub encoding: String,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub keep_blank: bool,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub candidate_for_url_encodings: Vec<String>,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());

use crate::{
    compat::{
        Result,
        encoding::{self, Errors},
    },
    models::Request,
};
impl Config {
    pub fn read(&self, body: &[u8]) -> Result<Vec<Request>> {
        plain_requests(
            body,
            &self.encoding,
            &self.candidate_for_url_encodings,
            self.keep_blank,
        )
    }
}
pub fn plain_requests(
    body: &[u8],
    codec: &str,
    candidates: &[String],
    keep_blank: bool,
) -> Result<Vec<Request>> {
    let text = encoding::decode_input(body, codec)?;
    text.split_terminator('\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            // Pythonは空行だけを除き、空白のみの行はrstrip後にpath=""として残す。
            let mut pieces = line
                .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
                .split('?');
            let path = pieces.next().unwrap_or("").to_string();
            let query = pieces.next();
            let url_encoding = match query {
                Some(query) => encoding::guess_url_encoding(query, candidates)?
                    .unwrap_or_else(|| "utf-8".into()),
                None => "utf-8".into(),
            };
            let qs = encoding::parse_query(
                query.unwrap_or(""),
                &url_encoding,
                keep_blank,
                Errors::Replace,
            )?;
            Ok(Request {
                path,
                qs,
                url_encoding,
                ..Request::default()
            })
        })
        .collect()
}

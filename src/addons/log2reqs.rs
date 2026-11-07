//! plain入力を正式Requestへ変換する。codecとquery解釈はcompatの責務。
use crate::{
    compat::{
        Result,
        encoding::{self, Errors},
    },
    models::Request,
};
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
            let line = line.trim_end();
            let mut pieces = line.split('?');
            let path = pieces.next().unwrap_or("").to_string();
            let query = pieces.next().unwrap_or("");
            let url_encoding =
                encoding::guess_url_encoding(query, candidates)?.unwrap_or_else(|| "utf-8".into());
            let qs = encoding::parse_query(query, &url_encoding, keep_blank, Errors::Replace)?;
            Ok(Request {
                path,
                qs,
                url_encoding,
                ..Request::default()
            })
        })
        .collect()
}

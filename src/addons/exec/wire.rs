//! 保存用serdeのnull除外を通信へ持ち込まない。モデル境界だけを明示変換する。
use super::json::protocol;
use crate::{addons::payload::*, compat::Result, config::Config, models::*};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

pub trait Wire: Serialize + DeserializeOwned {
    fn wire(&self) -> Result<Value> {
        value(self)
    }
    fn from_wire(value: Value) -> Result<Self> {
        decode(value)
    }
}
fn value<T: Serialize + ?Sized>(v: &T) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| protocol("$: cannot encode model"))
}
pub fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_path_to_error::deserialize(&value)
        .map_err(|e| protocol(format!("$.{}: {}", e.path(), reason(e.inner()))))
}
fn reason(e: &serde_json::Error) -> String {
    let text = e.to_string();
    // 不正な値そのものは転載せず、serdeが示す期待型と必須フィールドだけを残す。
    if text.starts_with("missing field") {
        text
    } else if text.starts_with("unknown field") {
        "unexpected model field".into()
    } else if let Some((_, expected)) = text.rsplit_once(", expected ") {
        format!("expected {expected}")
    } else {
        "expected declared model fields and types".into()
    }
}
fn nulls(v: &mut Value, fields: &[&str]) {
    if let Some(m) = v.as_object_mut() {
        for key in fields {
            m.entry((*key).to_string()).or_insert(Value::Null);
        }
    }
}
fn finite(v: f64, path: &str) -> Result<()> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(protocol(format!("{path}: expected finite number")))
    }
}
pub fn bytes(v: &[u8]) -> Value {
    json!(STANDARD.encode(v))
}
fn decode_bytes(v: &mut Value, path: &str) -> Result<()> {
    let text = v
        .as_str()
        .ok_or_else(|| protocol(format!("{path}: expected padded Base64 string")))?;
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| protocol(format!("{path}: expected padded Base64 string")))?;
    *v = json!(bytes);
    Ok(())
}
fn decode_response(v: &mut Value, path: &str) -> Result<()> {
    let body = v
        .get_mut("body")
        .ok_or_else(|| protocol(format!("{path}.body: expected Base64 string")))?;
    decode_bytes(body, &format!("{path}.body"))
}
fn field<'a>(v: &'a mut Value, key: &str) -> Result<&'a mut Value> {
    v.get_mut(key)
        .ok_or_else(|| protocol(format!("{key}: expected required field on object")))
}
impl Wire for () {}
impl Wire for Request {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"name":self.name,"method":self.method,"path":self.path,"qs":self.qs,"raw":self.raw,
            "form":self.form,"json":self.json,"headers":self.headers,"url_encoding":self.url_encoding}),
        )
    }
}
impl Wire for Response {
    fn wire(&self) -> Result<Value> {
        finite(self.elapsed, "response.elapsed")?;
        finite(self.elapsed_sec, "response.elapsed_sec")?;
        Ok(
            json!({"body":bytes(&self.body),"encoding":self.encoding,"headers":self.headers,"url":self.url,
            "status_code":self.status_code,"elapsed":self.elapsed,"elapsed_sec":self.elapsed_sec,"type":self.response_type}),
        )
    }
    fn from_wire(mut v: Value) -> Result<Self> {
        decode_response(&mut v, "response")?;
        decode(v)
    }
}
impl Wire for OutputSummary {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"response_dir":self.response_dir,"encoding":self.encoding,"logger":null}))
    }
}
impl Wire for ResponseSummary {
    fn wire(&self) -> Result<Value> {
        if let Some(n) = self.response_sec {
            finite(n, "response_sec")?;
        }
        let mut v = value(self)?;
        nulls(
            &mut v,
            &[
                "status_code",
                "byte",
                "response_sec",
                "content_type",
                "mime_type",
                "encoding",
                "file",
                "prop_file",
                "response_header",
            ],
        );
        v["response_header"] = json!(self.response_header);
        Ok(v)
    }
}
impl Wire for Trial {
    fn wire(&self) -> Result<Value> {
        let mut v = value(self)?;
        v["raw"] = json!(self.raw);
        v["form"] = json!(self.form);
        v["json"] = json!(self.json);
        v["diffs_by_cognition"] = json!(self.diffs_by_cognition);
        v["one"] = self.one.wire()?;
        v["other"] = self.other.wire()?;
        Ok(v)
    }
}
impl Wire for AccessPoint {
    fn wire(&self) -> Result<Value> {
        let mut v = value(self)?;
        nulls(
            &mut v,
            &["path", "query", "proxy", "default_response_encoding"],
        );
        nulls(&mut v["query"], &["overwrite", "remove"]);
        Ok(v)
    }
}
impl Wire for Notifier {
    fn wire(&self) -> Result<Value> {
        let mut v = value(self)?;
        nulls(&mut v, &["channel", "icon_emoji", "icon_url"]);
        Ok(v)
    }
}
fn notifiers(n: &Option<indexmap::IndexMap<String, Notifier>>) -> Result<Value> {
    match n {
        None => Ok(Value::Null),
        Some(n) => n
            .iter()
            .map(|(k, v)| Ok((k.clone(), v.wire()?)))
            .collect::<Result<serde_json::Map<_, _>>>()
            .map(Value::Object),
    }
}
impl Wire for Addon {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"name":self.name,"cls_name":self.cls_name,"config":self.config,"include":self.include,"tags":self.tags}),
        )
    }
}
impl Wire for Addons {
    fn wire(&self) -> Result<Value> {
        let mut v = Value::Object(Default::default());
        for layer in crate::addons::Layer::ALL {
            let list = layer
                .configured(self)
                .iter()
                .map(Wire::wire)
                .collect::<Result<Vec<_>>>()?;
            v[layer.name()] = if layer == crate::addons::Layer::Log2Reqs {
                list.into_iter().next().unwrap()
            } else {
                Value::Array(list)
            };
        }
        Ok(v)
    }
}
impl Wire for Summary {
    fn wire(&self) -> Result<Value> {
        let mut v = value(self)?;
        nulls(&mut v, &["default_encoding", "comparison"]);
        v["one"] = self.one.wire()?;
        v["other"] = self.other.wire()?;
        v["output"] = self.output.wire()?;
        Ok(v)
    }
}
impl Wire for Report {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"version":self.version,"key":self.key,"title":self.title,"description":self.description,
            "notifiers":notifiers(&self.notifiers)?,"summary":self.summary.wire()?,
            "trials":self.trials.iter().map(Wire::wire).collect::<Result<Vec<_>>>()?,
            "addons":self.addons.as_ref().map(Wire::wire).transpose()?,"retry_hash":self.retry_hash}),
        )
    }
    fn from_wire(v: Value) -> Result<Self> {
        let (r, warnings) = Report::from_value_with_warnings(v)
            .map_err(|_| protocol("report: expected valid Report"))?;
        if !warnings.is_empty() {
            return Err(protocol(format!(
                "{}: unexpected model field",
                warnings[0].split(':').next().unwrap_or("report")
            )));
        }
        Ok(r)
    }
}
impl Wire for Config {
    fn wire(&self) -> Result<Value> {
        let mut v = value(self)?;
        nulls(
            &mut v,
            &[
                "processes",
                "title",
                "description",
                "tags",
                "input_files",
                "notifiers",
            ],
        );
        v["one"] = self.one.wire()?;
        v["other"] = self.other.wire()?;
        v["output"] = self.output.wire()?;
        v["notifiers"] = notifiers(&self.notifiers)?;
        v["addons"] = self.addons.wire()?;
        Ok(v)
    }
}
impl Wire for Log2ReqsPayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"file":self.file}))
    }
    fn from_wire(v: Value) -> Result<Self> {
        Ok(Self {
            file: String::new(),
            requests: decode(v)?,
        })
    }
}
impl Wire for Reqs2ReqsPayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"requests":self.requests.iter().map(Wire::wire).collect::<Result<Vec<_>>>()?}))
    }
}
impl Wire for Res2ResPayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"response":self.response.wire()?,"req":self.req.wire()?,"tags":self.tags}))
    }
    fn from_wire(mut v: Value) -> Result<Self> {
        decode_response(field(&mut v, "response")?, "response")?;
        decode(v)
    }
}
impl Wire for Res2DictPayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"response":self.response.wire()?,"result":self.result}))
    }
    fn from_wire(mut v: Value) -> Result<Self> {
        if !matches!(
            v.get("result"),
            None | Some(Value::Null | Value::Object(_) | Value::Array(_))
        ) {
            return Err(protocol("result: expected object/array/null"));
        }
        decode_response(field(&mut v, "response")?, "response")?;
        decode(v)
    }
}
impl Wire for DumpPayload {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"response":self.response.wire()?,"body":bytes(&self.body),"encoding":self.encoding}),
        )
    }
    fn from_wire(mut v: Value) -> Result<Self> {
        decode_response(field(&mut v, "response")?, "response")?;
        decode_bytes(field(&mut v, "body")?, "body")?;
        decode(v)
    }
}
impl Wire for JudgementPayload {}
impl Wire for StoreCriterionPayload {}
impl Wire for JudgementReference {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"name":self.name,"path":self.path,"qs":self.qs,"headers":self.headers,
        "res_one":self.res_one.wire()?,"res_other":self.res_other.wire()?,"dict_one":self.dict_one,"dict_other":self.dict_other}),
        )
    }
}
impl Wire for StoreCriterionReference {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"status":self.status,"req":self.req.wire()?,"res_one":self.res_one.wire()?,"res_other":self.res_other.wire()?}),
        )
    }
}
impl Wire for DidChallengePayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"trial":self.trial.wire()?}))
    }
}
impl Wire for DidChallengeReference {
    fn wire(&self) -> Result<Value> {
        Ok(
            json!({"res_one":self.res_one.wire()?,"res_other":self.res_other.wire()?,"res_one_props":self.res_one_props,"res_other_props":self.res_other_props}),
        )
    }
}
impl Wire for FinalPayload {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"report":self.report.wire()?,"output_summary":self.output_summary.wire()?}))
    }
    fn from_wire(v: Value) -> Result<Self> {
        Report::from_wire(v.get("report").cloned().unwrap_or(Value::Null))?;
        if v.get("output_summary")
            .and_then(|o| o.get("logger"))
            .is_some_and(|l| !l.is_null())
        {
            return Err(protocol("output_summary.logger: expected null"));
        }
        decode(v)
    }
}
impl Wire for FinalReference {
    fn wire(&self) -> Result<Value> {
        Ok(json!({"notifiers":notifiers(&self.notifiers)?}))
    }
}

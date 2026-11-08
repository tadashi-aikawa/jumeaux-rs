//! Python addons/final/miroir.py と同じDynamoDB項目・S3成果物を保存する。
use super::Log;
use crate::{
    addons::payload::FinalPayload,
    compat::{Error, Result},
    models::Report,
    notification::Verbosity,
};
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_s3::primitives::ByteStream;
use serde::Deserialize;
use std::{collections::HashMap, io::Write, path::Path};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub table: String,
    pub bucket: String,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub prefix: Option<String>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub cache_max_age: i64,
    #[serde(default = "with_zip", deserialize_with = "null_with_zip")]
    pub with_zip: bool,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub assumed_role_arn: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub checklist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_stack: Option<LocalStack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<Endpoints>,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub when: Vec<When>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalStack {
    pub r#use: bool,
    #[serde(default = "endpoint", deserialize_with = "null_endpoint")]
    pub endpoint: String,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub dynamodb: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::models::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub s3: Option<String>,
}
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    NotEmpty,
    HasDifferent,
}
fn with_zip() -> bool {
    true
}
crate::models::defaults::null_default_with!(null_with_zip, bool, with_zip());
fn endpoint() -> String {
    "http://localhost".into()
}
crate::models::defaults::null_default_with!(null_endpoint, String, endpoint());

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.local_stack.is_some() && self.endpoints.is_some() {
            return Err(Error::new(
                "ValueError",
                "miroir config: endpoints and local_stack cannot be used together",
            ));
        }
        Ok(())
    }

    fn endpoint_url(&self, service: &str) -> Option<String> {
        if let Some(endpoints) = &self.endpoints {
            return match service {
                "s3" => endpoints.s3.clone(),
                _ => endpoints.dynamodb.clone(),
            };
        }
        self.local_stack.as_ref().filter(|s| s.r#use).map(|s| {
            format!(
                "{}:{}",
                s.endpoint,
                if service == "s3" { 4572 } else { 4569 }
            )
        })
    }

    fn item(&self, report: &Report) -> HashMap<String, AttributeValue> {
        use AttributeValue::{Bool, N, Null, S, Ss};
        let mut item = HashMap::from([
            ("hashkey".into(), S(report.key.clone())),
            ("title".into(), S(report.title.clone())),
            ("one_host".into(), S(report.summary.one.host.clone())),
            ("other_host".into(), S(report.summary.other.host.clone())),
            (
                "same_count".into(),
                N(report.summary.status.same.to_string()),
            ),
            (
                "different_count".into(),
                N(report.summary.status.different.to_string()),
            ),
            (
                "failure_count".into(),
                N(report.summary.status.failure.to_string()),
            ),
            ("begin_time".into(), S(report.summary.time.start.clone())),
            ("end_time".into(), S(report.summary.time.end.clone())),
            (
                "elapsed_sec".into(),
                N(report.summary.time.elapsed_sec.to_string()),
            ),
            ("with_zip".into(), Bool(self.with_zip)),
            (
                "retry_hash".into(),
                report.retry_hash.clone().map(S).unwrap_or(Null(true)),
            ),
            ("check_status".into(), S("todo".into())),
        ]);
        if let Some(description) = &report.description {
            item.insert("description".into(), S(description.clone()));
        }
        if let Some(checklist) = &self.checklist {
            item.insert("checklist".into(), S(checklist.clone()));
        }
        if !report.summary.tags.is_empty() {
            let mut tags = report.summary.tags.clone();
            tags.sort();
            tags.dedup();
            item.insert("tags".into(), Ss(tags));
        }
        item
    }

    pub async fn send(&self, payload: &FinalPayload, logs: &mut Vec<Log>) -> Result<()> {
        self.validate()?;
        let report = &payload.report;
        if self.when.contains(&When::NotEmpty) && report.trials.is_empty() {
            logs.push(Log::new(
                Verbosity::Normal,
                "Skip sending results to Miroir because trials are empty.",
            ));
            return Ok(());
        }
        if self.when.contains(&When::HasDifferent) && report.summary.status.different == 0 {
            logs.push(Log::new(
                Verbosity::Normal,
                "Skip sending results to Miroir because there are different status.",
            ));
            return Ok(());
        }
        logs.push(Log::new(Verbosity::Normal, MIROIR_AA));
        let mut sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        if let Some(role) = &self.assumed_role_arn {
            // boto3と同じく1回取得し、このfinal実行の両クライアントで共有する。
            // STSのendpointはSDKの環境変数・プロファイルに従う。
            let assumed = aws_sdk_sts::Client::new(&sdk)
                .assume_role()
                .role_arn(role)
                .role_session_name("jumeaux_with_miroir_add-on")
                .send()
                .await
                .map_err(aws_error)?;
            let credentials = assumed
                .credentials()
                .ok_or_else(|| Error::new("AWSError", "AssumeRole response has no credentials"))?;
            sdk = sdk
                .into_builder()
                .credentials_provider(aws_sdk_sts::config::SharedCredentialsProvider::new(
                    aws_sdk_sts::config::Credentials::new(
                        credentials.access_key_id(),
                        credentials.secret_access_key(),
                        Some(credentials.session_token().into()),
                        None,
                        "miroir AssumeRole",
                    ),
                ))
                .build();
        }
        let mut dynamo_config = aws_sdk_dynamodb::config::Builder::from(&sdk);
        if let Some(endpoint) = self.endpoint_url("dynamodb") {
            dynamo_config = dynamo_config.endpoint_url(endpoint);
        }
        aws_sdk_dynamodb::Client::from_conf(dynamo_config.build())
            .put_item()
            .table_name(&self.table)
            .set_item(Some(self.item(report)))
            .send()
            .await
            .map_err(aws_error)?;

        let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk);
        if let Some(endpoint) = self.endpoint_url("s3") {
            s3_config = s3_config.endpoint_url(endpoint).force_path_style(true);
        }
        let s3 = aws_sdk_s3::Client::from_conf(s3_config.build());
        let base = match &self.prefix {
            Some(prefix) => format!("{prefix}/results/{}", report.key),
            None => format!("results/{}", report.key),
        };
        let mut value = serde_json::to_value(report)
            .map_err(|e| Error::new("SerializeError", e.to_string()))?;
        let trials = value
            .as_object_mut()
            .expect("Report is an object")
            .shift_remove("trials")
            .expect("Report has trials");
        self.put(
            &s3,
            &format!("{base}/report-without-trials.json"),
            report_json(&value)?.into_bytes(),
            false,
        )
        .await?;
        self.put(
            &s3,
            &format!("{base}/trials.json"),
            crate::output::json_text(&trials, None)?.into_bytes(),
            false,
        )
        .await?;

        let root = Path::new(&payload.output_summary.response_dir).join(&report.key);
        for which in ["one", "one-props", "other", "other-props"] {
            let dir = root.join(which);
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| super::io_error(&dir, e))?
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|e| super::io_error(&dir, e))?;
            logs.push(Log::new(
                Verbosity::Normal,
                format!("Uploading {} {which} responses...", entries.len()),
            ));
            for entry in entries {
                let path = entry.path();
                logs.push(Log::new(
                    Verbosity::VeryVerbose,
                    format!("Put {}", path.display()),
                ));
                let bytes = std::fs::read(&path).map_err(|e| super::io_error(&path, e))?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::new("ValueError", "response filename is not UTF-8"))?;
                self.put(&s3, &format!("{base}/{which}/{name}"), bytes, true)
                    .await?;
            }
        }
        if self.with_zip {
            let value = serde_json::to_value(report)
                .map_err(|e| Error::new("SerializeError", e.to_string()))?;
            super::write_text(
                &root.join("report.json"),
                &crate::output::json_text(&value, Some(4))?,
                &payload.output_summary,
            )?;
            // ディスク上に一時ZIPを作る。レスポンス全件をメモリへ載せない。
            let zip_path = root.with_file_name(format!("{}.zip", report.key));
            archive(&root, &zip_path)?;
            logs.push(Log::new(
                Verbosity::VeryVerbose,
                format!("Put {}", zip_path.display()),
            ));
            let uploaded = async {
                let body = ByteStream::from_path(&zip_path)
                    .await
                    .map_err(|e| super::io_error(&zip_path, e))?;
                s3.put_object()
                    .bucket(&self.bucket)
                    .key(format!(
                        "{base}/{}.zip",
                        report.key.chars().take(7).collect::<String>()
                    ))
                    .cache_control(format!("max-age={}", self.cache_max_age))
                    .body(body)
                    .customize()
                    .mutate_request(|request| {
                        request.headers_mut().remove("content-type");
                    })
                    .send()
                    .await
                    .map_err(aws_error)?;
                Ok(())
            }
            .await;
            let removed =
                std::fs::remove_file(&zip_path).map_err(|e| super::io_error(&zip_path, e));
            uploaded?;
            removed?;
        }
        Ok(())
    }

    async fn put(
        &self,
        s3: &aws_sdk_s3::Client,
        key: &str,
        bytes: Vec<u8>,
        cache: bool,
    ) -> Result<()> {
        s3.put_object()
            .bucket(&self.bucket)
            .key(key)
            .set_cache_control(cache.then(|| format!("max-age={}", self.cache_max_age)))
            .body(ByteStream::from(bytes))
            // SDKが補うapplication/octet-streamも除き、boto3と同じ未指定にする。
            .customize()
            .mutate_request(|request| {
                request.headers_mut().remove("content-type");
            })
            .send()
            .await
            .map_err(aws_error)?;
        Ok(())
    }
}

fn aws_error(error: impl std::fmt::Debug) -> Error {
    Error::new("AWSError", format!("{error:?}"))
}

// json.dumps(report.to_dict(), ensure_ascii=False)は挿入順とカンマ後の空白を保持する。
fn report_json(value: &serde_json::Value) -> Result<String> {
    struct Formatter;
    impl serde_json::ser::Formatter for Formatter {
        fn begin_object_key<W: ?Sized + Write>(
            &mut self,
            w: &mut W,
            first: bool,
        ) -> std::io::Result<()> {
            if first { Ok(()) } else { w.write_all(b", ") }
        }
        fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> std::io::Result<()> {
            w.write_all(b": ")
        }
        fn begin_array_value<W: ?Sized + Write>(
            &mut self,
            w: &mut W,
            first: bool,
        ) -> std::io::Result<()> {
            if first { Ok(()) } else { w.write_all(b", ") }
        }
    }
    let mut bytes = Vec::new();
    serde::Serialize::serialize(
        value,
        &mut serde_json::Serializer::with_formatter(&mut bytes, Formatter),
    )
    .map_err(|e| Error::new("SerializeError", e.to_string()))?;
    Ok(String::from_utf8(bytes).expect("JSON is UTF-8"))
}

fn archive(root: &Path, destination: &Path) -> Result<()> {
    use chrono::{Datelike, Timelike};
    use zip::{ZipWriter, write::SimpleFileOptions};
    fn append(writer: &mut ZipWriter<std::fs::File>, root: &Path, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir).map_err(|e| super::io_error(dir, e))? {
            let entry = entry.map_err(|e| super::io_error(dir, e))?;
            let path = entry.path();
            let metadata = std::fs::metadata(&path).map_err(|e| super::io_error(&path, e))?;
            let name = path
                .strip_prefix(root)
                .expect("archive member under root")
                .to_str()
                .ok_or_else(|| Error::new("ValueError", "archive filename is not UTF-8"))?;
            let modified: chrono::DateTime<chrono::Local> = metadata
                .modified()
                .map_err(|e| super::io_error(&path, e))?
                .into();
            let timestamp = zip::DateTime::from_date_and_time(
                modified.year() as u16,
                modified.month() as u8,
                modified.day() as u8,
                modified.hour() as u8,
                modified.minute() as u8,
                modified.second() as u8,
            )
            .map_err(|e| super::io_error(&path, e))?;
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .last_modified_time(timestamp)
                .large_file(metadata.len() >= u32::MAX as u64);
            #[cfg(unix)]
            let options = {
                use std::os::unix::fs::PermissionsExt;
                options.unix_permissions(metadata.permissions().mode())
            };
            if metadata.is_dir() {
                writer
                    .add_directory(format!("{name}/"), options)
                    .map_err(|e| super::io_error(&path, e))?;
                if !entry
                    .file_type()
                    .map_err(|e| super::io_error(&path, e))?
                    .is_symlink()
                {
                    append(writer, root, &path)?;
                }
            } else {
                writer
                    .start_file(name, options)
                    .map_err(|e| super::io_error(&path, e))?;
                let mut file = std::fs::File::open(&path).map_err(|e| super::io_error(&path, e))?;
                std::io::copy(&mut file, writer).map_err(|e| super::io_error(&path, e))?;
            }
        }
        Ok(())
    }
    let file = std::fs::File::create(destination).map_err(|e| super::io_error(destination, e))?;
    let mut writer = ZipWriter::new(file);
    append(&mut writer, root, root)?;
    writer
        .finish()
        .map_err(|e| super::io_error(destination, e))?;
    Ok(())
}

const MIROIR_AA: &str = r"
        __  __ _           _
__/\__ |  \/  (_)_ __ ___ (_)_ __  __/\__
\    / | |\/| | | '__/ _ \| | '__| \    /
/_  _\ | |  | | | | | (_) | | |    /_  _\\
  \/   |_|  |_|_|_|  \___/|_|_|      \/
";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn endpoint_selection_matches_python() {
        for (extra, dynamodb, s3) in [
            (json!({}), None, None),
            (
                json!({"local_stack":{"use":true}}),
                Some("http://localhost:4569"),
                Some("http://localhost:4572"),
            ),
            (
                json!({"local_stack":{"use":true,"endpoint":"http://127.0.0.1"}}),
                Some("http://127.0.0.1:4569"),
                Some("http://127.0.0.1:4572"),
            ),
            (json!({"local_stack":{"use":false}}), None, None),
            (
                json!({"endpoints":{"s3":"http://s3.local:9000"}}),
                None,
                Some("http://s3.local:9000"),
            ),
            (
                json!({"endpoints":{"dynamodb":"http://dynamo.local:8000","s3":null}}),
                Some("http://dynamo.local:8000"),
                None,
            ),
        ] {
            let mut value = json!({"table":"t","bucket":"b"});
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let config: Config = serde_json::from_value(value).unwrap();
            assert_eq!(config.endpoint_url("dynamodb").as_deref(), dynamodb);
            assert_eq!(config.endpoint_url("s3").as_deref(), s3);
        }
    }
}

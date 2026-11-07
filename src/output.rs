//! output.encodingをレポート・summary・標準出力/標準エラーへ適用する境界。
use crate::{
    compat::{
        Error, Result,
        encoding::{self, Errors},
    },
    models::{OutputSummary, Report},
};
use std::io::Write;

/// ストリームごとに1つ持ち、BOMを先頭の1回だけ書く。
pub struct EncodedWriter<W> {
    writer: W,
    encoding: String,
    started: bool,
    designation: [u8; 3],
}
impl<W: Write> EncodedWriter<W> {
    pub fn new(writer: W, output: &OutputSummary) -> Result<Self> {
        encoding::encode("", &output.encoding, Errors::Strict)?;
        Ok(Self {
            writer,
            encoding: output.encoding.clone(),
            started: false,
            designation: *b"\x1b(B",
        })
    }
    pub fn write_text(&mut self, text: &str) -> Result<()> {
        let mut bytes = encoding::encode(text, &self.encoding, Errors::Strict)?;
        // ISO-2022-JPのTextIOWrapperはwrite/flushをまたいでdesignationを保持する。
        // 単発encodeの末尾resetを除き、次chunkで必要になったときだけ戻す。
        let label = self.encoding.to_ascii_lowercase().replace('-', "_");
        if matches!(label.as_str(), "iso2022_jp" | "iso_2022_jp" | "iso2022jp") && !bytes.is_empty()
        {
            if bytes.ends_with(b"\x1b(B") {
                bytes.truncate(bytes.len() - 3);
            }
            let next_designation = bytes
                .windows(3)
                .rev()
                .find(|w| matches!(*w, b"\x1b$B" | b"\x1b(B" | b"\x1b(J"))
                .map(|w| <[u8; 3]>::try_from(w).unwrap())
                .unwrap_or(*b"\x1b(B");
            if bytes.starts_with(&self.designation) {
                bytes.drain(..3);
            } else if self.designation != *b"\x1b(B" && !bytes.starts_with(b"\x1b") {
                bytes.splice(..0, b"\x1b(B".iter().copied());
            }
            self.designation = next_designation;
        }
        let prefix = encoding::encode("", &self.encoding, Errors::Strict)?;
        let bytes = if self.started && !prefix.is_empty() && bytes.starts_with(&prefix) {
            &bytes[prefix.len()..]
        } else {
            &bytes[..]
        };
        self.writer
            .write_all(bytes)
            .map_err(|e| Error::new("IOError", e.to_string()))?;
        self.started = true;
        Ok(())
    }
    pub fn flush(&mut self) -> Result<()> {
        self.writer
            .flush()
            .map_err(|e| Error::new("IOError", e.to_string()))
    }
    pub fn into_inner(self) -> W {
        self.writer
    }
    pub fn get_ref(&self) -> &W {
        &self.writer
    }
}

impl OutputSummary {
    pub fn writer<W: Write>(&self, writer: W) -> Result<EncodedWriter<W>> {
        EncodedWriter::new(writer, self)
    }
    pub fn stdout(&self) -> Result<EncodedWriter<std::io::Stdout>> {
        self.writer(std::io::stdout())
    }
    pub fn stderr(&self) -> Result<EncodedWriter<std::io::Stderr>> {
        self.writer(std::io::stderr())
    }
    pub fn write_summary<W: Write>(&self, writer: W, text: &str) -> Result<()> {
        let mut output = self.writer(writer)?;
        output.write_text(text)?;
        output.flush()
    }
    pub fn write_report<W: Write>(&self, writer: W, report: &Report) -> Result<()> {
        let mut value = serde_json::to_value(report)
            .map_err(|e| Error::new("SerializeError", e.to_string()))?;
        // OwlMixinのsort_keys=Trueに合わせる。nullの除外はモデル側で行う。
        sort_keys(&mut value);
        let mut bytes = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, PythonFormatter);
        serde::Serialize::serialize(&value, &mut serializer)
            .map_err(|e| Error::new("SerializeError", e.to_string()))?;
        let text = String::from_utf8(bytes).expect("JSON is UTF-8");
        self.write_summary(writer, &text)
    }
}
struct PythonFormatter;
impl serde_json::ser::Formatter for PythonFormatter {
    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}
fn sort_keys(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.sort_keys();
            for value in map.values_mut() {
                sort_keys(value);
            }
        }
        serde_json::Value::Array(list) => {
            for value in list {
                sort_keys(value);
            }
        }
        _ => (),
    }
}

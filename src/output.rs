//! output.encodingをレポート・summary・stdout・stderrへの直接書き込みに適用する。
//! ログは元のstderrへUTF-8で書き、このwriterを経由しない。
use crate::{
    compat::{
        Error, Result,
        encoding::{self, Errors},
    },
    models::{OutputSummary, Report},
};
use std::io::{Seek, Write};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

type Writers<W> = OnceLock<Mutex<HashMap<String, ProcessWriter<W>>>>;
static STDOUT: Writers<std::io::Stdout> = OnceLock::new();
static STDERR: Writers<std::io::Stderr> = OnceLock::new();

/// encodingごとの共有ハンドル。ロックは各書き込みの間だけ保持する。
pub struct ProcessWriter<W> {
    inner: Arc<Mutex<EncodedWriter<W>>>,
}
impl<W> Clone for ProcessWriter<W> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
impl<W: Write> ProcessWriter<W> {
    pub fn write_text(&self, text: &str) -> Result<()> {
        self.inner
            .lock()
            .map_err(|_| stream_lock_error())?
            .write_text(text)
    }
    pub fn flush(&self) -> Result<()> {
        self.inner.lock().map_err(|_| stream_lock_error())?.flush()
    }
}
fn stream_lock_error() -> Error {
    Error::new("IOError", "output stream lock poisoned")
}

fn process_writer<W: Write>(
    slot: &Writers<W>,
    writer: W,
    output: &OutputSummary,
    position: Option<u64>,
) -> Result<ProcessWriter<W>> {
    let mut writers = slot
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| stream_lock_error())?;
    if let Some(writer) = writers.get(&output.encoding) {
        return Ok(writer.clone());
    }
    let mut initial = EncodedWriter::new(writer, output)?;
    // TextIOWrapperはseek不能なpipe/TTYでUTF-16/32のBOMを書かない。
    // UTF-8-SIGはpipeでも書く。seek可能な出力先では位置0のときだけ書く。
    let bom = encoding::encode("", &output.encoding, Errors::Strict)?;
    initial.started = match position {
        Some(position) => position != 0,
        None => !bom.is_empty() && bom != [0xef, 0xbb, 0xbf],
    };
    let writer = ProcessWriter {
        inner: Arc::new(Mutex::new(initial)),
    };
    writers.insert(output.encoding.clone(), writer.clone());
    Ok(writer)
}

#[cfg(unix)]
fn stream_position(stream: &impl std::os::fd::AsFd) -> Option<u64> {
    let fd = stream.as_fd().try_clone_to_owned().ok()?;
    std::fs::File::from(fd).stream_position().ok()
}

#[cfg(windows)]
fn stream_position(stream: &impl std::os::windows::io::AsHandle) -> Option<u64> {
    let handle = stream.as_handle().try_clone_to_owned().ok()?;
    std::fs::File::from(handle).stream_position().ok()
}

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
    pub fn stdout(&self) -> Result<ProcessWriter<std::io::Stdout>> {
        process_writer(
            &STDOUT,
            std::io::stdout(),
            self,
            stream_position(&std::io::stdout()),
        )
    }
    pub fn stderr(&self) -> Result<ProcessWriter<std::io::Stderr>> {
        process_writer(
            &STDERR,
            std::io::stderr(),
            self,
            stream_position(&std::io::stderr()),
        )
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

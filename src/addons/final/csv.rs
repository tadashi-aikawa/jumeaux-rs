//! Python addons/final/csv.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub column_names: Vec<String>,
    pub output_path: String,
    #[serde(default, deserialize_with = "crate::models::defaults::null_default")]
    pub with_header: bool,
}

impl Config {
    pub fn render(&self, report: &crate::models::Report) -> crate::compat::Result<String> {
        use serde_json::{Value, json};
        let mut output = String::new();
        if self.with_header {
            row(&mut output, &self.column_names);
        }
        for t in &report.trials {
            let mut fields = json!({
                "seq": t.seq, "name": t.name, "method": t.method, "path": t.path,
                "headers": crate::output::json_text(&json!(t.headers), None)?,
                "queries": crate::output::json_text(&json!(t.queries), None)?,
                "request_time": t.request_time, "status": t.status,
            });
            for (side, s) in [("one", &t.one), ("other", &t.other)] {
                for (key, value) in [
                    ("url", json!(s.url)),
                    ("status", json!(s.status_code)),
                    ("byte", json!(s.byte)),
                    ("response_sec", json!(s.response_sec)),
                    ("content_type", json!(s.content_type)),
                    ("encoding", json!(s.encoding)),
                ] {
                    fields[format!("{side}.{key}")] = value;
                }
            }
            let columns: Vec<_> = self
                .column_names
                .iter()
                .map(|key| match &fields[key] {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    v => v.to_string(),
                })
                .collect();
            row(&mut output, &columns);
        }
        Ok(output)
    }
}

fn row(output: &mut String, columns: &[String]) {
    for (i, value) in columns.iter().enumerate() {
        if i != 0 {
            output.push(',');
        }
        // owlmixinのLF dialect。CR単独はquote条件ではない。
        if value.contains([',', '"', '\n']) || columns.len() == 1 && value.is_empty() {
            output.push('"');
            output.push_str(&value.replace('"', "\"\""));
            output.push('"');
        } else {
            output.push_str(value);
        }
    }
    output.push('\n');
}

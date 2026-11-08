//! Python addons/final/viewer.py の設定契約。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {}

pub const HTML: &[u8] = include_bytes!("../../../assets/viewer/index.html");

pub fn save(root: &std::path::Path, logs: &mut Vec<super::Log>) -> crate::compat::Result<()> {
    use crate::notification::Verbosity;
    let path = root.join("index.html");
    std::fs::write(&path, HTML).map_err(|e| super::io_error(&path, e))?;
    logs.push(super::Log::new(
        Verbosity::Normal,
        format!("[final/viewer] Create {}", path.display()),
    ));
    if !root.join("report.json").exists() {
        logs.push(super::Log::new(
            Verbosity::Warning,
            format!(
                "[final/viewer] report.json doesn't exist in {}.",
                root.display()
            ),
        ));
        logs.push(super::Log::new(Verbosity::Warning, "[final/viewer] Please use a `final/json` add-on to create report.json if you can't to access viewer correctlly."));
    }
    Ok(())
}

use crate::logging::Logger;
use jumeaux::{
    compat::{Error, Result},
    notification::Verbosity,
};
use std::{collections::BTreeSet, path::Path};
include!(concat!(env!("OUT_DIR"), "/templates.rs"));

pub fn run(name: Option<&str>) -> Result<u8> {
    let names: BTreeSet<_> = FILES
        .iter()
        .map(|(p, _)| p.split('/').next().unwrap())
        .collect();
    let Some(name) = name.filter(|s| names.contains(s)) else {
        let message = if name == Some("addon") {
            "init addon: Pythonアドオンの生成は未対応です。外部コマンドアドオンの移行仕様は未決です。".into()
        } else {
            format!(
                "Please specify a valid name.\n\n✨ [Valid names] ✨\n{}",
                names.into_iter().collect::<Vec<_>>().join("\n")
            )
        };
        eprintln!("{message}");
        return Ok(1);
    };
    for file in ["config.yml", "requests"] {
        let data = FILES
            .iter()
            .find(|(p, _)| *p == format!("{name}/{file}"))
            .expect("template file")
            .1;
        std::fs::write(file, data).map_err(|e| Error::new("IOError", format!("{file}: {e}")))?;
        Logger(0).log(Verbosity::Normal, format!("✨ [Create] {file}"));
    }
    std::fs::create_dir("api").map_err(|e| Error::new("IOError", format!("api: {e}")))?;
    let prefix = format!("{name}/");
    for (path, bytes) in FILES {
        let Some(relative) = path.strip_prefix(&prefix).filter(|p| p.starts_with("api/")) else {
            continue;
        };
        let path = Path::new(relative);
        std::fs::create_dir_all(path.parent().unwrap())
            .map_err(|e| Error::new("IOError", e.to_string()))?;
        std::fs::write(path, bytes)
            .map_err(|e| Error::new("IOError", format!("{relative}: {e}")))?;
    }
    Logger(0).log(
        Verbosity::Normal,
        "✨ [Create] templates with a api directory",
    );
    Ok(0)
}

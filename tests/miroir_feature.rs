//! 軽量ビルドでも設定を黙って無視せず、実行前に理由付きで拒否する。
#![cfg(not(feature = "miroir"))]

use jumeaux::{
    addons::{Layer, r#final::Pipeline, parse_config},
    models::Addon,
};
use serde_json::json;

#[test]
fn disabled_miroir_is_rejected_even_when_it_would_skip() {
    let addon: Addon = serde_json::from_value(json!({"name":"miroir","config":{
        "table":"t", "bucket":"b", "when":["not_empty"]
    }}))
    .unwrap();
    let error = parse_config(Layer::Final, &addon, "addons.final[0]").unwrap_err();
    assert_eq!(error.kind, "ConfigError");
    assert!(error.message.contains("disabled in this build"));
    assert!(error.message.contains("--features miroir"));
    assert!(Pipeline::build(&[addon]).is_err());

    let root = std::env::temp_dir().join(format!("miroir-disabled-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("config.yml");
    let mut config: serde_json::Value =
        serde_yaml_ng::from_str(include_str!("../compat/python-baseline/simple/config.yml"))
            .unwrap();
    config["addons"]["final"] = json!([{"name":"miroir", "config":{"table":"t", "bucket":"b"}}]);
    std::fs::write(&path, config.to_string()).unwrap();
    let error = jumeaux::config::load(&[path], &[]).unwrap_err();
    assert!(error.message.contains("--features miroir"));
    std::fs::write(root.join("requests"), "").unwrap();
    let cli = std::process::Command::new(env!("CARGO_BIN_EXE_jumeaux"))
        .current_dir(&root)
        .args(["run", "requests"])
        .output()
        .unwrap();
    assert_eq!(cli.status.code(), Some(1));
    assert!(
        String::from_utf8(cli.stderr)
            .unwrap()
            .contains("--features miroir")
    );
    assert!(!root.join("responses").exists());
    std::fs::remove_dir_all(root).unwrap();
}

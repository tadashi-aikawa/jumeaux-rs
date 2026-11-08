#[test]
#[ignore = "Python版の.venvとローカル受信先を使い旧サンプルと比較する"]
fn generated_samples_match_python_trials_and_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let python_root = std::env::var_os("JUMEAUX_PYTHON_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
                .join("git/github.com/tadashi-aikawa/jumeaux")
        });
    let result = std::process::Command::new(python_root.join(".venv/bin/python"))
        .arg("-B")
        .arg(root.join("compat/harness/exec_samples.py"))
        .env("JUMEAUX_BIN", env!("CARGO_BIN_EXE_jumeaux"))
        .env("JUMEAUX_PYTHON_ROOT", python_root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}

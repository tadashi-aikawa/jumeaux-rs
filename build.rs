use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=assets/templates");
    fn visit(root: &Path, path: &Path, files: &mut Vec<(String, String)>) {
        let mut entries = fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                visit(root, &entry, files);
            } else {
                files.push((
                    entry
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                    entry.canonicalize().unwrap().to_str().unwrap().into(),
                ));
            }
        }
    }
    let mut files = vec![];
    visit(
        Path::new("assets/templates"),
        Path::new("assets/templates"),
        &mut files,
    );
    let mut code = String::from("pub const FILES: &[(&str, &[u8])] = &[\n");
    for (name, path) in files {
        code.push_str(&format!("({name:?}, include_bytes!({path:?})),\n"));
    }
    code.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").unwrap()).join("templates.rs"),
        code,
    )
    .unwrap();
}

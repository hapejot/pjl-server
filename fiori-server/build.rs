use std::fs;
use std::path::{Path, PathBuf};

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_files(&path, files)?;
        } else if kind.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
            files.push(path);
        } else {
            panic!("unsupported web asset: {}", path.display());
        }
    }
    Ok(())
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let web_root = manifest.join("../web");
    if !web_root.is_dir() {
        panic!(
            "default web assets directory not found: {}",
            web_root.display()
        );
    }

    let mut files = Vec::new();
    collect_files(&web_root, &mut files).expect("cannot enumerate default web assets");
    files.sort();

    let mut generated = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for path in files {
        let relative = path
            .strip_prefix(&web_root)
            .expect("asset is under web root")
            .to_string_lossy()
            .replace('\\', "/");
        generated.push_str(&format!(
            "    ({relative:?}, include_bytes!({:?})),\n",
            path.to_string_lossy()
        ));
    }
    generated.push_str("];\n");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("embedded_web.rs");
    fs::write(out, generated).expect("cannot write embedded web asset index");
}

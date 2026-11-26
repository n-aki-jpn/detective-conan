use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let decks_dir = Path::new(&manifest_dir).join("../data/dummy/decks");

    println!("cargo:rerun-if-changed={}", decks_dir.display());

    let mut entries: Vec<(String, String)> = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&decks_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    println!("cargo:rerun-if-changed={}", path.display());
                    let abs_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    let path_str = abs_path.to_string_lossy().replace('\\', "/");
                    entries.push((stem.to_string(), path_str));
                }
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut code = String::from("const PRESET_DECKS: &[(&str, &str)] = &[\n");
    for (stem, path_str) in entries {
        code.push_str(&format!(
            "    ({:?}, include_str!({:?})),\n",
            stem, path_str
        ));
    }
    code.push_str("];\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("preset_decks_gen.rs");
    fs::write(dest_path, code).unwrap();
}

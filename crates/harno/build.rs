//! Embeds the repository's built-in tools in the binary: every folder of
//! `tools/` that holds a `tool.yml`, with the files at the top of it,
//! `Cargo.toml` aside. Sorted by name; what is deeper (a crate, tests) stays.
//!
//! A tool written in a compiled language ships no program: `harno tool add`
//! copies the files, and the project builds the program as its manifest says.

use std::path::{Path, PathBuf};

fn sorted(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| keep(p)).collect()).unwrap_or_default();
    v.sort();
    v
}

fn include(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    format!("include_str!({:?})", path.canonicalize().unwrap().to_string_lossy())
}

fn tools(root: &Path, out: &Path) {
    let dir = root.join("tools");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut s = String::from("&[\n");
    for t in sorted(&dir, |p| p.join("tool.yml").is_file()) {
        println!("cargo:rerun-if-changed={}", t.display());
        let name = t.file_name().unwrap().to_string_lossy().to_string();
        s.push_str(&format!("    ({name:?}, &[\n"));
        for f in sorted(&t, |p| p.is_file() && p.file_name().is_some_and(|n| n != "Cargo.toml")) {
            let file = f.file_name().unwrap().to_string_lossy().to_string();
            s.push_str(&format!("        ({file:?}, {}),\n", include(&f)));
        }
        s.push_str("    ]),\n");
    }
    s.push(']');
    std::fs::write(out.join("builtin_tools.rs"), s).unwrap();
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    tools(&root, &out);
}

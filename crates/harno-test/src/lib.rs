//! A sandbox for driving harno the way an agent does: a temporary project,
//! files written into it, and calls that return the text and exit code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

pub const SESSION: &str = "0196aaaa-0000-7000-8000-000000000001";

/// The steps of `Project::turns`.
const TURN: &str = r#"for step in "$@"; do
  case $step in
    +*) echo "${step#+}" ;;
    @*) until [ -e "${step#@}" ]; do sleep 0.01; done ;;
    *) until grep -qx -- "$step" .harno/log/harno/*/0001.raw 2>/dev/null; do sleep 0.01; done ;;
  esac
done
"#;

pub struct Project {
    pub dir: TempDir,
    pub vars: HashMap<String, String>,
}

#[derive(Debug)]
pub struct Call {
    pub out: String,
    pub code: i32,
}

impl Call {
    /// The output, asserting the call succeeded.
    #[track_caller]
    pub fn ok(&self) -> &str {
        assert_eq!(self.code, 0, "call failed:\n{}", self.out);
        &self.out
    }

    /// The output, asserting the call was refused.
    #[track_caller]
    pub fn refused(&self) -> &str {
        assert_ne!(self.code, 0, "call succeeded:\n{}", self.out);
        assert!(self.out.contains("refused: "), "not a refusal:\n{}", self.out);
        &self.out
    }

    pub fn instead(&self) -> Option<String> {
        self.out.lines().find_map(|l| l.strip_prefix("instead: ").map(|s| s.to_string()))
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.out).unwrap_or_else(|e| panic!("not JSON ({e}):\n{}", self.out))
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    /// `harno init` and an empty `docs/`.
    pub fn new() -> Project {
        let p = Project::bare();
        p.harno(&["init"]).ok();
        std::fs::create_dir_all(p.path("docs")).unwrap();
        p
    }

    /// The corpus most tests read: fixed ids, every node kind, links in both
    /// directions.
    ///
    /// ```text
    /// docs/                         # Docs {a00001}
    /// ├─ ## How to read {a00002}    links to a00013
    /// ├─ arch.md                    # Architecture {a00010} (with frontmatter)
    /// │  ├─ ## Node {a00011}
    /// │  │  └─ ### Fields {a00012}
    /// │  └─ ## Id {a00013}          links to README.md
    /// ├─ design.md                  # Design {a00020}
    /// │  └─ ## Why random ids {a00021}  links to arch.md#a00013
    /// └─ findings/                  # Findings {a00030}
    ///    ├─ 2026/                   # 2026 {a00040}
    ///    │  └─ B-043-turns.md       # B-043 Turns {a00041}
    ///    └─ B-013-language.md       # B-013 Language {a00031}
    ///       └─ ## Decision record {a00032}
    /// ```
    pub fn sample() -> Project {
        let p = Project::new();
        p.write("docs/README.md", SAMPLE_README)
            .write("docs/arch.md", SAMPLE_ARCH)
            .write("docs/design.md", SAMPLE_DESIGN)
            .write("docs/findings/README.md", "# Findings {a00030}\n\nRecords of decisions.\n")
            .write("docs/findings/2026/README.md", "# 2026 {a00040}\n\nThis year.\n")
            .write("docs/findings/2026/B-043-turns.md", "# B-043 Turns {a00041}\n\nTurns cost money.\n")
            .write(
                "docs/findings/B-013-language.md",
                "# B-013 Language {a00031}\n\nWhich language.\n\n## Decision record {a00032}\n\nEnglish.\n",
            );
        p
    }

    /// A temporary directory with nothing in it and the session set.
    pub fn bare() -> Project {
        let dir = tempfile::tempdir().unwrap();
        let mut vars = HashMap::new();
        vars.insert("HARNO_SESSION".to_string(), SESSION.to_string());
        Project { dir, vars }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn config(&self, yaml: &str) -> &Self {
        self.write(".harno/config.yml", yaml)
    }

    /// A command of the project: `.harno/tools/<name>/tool.yml`.
    pub fn tool(&self, name: &str, yaml: &str) -> &Self {
        self.write(&format!(".harno/tools/{name}/tool.yml"), yaml)
    }

    pub fn write(&self, rel: &str, text: &str) -> &Self {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
        self
    }

    #[track_caller]
    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    pub fn harno(&self, args: &[&str]) -> Call {
        self.harno_in(self.root(), args)
    }

    pub fn harno_in(&self, cwd: &Path, args: &[&str]) -> Call {
        self.call(harno::Env { cwd: cwd.to_path_buf(), vars: self.vars.clone(), watched: None }, args)
    }

    /// A call a human watches in a terminal: what it printed at the end, and
    /// what reached the terminal while it ran.
    pub fn harno_watched(&self, args: &[&str]) -> (Call, String) {
        let screen = Arc::new(Mutex::new(Vec::<u8>::new()));
        let watched: Arc<Mutex<dyn std::io::Write + Send>> = screen.clone();
        let call = self.call(harno::Env { cwd: self.root().to_path_buf(), vars: self.vars.clone(), watched: Some(watched) }, args);
        let seen = String::from_utf8(screen.lock().unwrap().clone()).unwrap();
        (call, seen)
    }

    fn call(&self, env: harno::Env, args: &[&str]) -> Call {
        let mut buf: Vec<u8> = Vec::new();
        let code = harno::main(args.iter().map(|s| s.to_string()).collect(), env, &mut buf);
        Call { out: String::from_utf8(buf).unwrap(), code }
    }

    /// `scripts/turn.sh`, a command that goes step by step, run as
    /// `sh scripts/turn.sh <step>…`, so that a test meets a call while it
    /// runs where it wants: `+x` writes the line x, `@f` waits until the file
    /// f is there, and any other step waits until the raw file of call 1
    /// holds that line.
    pub fn turns(&self) -> &Self {
        self.write("scripts/turn.sh", TURN)
    }

    /// Wait until the file `rel` reads `text`; fail after ten seconds.
    #[track_caller]
    pub fn wait_for(&self, rel: &str, text: &str) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let now = std::fs::read_to_string(self.path(rel)).ok();
            if now.as_deref() == Some(text) {
                return;
            }
            assert!(std::time::Instant::now() < until, "{rel} never read {text:?}, it reads {now:?}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The id written on the heading with this title in `rel`.
    #[track_caller]
    pub fn id_of(&self, rel: &str, title: &str) -> String {
        let text = self.read(rel);
        for line in text.lines() {
            let t = line.trim_start_matches('#').trim();
            if line.starts_with('#') && t.starts_with(title) {
                let rest = t[title.len()..].trim();
                if let Some(id) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                    return id.to_string();
                }
            }
        }
        panic!("no heading {title:?} with an id in {rel}:\n{text}");
    }

    /// Every `{id}` on a heading line in a file.
    pub fn ids_in(&self, rel: &str) -> Vec<String> {
        self.read(rel)
            .lines()
            .filter(|l| l.starts_with('#'))
            .filter_map(|l| {
                let l = l.trim_end();
                let open = l.rfind('{')?;
                l.ends_with('}').then(|| l[open + 1..l.len() - 1].to_string())
            })
            .collect()
    }

    /// The calls of the test session in the log of the gate.
    pub fn calls(&self) -> Vec<serde_json::Value> {
        self.lines(&format!(".harno/log/harno/{SESSION}.jsonl"))
    }

    fn lines(&self, rel: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.path(rel))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

pub const SAMPLE_README: &str = "# Docs {a00001}

The corpus root.

## How to read {a00002}

Read the [Id section](arch.md#a00013) first.
";

pub const SAMPLE_ARCH: &str = "---
title: Architecture
---
# Architecture {a00010}

The design in one file.

## Node {a00011}

A node is a heading.

### Fields {a00012}

Title, summary, body.

## Id {a00013}

Ids are random. See [the root](README.md).
";

pub const SAMPLE_DESIGN: &str = "# Design {a00020}

Why things are the way they are.

## Why random ids {a00021}

The rule is [in the Id section](arch.md#a00013).
";

/// A number from `show`'s field block: `  body       2840`.
#[track_caller]
pub fn field<T: std::str::FromStr>(out: &str, name: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    let prefix = format!("  {name} ");
    let line = out
        .lines()
        .find(|l| l.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no {name} in\n{out}"));
    line[prefix.len()..].trim().parse().unwrap()
}

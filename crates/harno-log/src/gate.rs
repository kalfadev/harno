//! The log of the gate: one line for every call that went through harno.
//!
//! `.harno/log/harno/<session>.jsonl` holds one JSON object per call, in
//! order. Every line has `n`, `ts` (when the call ended), `ms`, `cwd`,
//! `verb` and `args`; a call that ran commands adds `cmd` and `exit`,
//! `output` and `lines` (what the commands wrote) and `filtered` and
//! `kept` (what the caller read). A refused call carries `refused` alone:
//! nothing ran.
//!
//! Beside the file, `<session>/<n>.out` is what the caller read and
//! `<n>.raw` what the commands wrote before a filter cut it, kept only
//! when the two differ. `harno log raw <n>` reads it. While a call of `run`
//! runs, `<n>.raw` fills as its commands write, and `<n>.run` holds how the
//! call began, locked until its process ends: a call that died before its
//! line leaves it unlocked.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub const GATE_DIR: &str = ".harno/log/harno";

/// The session of a call without `HARNO_SESSION`: a human in a terminal
/// logs to the day, so that no file grows forever.
pub fn day_session() -> String {
    format!("manual-{}", &crate::log::now()[..10])
}

pub struct Gate {
    dir: PathBuf,
    pub session: String,
}

impl Gate {
    pub fn open(project: &Path, session: &str) -> std::io::Result<Gate> {
        let dir = project.join(GATE_DIR);
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir)?;
            crate::log::ensure_gitignored(project)?;
        }
        Ok(Gate { dir, session: session.to_string() })
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.session))
    }

    fn text_path(&self, n: u64, ext: &str) -> PathBuf {
        self.dir.join(&self.session).join(format!("{n:04}.{ext}"))
    }

    /// The number of the call, taken before it runs so that its filtered
    /// output can name it. The empty `<n>.out` holds the number: a call
    /// that dies before its line is written keeps it for itself.
    pub fn claim(&self) -> std::io::Result<u64> {
        std::fs::create_dir_all(self.dir.join(&self.session))?;
        let written = std::fs::read_to_string(self.path())
            .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count() as u64)
            .unwrap_or(0);
        let mut n = written + 1;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(self.text_path(n, "out")) {
                Ok(_) => return Ok(n),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e),
            }
        }
    }

    /// The line of call `n`: `n` and `ts` first, the fields after them.
    pub fn write(&self, n: u64, fields: Map<String, Value>) -> std::io::Result<()> {
        let mut obj = Map::new();
        obj.insert("n".into(), n.into());
        obj.insert("ts".into(), crate::log::now().into());
        for (k, v) in fields {
            obj.insert(k, v);
        }
        let line = format!("{}\n", serde_json::to_string(&Value::Object(obj)).unwrap_or_default());
        let mut f = OpenOptions::new().create(true).append(true).open(self.path())?;
        f.lock()?;
        f.write_all(line.as_bytes())?;
        f.unlock()?;
        let _ = std::fs::remove_file(self.text_path(n, "run"));
        Ok(())
    }

    /// What the caller read.
    pub fn write_out(&self, n: u64, text: &str) -> std::io::Result<()> {
        std::fs::write(self.text_path(n, "out"), text)
    }

    /// What the commands wrote before a filter cut it.
    pub fn write_raw(&self, n: u64, text: &str) -> std::io::Result<()> {
        std::fs::write(self.text_path(n, "raw"), text)
    }

    /// No raw output beside call `n`: the caller read all of it.
    pub fn drop_raw(&self, n: u64) {
        let _ = std::fs::remove_file(self.text_path(n, "raw"));
    }

    /// The raw file of call `n`, for its commands to write into as they run.
    pub fn live(&self, n: u64) -> std::io::Result<File> {
        File::create(self.text_path(n, "raw"))
    }

    /// Call `n` has begun: `<n>.run` holds `n`, `ts` and `fields`, locked for
    /// as long as the returned file is open, which is as long as the process
    /// lives, however it ends. `write` takes it away with the line.
    pub fn begin(&self, n: u64, fields: Map<String, Value>) -> std::io::Result<File> {
        let mut obj = Map::new();
        obj.insert("n".into(), n.into());
        obj.insert("ts".into(), crate::log::now().into());
        obj.extend(fields);
        let path = self.text_path(n, "run");
        let mut f = File::create(&path)?;
        // Unlocked it would read as a call that died.
        if let Err(e) = f.lock() {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        f.write_all(serde_json::to_string(&Value::Object(obj)).unwrap_or_default().as_bytes())?;
        Ok(f)
    }
}

/// The calls of `session` without a line: what each began with, and whether
/// it still runs, its `<n>.run` locked, or died before it wrote its line.
pub fn unfinished(project: &Path, session: &str) -> Vec<(Value, bool)> {
    if !crate::log::valid_session(session) {
        return Vec::new();
    }
    let Ok(dir) = std::fs::read_dir(project.join(GATE_DIR).join(session)) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "run") {
            continue;
        }
        let (Ok(text), Ok(f)) = (std::fs::read_to_string(&path), File::open(&path)) else { continue };
        let Ok(began) = serde_json::from_str::<Value>(&text) else { continue };
        found.push((began, matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))));
    }
    found
}

/// The unfiltered output of call `n` of `session`, when one was kept.
pub fn read_raw(project: &Path, session: &str, n: u64) -> Option<String> {
    if !crate::log::valid_session(session) {
        return None;
    }
    std::fs::read_to_string(project.join(GATE_DIR).join(session).join(format!("{n:04}.raw"))).ok()
}

/// What the caller read of call `n` of `session`.
pub fn read_out(project: &Path, session: &str, n: u64) -> Option<String> {
    if !crate::log::valid_session(session) {
        return None;
    }
    std::fs::read_to_string(project.join(GATE_DIR).join(session).join(format!("{n:04}.out"))).ok()
}

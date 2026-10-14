//! The shape of the two logs, as this tool reads them.
//!
//! analytics is a program of its own and takes no harno crate, so what it
//! knows about a log is here: the directories, a session id, the sessions
//! the page opens with, and the lines and texts of one session. Everything
//! else is text the page reads as it stands.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The log of a project.
pub const LOG_DIR: &str = ".harno/log";

/// The directory harno writes; every other directory beside it is the log
/// of a harness, written by its hooks.
pub const GATE: &str = "harno";

/// A session id safe to use as a file name.
pub fn valid_session(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}

/// The file of a session in one directory of the log.
pub fn file(project: &Path, dir: &str, session: &str) -> PathBuf {
    project.join(LOG_DIR).join(dir).join(format!("{session}.jsonl"))
}

/// The lines of one file, in order.
pub fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// The text beside line `n`: `<dir>/<session>/<n>.<ext>`.
pub fn text(project: &Path, dir: &str, session: &str, n: u64, ext: &str) -> Option<String> {
    if !valid_session(session) || !valid_session(dir) {
        return None;
    }
    std::fs::read_to_string(project.join(LOG_DIR).join(dir).join(session).join(format!("{n:04}.{ext}"))).ok()
}

/// The harnesses that wrote a log here, by name: a directory beside the
/// gate that holds a session file of its own. The directories of the texts
/// of a session hold none, so they are no harness.
pub fn harnesses(project: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(project.join(LOG_DIR)) else { return out };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == GATE || !valid_session(&name) || !e.path().is_dir() {
            continue;
        }
        let holds_a_session = std::fs::read_dir(e.path())
            .map(|d| d.flatten().any(|f| f.file_name().to_string_lossy().ends_with(".jsonl")))
            .unwrap_or(false);
        if holds_a_session {
            out.push(name);
        }
    }
    out.sort();
    out
}

/// The harness whose log holds this session, when one does.
pub fn harness_of(project: &Path, session: &str) -> Option<String> {
    harnesses(project).into_iter().find(|h| file(project, h, session).is_file())
}

/// One session, for the list the page opens with.
pub struct SessionInfo {
    pub id: String,
    pub ts: String,
    pub label: String,
    pub calls: usize,
    /// The harness whose log holds it, when one does.
    pub harness: Option<String>,
    /// The gate logged calls of it.
    pub gate: bool,
    /// What the gate saw first, for a list that shows the calls alone.
    pub gate_label: String,
    /// When the last line of either log was written.
    pub last: String,
    /// A harness opened it: its log has a session start.
    pub started: bool,
    /// The last session line of the harness is an end.
    pub ended: bool,
}

fn ts_of(row: Option<&Value>) -> String {
    row.and_then(|e| e.get("ts")).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn kind(e: &Value) -> &str {
    e.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

/// Every session of the log, newest first: the calls of the gate and the
/// words of the harness that ran it are two files of one name.
pub fn sessions(project: &Path) -> Vec<SessionInfo> {
    let mut ids: Vec<(String, Option<String>)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut dirs = vec![GATE.to_string()];
    dirs.extend(harnesses(project));
    for dir in &dirs {
        let Ok(read) = std::fs::read_dir(project.join(LOG_DIR).join(dir)) else { continue };
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".jsonl") else { continue };
            if seen.iter().any(|s| s == id) {
                continue;
            }
            seen.push(id.to_string());
            ids.push((id.to_string(), harness_of(project, id)));
        }
    }
    let mut out = Vec::new();
    for (id, harness) in ids {
        let calls = lines(&file(project, GATE, &id));
        let events = harness.as_ref().map(|h| lines(&file(project, h, &id))).unwrap_or_default();
        let first = [ts_of(calls.first()), ts_of(events.first())].into_iter().filter(|t| !t.is_empty()).min();
        let Some(ts) = first else { continue };
        let last = [ts_of(calls.last()), ts_of(events.last())].into_iter().max().unwrap_or_default();
        let prompt = events.iter().find(|e| kind(e) == "prompt");
        let label = prompt
            .and_then(|e| e.get("n").and_then(|n| n.as_u64()))
            .and_then(|n| text(project, harness.as_deref().unwrap_or(GATE), &id, n, "out"))
            .unwrap_or_else(|| {
                let verb = calls.first().and_then(|c| c.get("verb")).and_then(|v| v.as_str()).unwrap_or("?");
                format!("(no prompt) {verb}")
            });
        let started = events.iter().any(|e| kind(e) == "session" && e.get("event").and_then(|t| t.as_str()) == Some("start"));
        let ended = events
            .iter()
            .rev()
            .find(|e| kind(e) == "session")
            .is_some_and(|e| e.get("event").and_then(|t| t.as_str()) == Some("end"));
        let gate = !calls.is_empty();
        let first = calls.first();
        let gate_label = first
            .and_then(|c| c.get("cmd"))
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.as_str())
            .map(|c| c.to_string())
            .or_else(|| first.and_then(|c| c.get("verb")).and_then(|v| v.as_str()).map(|v| format!("harno {v}")))
            .unwrap_or_default();
        out.push(SessionInfo { id, ts, label, calls: calls.len(), harness, gate, gate_label, last, started, ended });
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

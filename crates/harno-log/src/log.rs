//! Where a log lives, and what a session of calls adds up to.
//!
//! `.harno/log/` holds the log of the gate, `harno/<session>.jsonl`
//! ([`crate::gate`]), and beside it the log of a harness, written by the
//! hooks of that harness in its own words, `<harness>/<session>.jsonl`.
//! harno writes and reads its own alone.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const LOG_DIR: &str = ".harno/log";

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A session id safe to use as a file name.
pub fn valid_session(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The log is the record of a machine about itself: it stays out of git.
pub(crate) fn ensure_gitignored(project: &Path) -> std::io::Result<()> {
    let path = project.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let covered = existing
        .lines()
        .map(|l| l.trim())
        .any(|l| matches!(l, ".harno/log/" | ".harno/log" | "/.harno/log/" | "/.harno/log" | ".harno/" | ".harno"));
    if covered {
        return Ok(());
    }
    let mut text = existing;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(".harno/log/\n");
    std::fs::write(path, text)
}

/// One session of the gate log.
pub struct SessionInfo {
    pub id: String,
    pub ts: String,
    pub calls: usize,
    /// When the last call of the session was written.
    pub last: String,
}

/// Every session the gate has a file for.
fn session_ids(project: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(read) = std::fs::read_dir(project.join(crate::gate::GATE_DIR)) else { return out };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(id) = name.strip_suffix(".jsonl") {
            out.insert(id.to_string());
        }
    }
    out
}

/// The calls of a session, in the order they were written.
pub fn calls(project: &Path, id: &str) -> Vec<Value> {
    let path: PathBuf = project.join(crate::gate::GATE_DIR).join(format!("{id}.jsonl"));
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Which commands of a call failed it, in command order: the `failed` its
/// line carries, as the filters of that call judged each exit code, whatever
/// the config says later. A line written before that field has its exit codes
/// alone, and one that is not 0 failed.
pub fn failed(call: &Value) -> Vec<bool> {
    if let Some(f) = call.get("failed").and_then(|v| v.as_array()) {
        return f.iter().map(|b| b.as_bool() != Some(false)).collect();
    }
    call.get("exit").and_then(|v| v.as_array()).into_iter().flatten().map(|x| x.as_i64() != Some(0)).collect()
}

fn ts_of(row: Option<&Value>) -> String {
    row.and_then(|e| e.get("ts")).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

/// Every session with calls, the one used last first: a session that
/// started early and is still at work comes before one that started later.
pub fn sessions(project: &Path) -> Vec<SessionInfo> {
    let mut out = Vec::new();
    for id in session_ids(project) {
        let rows = calls(project, &id);
        if rows.is_empty() {
            continue;
        }
        out.push(SessionInfo { id, ts: ts_of(rows.first()), calls: rows.len(), last: ts_of(rows.last()) });
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| b.id.cmp(&a.id)));
    out
}

/// The session a question about the log means when it names none: the one
/// asking, the way `log ls` reads it; before that one has a call, the one
/// used last.
pub fn default_session(project: &Path, current: &str) -> Option<String> {
    let all = sessions(project);
    all.iter().find(|i| i.id == current).or(all.first()).map(|i| i.id.clone())
}

/// What a session adds up to (`harno log stats`): what the calls returned,
/// and what the filters kept out of it.
#[derive(Default)]
pub struct Stats {
    pub session: String,
    pub ts: String,
    /// When its last call was written: `--all` puts the one used last first.
    pub last: String,
    /// Calls whose line is in; the call asking for the stats is not one.
    pub calls: usize,
    pub failed: usize,
    pub raw_calls: usize,
    /// Characters the calls returned, with the middle and the long ones.
    pub chars: u64,
    pub median: u64,
    pub p90: u64,
    /// What the filters kept out, and what the commands wrote before them.
    pub cut: u64,
    pub before_filters: u64,
}

/// The numbers of every session, the one used last first, or of one session.
pub fn stats(project: &Path, session: Option<&str>) -> Vec<Stats> {
    let mut out = Vec::new();
    for id in session_ids(project) {
        if session.is_some_and(|s| s != id) {
            continue;
        }
        let rows = calls(project, &id);
        if rows.is_empty() {
            continue;
        }
        out.push(add_up(id, &rows));
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| b.session.cmp(&a.session)));
    out
}

/// The whole log as one session, `total`: the calls of every session, from
/// the first call of the log to the last.
pub fn total(project: &Path) -> Stats {
    let rows: Vec<Value> = session_ids(project).iter().flat_map(|id| calls(project, id)).collect();
    let mut s = add_up("total".into(), &rows);
    let times = || rows.iter().map(|r| ts_of(Some(r))).filter(|t| !t.is_empty());
    s.ts = times().min().unwrap_or_default();
    s.last = times().max().unwrap_or_default();
    s
}

/// What the calls of a session add up to.
fn add_up(session: String, rows: &[Value]) -> Stats {
    let num = |e: &Value, k: &str| e.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let mut s = Stats { session, ts: ts_of(rows.first()), last: ts_of(rows.last()), ..Stats::default() };
    let mut sizes: Vec<u64> = Vec::new();
    for c in rows {
        s.calls += 1;
        if c.get("refused").is_some() || failed(c).contains(&true) {
            s.failed += 1;
        }
        // A raw read is the call harno log raw <n>: the verb and the first word after it.
        let raw = c.get("verb").and_then(|v| v.as_str()) == Some("log")
            && c.get("args").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.as_str()) == Some("raw");
        if raw {
            s.raw_calls += 1;
        }
        let read = num(c, "filtered");
        let wrote = if c.get("output").is_some() { num(c, "output") } else { read };
        s.chars += read;
        sizes.push(read);
        s.before_filters += wrote;
        s.cut += wrote.saturating_sub(read);
    }
    sizes.sort_unstable();
    if !sizes.is_empty() {
        s.median = sizes[sizes.len() / 2];
        s.p90 = sizes[(sizes.len() * 9 / 10).min(sizes.len() - 1)];
    }
    s
}

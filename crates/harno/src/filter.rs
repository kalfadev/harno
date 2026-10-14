//! Output filters for `harno run`: what a command's output looks like before
//! the agent reads it. The developer writes them under `run.filters` in the
//! config, inline, as a file, or as the `filter` of a tool folder
//! ([`crate::tool`]) with the scripts it runs.
//!
//! A filter is either fields or a script. Fields are applied in one fixed
//! order, whatever order they are written in:
//!
//! 1. ANSI colour codes are removed; a line rewritten with `\r` keeps its
//!    last form
//! 2. `replace` — every match of a pattern becomes its text, pair by pair
//! 3. `drop`
//! 4. `keep`
//! 5. `dedupe` — repeated neighbouring lines, written once with `(×N)`
//! 6. `head`, `tail`
//! 7. `width` — a line longer than it keeps its first characters and counts
//!    the rest, the way the header of a call counts a long command
//! 8. `on_empty`
//!
//! A filter never loses output: whatever it removes stays in the raw
//! output, and when the result would be worse than the raw output (longer,
//! empty, a failed script) the raw output is shown instead.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;

use crate::output::{Refusal, R};
use crate::run::{argv, split, Word};
use crate::tool::MANIFEST;

/// The key whose filter applies to every command.
pub const EVERY: &str = "*";

/// One filter as written in the config or in a filter file.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// In the `tool.yml` of a tool: the command the filter is written for,
    /// which `harno tool ls` and `harno tool add` show. Ignored elsewhere.
    #[serde(default)]
    pub command: Option<String>,
    /// The stream the fields or the script read, `stdout` or `stderr`; the
    /// other passes whole. Without it the two are one text, in the order
    /// they were written. Named for what the filter does, as `keep` and
    /// `drop` are: `stream` read as streaming the output.
    #[serde(default)]
    pub reads: Option<String>,
    /// Regex substitutions, in order, on every line before `drop` reads it.
    #[serde(default)]
    pub replace: Option<Vec<Replace>>,
    #[serde(default)]
    pub drop: Option<Vec<String>>,
    #[serde(default)]
    pub keep: Option<Vec<String>>,
    #[serde(default)]
    pub dedupe: Option<bool>,
    #[serde(default)]
    pub head: Option<usize>,
    #[serde(default)]
    pub tail: Option<usize>,
    /// Characters a line may hold. A line-based filter cannot reach inside a
    /// single long line, and one line is all some commands write.
    #[serde(default)]
    pub width: Option<usize>,
    #[serde(default)]
    pub on_empty: Option<String>,
    /// Exit codes of this command that are no failure of the call: `rg`
    /// writes no match and exits 1, which is an answer, not a break.
    #[serde(default)]
    pub ok: Option<Vec<i32>>,
    #[serde(default)]
    pub script: Option<String>,
    #[serde(default)]
    pub on_fail: Option<Box<Spec>>,
    #[serde(default)]
    pub tests: Vec<Test>,
    /// The folder of the tool a filter came from: its script runs there.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// A pair of `replace`: every match of `pattern` in a line becomes `with`,
/// where `$1` is the first group.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    pub pattern: String,
    pub with: String,
}

/// An example a filter must turn into the expected text; `harno tool test`
/// runs them. `exit` is the exit code the example pretends the command had.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Test {
    /// What the command wrote: stdout, when the filter names a stream.
    pub input: String,
    /// What it wrote to stderr, for a filter of one stream.
    #[serde(default)]
    pub stderr: String,
    pub expected: String,
    #[serde(default)]
    pub exit: i32,
}

impl Spec {
    fn has_fields(&self) -> bool {
        self.replace.is_some()
            || self.drop.is_some()
            || self.keep.is_some()
            || self.dedupe.is_some()
            || self.head.is_some()
            || self.tail.is_some()
            || self.width.is_some()
            || self.on_empty.is_some()
    }

    /// The filter itself, without what only the top level carries.
    fn fields(&self) -> Spec {
        Spec { command: None, on_fail: None, tests: Vec::new(), ..self.clone() }
    }

    fn validate(&self, name: &str, top: bool) -> R<()> {
        let bad = |why: String| Refusal::new(format!("filter '{name}': {why}"));
        if self.script.is_some() && self.has_fields() {
            return Err(bad("a filter is either a script or fields, not both".into()));
        }
        if !top && (self.on_fail.is_some() || !self.tests.is_empty() || self.command.is_some() || self.ok.is_some()) {
            return Err(bad("on_fail takes filter fields or a script, not on_fail, tests, command or ok".into()));
        }
        if self.ok.iter().flatten().any(|c| *c == 0) {
            return Err(bad("ok takes the exit codes that are no failure, and 0 is never one of them".into()));
        }
        if let Some(s) = self.reads.as_deref().filter(|s| *s != "stdout" && *s != "stderr") {
            return Err(bad(format!("reads is stdout or stderr, not {s:?}")));
        }
        let mut patterns: Vec<&String> = Vec::new();
        patterns.extend(self.replace.iter().flatten().map(|r| &r.pattern));
        patterns.extend(self.drop.iter().flatten());
        patterns.extend(self.keep.iter().flatten());
        for p in patterns {
            Regex::new(p).map_err(|e| bad(format!("bad regex {p:?}: {e}")))?;
        }
        if let Some(f) = &self.on_fail {
            f.validate(name, false)?;
        }
        Ok(())
    }
}

/// A filter from `run.filters`, resolved and checked.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The command it matches, or `*`.
    pub key: String,
    /// The file it came from, as the config wrote it; `None` when inline.
    pub path: Option<String>,
    pub spec: Spec,
    /// The key split into words, for matching.
    words: Vec<Word>,
}

#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub entries: Vec<Entry>,
}

/// Read `run.filters`: every value is a filter, the path of a tool folder
/// (the `filter` of its `tool.yml`), or the path of a filter file. A file
/// that is missing, a field harno does not know, or a regex that does not
/// compile refuses the config.
pub fn resolve(project: &Path, raw: &BTreeMap<String, serde_json::Value>) -> R<Filters> {
    let mut entries = Vec::new();
    for (key, value) in raw {
        let (spec, path) = match value {
            serde_json::Value::String(path) => {
                let full = project.join(path);
                let (file, dir) = if full.is_dir() { (full.join(MANIFEST), Some(full)) } else { (full, None) };
                let text = std::fs::read_to_string(&file).map_err(|e| {
                    Refusal::new(format!("filter '{key}': cannot read {}: {e}", file.strip_prefix(project).unwrap_or(&file).display()))
                })?;
                let mut spec = match &dir {
                    Some(_) => crate::tool::parse(&text)
                        .map_err(|e| Refusal::new(format!("filter '{key}': {path}/{}: {e}", MANIFEST)))?
                        .filter
                        .ok_or_else(|| Refusal::new(format!("filter '{key}': {path}/{} carries no filter", MANIFEST)))?,
                    None => parse_spec(&text).map_err(|e| Refusal::new(format!("filter '{key}': {path}: {e}")))?,
                };
                if let Some(f) = spec.on_fail.as_mut() {
                    f.dir = dir.clone();
                }
                spec.dir = dir;
                (spec, Some(path.clone()))
            }
            v => (
                serde_json::from_value(v.clone()).map_err(|e| Refusal::new(format!("filter '{key}': {e}")))?,
                None,
            ),
        };
        spec.validate(key, true)?;
        let words = if key == EVERY { Vec::new() } else { split(key)? };
        if key != EVERY && words.is_empty() {
            return Err(Refusal::new("run.filters has an empty command"));
        }
        entries.push(Entry { key: key.clone(), path, spec, words });
    }
    Ok(Filters { entries })
}

pub fn parse_spec(text: &str) -> Result<Spec, String> {
    serde_saphyr::from_str(text).map_err(crate::config::yaml_error)
}

impl Filters {
    fn every(&self) -> Option<&Spec> {
        self.entries.iter().find(|e| e.key == EVERY).map(|e| &e.spec)
    }

    /// Whether an exit code breaks the call. The `ok` of the filter of that
    /// command names the codes that do not: a command asked a question and
    /// answered it. Every other non-zero code is a failure.
    pub fn broke(&self, words: &[Word], exit: i32) -> bool {
        exit != 0 && !self.entry_for(words).and_then(|e| e.spec.ok.as_ref()).is_some_and(|codes| codes.contains(&exit))
    }

    /// Whether a filter this command may get, as it succeeds or fails, reads
    /// one stream: the command then runs with stdout and stderr apart.
    pub fn apart(&self, words: &[Word]) -> bool {
        [false, true].into_iter().any(|failed| self.choose(words, failed).is_some_and(|s| s.reads.is_some()))
    }

    /// The entry for a command: the longest key that is a word-by-word
    /// prefix of it, the way `run.allow` matches.
    pub fn entry_for(&self, words: &[Word]) -> Option<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.key != EVERY && prefix_of(&e.words, words))
            .max_by_key(|e| e.words.len())
    }

    /// The filter a command's output goes through, `None` when there is
    /// none. A command's own filter overrides `*` field by field; on a
    /// failure its `on_fail` replaces it.
    pub fn choose(&self, words: &[Word], failed: bool) -> Option<Spec> {
        let every = self.every();
        let base = every.map(|s| s.fields()).unwrap_or_default();
        let own = self.entry_for(words).map(|e| &e.spec);
        let spec = match (own, failed) {
            (Some(o), true) => merge(&base, o.on_fail.as_deref().unwrap_or(o)),
            (Some(o), false) => merge(&base, o),
            (None, true) => match every.and_then(|e| e.on_fail.as_deref()) {
                Some(f) => merge(&base, f),
                None => base,
            },
            (None, false) => base,
        };
        (spec.script.is_some() || spec.has_fields()).then_some(spec)
    }
}

/// `over`'s fields, and `base`'s where `over` leaves one out. A script is
/// never mixed with fields: a filter with a script stands alone, and fields
/// never inherit `base`'s script.
fn merge(base: &Spec, over: &Spec) -> Spec {
    if over.script.is_some() {
        return over.fields();
    }
    if base.script.is_some() {
        return Spec { script: None, ..over.fields() };
    }
    let o = over.fields();
    Spec {
        command: None,
        reads: o.reads.or_else(|| base.reads.clone()),
        replace: o.replace.or_else(|| base.replace.clone()),
        drop: o.drop.or_else(|| base.drop.clone()),
        keep: o.keep.or_else(|| base.keep.clone()),
        dedupe: o.dedupe.or(base.dedupe),
        head: o.head.or(base.head),
        tail: o.tail.or(base.tail),
        width: o.width.or(base.width),
        on_empty: o.on_empty.or_else(|| base.on_empty.clone()),
        ok: None,
        script: None,
        on_fail: None,
        tests: Vec::new(),
        dir: None,
    }
}

fn prefix_of(entry: &[Word], words: &[Word]) -> bool {
    !entry.is_empty()
        && entry.len() <= words.len()
        && entry.iter().zip(words).all(|(a, w)| match &a.pattern {
            Some(p) => glob::Pattern::new(p).is_ok_and(|g| g.matches(&w.text)),
            None => a.text == w.text,
        })
}

/// What a filter made of one output.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filtered {
    /// The stream a filter of the other one passes whole: stdout, which
    /// comes before what the filter made, or stderr, which comes after.
    pub before: String,
    pub after: String,
    /// The lines the agent reads, without the note about removed lines.
    pub lines: Vec<String>,
    /// Where the note goes: before this line (the cut `head`/`tail` made),
    /// or at the end when `None`.
    pub cut_at: Option<usize>,
    /// Lines of the raw output that are not in `lines`.
    pub removed: usize,
    /// Lines that kept their first `width` characters and lost the rest.
    pub narrowed: usize,
}

/// The outcome of filtering one output.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The raw output is shown as it is.
    Raw { warning: Option<String> },
    Filtered(Filtered),
}

/// Filter `raw`, the output of a command that exited with `exit`; with
/// `streams`, what it wrote to stdout and to stderr apart.
pub fn apply(spec: &Spec, raw: &str, streams: Option<&(String, String)>, exit: i32, project: &Path, timeout: Duration, call: u64) -> Outcome {
    let (text, before, after) = parts(spec, raw, streams.map(|(o, e)| (o.as_str(), e.as_str())));
    let filtered = match transform(spec, text, exit, project, timeout) {
        Outcome::Filtered(f) => Filtered { before: before.to_string(), after: after.to_string(), ..f },
        raw_output => return raw_output,
    };
    // Never worse than the raw output: a result as long as the raw output
    // (its note counted), or the same text, shows the raw output. A command
    // that wrote nothing is outside the rule: there is nothing to be worse
    // than, and an on_empty text is what says the silence was the answer.
    let probe = render(&filtered, call);
    if !raw.trim().is_empty() && (probe == raw || probe.chars().count() >= raw.chars().count()) {
        return Outcome::Raw { warning: None };
    }
    Outcome::Filtered(filtered)
}

/// What the filter makes of `raw`, before the length is compared.
fn transform(spec: &Spec, raw: &str, exit: i32, project: &Path, timeout: Duration) -> Outcome {
    let raw_lines = raw.lines().count();
    let filtered = match &spec.script {
        Some(script) => match run_script(script, spec.dir.as_deref().unwrap_or(project), raw, exit, timeout) {
            Ok(out) if !out.trim().is_empty() => {
                let lines: Vec<String> = out.lines().map(|l| l.to_string()).collect();
                let removed = raw_lines.saturating_sub(lines.len());
                Filtered { lines, cut_at: None, removed, narrowed: 0, ..Filtered::default() }
            }
            Ok(_) => {
                return Outcome::Raw { warning: Some(format!("warning: filter script {script} printed nothing — output unfiltered")) }
            }
            Err(why) => {
                return Outcome::Raw { warning: Some(format!("warning: filter script {script} {why} — output unfiltered")) }
            }
        },
        None => match fields(spec, raw) {
            Some(f) => f,
            None => return Outcome::Raw { warning: None },
        },
    };
    Outcome::Filtered(filtered)
}

/// The fields, in their fixed order. `None` when nothing is left and there
/// is no `on_empty`: the raw output is shown instead.
fn fields(spec: &Spec, raw: &str) -> Option<Filtered> {
    let re = |ps: &Option<Vec<String>>| -> Vec<Regex> {
        ps.iter().flatten().filter_map(|p| Regex::new(p).ok()).collect()
    };
    let total = raw.lines().count();

    // 1. What a terminal would show: no colour codes, the last form of a
    // line rewritten with `\r`.
    let mut lines: Vec<String> = raw.lines().map(clean).collect();

    // 2. replace: each pair on what the one before it left, so the line
    // fields read the short form.
    for p in spec.replace.iter().flatten() {
        let Ok(r) = Regex::new(&p.pattern) else { continue };
        for l in lines.iter_mut() {
            *l = r.replace_all(l, p.with.as_str()).into_owned();
        }
    }

    // 3. drop
    let drop = re(&spec.drop);
    lines.retain(|l| !drop.iter().any(|r| r.is_match(l)));

    // 4. keep
    let keep = re(&spec.keep);
    if !keep.is_empty() {
        lines.retain(|l| keep.iter().any(|r| r.is_match(l)));
    }

    // 5. dedupe: a line repeated on the lines right after it is written
    // once. Only neighbours: the same line further on may mean something
    // else (a second `failures:`, a closing brace).
    if spec.dedupe == Some(true) {
        let mut out: Vec<(String, usize)> = Vec::new();
        for l in lines {
            match out.last_mut() {
                Some((prev, n)) if *prev == l && !l.trim().is_empty() => *n += 1,
                _ => out.push((l, 1)),
            }
        }
        lines = out.into_iter().map(|(l, n)| if n > 1 { format!("{l} (×{n})") } else { l }).collect();
    }

    // 6. head, tail
    let head = spec.head.unwrap_or(0);
    let tail = spec.tail.unwrap_or(0);
    let mut cut_at = None;
    if (head > 0 || tail > 0) && lines.len() > head + tail {
        let mut kept: Vec<String> = lines[..head].to_vec();
        kept.extend_from_slice(&lines[lines.len() - tail..]);
        cut_at = Some(head);
        lines = kept;
    }

    // 7. width: a line-based filter cannot reach inside one long line, and
    // the line stays a line — its first characters, then its length, as a
    // header counts a long command.
    let mut narrowed = 0;
    if let Some(width) = spec.width.filter(|w| *w > 0) {
        for l in lines.iter_mut() {
            let chars = l.chars().count();
            if chars > width {
                let cut: String = l.chars().take(width).collect();
                *l = format!("{} … {chars} chars", cut.trim_end());
                narrowed += 1;
            }
        }
    }

    // 8. on_empty
    let kept = lines.len();
    if lines.iter().all(|l| l.trim().is_empty()) {
        let text = spec.on_empty.as_ref()?;
        // The text stands alone: on_empty is a project saying that this
        // output holds nothing to read, so the note that offers it back
        // would be the longer half of the answer. The log keeps it.
        return Some(Filtered { lines: text.lines().map(|l| l.to_string()).collect(), ..Filtered::default() });
    }
    Some(Filtered { lines, cut_at, removed: total.saturating_sub(kept), narrowed, ..Filtered::default() })
}

/// A line as a terminal would show it.
fn clean(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.rsplit('\r').next().unwrap_or(line);
    strip_ansi(line)
}

/// Remove ANSI escape sequences: CSI (`\x1b[…m`), OSC (`\x1b]…\x07`) and
/// two-character escapes.
pub fn strip_ansi(s: &str) -> String {
    if !s.contains('\x1b') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates, then one final byte.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The text the agent reads: the filtered lines, and where lines were
/// removed, how to get them back.
pub fn render(f: &Filtered, call: u64) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let what = match (f.removed, f.narrowed) {
        (0, n) => format!("{n} long line{} cut", plural(n)),
        (r, 0) => format!("{r} line{}", plural(r)),
        (r, n) => format!("{r} line{}, {n} cut", plural(r)),
    };
    let note = format!("… {what}: harno log raw {call}");
    let mut lines: Vec<&str> = f.lines.iter().map(|s| s.as_str()).collect();
    if f.removed > 0 || f.narrowed > 0 {
        let at = f.cut_at.unwrap_or(lines.len()).min(lines.len());
        lines.insert(at, &note);
    }
    let mut s = lines.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    // The stream the filter left alone, whole, in its place: stdout before, stderr after.
    format!("{}{s}{}", ended(&f.before), ended(&f.after))
}

/// `text` ending with a newline, when it holds any text.
fn ended(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

/// The text a filter reads, and the text before and after it that passes
/// whole: with `reads` and the two streams apart, the stream it names; else
/// all of `raw`.
fn parts<'a>(spec: &Spec, raw: &'a str, streams: Option<(&'a str, &'a str)>) -> (&'a str, &'a str, &'a str) {
    match (spec.reads.as_deref(), streams) {
        (Some("stdout"), Some((out, err))) => (out, "", err),
        (Some("stderr"), Some((out, err))) => (err, out, ""),
        _ => (raw, "", ""),
    }
}

/// What a filter makes of a test's input: the filtered lines without the
/// note, or the input itself when nothing is left or a script fails. The
/// length rule is left out: it depends on the size of the real output, and
/// a test's input is a short example.
pub fn test_output(spec: &Spec, input: &str, stderr: &str, exit: i32, project: &Path, timeout: Duration) -> String {
    let merged = format!("{input}{stderr}");
    let (text, before, after) = parts(spec, &merged, Some((input, stderr)));
    match transform(spec, text, exit, project, timeout) {
        Outcome::Filtered(f) => {
            let mut s = f.lines.join("\n");
            if !s.is_empty() {
                s.push('\n');
            }
            format!("{}{s}{}", ended(before), ended(after))
        }
        Outcome::Raw { .. } => merged,
    }
}

/// A test that did not give what it expected.
pub struct Failure {
    pub filter: String,
    pub test: usize,
    pub expected: String,
    pub got: String,
}

/// Run every filter's tests. A test runs the filter as `run` would choose
/// it for that exit code: `on_fail` for a non-zero `exit`, and the fields
/// `*` gives.
pub fn run_tests(filters: &Filters, project: &Path, timeout: Duration) -> (usize, Vec<Failure>) {
    let mut ran = 0;
    let mut failures = Vec::new();
    for e in &filters.entries {
        for (i, t) in e.spec.tests.iter().enumerate() {
            ran += 1;
            let spec = if e.key == EVERY {
                filters.choose(&[], t.exit != 0)
            } else {
                filters.choose(&e.words, t.exit != 0)
            };
            let got = match spec {
                Some(s) => test_output(&s, &t.input, &t.stderr, t.exit, project, timeout),
                None => format!("{}{}", t.input, t.stderr),
            };
            if got.trim_end_matches('\n') != t.expected.trim_end_matches('\n') {
                let name = match &e.path {
                    Some(p) => format!("{} ({p})", e.key),
                    None => e.key.clone(),
                };
                failures.push(Failure { filter: name, test: i + 1, expected: t.expected.clone(), got });
            }
        }
    }
    (ran, failures)
}

/// Run a filter script in `dir`, its folder or the project: the output on
/// stdin, the exit code in `HARNO_EXIT`, the filtered output on stdout.
fn run_script(script: &str, dir: &Path, input: &str, exit: i32, timeout: Duration) -> Result<String, String> {
    let project = dir;
    let words = split(script).map_err(|e| e.reason)?;
    if words.is_empty() {
        return Err("is empty".into());
    }
    let args = argv(&words, project);
    let program = if args[0].contains('/') && !Path::new(&args[0]).is_absolute() {
        project.join(&args[0]).to_string_lossy().to_string()
    } else {
        args[0].clone()
    };
    let mut child = Command::new(&program)
        .args(&args[1..])
        .current_dir(project)
        .env("HARNO_EXIT", exit.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("did not start ({e})"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = input.to_string();
    std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if !timeout.is_zero() && started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(format!("failed ({e})")),
        }
    };
    let out = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    if !status.success() {
        return Err(format!("failed (exit {})", status.code().unwrap_or(1)));
    }
    Ok(String::from_utf8_lossy(&out).to_string())
}

/// A config key as YAML writes it: plain when that reads back the same.
pub fn yaml_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key.chars().all(|c| c.is_alphanumeric() || " -_./+".contains(c))
        && !key.starts_with(['-', ' '])
        && !key.ends_with(' ');
    if plain {
        key.to_string()
    } else {
        serde_json::to_string(key).unwrap()
    }
}

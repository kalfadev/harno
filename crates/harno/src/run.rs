//! `harno run`: every command the agent runs goes through here.
//!
//! A command is an argument vector, never a shell string: `&&`, `|`, `$VAR`
//! and `;` are plain arguments. The string is split the way a shell would
//! split words (quotes group, backslash escapes), and an unquoted `*`, `?`
//! or `[` is expanded as a glob against the project directory.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::config::{Allow, Config, Preset, ARGS};
use crate::filter::{self, Outcome};
use crate::output::{Output, Refusal, R};
use crate::sandbox::Sandbox;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    /// The word with quotes removed.
    pub text: String,
    /// A glob pattern when the word has an unquoted wildcard.
    pub pattern: Option<String>,
}

pub fn split(s: &str) -> R<Vec<Word>> {
    let mut words = Vec::new();
    let mut cur: Option<(String, String, bool)> = None; // text, pattern, has wildcard
    let mut chars = s.chars().peekable();
    let push = |cur: &mut Option<(String, String, bool)>, c: char, quoted: bool| {
        let w = cur.get_or_insert_with(|| (String::new(), String::new(), false));
        w.0.push(c);
        if !quoted && matches!(c, '*' | '?' | '[') {
            w.1.push(c);
            w.2 = true;
        } else {
            w.1.push_str(&glob::Pattern::escape(&c.to_string()));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if let Some((t, p, wild)) = cur.take() {
                    words.push(Word { text: t, pattern: wild.then_some(p) });
                }
            }
            '\'' => {
                cur.get_or_insert_with(|| (String::new(), String::new(), false));
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => push(&mut cur, c, true),
                        None => return Err(Refusal::new(format!("unclosed ' in {s:?}"))),
                    }
                }
            }
            '"' => {
                cur.get_or_insert_with(|| (String::new(), String::new(), false));
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => push(&mut cur, c, true),
                            Some(c) => {
                                push(&mut cur, '\\', true);
                                push(&mut cur, c, true);
                            }
                            None => return Err(Refusal::new(format!("unclosed \" in {s:?}"))),
                        },
                        Some(c) => push(&mut cur, c, true),
                        None => return Err(Refusal::new(format!("unclosed \" in {s:?}"))),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(c) => push(&mut cur, c, true),
                None => push(&mut cur, '\\', true),
            },
            c => push(&mut cur, c, false),
        }
    }
    if let Some((t, p, wild)) = cur.take() {
        words.push(Word { text: t, pattern: wild.then_some(p) });
    }
    Ok(words)
}

/// The argument vector: globs expanded against `dir`, sorted; a glob that
/// matches nothing stays as written.
pub fn argv(words: &[Word], dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for w in words {
        let Some(p) = &w.pattern else {
            out.push(w.text.clone());
            continue;
        };
        let full = if Path::new(p).is_absolute() { p.clone() } else { format!("{}/{p}", glob::Pattern::escape(&dir.to_string_lossy())) };
        let mut matches: Vec<String> = glob::glob(&full)
            .map(|paths| {
                paths
                    .flatten()
                    .map(|m| {
                        if Path::new(p).is_absolute() {
                            m.to_string_lossy().to_string()
                        } else {
                            m.strip_prefix(dir).unwrap_or(&m).to_string_lossy().to_string()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        matches.sort();
        if matches.is_empty() {
            out.push(w.text.clone());
        } else {
            out.extend(matches);
        }
    }
    out
}

/// Word-by-word prefix match: `cargo build` lets `cargo build --release`
/// through but not `cargo buildx`. A word with a wildcard in the allow entry
/// is a glob. Of the entries that match, the longest: its timeout counts.
pub fn allowed<'a>(allow: &'a [Allow], words: &[Word]) -> Option<&'a Allow> {
    allow
        .iter()
        .filter(|entry| {
            let Ok(e) = split(&entry.command) else { return false };
            !e.is_empty()
                && e.len() <= words.len()
                && e.iter().zip(words).all(|(a, w)| match &a.pattern {
                    Some(p) => glob::Pattern::new(p).is_ok_and(|g| g.matches(&w.text)),
                    None => a.text == w.text,
                })
        })
        .max_by_key(|entry| split(&entry.command).map(|e| e.len()).unwrap_or(0))
}

/// The refusal of words given apart that make one allowed command, `harno run
/// rg -n x src`: the caller left out the quotes that make them one.
fn one_command(config: &Config, items: &[String]) -> Option<Refusal> {
    let line = items.iter().map(|w| crate::output::quote(w)).collect::<Vec<_>>().join(" ");
    allowed(&config.run.allow, &split(&line).ok()?)?;
    Some(Refusal::new(format!("'{line}' is one command, given as {} words", items.len())).instead(format!("harno run {}", quote_always(&line))))
}

/// Why a command is refused: the program when nothing in the list names it,
/// otherwise the whole command.
pub fn refusal_name(allow: &[Allow], words: &[Word], raw: &str) -> String {
    let prog = words.first().map(|w| w.text.as_str()).unwrap_or("");
    let named = allow.iter().any(|e| split(&e.command).ok().and_then(|v| v.first().cloned()).is_some_and(|w| w.text == prog));
    if named {
        raw.to_string()
    } else {
        prog.to_string()
    }
}

/// How long a command may run: the timeout of the allow entry that matches
/// it, else `run.timeout`. A command of a preset or a verify line takes it
/// too, though a preset skips the allow check.
pub fn timeout_of(config: &Config, command: &str) -> Duration {
    let own = split(command).ok().and_then(|w| allowed(&config.run.allow, &w).and_then(|a| a.timeout));
    Duration::from_secs(own.unwrap_or(config.run.timeout))
}

#[derive(Debug, Clone)]
pub struct Ran {
    pub index: usize,
    pub command: String,
    pub code: i32,
    /// The command broke the call: a non-zero exit its filter does not call
    /// `ok`. The code itself stays as it was, in the line and in the log.
    pub failed: bool,
    pub status: String,
    pub output: String,
    pub secs: f64,
    /// What the command wrote, when a filter changed its output.
    pub raw: Option<String>,
    /// What it wrote to stdout and to stderr, when a filter of one stream
    /// had it run with the two apart.
    pub streams: Option<(String, String)>,
}

/// A piece of what a command wrote, and whether it came on stderr.
type Chunk = (bool, Vec<u8>);

/// The terminal of a human who watches a call, or what stands in for it.
pub type Screen = Arc<Mutex<dyn Write + Send>>;

/// Where the output of a call goes while its commands run: the raw file of
/// the call, which `harno log raw <n>` reads before the call ends, and the
/// terminal when a human watches it. A line goes out whole, under the header
/// of its command when the call runs several and another one wrote last.
#[derive(Clone)]
pub struct Live(Arc<Mutex<Sink>>);

struct Sink {
    file: Option<File>,
    screen: Option<Screen>,
    watched: bool,
    headers: bool,
    last: Option<usize>,
}

impl Live {
    /// None when there is neither a file nor a human to write to.
    pub fn new(file: Option<File>, screen: Option<Screen>) -> Option<Live> {
        let watched = screen.is_some();
        (file.is_some() || watched).then(|| Live(Arc::new(Mutex::new(Sink { file, screen, watched, headers: false, last: None }))))
    }

    /// A human reads the output as it comes: the call neither filters it
    /// nor prints it again.
    pub fn watched(&self) -> bool {
        self.0.lock().unwrap().watched
    }

    fn several(&self, yes: bool) {
        self.0.lock().unwrap().headers = yes;
    }

    /// The commands ended: nothing written after reaches the file or the
    /// terminal, since a process they left behind is not the call.
    fn close(&self) {
        let mut s = self.0.lock().unwrap();
        s.file = None;
        s.screen = None;
    }

    fn write(&self, index: usize, command: &str, lines: &[u8]) {
        let mut s = self.0.lock().unwrap();
        if s.file.is_none() && s.screen.is_none() {
            return;
        }
        let mut text = Vec::new();
        if s.headers && s.last != Some(index) {
            text.extend_from_slice(format!("--- {index}: {}\n", headline(command, HEADER)).as_bytes());
        }
        s.last = Some(index);
        text.extend_from_slice(lines);
        if let Some(f) = s.file.as_mut() {
            let _ = f.write_all(&text);
        }
        if let Some(screen) = &s.screen {
            let mut out = screen.lock().unwrap();
            let _ = out.write_all(&text);
            let _ = out.flush();
        }
    }
}

/// Run one command. stdout and stderr share one pipe, so the output reads in
/// the order it was written; `apart` gives each its own pipe, and the output
/// is the two in the order their chunks arrived, with each stream on its own
/// as well. With a sandbox the command writes in the project and nowhere else.
/// With `live` each whole line goes there as well, as it comes. A timeout of
/// zero is none: the command runs until it ends or is stopped.
pub fn exec(command: &str, dir: &Path, timeout: Duration, apart: bool, sandbox: Option<&Sandbox>, live: Option<(&Live, usize)>) -> (i32, String, String, f64, Option<(String, String)>) {
    let started = Instant::now();
    let words = match split(command) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => return (2, "FAIL empty command".into(), String::new(), 0.0, None),
        Err(e) => return (2, format!("FAIL {}", e.reason), String::new(), 0.0, None),
    };
    let args = argv(&words, dir);
    let program = if args[0].contains('/') && !Path::new(&args[0]).is_absolute() {
        dir.join(&args[0]).to_string_lossy().to_string()
    } else {
        args[0].clone()
    };
    let name = args[0].clone();
    // The sandbox starts the program, so the name is looked up here: a
    // command that is not there answers the way it always did.
    let (program, args) = match sandbox {
        Some(s) => match which(&program) {
            Some(found) => s.command(&found.to_string_lossy(), &args[1..]),
            None => return (127, "FAIL not found".into(), format!("{name}: command not found\n"), secs(started), None),
        },
        None => (program, args[1..].to_vec()),
    };
    let (reader, writer) = match std::io::pipe() {
        Ok(p) => p,
        Err(e) => return (1, format!("FAIL {e}"), String::new(), 0.0, None),
    };
    let mut cmd = Command::new(&program);
    cmd.args(&args).current_dir(dir).stdin(Stdio::null());
    crate::signals::own_group(&mut cmd);
    if let Some(s) = sandbox {
        if let Err(e) = s.restrict(&mut cmd) {
            return (126, format!("FAIL sandbox: {e}"), String::new(), secs(started), None);
        }
    }
    // Each reader is a pipe and whether it is stderr. One pipe keeps the
    // order the two streams were written in exactly; two tell them apart,
    // in the order their chunks arrive.
    let mut readers = vec![(false, reader)];
    if apart {
        let (err_reader, err_writer) = match std::io::pipe() {
            Ok(p) => p,
            Err(e) => return (1, format!("FAIL {e}"), String::new(), 0.0, None),
        };
        readers.push((true, err_reader));
        cmd.stdout(writer);
        cmd.stderr(err_writer);
    } else {
        match writer.try_clone() {
            Ok(w2) => {
                cmd.stdout(w2);
                cmd.stderr(writer);
            }
            Err(e) => return (1, format!("FAIL {e}"), String::new(), 0.0, None),
        }
    }
    let child = cmd.spawn();
    drop(cmd);
    let mut child = match child {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (127, "FAIL not found".into(), format!("{name}: command not found\n"), secs(started), None)
        }
        Err(e) => return (126, format!("FAIL {e}"), String::new(), secs(started), None),
    };
    crate::signals::hold(child.id());
    let buf: Arc<Mutex<Vec<Chunk>>> = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    let pipes = readers.len();
    for (is_err, mut reader) in readers {
        let (buf, tx) = (buf.clone(), tx.clone());
        let live = live.map(|(l, index)| (l.clone(), index, command.to_string()));
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            // What came after the last newline, held until its line is whole.
            let mut part: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buf.lock().unwrap().push((is_err, chunk[..n].to_vec()));
                        if let Some((l, index, command)) = &live {
                            part.extend_from_slice(&chunk[..n]);
                            if let Some(end) = part.iter().rposition(|b| *b == b'\n') {
                                l.write(*index, command, &part[..=end]);
                                part = part.split_off(end + 1);
                            }
                        }
                    }
                }
            }
            if let Some((l, index, command)) = &live {
                if !part.is_empty() {
                    part.push(b'\n');
                    l.write(*index, command, &part);
                }
            }
            let _ = tx.send(());
        });
    }
    drop(tx);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if !timeout.is_zero() && started.elapsed() >= timeout {
                    // The command and all it started: one left would hold its output open.
                    crate::signals::kill_group(child.id());
                    let _ = child.kill();
                    timed_out = true;
                    break child.wait().ok();
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => break None,
        }
    };
    crate::signals::release(child.id());
    // A call stopped by a signal leaves nothing of its commands behind, a
    // process that ignored the signal included.
    if crate::signals::stopped() {
        crate::signals::kill_group(child.id());
    }
    // A background grandchild may hold a pipe open; do not wait for it.
    let until = Instant::now() + Duration::from_millis(if timed_out { 100 } else { 2000 });
    for _ in 0..pipes {
        if rx.recv_timeout(until.saturating_duration_since(Instant::now())).is_err() {
            break;
        }
    }
    let chunks = buf.lock().unwrap();
    let text = |stderr: Option<bool>| {
        let bytes: Vec<u8> = chunks.iter().filter(|(e, _)| stderr.is_none_or(|s| *e == s)).flat_map(|(_, b)| b.iter().copied()).collect();
        String::from_utf8_lossy(&bytes).to_string()
    };
    let output = text(None);
    let streams = apart.then(|| (text(Some(false)), text(Some(true))));
    let elapsed = secs(started);
    if timed_out {
        return (124, "FAIL timeout".into(), output, elapsed, streams);
    }
    let code = status.map(crate::signals::code).unwrap_or(1);
    let st = if code == 0 { "ok".to_string() } else { format!("FAIL exit {code}") };
    (code, st, output, elapsed, streams)
}

/// Where a program is: a path as it is written, a name looked up in PATH.
/// Windows names a program with its extension, `cargo.exe`: there every
/// extension of PATHEXT is tried after the bare name.
fn which(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if program.contains('/') || program.contains(std::path::MAIN_SEPARATOR) {
        return path.is_file().then(|| path.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()).split(';').filter(|e| !e.is_empty()).map(String::from).collect()
    } else {
        Vec::new()
    };
    let var = std::env::var_os("PATH")?;
    std::env::split_paths(&var).find_map(|d| {
        std::iter::once(d.join(program)).chain(exts.iter().map(|e| d.join(format!("{program}{e}")))).find(|c| c.is_file())
    })
}

fn secs(started: Instant) -> f64 {
    started.elapsed().as_secs_f64()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Seq,
    Par,
}

pub struct RunArgs {
    pub mode: Option<Mode>,
    pub commands: Vec<String>,
    pub raw: bool,
    pub why: Option<String>,
    /// The words after `--`: arguments for a preset {args}.
    pub args: Option<Vec<String>>,
    /// The number of the call in the log of the gate, which a filtered
    /// output names so that the agent can read what was cut.
    pub call: Option<u64>,
    /// Where the output goes while the commands run.
    pub live: Option<Live>,
}

pub fn run(project: &Path, config: &Config, a: RunArgs, json_out: bool) -> R<Output> {
    if a.commands.is_empty() {
        return Err(Refusal::new("run needs a command"));
    }
    let timeout = Duration::from_secs(config.run.timeout);
    if a.raw {
        if a.mode.is_some() || a.commands.len() != 1 {
            return Err(Refusal::new("--raw takes exactly one command"));
        }
        if a.why.as_deref().is_none_or(|w| w.trim().is_empty()) {
            return Err(Refusal::new("--raw needs --why")
                .instead(format!("harno run --raw {} --why \"<reason>\"", quote_always(&a.commands[0]))));
        }
    }
    if a.args.is_some() && !(a.mode.is_none() && a.commands.len() == 1 && config.run.presets.contains_key(&a.commands[0]) && !a.raw) {
        return Err(Refusal::new("arguments after -- go to a preset's {args}; give one preset name before --")
            .instead(format!("harno run {}", quote_always(&[a.commands.clone(), a.args.clone().unwrap_or_default()].concat().join(" ")))));
    }
    if a.why.is_some() && !a.raw {
        return Err(Refusal::new("--why goes with --raw"));
    }
    // Every item is a unit: a command, or a preset with its own commands
    // run its own way; --seq and --par order the units.
    let (mode, units) = if a.raw {
        (Mode::Seq, vec![Unit { mode: Mode::Seq, commands: vec![(1, a.commands[0].clone(), timeout_of(config, &a.commands[0]))] }])
    } else {
        let mut n = 0;
        // Words given apart that make one allowed command are one command the
        // caller did not quote: say so, rather than that a word is no command.
        let units = match a.commands.iter().map(|c| unit(config, c, a.args.as_deref(), &mut n)).collect::<R<Vec<_>>>() {
            Err(r) if a.mode.is_none() && a.commands.len() > 1 => return Err(one_command(config, &a.commands).unwrap_or(r)),
            u => u?,
        };
        // Without a flag, commands run in sequence; a lone preset runs its own way.
        let alone = if units.len() == 1 { units[0].mode } else { Mode::Seq };
        (a.mode.unwrap_or(alone), units)
    };

    let total: usize = units.iter().map(|u| u.commands.len()).sum();
    if let Some(l) = &a.live {
        l.several(total > 1);
    }
    let sandbox = Sandbox::of(config, project);
    let filters = &config.run.resolved;
    let mut ran: Vec<Ran> = Vec::new();
    match mode {
        Mode::Seq => {
            for u in &units {
                let done = exec_unit(u, project, sandbox.as_ref(), filters, a.live.as_ref());
                let failed = done.iter().any(|r| r.failed);
                ran.extend(done);
                if failed {
                    break;
                }
            }
        }
        Mode::Par => {
            let (tx, rx) = mpsc::channel();
            for u in units {
                let tx = tx.clone();
                let dir = project.to_path_buf();
                let sandbox = sandbox.clone();
                let filters = filters.clone();
                let live = a.live.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(exec_unit(&u, &dir, sandbox.as_ref(), &filters, live.as_ref()));
                });
            }
            drop(tx);
            // Printed in the order they finish.
            for done in rx {
                ran.extend(done);
            }
        }
    }

    // What a process the commands left behind writes from here on is not
    // theirs, and the call puts what it keeps into the raw file itself.
    if let Some(l) = &a.live {
        l.close();
    }
    // A human who watched the output come reads no filter of it and not all
    // of it again: the call keeps it whole and says how each command ended.
    let watched = a.live.as_ref().is_some_and(Live::watched);
    let mut warnings = Vec::new();
    for r in &mut ran {
        if watched {
            r.raw = Some(std::mem::take(&mut r.output));
        } else if let Some(w) = filter_output(project, config, r, timeout, a.call) {
            warnings.push(w);
        }
    }

    let failed = ran.iter().filter(|r| r.failed).count();
    let code = match mode {
        Mode::Seq => ran.iter().find(|r| r.failed).map(|r| r.code).unwrap_or(0),
        Mode::Par => {
            let mut by_index = ran.clone();
            by_index.sort_by_key(|r| r.index);
            by_index.iter().find(|r| r.failed).map(|r| r.code).unwrap_or(0)
        }
    };
    // Every exit code in command order, and whether it failed the call: the log
    // keeps the judgement of the filters of this call, not of a later config.
    let mut exits: Vec<(usize, i32, bool)> = ran.iter().map(|r| (r.index, r.code, r.failed)).collect();
    exits.sort();
    // Every failure in one line, so one call tells which commands broke. An
    // exit the filter calls ok is not one of them.
    let mut broke: Vec<(usize, i32)> = ran.iter().filter(|r| r.failed).map(|r| (r.index, r.code)).collect();
    broke.sort();
    let failures: Vec<String> = broke.iter().map(|(i, c)| format!("{i}: exit {c}")).collect();
    let footer = match mode {
        Mode::Seq if failed > 0 => format!("stopped: {}/{total} ran — {}", ran.len(), failures.join(", ")),
        Mode::Seq => format!("{total}/{total} ran"),
        Mode::Par if failed > 0 => format!("{} ran, {failed} failed — {}", ran.len(), failures.join(", ")),
        Mode::Par => format!("{} ran, 0 failed", ran.len()),
    };

    let text = if json_out {
        serde_json::to_string_pretty(&json!({
            "commands": ran.iter().map(|r| json!({
                "index": r.index, "command": r.command, "exit": r.code,
                "status": r.status, "secs": (r.secs * 10.0).round() / 10.0, "output": r.output,
                "filtered": r.raw.is_some()
            })).collect::<Vec<_>>(),
            "summary": footer,
            "exits": exits.iter().map(|e| e.1).collect::<Vec<_>>(),
            "exit": code,
        }))
        .unwrap()
            + "\n"
    } else {
        blocks(&ran, &footer, false, total)
    };
    let mut out = Output::text(text);
    out.code = code;
    out.failed = Some(exits.iter().map(|e| e.2).collect());
    out.exits = Some(exits.into_iter().map(|e| e.1).collect());
    out.why = if a.raw { a.why.clone() } else { None };
    out.advice = warnings;
    let mut by_index: Vec<&Ran> = ran.iter().collect();
    by_index.sort_by_key(|r| r.index);
    out.commands = by_index.iter().map(|r| r.command.clone()).collect();
    // What the commands wrote, kept whole when a filter cut any of it.
    out.raw_text = ran.iter().any(|r| r.raw.is_some()).then(|| blocks(&ran, &footer, true, total));
    Ok(out)
}

/// How much of a command a header shows. The caller wrote it a moment ago and
/// the number of a block tells the commands of a preset apart, so every header
/// keeps the same little of it.
const HEADER: usize = 60;

/// A wait a call of one command says out loud, in seconds.
const SLOW: f64 = 1.0;

/// The command in a header. The caller wrote it a moment ago, so a long
/// argument, a plan or a body, would cost its own text again here: the header
/// keeps the first line, cut, and counts the characters. The line of the log
/// and the page of analytics keep the command whole, and a refusal keeps its
/// `instead:` whole as well, since that one is the next call to run.
fn headline(command: &str, max: usize) -> String {
    let chars = command.chars().count();
    let first = command.lines().next().unwrap_or("");
    if first.chars().count() == chars && chars <= max {
        return command.to_string();
    }
    let cut: String = first.chars().take(max).collect();
    format!("{} … {chars} chars", cut.trim_end())
}

/// What a block holds: the filtered output, or what the command wrote when the
/// caller asked for the whole of it.
fn shown(r: &Ran, raw: bool) -> &str {
    if raw {
        r.raw.as_deref().unwrap_or(&r.output)
    } else {
        r.output.as_str()
    }
}

/// The call as text. A call of one command gives back what the command wrote
/// and nothing more: the caller wrote that command and reads its whole output,
/// so a line follows only where the output cannot say how the command ended —
/// it failed, it wrote nothing, or it took a wait worth knowing. Several
/// commands each get a header, which says as little: a failure, an exit its
/// filter calls ok and a wait end it, and a command that passed at once is its
/// command alone, since the footer counts it. The call ends with the footer.
/// With `raw`, what the commands wrote before their filters.
fn blocks(ran: &[Ran], footer: &str, raw: bool, total: usize) -> String {
    if let [r] = ran {
        // Only where the call asked for this one command: a chain that
        // stopped early keeps its header and its summary.
        if total == 1 {
            let mut s = shown(r, raw).to_string();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            if r.failed || r.secs >= SLOW || s.trim().is_empty() {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(&format!("{} · {:.1}s\n", r.status, r.secs));
            }
            return s;
        }
    }
    let mut s = String::new();
    for r in ran {
        let end = if r.failed || r.status != "ok" {
            format!(" --- {} · {:.1}s", r.status, r.secs)
        } else if r.secs >= SLOW {
            format!(" · {:.1}s", r.secs)
        } else {
            String::new()
        };
        s.push_str(&format!("--- {}: {}{end}\n", r.index, headline(&r.command, HEADER)));
        let out = shown(r, raw);
        s.push_str(out);
        if !out.is_empty() && !out.ends_with('\n') {
            s.push('\n');
        }
    }
    s.push_str(&format!("\n{footer}\n"));
    s
}

/// What each command of a call gave its caller, in characters ([`text_per_command`]).
pub fn shown_per_command(text: &str, commands: usize) -> Vec<u64> {
    text_per_command(text, commands).iter().map(|t| t.chars().count() as u64).collect()
}

/// What each command of a call gave its caller, read back from the text
/// `blocks` wrote: a lone command is the whole text; several are cut at their
/// headers, and the headers and the footer are no command's.
pub fn text_per_command(text: &str, commands: usize) -> Vec<String> {
    let mut texts = vec![String::new(); commands];
    let lines: Vec<&str> = text.lines().collect();
    if !lines.iter().any(|l| l.starts_with("--- 1: ")) {
        if commands == 1 {
            texts[0] = text.to_string();
        }
        return texts;
    }
    // The footer is the last line, after a blank one.
    let body = &lines[..lines.len().saturating_sub(2)];
    let header = |l: &str| l.strip_prefix("--- ").and_then(|r| r.split_once(": ")).and_then(|(k, _)| k.parse::<usize>().ok()).filter(|k| (1..=commands).contains(k));
    // A header ends with the seconds, or with the command alone when it passed
    // at once. An older log wrote a command of several lines whole, so its
    // header runs on to the line with the status, before the next header.
    let timed = |l: &str| l.rsplit_once(" · ").is_some_and(|(_, t)| t.strip_suffix('s').is_some_and(|x| x.parse::<f64>().is_ok()));
    let mut at = None;
    let mut skip = 0;
    for (n, l) in body.iter().enumerate() {
        if skip > 0 {
            skip -= 1;
        } else if let Some(k) = header(l) {
            at = Some(k - 1);
            if !timed(l) {
                let rest = &body[n + 1..];
                let next = rest.iter().position(|l| header(l).is_some()).unwrap_or(rest.len());
                skip = rest[..next].iter().position(|l| l.contains(" --- ") && timed(l)).map_or(0, |p| p + 1);
            }
        } else if let Some(i) = at {
            texts[i].extend([*l, "\n"]);
        }
    }
    texts
}

/// Commands with their number across the call and their timeout.
type Commands = Vec<(usize, String, Duration)>;

/// One item of `run`: an allowed command, or a preset with its commands,
/// run the preset's way. Commands are numbered across the whole call.
struct Unit {
    mode: Mode,
    commands: Commands,
}

/// An item as a unit. A preset's arguments come after `--` inside the item
/// (`"test -- parser"`), or from the command line for a lone preset. A
/// preset's name wins over a program of the same name.
fn unit(config: &Config, item: &str, given: Option<&[String]>, n: &mut usize) -> R<Unit> {
    let words = split(item)?;
    let Some(first) = words.first() else {
        return Err(Refusal::new("empty command"));
    };
    if !config.run.presets.contains_key(&first.text) {
        if allowed(&config.run.allow, &words).is_none() {
            return Err(Refusal::new(format!("'{}' is not in run.allow", refusal_name(&config.run.allow, &words, item)))
                .instead(format!("harno run --raw {} --why \"<reason>\"", quote_always(item))));
        }
        *n += 1;
        return Ok(Unit { mode: Mode::Seq, commands: vec![(*n, item.to_string(), timeout_of(config, item))] });
    }
    let args = preset_args(&words, given)?;
    let (mode, commands) = expand(config, &first.text, &args, &mut Vec::new(), n)?;
    Ok(Unit { mode, commands })
}

/// The preset whose command `command` is, among the presets the words of a
/// call name (`named`: `harno run <preset>`, an item of `--seq` or `--par`)
/// and the presets they include: the one whose command fits it, `{args}`
/// taken as any text, with the most text of its own, so of two presets that
/// run the same command the one the call named wins, and a preset inside
/// another is the inner one. `None` for a command typed by hand.
pub fn preset_of(config: &Config, named: &[String], command: &str) -> Option<String> {
    let presets = &config.run.presets;
    let first = |s: &str| split(s).ok().and_then(|w| w.first().map(|w| w.text.clone())).filter(|n| presets.contains_key(n));
    let mut seen: Vec<String> = Vec::new();
    for n in named.iter().filter_map(|a| first(a)) {
        if !seen.contains(&n) {
            seen.push(n);
        }
    }
    let mut i = 0;
    while i < seen.len() {
        let p = &presets[&seen[i]];
        for n in p.seq.iter().chain(p.par.iter()).flatten().filter_map(|c| first(c)) {
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
        i += 1;
    }
    let mut best: Option<(usize, &String)> = None;
    for name in &seen {
        let p = &presets[name];
        for c in p.seq.iter().chain(p.par.iter()).flatten().filter(|c| first(c).is_none_or(|f| &f == name)) {
            if fits(c, command) || fits(c, &format!("{command} ")) {
                let own = c.len() - c.matches(ARGS).count() * ARGS.len();
                if best.is_none_or(|(b, _)| own > b) {
                    best = Some((own, name));
                }
            }
        }
    }
    best.map(|(_, n)| n.clone())
}

/// Whether `command` is the preset command `template` with any text where
/// `{args}` stands. A template without arguments ran as `with_args` left it,
/// its end trimmed; the caller tries the command with a space after it too.
fn fits(template: &str, command: &str) -> bool {
    if !template.contains(ARGS) {
        return template == command;
    }
    let parts: Vec<&str> = template.split(ARGS).collect();
    let Some(mut rest) = command.strip_prefix(parts[0]) else { return false };
    for mid in &parts[1..parts.len() - 1] {
        let Some(i) = rest.find(mid) else { return false };
        rest = &rest[i + mid.len()..];
    }
    rest.ends_with(parts[parts.len() - 1])
}

/// The arguments of a preset item: the words after `--`, or for a lone
/// preset those of the command line.
fn preset_args(words: &[Word], given: Option<&[String]>) -> R<Vec<String>> {
    let name = &words[0].text;
    match words.get(1) {
        None => Ok(given.map(<[String]>::to_vec).unwrap_or_default()),
        Some(w) if w.text == "--" => Ok(words[2..].iter().map(|w| w.text.clone()).collect()),
        Some(_) => Err(Refusal::new(format!("preset {name} takes its arguments after --: \"{name} -- <arg>…\""))),
    }
}

/// A preset as commands, its arguments in place, numbered across the call.
/// An item that names another preset is replaced by the commands of that
/// preset, which run the way of the preset around them: a seq preset of
/// several commands inside a par one would lose its order and is refused, and
/// so is a preset that includes itself. An item that names its own preset is
/// the program of that name: `mdmap: {seq: ["mdmap {args}"]}` runs `mdmap`.
fn expand(config: &Config, name: &str, args: &[String], outer: &mut Vec<String>, n: &mut usize) -> R<(Mode, Commands)> {
    if outer.iter().any(|o| o == name) {
        return Err(Refusal::new(format!("preset {} > {name} includes itself", outer.join(" > "))));
    }
    let p: &Preset = &config.run.presets[name];
    let takes = p.seq.iter().chain(p.par.iter()).flatten().any(|c| c.contains(ARGS));
    if !args.is_empty() && !takes {
        return Err(Refusal::new(format!("preset {name} takes no arguments; it has no {ARGS}")));
    }
    if !p.defaults.is_empty() && !takes {
        return Err(Refusal::new(format!("preset {name} has defaults but no {ARGS} to put them in")));
    }
    let args = &with_defaults(p, args);
    let (mode, list) = match (&p.seq, &p.par) {
        (Some(s), None) => (Mode::Seq, s),
        (None, Some(s)) => (Mode::Par, s),
        _ => return Err(Refusal::new(format!("preset {name} needs exactly one of seq or par"))),
    };
    outer.push(name.to_string());
    let mut commands = Vec::new();
    for c in list {
        let c = with_args(c, args)?;
        let words = split(&c)?;
        let Some(inner) = words.first().filter(|w| w.text != name && config.run.presets.contains_key(&w.text)) else {
            *n += 1;
            let t = timeout_of(config, &c);
            commands.push((*n, c, t));
            continue;
        };
        let (inner_mode, inner_commands) = expand(config, &inner.text, &preset_args(&words, None)?, outer, n)?;
        if mode == Mode::Par && inner_mode == Mode::Seq && inner_commands.len() > 1 {
            return Err(Refusal::new(format!("preset {name} runs in parallel; preset {} inside it would lose its order", inner.text)));
        }
        commands.extend(inner_commands);
    }
    outer.pop();
    Ok((mode, commands))
}

fn exec_one(index: usize, command: &str, dir: &Path, timeout: Duration, sandbox: Option<&Sandbox>, filters: &filter::Filters, live: Option<&Live>) -> Ran {
    let apart = split(command).is_ok_and(|w| filters.apart(&w));
    let (code, mut status, output, secs, streams) = exec(command, dir, timeout, apart, sandbox, live.map(|l| (l, index)));
    let failed = match split(command) {
        Ok(words) => filters.broke(&words, code),
        Err(_) => code != 0,
    };
    if code != 0 && !failed {
        status = format!("ok, exit {code}");
    }
    Ran { index, command: command.to_string(), code, failed, status, output, secs, raw: None, streams }
}

/// A unit's commands, run its own way: in order, stopping at the first
/// failure, or all at once, in the order they finish.
fn exec_unit(u: &Unit, dir: &Path, sandbox: Option<&Sandbox>, filters: &filter::Filters, live: Option<&Live>) -> Vec<Ran> {
    match u.mode {
        Mode::Seq => {
            let mut out = Vec::new();
            for (k, c, t) in &u.commands {
                let r = exec_one(*k, c, dir, *t, sandbox, filters, live);
                let failed = r.failed;
                out.push(r);
                if failed {
                    break;
                }
            }
            out
        }
        Mode::Par => {
            let (tx, rx) = mpsc::channel();
            for (k, c, t) in u.commands.clone() {
                let tx = tx.clone();
                let dir = dir.to_path_buf();
                let sandbox = sandbox.cloned();
                let filters = filters.clone();
                let live = live.cloned();
                std::thread::spawn(move || {
                    let _ = tx.send(exec_one(k, &c, &dir, t, sandbox.as_ref(), &filters, live.as_ref()));
                });
            }
            drop(tx);
            rx.into_iter().collect()
        }
    }
}

/// Put the output of a command through its filter. When the filter changed
/// it, the command keeps what it wrote, so that the call can save it beside
/// its own line; what the agent reads says how to read it back. Returns a
/// warning to print.
fn filter_output(project: &Path, config: &Config, r: &mut Ran, timeout: Duration, call: Option<u64>) -> Option<String> {
    let words = split(&r.command).ok()?;
    let spec = config.run.resolved.choose(&words, r.failed)?;
    // Never a change that cannot be undone: a call without a line of its
    // own has nowhere to keep what the command wrote, so it is shown whole.
    let Some(call) = call else { return Some("warning: the call has no log line — output unfiltered".to_string()) };
    let f = match filter::apply(&spec, &r.output, r.streams.as_ref(), r.code, project, timeout, call) {
        Outcome::Raw { warning } => return warning,
        Outcome::Filtered(f) => f,
    };
    r.raw = Some(std::mem::replace(&mut r.output, filter::render(&f, call)));
    None
}

/// A preset command with `{args}` replaced by the arguments. As a word of its
/// own it takes them all, each one word that is not split or expanded again.
/// Inside a word or a quoted string it takes exactly one, written as it is,
/// so that one may hold only letters, digits and `_ . : / -`: nothing a
/// shell, a quote or a glob reads, whatever program the preset starts.
fn with_args(command: &str, args: &[String]) -> R<String> {
    let words: Vec<String> = args.iter().map(|a| single_quote(a)).collect();
    let (mut out, mut last) = (String::new(), 0);
    for (at, whole) in args_at(command) {
        out.push_str(&command[last..at]);
        out.push_str(&if whole { words.join(" ") } else { inside(command, args)? });
        last = at + ARGS.len();
    }
    out.push_str(&command[last..]);
    Ok(out.trim_end().to_string())
}

/// The one argument `{args}` takes inside a word.
fn inside(command: &str, args: &[String]) -> R<String> {
    let [arg] = args else {
        return Err(Refusal::new(format!("{ARGS} inside a word takes one argument, not {}: {command}", args.len())));
    };
    if arg.is_empty() || !arg.chars().all(|c| c.is_ascii_alphanumeric() || "_.:/-".contains(c)) {
        return Err(Refusal::new(format!("{ARGS} inside a word takes only letters, digits and _ . : / -, not {arg:?}: {command}")));
    }
    Ok(arg.clone())
}

/// Where `{args}` stands in a preset command, read the way `split` reads it,
/// and whether it is a word of its own: unquoted, at the start of a word, with
/// a space or the end after it.
fn args_at(command: &str) -> Vec<(usize, bool)> {
    let b = command.as_bytes();
    let (mut quote, mut start, mut found, mut i) = (None, true, Vec::new(), 0);
    while i < b.len() {
        if b[i..].starts_with(ARGS.as_bytes()) {
            let end = i + ARGS.len();
            found.push((i, quote.is_none() && start && b.get(end).is_none_or(u8::is_ascii_whitespace)));
            (i, start) = (end, false);
            continue;
        }
        let c = b[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(b'"') | None if c == b'\\' => i += 1,
            Some(_) => {}
            None if c == b'\'' || c == b'"' => quote = Some(c),
            None => {}
        }
        start = quote.is_none() && c.is_ascii_whitespace();
        i += 1;
    }
    found
}

/// The arguments of preset `p`: its defaults whose flag the caller did not
/// write, then the caller's. A short flag (`-n`) counts as written in `-n 5`
/// and `-n5`, a long one in `--max-count 5` and `--max-count=5`.
fn with_defaults(p: &Preset, args: &[String]) -> Vec<String> {
    let written = |flag: &str| {
        let short = flag.len() == 2 && flag.starts_with('-') && !flag.starts_with("--");
        args.iter().any(|a| a == flag || a.starts_with(&format!("{flag}=")) || (short && a.starts_with(flag) && !a.starts_with("--")))
    };
    let mut out = Vec::new();
    for (flag, value) in p.defaults.iter().filter(|(f, _)| !written(f)) {
        out.push(flag.clone());
        if !value.is_empty() {
            out.push(value.clone());
        }
    }
    out.extend(args.iter().cloned());
    out
}

/// A verify line that names a preset of one command: that command, the other
/// words of the line as its arguments. None for anything else, a preset of
/// several commands or arguments to a preset that takes none.
pub fn one_command_preset(config: &Config, words: &[Word]) -> Option<String> {
    let p = config.run.presets.get(&words.first()?.text)?;
    let args: Vec<String> = words[1..].iter().map(|w| w.text.clone()).collect();
    match (&p.seq, &p.par) {
        (Some(seq), None) if seq.len() == 1 && (args.is_empty() || seq[0].contains(ARGS)) => with_args(&seq[0], &with_defaults(p, &args)).ok(),
        _ => None,
    }
}

/// Quote a word so `split` reads it back exactly: nothing inside single
/// quotes is special.
fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn quote_always(s: &str) -> String {
    crate::output::dq(s)
}

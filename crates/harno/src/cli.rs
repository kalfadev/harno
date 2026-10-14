//! Argument parsing, dispatch, warnings and the call log.

use std::collections::{BTreeMap, HashMap};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Map, Value};

use crate::config::{self, Config, CONFIG_PATH, DEFAULT_CONFIG};
use crate::filter;
use crate::gate::{self, Gate};
use crate::log;
use crate::output::{quote, truncate, Output, Refusal, R};
use crate::run::{self, Mode, RunArgs};
use crate::tool;

/// Everything a call reads from its surroundings.
pub struct Env {
    pub cwd: PathBuf,
    pub vars: HashMap<String, String>,
    /// The terminal of a human who watches the output as it comes: stdout,
    /// when it is one.
    pub watched: Option<run::Screen>,
}

impl Env {
    pub fn from_process() -> Env {
        Env {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            vars: std::env::vars().collect(),
            watched: std::io::stdout().is_terminal().then(|| Arc::new(Mutex::new(std::io::stdout())) as run::Screen),
        }
    }
}

pub const USAGE: &str = "usage: harno <verb> [args]

run
  harno run \"<command>\"
  harno run --seq \"<command>\" \"<preset> [-- <arg>…]\"…
  harno run --par \"<command>\" \"<preset> [-- <arg>…]\"…
  harno run <preset> [<arg>…]        every word after a lone preset is its argument
  harno run --raw \"<command>\" --why \"<reason>\"
log
  harno log ls [<from> [<to>]]       the calls of the session, one line each; the last 20
  harno log raw <n> [--session <id>]  the whole output a filter shortened
  harno log stats [<session>] [--all]   what a session of calls adds up to: calls, characters
  harno log analytics [<session>] [--all]  the commands with no filter of their own, most shown first
  harno log fails [<session>] [--all]      the commands that failed and the calls refused, most first
tools
  harno tool ls                      the built-in tools and the project's, each a folder in .harno/tools/ with a tool.yml
  harno tool add <name>…             copy built-in tools into .harno/tools/, with the presets and the filter line each tool.yml declares
  harno tool remove <name>
  harno tool update [<name>]
  harno tool test                    the tests of every filter in run.filters
  a tool runs through a preset that names its program: harno run <preset> <arg>…
project
  harno init

Every verb takes --json.
harno help, -h, --help or no verb prints this; in a project, then the presets and run.allow of its config.
harno help --verbose prints the manual: what harno is, the config, run, filters, the log, tools.
A call works in the nearest directory upward with .harno/config.yml; harno init writes one.
";

/// The manual `harno help --verbose` prints; `{tools}` becomes the built-in
/// tools of this binary.
pub const MANUAL: &str = r#"harno: the harness an agent works through

WHAT IT IS
  harno sits between a coding agent and the machine. The agent runs every command
  through harno, and harno
    - runs only what .harno/config.yml allows (run.allow, run.presets),
    - filters the output before the agent reads it; the whole output stays one call away,
    - logs every call, prompt, tool call and answer of a session, verbatim, in .harno/log/,
    - adds tools: folders copied from the ones built in, a program, a filter, hook scripts.
  harno calls no model. A call works in the nearest directory upward that holds
  .harno/config.yml.

START
  harno init                      write .harno/config.yml, every field with its default
  harno tool add claude-code      the hooks that wire Claude Code to harno (TOOLS)
  harno help                      the verbs, then the presets and run.allow of the project

CONFIG: .harno/config.yml (an unknown field is refused)
  version: 1
  run:
    timeout: 900                  seconds a command may run; 0 is no limit
    allow: []                     what harno run "<command>" may start, matched on its first words;
                                  an entry is a command or {command: <words>, timeout: <seconds>},
                                  and the longest match gives the timeout
    presets: {}                   <name>: {seq: [...]} or {par: [...]}; an item is a command or
                                  another preset; {args} takes the words given after the preset,
                                  and inside a word one word of letters, digits and _ . : / -;
                                  preset commands skip run.allow, on purpose; an item
                                  named like its own preset is that program;
                                  defaults: {"-n": "20"} puts -n 20 before the arguments unless
                                  the caller writes -n itself (-n 5, -n5); "" is a flag alone
    filters:                      <command>: a filter (FILTERS); "*" is every command
      "*": { dedupe: true, head: 100, tail: 100 }
    sandbox:                      the OS sandbox every command of a call runs in
      enabled: false              true: a command writes in the project and nowhere else
      allow:                      what a command may touch outside the project; ~ is the home
        write: []                 paths it may write beside the project
        read: ["*"]               paths it may read; ["*"] is every path, and a narrower list
                                  is the whole of what may be read
      deny:
        read: []                  paths it may not read; a path here beats every allow, the
                                  project included. Empty denies nothing
  deny:
    calls: []                     calls harno refuses: a verb and the arguments that must all be
                                  present, such as "run mdmap ls --body"; * is any one argument
  warn:
    output: 20000                 characters; a longer output starts with advice; 0 is off

RUN
  harno run "<command>"                          one command of run.allow
  harno run <preset> [<arg>…]                    every word after a lone preset is its argument, --json too
  harno run --seq "<a>" "<preset> -- <args>"…    in order; stops at the first failure
  harno run --par "<a>" "<b>"…                   at once; every exit code is reported
  harno run --raw "<command>" --why "<reason>"   anything else; the reason is logged
  A command is an argument vector, not a shell line: &&, |, ;, $VAR are plain words;
  quotes group words; an unquoted *, ? or [ expands to the matching paths.
  A call of one command gives back what the command wrote and nothing else; a failure, an
  empty output or a wait of a second or more adds one line, "FAIL exit 1 · 0.2s". Several
  commands each print a block under "--- <n>: <command>", 60 characters of the command,
  and a failure or a wait ends that line the same way, " --- FAIL exit 1 · 0.2s" or
  " · 1.3s"; the call ends with a summary: "3/3 ran", "stopped: 2/3 ran — 2: exit 1",
  "3 ran, 1 failed — 3: exit 1".
  harno exits with the first failing exit code, in command order.
  While a command runs, each line it writes goes to the raw file of the call, and to the
  terminal when a human watches (stdout is one, no --json): such a call is not filtered and
  ends with the headers and the summary alone. A command that runs until it is stopped, a
  server, takes timeout 0 and starts in the background of the harness: harno log ls shows
  it running and harno log raw <n> what it wrote so far.
  Every command runs in a process group of its own: a timeout stops it and all it started,
  and SIGTERM, SIGINT or SIGHUP to harno go on to them, the call ending with its line and
  exit 128 + the signal; a signal harno started with ignored stays ignored (nohup).
  With run.sandbox.enabled every command, --raw and a preset alike, runs in the sandbox of
  the platform: sandbox-exec on macOS, Landlock on Linux. Where none can start the commands
  run without one and every call ends with a warning saying why.

FILTERS
  A value of run.filters is inline fields, a tool folder (the filter of its tool.yml) or a filter file.
  The fields apply in this order, whatever order they are written in:
    colour codes and carriage returns are removed
    replace: [{pattern, with}…]   every match in a line becomes with, $1 its first group
    drop: [<regex>…]              remove the matching lines
    keep: [<regex>…]              keep only the matching lines
    dedupe: true                  a repeated neighbour line once, with (×N)
    head: <n>, tail: <n>          keep the first n and the last n lines; 0 does not cut
    width: <n>                    a longer line keeps its first n characters and counts the rest
    on_empty: "<text>"            printed when nothing is left
    ok: [<exit>…]                 exit codes of the command that are no failure of the call
    reads: stdout|stderr          the fields read that stream alone and the other passes whole,
                                  stdout first; the command then runs with the two apart
  on_fail: {…}                    the filter of a failing command
  script: <command>               the output goes to a program on stdin, the exit code in
                                  HARNO_EXIT; what it prints is the output
  tests: [{input, stderr, expected, exit}]  examples that harno tool test checks
  A command filter overrides "*" field by field; the longest matching command wins.
  Where lines are cut the output says "… <n> lines: harno log raw <call>";
  an on_empty text stands alone, and what it replaced stays in the log.
  The raw output is shown instead when a filter would make it longer or empty, or its
  script fails. A command that wrote nothing keeps its on_empty text: there is nothing
  for it to be worse than, and a call that gives back nothing at all says nothing.

LOG: .harno/log/ (added to .gitignore)
  harno/<session>.jsonl holds one line per call that went through harno, with
    n, ts, ms, cwd, verb, args, and for a call that ran commands cmd, exit and
    failed (whether each exit code failed the call, as its filter judged it),
    output and lines (what the commands wrote) and filtered and kept (what the
    caller read). A refused call carries refused alone: nothing ran. Beside the
    file, <session>/<n>.out is what the caller read and <n>.raw what the
    commands wrote before a filter cut it; while a call of run runs, <n>.raw
    fills as its commands write and <n>.run says how the call began. The
    session comes from HARNO_SESSION;
    without it the calls of a day go to the session manual-<date>, and the first
    call of that log warns.
  <harness>/<session>.jsonl holds the words of a session, written by the hooks of that
    harness in its own words: for Claude Code claude-code/<session>.jsonl, the prompt,
    every tool call and the answer, and for a call that went through harno the lines of
    the gate log it claims. Long text sits beside it: <session>/<n>.out and <n>.in.
  harno log ls [<from> [<to>]] [--session <id>]  the calls of a session, one line each; the last
                                  20 without a range; a call without its line yet is running
                                  or died
  harno log raw <n> [--session <id>]  the whole output a filter shortened
  harno log stats [<session>] [--all]   what a session of calls adds up to: calls, failures,
                                  characters, what the filters cut
  harno log analytics [<session>] [--all]  the commands with no filter of their own, most
                                  characters shown first: the next filter to write
  harno log fails [<session>] [--all]  the commands that failed by name and exit code, most
                                  first, with the last line of the latest; the refused calls

TOOLS
  A tool is a folder a project copies from the ones built into harno:
    harno tool ls | add <name>… | remove <name> | update [<name>] | test
  .harno/tools/<name>/tool.yml is what harno reads of it, every field optional:
    presets: {…}                  the run.presets the tool needs, written as the config takes them
    filter: {…}                   an output filter (FILTERS) run.filters names the folder of
    config: {…}                   settings harno leaves to the tool itself
  add copies the folder and writes what the manifest declares into .harno/config.yml: the
  presets of its presets: block as they are written, comments and all, and the run.filters
  line of the filter it carries. A key the config already has is left as it is, and a
  manifest that declares neither leaves the config untouched. remove takes the folder out
  and leaves the config as it is; update brings the
  files back to what the binary carries, shows each change and keeps the config: block of
  the project. harno ships no program of a tool: one written in a compiled language is
  built as its tool.yml says. A tool is not a verb: its call runs through its preset,
  filtered and logged like any other.
  Built in: {tools}.
  mdmap, the docs as one tree, is no built-in tool but a program of its own repository:
    install curl -LsSf https://github.com/kalfadev/mdmap/releases/latest/download/mdmap-installer.sh | sh,
            or cargo install --git https://github.com/kalfadev/mdmap --locked; into ~/.cargo/bin
    preset  mdmap: { seq: ["mdmap {args}"] }
    config  mdmap.yml at the root of the project; mdmap reads it itself
    verbs   harno run mdmap help
  analytics, the log of the project on one local read-only page:
    program built from a harno checkout with
            cargo install --path <harno checkout>/tools/analytics --locked --root .harno/tools/analytics
    preset  analytics: { seq: [".harno/tools/analytics/bin/analytics {args}"] }
    call    harno run analytics [--port N] [--no-open] [--background | --stop]
  claude-code, the hooks that write the log of a Claude Code session beside the log of the
  gate, and the short view of it, harno run session. The hooks reach
  .claude/settings.json through a preset, which needs python3; other hooks there stay:
    claude-code: { seq: ["python3 .harno/tools/claude-code/install.py {args}"] }
    harno run claude-code install | remove
    strict: true      a Bash hook refuses every command that is not harno (needs shfmt);
                      after a change run harno run claude-code install again
  The first line of CLAUDE.md that starts with "$ " is the start command: its output goes
  into the first prompt of a session.
  The other built-in tools are scripts; their tool.yml says how a preset calls them.

OUTPUT
  Every verb takes --json. A refusal is "refused: <reason>", then "instead: <command>" when
  there is a next call, and exits 1. A long output starts with advice to narrow the call;
  it is never cut.
"#;

/// `harno help --verbose`: the manual, with the built-in tools of this binary.
fn manual() -> String {
    let names = tool::BUILTIN.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    MANUAL.replace("{tools}", &names)
}


pub fn main(args: Vec<String>, env: Env, out: &mut dyn Write) -> i32 {
    let Some(verb) = args.first().cloned() else {
        let _ = out.write_all(help(&env.cwd, false).as_bytes());
        return 0;
    };
    // A command gets its arguments as written, --json included.
    let given: Vec<String> = args[1..].to_vec();
    let mut rest = given.clone();
    let mut json = if let Some(p) = rest.iter().position(|a| a == "--json") {
        rest.remove(p);
        true
    } else {
        false
    };
    let emit = |out: &mut dyn Write, text: &str| {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    };
    let refuse = move |out: &mut dyn Write, r: &Refusal| {
        let text = if json {
            serde_json::to_string_pretty(&r.to_json()).unwrap() + "\n"
        } else {
            r.render()
        };
        let _ = out.write_all(text.as_bytes());
        1
    };

    match verb.as_str() {
        "help" | "--help" | "-h" => {
            return match rest.as_slice() {
                [] => {
                    emit(out, &help(&env.cwd, false));
                    0
                }
                [v] if v == "--verbose" => {
                    emit(out, &help(&env.cwd, true));
                    0
                }
                _ => refuse(out, &Refusal::new("help takes --verbose").instead("harno help --verbose")),
            };
        }
        "init" => {
            return match init(&env.cwd) {
                Ok(t) => {
                    emit(out, &t);
                    0
                }
                Err(r) => refuse(out, &r),
            }
        }
        _ => {}
    }

    let Some(project) = config::find_project(&env.cwd) else {
        return refuse(
            out,
            &Refusal::new(format!("no {CONFIG_PATH} here or in any parent directory")).instead("harno init"),
        );
    };
    let config = match config::load(&project) {
        Ok(c) => c,
        Err(r) => return refuse(out, &r),
    };

    // After a lone preset every word is its argument, --json too.
    if verb == "run" && json && given.first().is_some_and(|w| config.run.presets.contains_key(w)) {
        rest = given.clone();
        json = false;
    }
    // The call takes its number before it runs, so that an output a filter
    // cut can name the line it will be read back from. Without a session
    // hook (a human in a terminal) the calls of a day go to one session,
    // and only the call that starts its log says so.
    let session_var = env.vars.get("HARNO_SESSION").filter(|s| log::valid_session(s)).cloned();
    let session = session_var.clone().unwrap_or_else(gate::day_session);
    let gate = Gate::open(&project, &session).ok();
    let fresh = gate.as_ref().is_some_and(|g| !g.path().exists());
    let call = gate.as_ref().and_then(|g| g.claim().ok());
    // A call of run says it runs for as long as it runs, and its commands
    // write into its raw file as they go: a call that does not end, a server,
    // is read before it does, `log ls` shows it running and `log raw <n>`
    // what it wrote.
    let mut live = None;
    let mut _running = None;
    if let (Some(g), Some(n), "run") = (&gate, call, verb.as_str()) {
        let mut began = Map::new();
        began.insert("verb".into(), verb.clone().into());
        began.insert("args".into(), json!(rest));
        began.insert("cwd".into(), here(&project, &env.cwd).into());
        _running = g.begin(n, began).ok();
        live = run::Live::new(g.live(n).ok(), env.watched.clone().filter(|_| !json));
    }
    // A warning is one line without its newline: the text form puts them in
    // front of the output, --json makes a field of them.
    let mut warnings: Vec<String> = Vec::new();
    if session_var.is_none() && fresh {
        warnings.push(format!("warning: no session hook — calls log to session {session}"));
    }
    let started = Instant::now();
    let warn_output = config.warn.output;
    let size_warning = |size: usize, how: &str| {
        (warn_output > 0 && size > warn_output).then(|| format!("warning: output {size} > {warn_output} — {how}"))
    };

    let result = match denied(&config, &verb, &rest) {
        Some(r) => Err(r),
        None if verb == "run" => run_verb(&rest, &project, &config, call, live, json),
        None => dispatch(&verb, &rest, &project, &config, &session, json),
    };
    let mut output = match result {
        Ok(o) => o,
        Err(r) => {
            let text = if json { serde_json::to_string_pretty(&r.to_json()).unwrap() + "\n" } else { r.render() };
            Output { text, code: 1, refused: Some(r.reason), ..Default::default() }
        }
    };

    // Warnings go first, into the output and into the copy the log keeps.
    warnings.extend(output.advice.iter().cloned());
    let how = match verb.as_str() {
        "run" => "narrow the command or filter it in run.filters",
        "log" => "the whole output, on purpose",
        _ => "narrow the call",
    };
    warnings.extend(size_warning(output.text.chars().count(), how));
    // The sandbox is what holds a command inside the project: while none is
    // running the commands run without one, and the call says so where the
    // reader arrives, after the output.
    let last = config
        .run
        .sandbox
        .enabled
        .then(crate::sandbox::unavailable)
        .flatten()
        .map(crate::sandbox::notice);
    let head: String = warnings.iter().map(|w| format!("{w}\n")).collect();
    let footer = last.as_deref().map(|w| format!("{w}\n")).unwrap_or_default();
    let raw_text = output.raw_text.as_ref().map(|t| format!("{head}{t}{footer}"));
    // A reader of --json takes one JSON value and nothing else, so with it
    // every warning goes into the object instead of in front of it.
    warnings.extend(last);
    output.text = match json.then(|| in_json(&output.text, &warnings)).flatten() {
        Some(text) => text,
        None => format!("{head}{}{footer}", output.text),
    };

    if let (Some(gate), Some(n)) = (&gate, call) {
        let _ = gate.write_out(n, &output.text);
        match &raw_text {
            Some(raw) => {
                let _ = gate.write_raw(n, raw);
            }
            None => gate.drop_raw(n),
        }
        let ms = started.elapsed().as_millis() as u64;
        let line = call_line(&verb, &rest, json, ms, here(&project, &env.cwd), &output, raw_text.as_deref());
        let _ = gate.write(n, line);
    }
    emit(out, &output.text);
    output.code
}

/// `harno help`: the usage, or with `--verbose` the manual, and, in a project,
/// what its config lets the agent run: every preset with its commands, every
/// `run.allow` entry.
fn help(cwd: &Path, verbose: bool) -> String {
    let mut s = if verbose { manual() } else { USAGE.to_string() };
    let Some(project) = config::find_project(cwd) else { return s };
    let config = match config::load(&project) {
        Ok(c) => c,
        Err(r) => return s + &format!("\n{CONFIG_PATH}: {}\n", r.reason),
    };
    let presets = &config.run.presets;
    if !presets.is_empty() {
        s.push_str("\npresets of this project: harno run <preset> [<arg>…], the arguments where {args} is\n");
        let width = presets.keys().map(|k| k.chars().count()).max().unwrap_or(0);
        for (name, p) in presets {
            let (kind, items) = match (&p.seq, &p.par) {
                (_, Some(par)) => ("par: ", par),
                (Some(seq), None) => ("seq: ", seq),
                (None, None) => continue,
            };
            let kind = if items.len() == 1 { "" } else { kind };
            let mut line = items.iter().map(|c| c.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(", ");
            if !p.defaults.is_empty() {
                let d: Vec<String> = p.defaults.iter().map(|(f, v)| if v.is_empty() { f.clone() } else { format!("{f} {v}") }).collect();
                line = format!("{line}; defaults {}", d.join(" "));
            }
            s.push_str(&format!("  {name:<width$}  {}\n", truncate(&format!("{kind}{line}"), 100)));
        }
    }
    let allow = &config.run.allow;
    if !allow.is_empty() {
        s.push_str("\nrun.allow of this project: harno run \"<command>\", matched on its first words\n");
        let mut line = String::new();
        for a in allow {
            let item = match a.timeout {
                Some(t) => format!("{} ({t}s)", a.command),
                None => a.command.clone(),
            };
            if !line.is_empty() && line.chars().count() + item.chars().count() + 2 > 100 {
                s.push_str(&format!("  {line},\n"));
                line.clear();
            }
            if !line.is_empty() {
                line.push_str(", ");
            }
            line.push_str(&item);
        }
        s.push_str(&format!("  {line}\n"));
    }
    s
}

/// With `--json` the warnings are a field of the object, their `warning: `
/// prefix off. None when there is no warning, or when the output is not a
/// JSON object: then they stay in front of it, as in the text form.
fn in_json(text: &str, warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        return None;
    }
    let mut value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object_mut()?;
    let lines: Vec<Value> = warnings.iter().map(|w| Value::from(w.strip_prefix("warning: ").unwrap_or(w.as_str()))).collect();
    object.insert("warnings".into(), Value::Array(lines));
    Some(serde_json::to_string_pretty(&value).unwrap() + "\n")
}

/// The line of a call in the log of the gate: what the agent asked, what
/// ran, and how much of what the commands wrote reached the caller.
fn call_line(verb: &str, rest: &[String], json: bool, ms: u64, cwd: String, output: &Output, raw: Option<&str>) -> Map<String, Value> {
    let mut args = rest.to_vec();
    if json {
        args.push("--json".into());
    }
    let mut why = None;
    if verb == "run" {
        if let Some(p) = args.iter().position(|a| a == "--why") {
            why = args.get(p + 1).cloned();
            args.drain(p..(p + 2).min(args.len()));
        }
    }
    let mut m = Map::new();
    m.insert("ms".into(), ms.into());
    m.insert("cwd".into(), cwd.into());
    m.insert("verb".into(), verb.into());
    m.insert("args".into(), json!(args));
    if let Some(w) = why {
        m.insert("why".into(), w.into());
    }
    // A refused call carries its reason alone: nothing ran.
    if let Some(reason) = &output.refused {
        m.insert("refused".into(), reason.clone().into());
        return m;
    }
    if !output.commands.is_empty() {
        m.insert("cmd".into(), json!(output.commands));
    }
    if let Some(exits) = &output.exits {
        m.insert("exit".into(), json!(exits));
    }
    if let Some(failed) = &output.failed {
        m.insert("failed".into(), json!(failed));
    }
    let wrote = raw.unwrap_or(&output.text);
    m.insert("output".into(), wrote.chars().count().into());
    m.insert("filtered".into(), output.text.chars().count().into());
    m.insert("lines".into(), wrote.lines().count().into());
    m.insert("kept".into(), output.text.lines().count().into());
    m
}

/// Where the call was made, as the project sees it: `.` at its root.
fn here(project: &Path, cwd: &Path) -> String {
    match cwd.strip_prefix(project) {
        Ok(p) if p.as_os_str().is_empty() => ".".to_string(),
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => cwd.to_string_lossy().to_string(),
    }
}

/// A call matching an entry of `deny.calls`: same verb, and every other
/// word of the entry among the call's arguments (`*` matches any one).
fn denied(config: &Config, verb: &str, rest: &[String]) -> Option<Refusal> {
    for entry in &config.deny.calls {
        let words: Vec<String> = match run::split(entry) {
            Ok(w) => w.into_iter().map(|w| w.text).collect(),
            Err(_) => continue,
        };
        let words = if words.first().is_some_and(|w| w == "harno") { &words[1..] } else { &words[..] };
        let Some((v, want)) = words.split_first() else { continue };
        let v = v.as_str();
        if v != verb {
            continue;
        }
        let matches = want.iter().all(|w| (w == "*" && !rest.is_empty()) || rest.iter().any(|a| a == w));
        if !matches {
            continue;
        }
        let mut r = Refusal::new(format!("'{entry}' is in deny.calls in {CONFIG_PATH}"));
        // Without the denied flags the call may be allowed.
        if want.iter().any(|w| w.starts_with("--")) {
            let lean: Vec<String> = rest.iter().filter(|a| !(a.starts_with("--") && want.contains(a))).cloned().collect();
            if denied(config, verb, &lean).is_none() {
                r = r.instead(command(verb, &lean));
            }
        }
        return Some(r);
    }
    None
}

fn command(verb: &str, args: &[String]) -> String {
    let mut s = format!("harno {verb}");
    for a in args {
        s.push(' ');
        s.push_str(&quote(a));
    }
    s
}

/// Every verb but run, which `main` calls itself with the number of the call
/// and where its output goes while it runs.
fn dispatch(verb: &str, rest: &[String], project: &Path, config: &Config, session: &str, json: bool) -> R<Output> {
    match verb {
        "log" => log_verb(rest, project, config, session, json),
        "tool" => tool_verb(rest, project, config, json),
        _ => Err(Refusal::new(format!("unknown verb '{verb}'")).instead("harno help")),
    }
}

fn unknown_option(verb: &str, a: &str) -> Refusal {
    Refusal::new(format!("unknown option {a} for {verb}")).instead("harno help")
}

fn value<'a>(rest: &'a [String], i: usize, flag: &str) -> R<&'a String> {
    rest.get(i + 1).ok_or_else(|| Refusal::new(format!("{flag} needs a value")))
}

fn run_verb(rest: &[String], project: &Path, config: &Config, call: Option<u64>, live: Option<run::Live>, json: bool) -> R<Output> {
    let mut a = RunArgs { mode: None, commands: Vec::new(), raw: false, why: None, args: None, call, live };
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--seq" | "--par" => {
                let m = if rest[i] == "--seq" { Mode::Seq } else { Mode::Par };
                if a.mode.is_some_and(|x| x != m) {
                    return Err(Refusal::new("--seq and --par exclude each other"));
                }
                a.mode = Some(m);
            }
            "--raw" => a.raw = true,
            "--why" => {
                a.why = Some(value(rest, i, "--why")?.clone());
                i += 1;
            }
            "--" => {
                a.args = Some(rest[i + 1..].to_vec());
                break;
            }
            // A lone preset: every word after it is its argument, `--` first or not.
            x if a.commands.is_empty() && a.mode.is_none() && !a.raw && config.run.presets.contains_key(x) => {
                a.commands.push(x.to_string());
                let from = if rest.get(i + 1).is_some_and(|w| w == "--") { i + 2 } else { i + 1 };
                a.args = Some(rest[from..].to_vec());
                break;
            }
            x if x.starts_with("--") && a.commands.is_empty() => return Err(unknown_option("run", x)),
            x => a.commands.push(x.to_string()),
        }
        i += 1;
    }
    run::run(project, config, a, json)
}

/// `harno log raw <n> [--session <id>]`: the whole output of a call that a
/// filter shortened, of this session or of another one. `stats` goes to
/// [`stats_verb`], `analytics` to [`analytics_verb`], `fails` to [`fails_verb`].
fn log_verb(rest: &[String], project: &Path, config: &Config, session: &str, json: bool) -> R<Output> {
    let usage = || Refusal::new("log takes ls [<from> [<to>]], raw <n> [--session <id>], stats [<session>], analytics [<session>] or fails [<session>]");
    if rest.first().is_some_and(|a| a == "fails") {
        return fails_verb(rest, project, config, session, json);
    }
    if rest.first().is_some_and(|a| a == "stats") {
        return stats_verb(rest, project, session, json);
    }
    if rest.first().is_some_and(|a| a == "analytics") {
        return analytics_verb(rest, project, config, session, json);
    }
    if rest.first().is_some_and(|a| a == "ls") {
        return ls_verb(rest, project, session, json);
    }
    if rest.first().is_none_or(|a| a != "raw") {
        return Err(usage());
    }
    let mut n: Option<u64> = None;
    let mut of = session.to_string();
    let mut i = 1;
    while i < rest.len() {
        match rest[i].as_str() {
            "--session" => {
                of = value(rest, i, "--session")?.clone();
                i += 1;
            }
            x if n.is_none() => n = x.parse().ok(),
            _ => return Err(usage()),
        }
        i += 1;
    }
    let Some(n) = n else { return Err(usage()) };
    let text = gate::read_raw(project, &of, n).ok_or_else(|| {
        Refusal::new(format!("no raw output for call {n} of session {of}; the number is in the note a filtered output ends with"))
    })?;
    Ok(Output::text(if json {
        serde_json::to_string_pretty(&json!({"raw": n, "session": of, "output": text})).unwrap() + "\n"
    } else {
        text
    }))
}

/// `harno log ls [<from> [<to>]] [--session <id>]`: the calls of a session,
/// one line each. Without a range the last 20, since a session holds many
/// and the last ones are the ones a question is about.
fn ls_verb(rest: &[String], project: &Path, session: &str, json: bool) -> R<Output> {
    let mut range: Vec<u64> = Vec::new();
    let mut of = session.to_string();
    let mut i = 1;
    while i < rest.len() {
        match rest[i].as_str() {
            "--session" => {
                of = value(rest, i, "--session")?.clone();
                i += 1;
            }
            x => match x.parse::<u64>() {
                Ok(n) if range.len() < 2 => range.push(n),
                _ => return Err(Refusal::new(format!("log ls takes <from> [<to>], not {x}")).instead("harno log ls")),
            },
        }
        i += 1;
    }
    let n_of = |c: &Value| c.get("n").and_then(|v| v.as_u64()).unwrap_or(0);
    let mut all = log::calls(project, &of);
    // A call without its line yet: running, or it died before it wrote one.
    let done: Vec<u64> = all.iter().map(n_of).collect();
    for (mut c, running) in gate::unfinished(project, &of) {
        if done.contains(&n_of(&c)) {
            continue;
        }
        if let Some(o) = c.as_object_mut() {
            o.insert("state".into(), json!(if running { "running" } else { "died" }));
            all.push(c);
        }
    }
    if all.is_empty() {
        return Err(Refusal::new(format!("no calls logged in session {of}")).instead("harno log stats --all"));
    }
    let (from, to) = match range.as_slice() {
        [a, b] => (*a, *b),
        [a] => (*a, u64::MAX),
        _ => (0, u64::MAX),
    };
    let mut rows: Vec<&Value> = all.iter().filter(|c| (from..=to).contains(&n_of(c))).collect();
    // A line is written when its call ends, so a long call comes after later ones: read them by number.
    rows.sort_by_key(|c| n_of(c));
    if range.is_empty() && rows.len() > 20 {
        rows = rows.split_off(rows.len() - 20);
    }
    if json {
        return Ok(Output::text(serde_json::to_string_pretty(&json!({"session": of, "calls": rows})).unwrap() + "\n"));
    }
    let mut text = format!("session {of}\n{:>4}  {:<8}  {:>6}  {:<15}  {:<8}  call\n", "n", "time", "took", "shown", "result");
    for c in rows {
        text.push_str(&ls_line(c));
    }
    Ok(Output::text(text))
}

/// One call in a line: its number, the time of day, how long it took, what
/// came back (of what the commands wrote, where a filter cut it), how it
/// ended, and last the call as the agent wrote it, on one line and cut,
/// since a plan runs to hundreds of lines.
fn ls_line(c: &Value) -> String {
    let num = |k: &str| c.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let n = num("n");
    let time = c.get("ts").and_then(|v| v.as_str()).unwrap_or("").get(11..19).unwrap_or("").to_string();
    let secs = num("ms") as f64 / 1000.0;
    let verb = c.get("verb").and_then(|v| v.as_str()).unwrap_or("?");
    let args: Vec<String> =
        c.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(quote).collect()).unwrap_or_default();
    let call = truncate(&format!("{verb} {}", args.join(" ")).trim_end().replace('\n', " ⏎ "), 100);
    let took = format!("{secs:.1}s");
    // A call without its line: when it began, and what became of it.
    if let Some(state) = c.get("state").and_then(|v| v.as_str()) {
        return format!("{n:>4}  {time:<8}  {:>6}  {:<15}  {state:<8}  {call}\n", "", "");
    }
    let (shown, result, why) = match c.get("refused").and_then(|v| v.as_str()) {
        Some(why) => (String::new(), "refused".to_string(), format!("  ({})", truncate(why, 60))),
        None => {
            let (read, wrote) = (num("filtered"), num("output"));
            let shown = if wrote > read { format!("{} of {}", size(read), size(wrote)) } else { size(read) };
            // The first exit code that failed the call: one its filter calls ok did not.
            let exits = c.get("exit").and_then(|v| v.as_array()).cloned().unwrap_or_default();
            let failed = exits.iter().zip(log::failed(c)).find(|(_, f)| *f).and_then(|(x, _)| x.as_i64());
            (shown, failed.map_or_else(|| "ok".to_string(), |x| format!("exit {x}")), String::new())
        }
    };
    format!("{n:>4}  {time:<8}  {took:>6}  {shown:<15}  {result:<8}  {call}{why}\n")
}

/// A count of characters the way a reader takes it in: 950, 9.7k, 4.4M.
fn size(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_949 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// A time of the log as a reader takes it in, `2026-09-22 19:32 UTC`; the
/// date left out when it is that of `day`.
fn clock(ts: &str, day: Option<&str>) -> String {
    let (date, time) = (ts.get(..10).unwrap_or(ts), ts.get(11..16).unwrap_or(""));
    if day.and_then(|d| d.get(..10)) == Some(date) {
        format!("{time} UTC")
    } else {
        format!("{date} {time} UTC")
    }
}

/// `harno log stats [<session>] [--all]`: what a session adds up to. One
/// session, since a log holds many: the one asking ([`log::default_session`]),
/// a session by its id, or every one with `--all`, the one used last first.
/// The call asking is not in its own numbers: a call counts when its result
/// is written, after this one prints.
fn stats_verb(rest: &[String], project: &Path, current: &str, json: bool) -> R<Output> {
    let mut every = false;
    let mut session: Option<&str> = None;
    for a in &rest[1..] {
        match a.as_str() {
            "--all" => every = true,
            s if session.is_none() && !s.starts_with("--") => session = Some(s),
            _ => return Err(Refusal::new(format!("log stats takes a session or --all, not {a}")).instead("harno log stats")),
        }
    }
    let chosen = if session.is_none() && !every { log::default_session(project, current) } else { None };
    let all = log::stats(project, session.or(chosen.as_deref()));
    if all.is_empty() {
        let Some(s) = session else { return Ok(Output::text(format!("no sessions in {}\n", log::LOG_DIR))) };
        let r = Refusal::new(format!("no session {s} in {}", log::LOG_DIR));
        return Err(match log::sessions(project).first() {
            Some(i) => r.instead(format!("harno log stats {}", i.id)),
            None => r.instead("harno log stats"),
        });
    }
    // One session is what a question about the log means; the rest are named.
    let more = if session.is_none() && !every { log::sessions(project).len().saturating_sub(1) } else { 0 };
    // Every session is a table: a row each, and the whole log as one under them.
    let total = every.then(|| log::total(project));
    if json {
        let rows: Vec<Value> = all.iter().map(stats_json).collect();
        let mut v = json!({"sessions": rows, "more": more});
        if let Some(t) = &total {
            v["total"] = stats_json(t);
        }
        return Ok(Output::text(serde_json::to_string_pretty(&v).unwrap() + "\n"));
    }
    let mut text = match &total {
        Some(t) => stats_table(&all, t),
        None => all.iter().map(stats_block).collect::<Vec<String>>().join("\n"),
    };
    if more > 0 {
        text.push_str(&format!("\n{more} more session{} — harno log stats --all\n", if more == 1 { "" } else { "s" }));
    }
    Ok(Output::text(text))
}

/// One session as JSON: the numbers the text block prints.
fn stats_json(s: &log::Stats) -> Value {
    json!({
        "session": s.session,
        "ts": s.ts,
        "calls": s.calls,
        "failed": s.failed,
        "raw_calls": s.raw_calls,
        "chars": s.chars,
        "median": s.median,
        "p90": s.p90,
        "cut": s.cut,
        "before_filters": s.before_filters,
    })
}

/// One session as text: the session line and the numbers under it.
fn stats_block(s: &log::Stats) -> String {
    let share = (s.cut * 100).checked_div(s.before_filters).unwrap_or(0);
    let reads = if s.raw_calls == 1 { "raw read" } else { "raw reads" };
    let mut t = format!("session {}\n", s.session);
    t.push_str(&format!("  started   {}, last call {}\n", clock(&s.ts, None), clock(&s.last, Some(&s.ts))));
    t.push_str(&format!("  calls     {}, {} failed, {} {reads}\n", s.calls, s.failed, s.raw_calls));
    t.push_str(&format!("  returned  {} chars, median {}, p90 {} a call\n", size(s.chars), size(s.median), size(s.p90)));
    t.push_str(&format!("  filters   cut {} of {} chars ({share}%)\n", size(s.cut), size(s.before_filters)));
    t
}

/// Every session as a row of the numbers [`stats_block`] prints, the one used
/// last first, and the whole log as one under them, `total`.
fn stats_table(all: &[log::Stats], total: &log::Stats) -> String {
    let w = all.iter().map(|s| s.session.len()).max().unwrap_or(0).max(7);
    let mut t = format!(
        "{:<w$}  {:<20}  {:<20}  {:>5}  {:>6}  {:>3}  {:>8}  {:>6}  {:>6}  {:>8}  {:>12}\n",
        "session", "started", "last call", "calls", "failed", "raw", "returned", "median", "p90", "written", "cut"
    );
    for (name, s) in all.iter().map(|s| (s.session.as_str(), s)).chain([("total", total)]) {
        let share = (s.cut * 100).checked_div(s.before_filters).unwrap_or(0);
        let cut = format!("{} ({share}%)", size(s.cut));
        t.push_str(&format!(
            "{name:<w$}  {:<20}  {:<20}  {:>5}  {:>6}  {:>3}  {:>8}  {:>6}  {:>6}  {:>8}  {cut:>12}\n",
            clock(&s.ts, None),
            clock(&s.last, Some(&s.ts)),
            s.calls,
            s.failed,
            s.raw_calls,
            size(s.chars),
            size(s.median),
            size(s.p90),
            size(s.before_filters)
        ));
    }
    t
}

/// The name a command counts under in `log analytics`: the `run.allow` entry
/// that lets it through, the way a filter names a command; else its first
/// word, with the script when that word is an interpreter running one, since
/// `python3` alone would put every script of a project on one line.
fn command_key(allow: &[config::Allow], words: &[run::Word]) -> String {
    const SCRIPTS: [&str; 5] = [".py", ".sh", ".js", ".mjs", ".rb"];
    if let Some(a) = run::allowed(allow, words) {
        return a.command.clone();
    }
    match words {
        [first, script, ..] if SCRIPTS.iter().any(|e| script.text.ends_with(e)) => format!("{} {}", first.text, script.text),
        _ => words[0].text.clone(),
    }
}

/// `harno log analytics [<session>] [--all]`: the commands with no filter of
/// their own, most characters shown first, so the first line is the next
/// filter or preset to write; then, the same way, the commands a filter of
/// their own names, each with how much of what it wrote reached the caller.
/// A command counts under the preset that ran it ([`run::preset_of`]), else
/// under [`command_key`], with what its block gave the caller. The presets
/// and filters are those of the config today; the sessions are chosen as
/// `stats` chooses them.
fn analytics_verb(rest: &[String], project: &Path, config: &Config, current: &str, json: bool) -> R<Output> {
    let (ids, more) = sessions_asked(rest, project, current, "analytics")?;
    let filters = filter::resolve(project, &config.run.filters)?;
    // A row: whether a filter of its own names the command, whether a preset
    // ran it, and its name; runs, characters shown and characters written.
    let mut by: BTreeMap<(bool, bool, String), (u64, u64, u64)> = BTreeMap::new();
    for id in &ids {
        for c in log::calls(project, id) {
            let (Some(cmd), Some(n)) = (c.get("cmd").and_then(|v| v.as_array()), c.get("n").and_then(|v| v.as_u64())) else { continue };
            let text = gate::read_out(project, id, n).unwrap_or_default();
            // What the commands wrote, kept where a filter cut it; elsewhere it is what they gave.
            let raw = gate::read_raw(project, id, n).map(|r| run::shown_per_command(&r, cmd.len()));
            let named = named_presets(&c);
            for (i, (command, chars)) in cmd.iter().zip(run::shown_per_command(&text, cmd.len())).enumerate() {
                let Some(line) = command.as_str() else { continue };
                let Ok(words) = run::split(line) else { continue };
                if words.is_empty() {
                    continue;
                }
                let own = filters.entry_for(&words).is_some();
                let key = match run::preset_of(config, &named, line) {
                    Some(p) => (own, true, p),
                    None => (own, false, command_key(&config.run.allow, &words)),
                };
                let row = by.entry(key).or_default();
                row.0 += 1;
                row.1 += chars;
                row.2 += raw.as_ref().and_then(|r| r.get(i).copied()).unwrap_or(chars);
            }
        }
    }
    type Row = (bool, bool, String, u64, u64, u64);
    let mut rows: Vec<Row> = by.into_iter().map(|((own, p, k), (runs, chars, raw))| (own, p, k, runs, chars, raw)).collect();
    rows.sort_by(|a, b| b.4.cmp(&a.4).then_with(|| a.2.cmp(&b.2)));
    let (with, without): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(|r| r.0);
    if json {
        let row = |(_, preset, k, runs, chars, raw): &Row| {
            let mut m = Map::new();
            m.insert((if *preset { "preset" } else { "command" }).into(), json!(k));
            m.insert("runs".into(), json!(runs));
            m.insert("chars".into(), json!(chars));
            m.insert("raw".into(), json!(raw));
            Value::Object(m)
        };
        let commands: Vec<Value> = without.iter().map(row).collect();
        let filtered: Vec<Value> = with.iter().map(row).collect();
        return Ok(Output::text(serde_json::to_string_pretty(&json!({"sessions": ids, "commands": commands, "filtered": filtered, "more": more})).unwrap() + "\n"));
    }
    let mut text = match ids.as_slice() {
        [] => return Ok(Output::text(format!("no sessions in {}\n", log::LOG_DIR))),
        [one] => format!("session {one}\n"),
        many => format!("{} sessions\n", many.len()),
    };
    // A table under its title: written is what the commands wrote, shown
    // what reached the caller, kept the share. The name is last and whole: a
    // long one, a script under a temporary path, widens no column.
    let table = |title: &str, rows: &[Row]| {
        let mut t = format!("\n{title}\n  {:>5}  {:>8}  {:>8}  {:>4}  name\n", "runs", "written", "shown", "kept");
        for (_, preset, k, runs, chars, raw) in rows {
            let kept = format!("{}%", (chars * 100).checked_div(*raw).unwrap_or(100).min(100));
            let name = if *preset { format!("preset {k}") } else { k.clone() };
            t.push_str(&format!("  {runs:>5}  {:>8}  {:>8}  {kept:>4}  {name}\n", size(*raw), size(*chars)));
        }
        t
    };
    if without.is_empty() && with.is_empty() {
        text.push_str("no command ran\n");
    } else if without.is_empty() {
        text.push_str("\nwithout a filter of their own: none\n");
    } else {
        text.push_str(&table("without a filter of their own, most shown first", &without));
    }
    if !with.is_empty() {
        text.push_str(&table("with a filter of their own, most shown first", &with));
    }
    if more > 0 {
        text.push_str(&format!("\n{more} more session{} — harno log analytics --all\n", if more == 1 { "" } else { "s" }));
    }
    Ok(Output::text(text))
}

/// The sessions a question about the log means (`analytics`, `fails`): one by
/// its id, every one with `--all`, else the one asking, as `stats` chooses;
/// and how many others the log holds, for the line that names them.
fn sessions_asked(rest: &[String], project: &Path, current: &str, verb: &str) -> R<(Vec<String>, usize)> {
    let mut every = false;
    let mut session: Option<&str> = None;
    for a in &rest[1..] {
        match a.as_str() {
            "--all" => every = true,
            s if session.is_none() && !s.starts_with("--") => session = Some(s),
            _ => return Err(Refusal::new(format!("log {verb} takes a session or --all, not {a}")).instead(format!("harno log {verb}"))),
        }
    }
    let all = log::sessions(project);
    let ids: Vec<String> = match session {
        Some(s) if all.iter().any(|i| i.id == s) => vec![s.to_string()],
        Some(s) => return Err(Refusal::new(format!("no session {s} in {}", log::LOG_DIR)).instead(format!("harno log {verb}"))),
        None if every => all.iter().map(|i| i.id.clone()).collect(),
        None => log::default_session(project, current).into_iter().collect(),
    };
    let more = if session.is_none() && !every { all.len().saturating_sub(1) } else { 0 };
    Ok((ids, more))
}

/// The words a call of `run` was given, which name its presets ([`run::preset_of`]).
fn named_presets(c: &Value) -> Vec<String> {
    match c.get("verb").and_then(|v| v.as_str()) {
        Some("run") => c.get("args").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    }
}

/// `harno log fails [<session>] [--all]`: the commands that failed, by the
/// name they count under in `analytics` and their exit code, most first, each
/// with the last line the latest of them gave its caller; which failed is what
/// the call wrote ([`log::failed`]), as `stats` and `log ls` read it. Then the
/// refused calls by their reason with what it quotes left out
/// ([`reason_shape`]), with the latest reason whole where it says more. The
/// sessions are chosen as `analytics` chooses them.
fn fails_verb(rest: &[String], project: &Path, config: &Config, current: &str, json: bool) -> R<Output> {
    let (ids, more) = sessions_asked(rest, project, current, "fails")?;
    // A row: how often, and the time and the line of the latest.
    type Row = (u64, String, String);
    let mut failed: BTreeMap<(String, Option<i64>), Row> = BTreeMap::new();
    let mut refused: BTreeMap<String, Row> = BTreeMap::new();
    let latest = |row: &mut Row, ts: &str, line: String| {
        row.0 += 1;
        if ts >= row.1.as_str() {
            row.1 = ts.to_string();
            row.2 = line;
        }
    };
    let (mut calls, mut failing) = (0u64, 0u64);
    for id in &ids {
        for c in log::calls(project, id) {
            calls += 1;
            let ts = c.get("ts").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(reason) = c.get("refused").and_then(|v| v.as_str()) {
                failing += 1;
                latest(refused.entry(reason_shape(reason)).or_default(), ts, reason.lines().next().unwrap_or("").to_string());
                continue;
            }
            let (Some(cmd), Some(exits), Some(n)) = (c.get("cmd").and_then(|v| v.as_array()), c.get("exit").and_then(|v| v.as_array()), c.get("n").and_then(|v| v.as_u64())) else { continue };
            // Which commands failed is what the call wrote, as stats and log ls read it.
            let broke = log::failed(&c);
            if !broke.contains(&true) {
                continue;
            }
            failing += 1;
            let text = gate::read_out(project, id, n).unwrap_or_default();
            let named = named_presets(&c);
            for (((command, code), out), broke) in cmd.iter().zip(exits).zip(run::text_per_command(&text, cmd.len())).zip(broke) {
                let code = code.as_i64();
                let Some(line) = command.as_str() else { continue };
                if !broke {
                    continue;
                }
                let Ok(words) = run::split(line) else { continue };
                if words.is_empty() {
                    continue;
                }
                let name = match run::preset_of(config, &named, line) {
                    Some(p) => format!("preset {p}"),
                    None => command_key(&config.run.allow, &words),
                };
                latest(failed.entry((name, code)).or_default(), ts, last_line(&out));
            }
        }
    }
    let mut failed: Vec<((String, Option<i64>), Row)> = failed.into_iter().collect();
    failed.sort_by(|a, b| (b.1).0.cmp(&(a.1).0).then_with(|| a.0.cmp(&b.0)));
    let mut refused: Vec<(String, Row)> = refused.into_iter().collect();
    refused.sort_by(|a, b| (b.1).0.cmp(&(a.1).0).then_with(|| a.0.cmp(&b.0)));
    if json {
        let commands: Vec<Value> = failed.iter().map(|((name, code), (runs, _, line))| json!({"name": name, "exit": code, "runs": runs, "last": line})).collect();
        let reasons: Vec<Value> = refused.iter().map(|(shape, (runs, _, reason))| json!({"runs": runs, "shape": shape, "reason": reason})).collect();
        let v = json!({"sessions": ids, "calls": calls, "failed": failing, "commands": commands, "refused": reasons, "more": more});
        return Ok(Output::text(serde_json::to_string_pretty(&v).unwrap() + "\n"));
    }
    let mut text = match ids.as_slice() {
        [] => return Ok(Output::text(format!("no sessions in {}\n", log::LOG_DIR))),
        [one] => format!("session {one}\n"),
        many => format!("{} sessions\n", many.len()),
    };
    text.push_str(&format!("{failing} of {calls} calls failed\n"));
    if !failed.is_empty() {
        // The name is as wide as the widest up to 40: a longer one widens its own line alone.
        let w = failed.iter().map(|((n, _), _)| n.chars().count()).max().unwrap_or(0).min(40);
        text.push_str(&format!("\ncommands that failed, most first\n  {:>5}  {:>4}  {:<w$}  last line of the latest\n", "runs", "exit", "name"));
        for ((name, code), (runs, _, line)) in &failed {
            let code = code.map_or("-".to_string(), |c| c.to_string());
            text.push_str(&format!("  {runs:>5}  {code:>4}  {name:<w$}  {line}\n"));
        }
    }
    if !refused.is_empty() {
        text.push_str(&format!("\nrefused, most first\n  {:>5}  reason, what it quotes left out\n", "runs"));
        for (shape, (runs, _, reason)) in &refused {
            let latest = if reason == shape { String::new() } else { format!(" — latest: {reason}") };
            text.push_str(&format!("  {runs:>5}  {shape}{latest}\n"));
        }
    }
    if more > 0 {
        text.push_str(&format!("\n{more} more session{} — harno log fails --all\n", if more == 1 { "" } else { "s" }));
    }
    Ok(Output::text(text))
}

/// A reason a call was refused without what it quotes, so that two commands
/// refused for one reason count as one.
fn reason_shape(reason: &str) -> String {
    const QUOTES: &str = "\u{27}\u{22}";
    let mut out = String::new();
    let mut open: Option<char> = None;
    for c in reason.lines().next().unwrap_or("").chars() {
        match open {
            Some(q) if c == q => {
                open = None;
                out.push(c);
            }
            Some(_) => {}
            None if QUOTES.contains(c) => {
                open = Some(c);
                out.push(c);
                out += "…";
            }
            None => out.push(c),
        }
    }
    out
}

/// The last line a command gave its caller, cut at 120 characters, without
/// the lines harno adds to an output: the footer of a lone command, the note
/// of a cut, and the line that says how the sandbox stands.
fn last_line(out: &str) -> String {
    let harno = |l: &str| {
        (l.starts_with("FAIL ") && l.contains(" · "))
            || (l.starts_with("… ") && l.contains("harno log raw"))
            || l.starts_with("warning: run.sandbox ")
            || l.starts_with("note: run.sandbox ")
    };
    let line = out.lines().rev().map(str::trim).find(|l| !l.is_empty() && !harno(l)).unwrap_or("(no output)");
    match line.char_indices().nth(120) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_string(),
    }
}

/// `harno tool ls|add|remove|update|test`: the built-in tools, the project's
/// copies of them, and the tests the filters of the config carry.
fn tool_verb(rest: &[String], project: &Path, config: &Config, json: bool) -> R<Output> {
    let usage = || Refusal::new("tool takes ls, add <name>…, remove <name>, update [<name>] or test").instead("harno tool ls");
    let (sub, args) = rest.split_first().ok_or_else(usage)?;
    let text = match (sub.as_str(), args) {
        ("ls", []) => tool::ls(project, json)?,
        ("add", names) if !names.is_empty() => tool::add_all(project, names)?,
        ("remove", [name]) => tool::remove(project, config, name)?,
        ("update", []) => tool::update(project, None)?,
        ("update", [name]) => tool::update(project, Some(name))?,
        ("test", []) => return Ok(tool::test(project, config, json)),
        _ => return Err(usage()),
    };
    Ok(Output::text(text))
}


fn init(cwd: &Path) -> R<String> {
    let path = cwd.join(CONFIG_PATH);
    if path.exists() {
        return Err(Refusal::new(format!("{CONFIG_PATH} already exists")));
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, DEFAULT_CONFIG)?;
    Ok(format!("created {CONFIG_PATH}\n"))
}

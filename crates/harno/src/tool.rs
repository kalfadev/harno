//! Tools: a folder under `.harno/tools/` with a `tool.yml`, copied from the
//! ones built into harno (the folders of `tools/` in the repository). A tool
//! is a program a preset runs, an output filter of `run.filters`, the hook
//! scripts that wire an agent harness to harno, or several of these in one
//! folder. `add` puts the folder in place and writes into `.harno/config.yml`
//! what the manifest of the tool declares: the presets of its `presets:` block
//! and, when it carries a filter, the `run.filters` line that names its folder.
//! A manifest that declares neither leaves the config as it is.
//!
//! `build.rs` embeds the files at the top of each built-in folder, and `add`
//! copies them to `.harno/tools/<name>/`. What is deeper in the folder, a
//! Rust crate or its tests, stays in the repository: a tool written in a
//! compiled language is built by the project, as its manifest says.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use similar::{ChangeTag, TextDiff};

use crate::config::{self, Config, Preset, CONFIG_PATH};
use crate::filter::{self, Spec};
use crate::output::{atomic_write, Output, Refusal, R};

/// Where the tools of a project are.
pub const DIR: &str = ".harno/tools";

/// The file that makes a folder a tool.
pub const MANIFEST: &str = "tool.yml";

/// The files of one built-in tool, `(file, text)`.
pub type Files = &'static [(&'static str, &'static str)];

/// The built-in tools of this binary, `(name, files)`, embedded by `build.rs`.
pub const BUILTIN: &[(&str, Files)] = include!(concat!(env!("OUT_DIR"), "/builtin_tools.rs"));

/// `tool.yml`: the filter the tool carries, the presets `add` writes into the
/// config, and `config`, which harno leaves to the tool itself.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolFile {
    #[serde(default)]
    pub filter: Option<Spec>,
    /// The `run.presets` the tool needs, read here to be sure they load; what
    /// `add` writes is the text of the block, comments and all.
    #[serde(default)]
    pub presets: BTreeMap<String, Preset>,
    #[serde(default, rename = "config")]
    _config: serde_json::Value,
}

/// A `tool.yml` as written; a file of comments only is a tool without settings.
pub fn parse(text: &str) -> Result<ToolFile, String> {
    serde_saphyr::from_str::<Option<ToolFile>>(text).map(Option::unwrap_or_default).map_err(crate::config::yaml_error)
}

/// A tool name is one word, as a verb is.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The files of the built-in tool with this name.
fn files(name: &str) -> R<Files> {
    BUILTIN
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
        .ok_or_else(|| Refusal::new(format!("no built-in tool '{name}'")).instead("harno tool ls"))
}

/// The `tool.yml` of the built-in tool with this name.
pub fn builtin(name: &str) -> Option<&'static str> {
    files(name).ok()?.iter().find(|(f, _)| *f == MANIFEST).map(|(_, t)| *t)
}

fn dir_of(name: &str) -> String {
    format!("{DIR}/{name}")
}

fn manifest_of(name: &str) -> String {
    format!("{DIR}/{name}/{MANIFEST}")
}

fn added(project: &Path, name: &str) -> bool {
    project.join(manifest_of(name)).is_file()
}

/// The tools of the project: the folders with a manifest, by name.
fn names(project: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(project.join(DIR)) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().join(MANIFEST).is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| valid_name(n))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

/// The `tool.yml` of a tool of the project.
fn read(project: &Path, name: &str) -> R<ToolFile> {
    let file = manifest_of(name);
    let text = std::fs::read_to_string(project.join(&file)).map_err(|e| Refusal::new(format!("cannot read {file}: {e}")))?;
    parse(&text).map_err(|e| Refusal::new(format!("{file}: {e}")))
}

/// The command the filter of a tool is written for.
fn filter_command(text: &str) -> Option<String> {
    parse(text).ok()?.filter?.command
}

/// The lines of a top-level block of a manifest: the key line and every line
/// under it until the next top-level key.
fn block(text: &str, key: &str) -> Option<(usize, usize)> {
    let head = format!("{key}:");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.trim_end() == head || l.starts_with(&format!("{head} ")))?;
    let top = |l: &str| !l.is_empty() && !l.starts_with(' ') && !l.starts_with('#');
    let end = lines[start + 1..].iter().position(|l| top(l)).map_or(lines.len(), |k| start + 1 + k);
    Some((start, end))
}

/// What `add` writes under one section of the config, `(key, its lines)`.
type Entries = Vec<(String, Vec<String>)>;

/// The presets of a manifest as it writes them, `(name, lines)`, the comment
/// lines above a preset with it: what `add` writes is that text.
fn preset_entries(text: &str) -> Entries {
    let Some((start, end)) = block(text, "presets") else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    let (mut out, mut pending): (Vec<(String, Vec<String>)>, Vec<String>) = (Vec::new(), Vec::new());
    for l in &lines[start + 1..end] {
        let t = l.trim_start();
        if t.is_empty() {
            continue;
        }
        pending.push((*l).to_string());
        if t.starts_with('#') {
            continue;
        }
        match (l.len() - t.len() == 2).then(|| t.split_once(':')).flatten() {
            // A preset of its own, with the comment lines that stand above it.
            Some((name, _)) => out.push((name.trim().to_string(), std::mem::take(&mut pending))),
            // A line of the preset above: one written over several lines.
            None => {
                if let Some(last) = out.last_mut() {
                    last.1.append(&mut pending);
                }
            }
        }
    }
    out
}

/// The key a line under a block names, quotes off: `  name: …` gives `name`.
fn key_of(line: &str, indent: usize) -> Option<String> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') || line.len() - t.len() != indent {
        return None;
    }
    Some(t.split_once(':')?.0.trim().trim_matches('"').to_string())
}

/// `entries` under `run.<section>:` of the config text, which is edited as
/// text: what is in the file, comments and all, stays as it is, and a key the
/// section already has is left alone. Gives the text, the keys written and
/// the keys left; a section written as a map on one line is left alone whole.
fn into_section(text: &str, section: &str, entries: &[(String, Vec<String>)]) -> (String, Vec<String>, Vec<String>) {
    // The text of the file is the shape harno keeps: a read and write through
    // a YAML crate would give the config back without a comment in it.
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let top = |l: &String| !l.is_empty() && !l.starts_with(' ') && !l.starts_with('#');
    let run = match lines.iter().position(|l| l.trim_end() == "run:") {
        Some(i) => i,
        None => {
            lines.push("run:".to_string());
            lines.len() - 1
        }
    };
    let run_end = lines[run + 1..].iter().position(top).map_or(lines.len(), |k| run + 1 + k);
    let head = format!("  {section}:");
    let sec = match lines[run + 1..run_end].iter().position(|l| l.trim_end() == head || l.starts_with(&format!("{head} "))) {
        Some(k) => run + 1 + k,
        None => {
            lines.insert(run_end, head.clone());
            run_end
        }
    };
    let left_whole = || (text.to_string(), Vec::new(), entries.iter().map(|(k, _)| k.clone()).collect::<Vec<String>>());
    if lines[sec].trim_end() != head {
        // `presets: {}` of a new config makes way for the lines; a map with
        // something in it on one line is the humans to open up.
        if lines[sec].trim_end() != format!("{head} {{}}") {
            return left_whole();
        }
        lines[sec] = head.clone();
    }
    let (mut end, mut at) = (sec + 1, sec + 1);
    while end < lines.len() {
        let t = lines[end].trim_start();
        if !t.is_empty() && lines[end].len() - t.len() < 4 {
            break;
        }
        if !t.is_empty() {
            at = end + 1;
        }
        end += 1;
    }
    let has: Vec<String> = lines[sec + 1..end].iter().filter_map(|l| key_of(l, 4)).collect();
    let (mut wrote, mut written, mut skipped) = (Vec::new(), Vec::new(), Vec::new());
    for (key, block) in entries {
        if has.contains(key) {
            skipped.push(key.clone());
            continue;
        }
        written.push(key.clone());
        wrote.extend(block.iter().map(|l| format!("  {l}")));
    }
    if wrote.is_empty() {
        return (text.to_string(), written, skipped);
    }
    let mut out: Vec<String> = lines[..at].to_vec();
    out.extend(wrote);
    out.extend_from_slice(&lines[at..]);
    (out.join("\n") + "\n", written, skipped)
}

/// What the manifest of a tool declares into the config of the project, one
/// line of report for each section. A config the result does not load is put
/// back as it was: a config harno cannot read refuses every call.
fn into_config(project: &Path, sections: &[(&str, Entries)]) -> R<String> {
    let path = project.join(CONFIG_PATH);
    let before = std::fs::read_to_string(&path).map_err(|e| Refusal::new(format!("cannot read {CONFIG_PATH}: {e}")))?;
    let (mut text, mut s) = (before.clone(), String::new());
    for (section, entries) in sections {
        if entries.is_empty() {
            continue;
        }
        let (out, written, skipped) = into_section(&text, section, entries);
        text = out;
        if !written.is_empty() {
            s.push_str(&format!("{CONFIG_PATH}: run.{section} takes {}\n", written.join(", ")));
        }
        if !skipped.is_empty() {
            s.push_str(&format!("{CONFIG_PATH}: run.{section} has {} already, left as it is\n", skipped.join(", ")));
        }
    }
    if text != before {
        atomic_write(&path, &text)?;
        if let Err(r) = config::load(project) {
            atomic_write(&path, &before)?;
            return Err(Refusal::new(format!("the tool does not fit {CONFIG_PATH}, which is as it was: {}", r.reason)));
        }
    }
    Ok(s)
}

/// `new` with the `config:` block of `old` in place of its own, so an update
/// brings the files of the tool and keeps the settings of the project.
fn keep_config(old: &str, new: &str) -> String {
    let (Some((os, oe)), Some((ns, ne))) = (block(old, "config"), block(new, "config")) else { return new.to_string() };
    let (o, n): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let mut out: Vec<&str> = n[..ns].to_vec();
    out.extend(&o[os..oe]);
    out.extend(&n[ne..]);
    let mut s = out.join("\n");
    if new.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// The files of a built-in tool the project has no folder of yet.
fn addable(project: &Path, name: &str) -> R<Files> {
    let files = files(name)?;
    let dir = dir_of(name);
    if project.join(&dir).exists() {
        return Err(Refusal::new(format!("{dir}/ already exists")).instead(format!("harno tool update {name}")));
    }
    Ok(files)
}

/// Copy a built-in tool into the project; refused when its folder exists.
fn copy(project: &Path, name: &str) -> R<String> {
    let files = addable(project, name)?;
    let dir = dir_of(name);
    let mut s = String::new();
    for (file, text) in files {
        atomic_write(&project.join(&dir).join(file), text)?;
        s.push_str(&format!("added {dir}/{file}\n"));
    }
    Ok(s)
}

/// Remove the folder of a tool, whatever is in it.
fn delete(project: &Path, name: &str) -> R<String> {
    let dir = dir_of(name);
    if !valid_name(name) || !project.join(&dir).is_dir() {
        return Err(Refusal::new(format!("no tool '{name}' in this project")).instead("harno tool ls"));
    }
    std::fs::remove_dir_all(project.join(&dir))?;
    Ok(format!("removed {dir}/\n"))
}

/// The added built-in tools `update` works on: one by name, or all.
fn to_update(project: &Path, name: Option<&String>) -> R<Vec<String>> {
    match name {
        Some(n) => {
            files(n)?;
            if !project.join(dir_of(n)).is_dir() {
                return Err(Refusal::new(format!("{}/ is not added", dir_of(n))).instead(format!("harno tool add {n}")));
            }
            Ok(vec![n.clone()])
        }
        None => Ok(BUILTIN.iter().map(|(n, _)| n.to_string()).filter(|n| project.join(dir_of(n)).is_dir()).collect()),
    }
}

/// The files of an added built-in tool brought back to what the binary
/// carries, each change shown; `(report, changed)`.
fn refresh(project: &Path, name: &str) -> R<(String, bool)> {
    let mut s = String::new();
    let mut changed = false;
    for (file, new) in files(name)? {
        let path = format!("{}/{file}", dir_of(name));
        let old = std::fs::read_to_string(project.join(&path)).unwrap_or_default();
        // The settings of the project stay: the `config:` block of its manifest.
        let kept = if *file == MANIFEST { keep_config(&old, new) } else { new.to_string() };
        let new = kept.as_str();
        if old == new {
            continue;
        }
        changed = true;
        s.push_str(&format!("{name}: updated {path}\n"));
        for c in TextDiff::from_lines(old.as_str(), new).iter_all_changes() {
            let sign = match c.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => continue,
            };
            s.push_str(&format!("  {sign} {}", c.value()));
            if !c.value().ends_with('\n') {
                s.push('\n');
            }
        }
        atomic_write(&project.join(&path), new)?;
    }
    if !changed {
        s.push_str(&format!("{name}: up to date\n"));
    }
    Ok((s, changed))
}

/// `harno tool ls`: the built-in tools, marked when added and with the
/// command a filter is for, and the tools of the project; every `tool.yml`
/// must read.
pub fn ls(project: &Path, json: bool) -> R<String> {
    let names = names(project);
    for n in &names {
        read(project, n)?;
    }
    let command_of = |n: &str| builtin(n).and_then(filter_command);
    if json {
        let builtin: Vec<_> = BUILTIN
            .iter()
            .map(|(n, _)| json!({"name": n, "filter": command_of(n), "added": added(project, n)}))
            .collect();
        return Ok(serde_json::to_string_pretty(&json!({ "builtin": builtin, "project": names })).unwrap() + "\n");
    }
    let mut s = String::from("built-in (harno tool add <name>)\n");
    let w = BUILTIN.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(0);
    for (n, _) in BUILTIN {
        let filter = command_of(n).map(|c| format!("filter of {c}")).unwrap_or_default();
        let added = if added(project, n) { "added" } else { "" };
        s.push_str(format!("  {n:<w$}   {filter:<22}   {added}").trim_end());
        s.push('\n');
    }
    s.push_str(&format!("\n{DIR}/\n"));
    if names.is_empty() {
        s.push_str("  (none)\n");
    }
    for n in &names {
        s.push_str(&format!("  {n}\n"));
    }
    Ok(s)
}

/// `harno tool add <name>…`: every name is checked before the first one is
/// added, so a wrong name adds nothing; then each is added as [`add`] does.
pub fn add_all(project: &Path, names: &[String]) -> R<String> {
    for (i, name) in names.iter().enumerate() {
        if names[..i].contains(name) {
            return Err(Refusal::new(format!("tool add names {name} twice")));
        }
        addable(project, name)?;
    }
    names.iter().map(|name| add(project, name)).collect()
}

/// A built-in tool into the project: its files into `.harno/tools/`, and
/// what its manifest declares into the config: the presets of its `presets:`
/// block as they are written, and the `run.filters` line of the filter it
/// carries. Nothing of the tool is left behind when the config refuses it.
fn add(project: &Path, name: &str) -> R<String> {
    let mut s = copy(project, name)?;
    let back = |r: Refusal| {
        let _ = std::fs::remove_dir_all(project.join(dir_of(name)));
        Err(Refusal::new(format!("{}: nothing was added", r.reason)))
    };
    if let Err(r) = read(project, name) {
        return back(Refusal::new(format!("the built-in tool {name} does not load: {}", r.reason)));
    }
    let manifest = builtin(name).unwrap_or_default();
    let filter = match filter_command(manifest) {
        Some(c) => vec![(c.clone(), vec![format!("  {}: {}", filter::yaml_key(&c), dir_of(name))])],
        None => Vec::new(),
    };
    match into_config(project, &[("presets", preset_entries(manifest)), ("filters", filter)]) {
        Ok(report) => s.push_str(&report),
        Err(r) => return back(r),
    }
    Ok(s)
}

/// `harno tool remove <name>`: the folder of a tool, whatever is in it. The
/// config is not touched, so a folder `run.filters` still names is refused:
/// without it every call would be refused by a filter that cannot be read.
pub fn remove(project: &Path, config: &Config, name: &str) -> R<String> {
    let dir = dir_of(name);
    let named = config
        .run
        .resolved
        .entries
        .iter()
        .find(|e| e.path.as_deref().map(|p| p.trim_start_matches("./").trim_end_matches('/')) == Some(dir.as_str()));
    if let Some(e) = named {
        return Err(Refusal::new(format!("run.filters in {CONFIG_PATH} names {dir} for {}: take that line out first", e.key)));
    }
    delete(project, name)
}

/// `harno tool update [<name>]`: the files of added built-in tools brought
/// back to what the binary carries, each change shown.
pub fn update(project: &Path, name: Option<&String>) -> R<String> {
    let names = to_update(project, name)?;
    if names.is_empty() {
        return Ok("no built-in tool is added\n".into());
    }
    let mut s = String::new();
    for n in names {
        s.push_str(&refresh(project, &n)?.0);
    }
    // A filter that no longer loads refuses the config: say so now.
    config::load(project).map_err(|r| Refusal::new(format!("the updated tools break {CONFIG_PATH}: {}", r.reason)))?;
    Ok(s)
}

/// `harno tool test`: the tests the filters of `run.filters` carry, how many
/// ran, and each failure with the text it expected and the text it got.
pub fn test(project: &Path, config: &Config, json: bool) -> Output {
    let timeout = Duration::from_secs(config.run.timeout);
    let (ran, failures) = filter::run_tests(&config.run.resolved, project, timeout);
    let plural = if ran == 1 { "" } else { "s" };
    let mut out = if json {
        let failed: Vec<_> = failures
            .iter()
            .map(|f| json!({"filter": f.filter, "test": f.test, "expected": f.expected, "got": f.got}))
            .collect();
        Output::text(serde_json::to_string_pretty(&json!({"ran": ran, "failed": failed})).unwrap() + "\n")
    } else if failures.is_empty() {
        Output::text(format!("{ran} filter test{plural} passed\n"))
    } else {
        let mut s = String::new();
        for f in &failures {
            s.push_str(&format!("filter {}: test {} failed\n", f.filter, f.test));
            s.push_str(&indent_block("expected", &f.expected));
            s.push_str(&indent_block("got", &f.got));
        }
        s.push_str(&format!("\n{} of {ran} filter test{plural} failed\n", failures.len()));
        Output::text(s)
    };
    out.code = if failures.is_empty() { 0 } else { 1 };
    out
}

fn indent_block(label: &str, text: &str) -> String {
    let mut s = format!("  {label}:\n");
    for l in text.trim_end_matches('\n').lines() {
        s.push_str("    ");
        s.push_str(l);
        s.push('\n');
    }
    s
}

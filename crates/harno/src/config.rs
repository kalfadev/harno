//! `.harno/config.yml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::filter::{self, Filters};
use crate::output::{Refusal, R};

pub const CONFIG_PATH: &str = ".harno/config.yml";
pub const VERSION: u64 = 1;

/// The file `harno init` writes: every field, with its default.
pub const DEFAULT_CONFIG: &str = "version: 1
run:
  timeout: 900
  allow: []
  presets: {}
  filters:
    \"*\": { dedupe: true, head: 100, tail: 100 }
  sandbox:
    enabled: false
    allow:
      # outside the project: what a command may write, and what it may read
      # ([\"*\"] is every path)
      write: []
      read: [\"*\"]
    deny:
      # paths closed to reads; a path here beats every allow
      read: []
deny:
  calls: []
warn:
  output: 20000
";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u64,
    #[serde(default)]
    pub run: RunConfig,
    #[serde(default)]
    pub deny: DenyConfig,
    #[serde(default)]
    pub warn: WarnConfig,
}

/// harno calls nobody may make.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DenyConfig {
    /// Call patterns: a verb and the arguments that must all be present,
    /// e.g. `harno ls --body` (`harno` may be left out); `*` stands for any
    /// one argument.
    #[serde(default, deserialize_with = "strings")]
    pub calls: Vec<String>,
}

/// Sizes above which a call's output starts with advice.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarnConfig {
    /// Characters of output; 0 turns the warning off.
    #[serde(default = "warn_output")]
    pub output: usize,
}

fn warn_output() -> usize {
    20_000
}

impl Default for WarnConfig {
    fn default() -> Self {
        WarnConfig { output: warn_output() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    #[serde(default = "run_timeout")]
    pub timeout: u64,
    #[serde(default, deserialize_with = "allows")]
    pub allow: Vec<Allow>,
    #[serde(default)]
    pub presets: BTreeMap<String, Preset>,
    /// Command → filter, or the path of a filter file; resolved into
    /// `resolved` when the config is loaded.
    #[serde(default, deserialize_with = "map_or_null")]
    pub filters: BTreeMap<String, Value>,
    #[serde(default)]
    pub sandbox: SandboxConfig,
    #[serde(skip)]
    pub resolved: Filters,
}

/// `run.sandbox`: the OS sandbox every command of a call runs in. With
/// `enabled` a command writes in the project and nowhere else; what it may
/// touch outside is `allow`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow: SandboxAllow,
    #[serde(default)]
    pub deny: SandboxDeny,
}

/// `run.sandbox.allow`: what a command may touch outside the project.
/// `write` names the paths it may write beside the project, which is always
/// writable, as are the devices a program needs. `read` names the paths it
/// may read: `["*"]` is every path, and a narrower list is the whole of what
/// may be read. A path of `deny.read` beats every allow, the project and the
/// system directories included. A `~` is the home directory.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxAllow {
    #[serde(default, deserialize_with = "strings")]
    pub write: Vec<String>,
    #[serde(default = "everywhere", deserialize_with = "strings")]
    pub read: Vec<String>,
}

/// What `allow.read` says while nobody narrowed it: every path.
fn everywhere() -> Vec<String> {
    vec![EVERYWHERE.to_string()]
}

/// Writing is the one harno closes on its own, since keeping the writes in
/// is what a project asks the sandbox for; reading stays as it was.
impl Default for SandboxAllow {
    fn default() -> Self {
        SandboxAllow { write: Vec::new(), read: everywhere() }
    }
}

/// `run.sandbox.deny`: what a command may not read. A path here beats every
/// allow. An empty list denies nothing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxDeny {
    #[serde(default, deserialize_with = "strings")]
    pub read: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    #[serde(default, deserialize_with = "opt_strings")]
    pub seq: Option<Vec<String>>,
    #[serde(default, deserialize_with = "opt_strings")]
    pub par: Option<Vec<String>>,
    /// Arguments put before the caller's, flag by flag: a flag the caller
    /// writes itself (`-n 5`, `-n5`, `--max-count=5`) replaces the default of
    /// that flag, and the others stay. An empty value is a flag alone.
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}

/// An entry of `run.allow`: the words a command starts with, and the seconds
/// it may run when that is not `run.timeout`, 0 for no limit. Written as the command alone,
/// or as `{command: <words>, timeout: <seconds>}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    pub command: String,
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// A list of commands. YAML reads a bare `false` or `true` as a boolean
/// and `7` as a number; in a command list they are command names.
fn strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let values: Vec<Value> = Deserialize::deserialize(d)?;
    values.into_iter().map(command::<D::Error>).collect()
}

/// `run.allow`: each entry a command, or a map with its own timeout.
pub fn allows<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Allow>, D::Error> {
    let values: Vec<Value> = Deserialize::deserialize(d)?;
    values
        .into_iter()
        .map(|v| match v {
            Value::Object(_) => serde_json::from_value(v).map_err(serde::de::Error::custom),
            other => command::<D::Error>(other).map(|command| Allow { command, timeout: None }),
        })
        .collect()
}

fn command<E: serde::de::Error>(v: Value) -> Result<String, E> {
    match v {
        Value::String(s) => Ok(s),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(E::custom(format!("expected a command, got {other:?}"))),
    }
}

fn opt_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    strings(d).map(Some)
}

/// `filters:` with nothing under it reads as an empty map.
fn map_or_null<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Value>, D::Error> {
    let m: Option<BTreeMap<String, Value>> = Deserialize::deserialize(d)?;
    Ok(m.unwrap_or_default())
}

fn run_timeout() -> u64 {
    900
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            timeout: run_timeout(),
            allow: Vec::new(),
            presets: BTreeMap::new(),
            filters: BTreeMap::new(),
            sandbox: SandboxConfig::default(),
            resolved: Filters::default(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> R<Config> {
        // The version is read first and on its own: a number harno does not
        // know means the rest of the file cannot be trusted either.
        let value: Value = serde_saphyr::from_str(text)
            .map_err(|e| Refusal::new(format!("{CONFIG_PATH} is not valid YAML: {}", yaml_error(e))))?;
        match value.get("version").and_then(|v| v.as_u64()) {
            Some(VERSION) => {}
            Some(v) => {
                return Err(Refusal::new(format!(
                    "{CONFIG_PATH} has version {v}; this harno reads version {VERSION}"
                )))
            }
            None => return Err(Refusal::new(format!("{CONFIG_PATH} has no version"))),
        }
        let config: Config = serde_json::from_value(value)
            .map_err(|e| Refusal::new(format!("{CONFIG_PATH}: {e}")))?;
        Ok(config)
    }
}

/// The directory holding `.harno/config.yml`, searching upward from `cwd`.
pub fn find_project(cwd: &Path) -> Option<PathBuf> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if d.join(CONFIG_PATH).is_file() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

pub fn load(project: &Path) -> R<Config> {
    let text = std::fs::read_to_string(project.join(CONFIG_PATH))?;
    let mut config = Config::parse(&text)?;
    config.run.resolved = filter::resolve(project, &config.run.filters)?;
    Ok(config)
}

/// A YAML error as one line: serde-saphyr puts a snippet of the file below
/// the message, and a refusal is one line.
pub fn yaml_error(e: serde_saphyr::Error) -> String {
    e.to_string().lines().next().unwrap_or_default().to_string()
}

/// The placeholder a preset command takes arguments in.
pub const ARGS: &str = "{args}";

/// The entry of `run.sandbox.allow.read` that names every path.
pub const EVERYWHERE: &str = "*";

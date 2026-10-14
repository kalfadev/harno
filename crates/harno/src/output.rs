//! What a verb hands back to the dispatcher: text, exit code, and the
//! metadata the log records.

use std::collections::BTreeSet;
use std::fmt;

/// A refusal: `refused: <reason>` plus, when there is one, the runnable
/// command the agent should try next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
    pub instead: Option<String>,
}

impl Refusal {
    pub fn new(reason: impl Into<String>) -> Self {
        Refusal { reason: reason.into(), instead: None }
    }

    pub fn instead(mut self, command: impl Into<String>) -> Self {
        self.instead = Some(command.into());
        self
    }

    pub fn render(&self) -> String {
        let mut s = format!("refused: {}\n", self.reason);
        if let Some(i) = &self.instead {
            s.push_str(&format!("instead: {i}\n"));
        }
        s
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("refused".into(), self.reason.clone().into());
        if let Some(i) = &self.instead {
            m.insert("instead".into(), i.clone().into());
        }
        serde_json::Value::Object(m)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl From<std::io::Error> for Refusal {
    fn from(e: std::io::Error) -> Self {
        Refusal::new(format!("io error: {e}"))
    }
}

pub type R<T> = Result<T, Refusal>;

/// The result of a verb.
#[derive(Debug, Default)]
pub struct Output {
    pub text: String,
    pub code: i32,
    /// Exit codes of the commands `run` executed.
    pub exits: Option<Vec<i32>>,
    /// Whether each of those failed the call, as the filter of its command
    /// judged it: an exit code its `ok` names did not.
    pub failed: Option<Vec<bool>>,
    /// `run --raw --why`.
    pub why: Option<String>,
    /// Warning lines about what the call printed, e.g. a node to split.
    pub advice: Vec<String>,
    /// Files the call read; check problems in them produce the warning.
    pub touched: BTreeSet<String>,
    /// The commands `run` executed, in the order they started.
    pub commands: Vec<String>,
    /// What the commands wrote before a filter cut it, when one did.
    pub raw_text: Option<String>,
    /// Why the call was refused: nothing ran.
    pub refused: Option<String>,
}

impl Output {
    pub fn text(text: impl Into<String>) -> Self {
        Output { text: text.into(), ..Default::default() }
    }
}

/// Write a file through a temp file and a rename, so a reader never sees
/// half of it.
pub fn atomic_write(path: &std::path::Path, text: &str) -> R<()> {
    use std::io::Write as _;
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.harno-tmp-{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// At most `max` characters, the cut marked with an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

/// Always double-quoted, as titles are in the examples.
pub fn dq(s: &str) -> String {
    let q = quote(s);
    if q.starts_with('"') {
        q
    } else {
        format!("\"{q}\"")
    }
}

/// Quote an argument for a suggested command line.
pub fn quote(s: &str) -> String {
    let plain = !s.is_empty()
        && s.chars().all(|c| c.is_alphanumeric() || "-_./:=,@+".contains(c));
    if plain {
        return s.to_string();
    }
    let mut q = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q
}

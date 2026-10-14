//! What a call prints: a refusal the human can act on, and the one place that
//! cuts a text to a length.

use std::fmt;

#[derive(Debug)]
pub struct Refusal {
    pub reason: String,
    pub instead: Option<String>,
}

impl Refusal {
    pub fn new(reason: impl Into<String>) -> Self {
        Refusal { reason: reason.into(), instead: None }
    }

    /// The next call the human can make instead.
    pub fn instead(mut self, command: impl Into<String>) -> Self {
        self.instead = Some(command.into());
        self
    }

    pub fn render(&self) -> String {
        match &self.instead {
            Some(c) => format!("refused: {}\ninstead: {c}\n", self.reason),
            None => format!("refused: {}\n", self.reason),
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl From<std::io::Error> for Refusal {
    fn from(e: std::io::Error) -> Self {
        Refusal::new(e.to_string())
    }
}

pub type R<T> = Result<T, Refusal>;

/// `s` cut to `max` characters, with one ellipsis where it was cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

//! analytics: the session log of a harno project on one local read-only page.
//!
//! It reads a log and nothing else: the JSONL file of a session, the texts
//! beside it and the raw outputs, all under `.harno/log/`. So it needs no
//! harness and no docs: a directory with a log is all it asks for, and no
//! harno crate: the shape of a log is [`log`].
//!
//! The binary is a thin wrapper around [`main`], which takes its arguments,
//! its directory and its output explicitly so the tests can drive it
//! in-process.

pub mod log;
pub mod out;
pub mod serve;

use std::io::Write;
use std::path::{Path, PathBuf};

use out::{Refusal, R};

pub const USAGE: &str = "usage: harno run analytics [options]

The session log on one local read-only page: the timeline of a session, every
call with what it returned, and the raw output a filter cut.

  harno run analytics                  serve until the call ends, opened in the browser
  harno run analytics --port N         listen on this port instead of a free one
  harno run analytics --no-open        leave the browser alone
  harno run analytics --background     serve on after the call, until --stop
  harno run analytics --stop           end a background server

The log is .harno/log/ of the nearest directory upward that has one.
";

/// The nearest directory upward whose `.harno/log/` exists.
pub fn find_project(cwd: &Path) -> Option<PathBuf> {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        if d.join(log::LOG_DIR).is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

pub fn main(args: Vec<String>, cwd: PathBuf, out: &mut dyn Write) -> i32 {
    if args.first().is_some_and(|a| matches!(a.as_str(), "help" | "--help" | "-h")) {
        let _ = out.write_all(USAGE.as_bytes());
        return 0;
    }
    let result = match find_project(&cwd) {
        Some(project) => serve::serve(&args, &project, out),
        None => Err(Refusal::new(format!("no {} here or in any parent directory", log::LOG_DIR))),
    };
    match result {
        Ok(()) => 0,
        Err(r) => {
            let _ = out.write_all(r.render().as_bytes());
            let _ = out.flush();
            1
        }
    }
}

/// The result type of the tool, for its modules.
pub type Result<T> = R<T>;

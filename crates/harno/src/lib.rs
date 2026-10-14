//! harno: the harness an agent works through. Every command goes through
//! `run` (allowed, preset, filtered), and every call and every harness event
//! is logged as it was. Tools, each a folder in `.harno/tools/`, and programs
//! of their own do the rest through a preset that names their program:
//! `harno run mdmap` reads and writes the docs, `harno run analytics` shows
//! the log to the human.
//!
//! The binary is a thin wrapper around [`cli::main`], which takes its
//! arguments, environment and stdin explicitly so the behaviour tests can
//! drive it in-process.

pub mod cli;
pub mod config;
pub mod filter;
pub mod output;
pub mod run;
pub mod sandbox;
pub mod signals;
pub mod tool;

// The log is its own crate, harno-log: harno writes through it, and a program
// that only reads a log takes the crate without the harness.
pub use harno_log::{gate, log};

pub use cli::{main, Env};

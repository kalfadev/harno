//! The session log: what harno writes around every call, the harness events
//! that reach it from outside, and the numbers a session adds up to.
//!
//! It is a crate of its own because reading a log is not being a harness: a
//! program that only analyses sessions takes harno-log and nothing else.
//! harno re-exports it as its own `log`, `outside` and `id`.

pub mod gate;
pub mod log;

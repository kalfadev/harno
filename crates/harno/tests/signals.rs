//! A signal to harno reaches the commands it runs and all they started, and
//! the call ends with its line; a timeout stops them all as well. These drive
//! the binary: a signal to a test that runs harno in-process stops the test.
#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const SESSION: &str = "0196cccc-0000-7000-8000-000000000001";

/// A command that starts a process in the background, writes who both are,
/// and becomes a sleep: what a signal does to it is the default, where a
/// shell chooses its own exit code (bash 3.2 ends a wait on SIGINT with 129).
/// The one in the background ignores SIGINT, as one a shell starts with & does.
const TREE: &str = "sh -c 'sleep 60 & echo $! > grandchild; echo $$ > child; exec sleep 61'";

fn project(timeout: u64) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".harno")).unwrap();
    std::fs::write(dir.path().join(".harno/config.yml"), format!("version: 1\nrun:\n  timeout: {timeout}\n  allow: [sh -c]\n")).unwrap();
    dir
}

/// harno running `TREE`, with the signal `ignored` ignored from its start, as
/// `nohup` does.
fn start(dir: &Path, ignored: Option<i32>) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harno"));
    cmd.args(["run", TREE]).current_dir(dir).env("HARNO_SESSION", SESSION).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(sig) = ignored {
        // SAFETY: signal is async-signal-safe, the one call between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                libc::signal(sig, libc::SIG_IGN);
                Ok(())
            });
        }
    }
    cmd.spawn().unwrap()
}

/// Wait until `f` gives a value; fail after ten seconds.
fn until<T>(mut f: impl FnMut() -> Option<T>, what: &str) -> T {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < end, "never: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The pid a command wrote into `name`, once it is there.
fn pid_in(dir: &Path, name: &str) -> i32 {
    until(|| std::fs::read_to_string(dir.join(name)).ok().and_then(|t| t.trim().parse().ok()), &format!("{name} written"))
}

/// Wait until process `pid` is gone; a zombie waiting for its parent counts.
fn gone(pid: i32) {
    let zombie = || Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim_start().starts_with("Z"));
    // SAFETY: kill with signal 0 only asks whether the process is there.
    until(|| (unsafe { libc::kill(pid, 0) } != 0 || zombie()).then_some(()), &format!("process {pid} gone"));
}

/// The exit codes in the line of the call.
fn exits(dir: &Path) -> serde_json::Value {
    let log = std::fs::read_to_string(dir.join(format!(".harno/log/harno/{SESSION}.jsonl"))).unwrap();
    let line: serde_json::Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    line["exit"].clone()
}

#[test]
fn a_signal_to_harno_ends_its_commands_and_all_they_started_and_the_call_with_its_line() {
    // SIGINT too, which the process in the background ignores, as one a
    // shell starts with & does: the call leaves nothing of its commands.
    for (sig, code) in [(libc::SIGTERM, 143), (libc::SIGINT, 130), (libc::SIGHUP, 129)] {
        let dir = project(20);
        let mut harno = start(dir.path(), None);
        let (child, grandchild) = (pid_in(dir.path(), "child"), pid_in(dir.path(), "grandchild"));
        // SAFETY: a signal to the harno this test started.
        unsafe { libc::kill(harno.id() as i32, sig) };
        assert_eq!(harno.wait().unwrap().code(), Some(code), "signal {sig}");
        gone(child);
        gone(grandchild);
        assert_eq!(exits(dir.path()), serde_json::json!([code]), "signal {sig}");
        assert!(!dir.path().join(format!(".harno/log/harno/{SESSION}/0001.run")).exists());
    }
}

#[test]
fn a_signal_ignored_when_harno_started_stays_ignored() {
    // As under nohup: SIGHUP goes by, and SIGTERM, sent after it, ends the
    // call. Were SIGHUP caught, it would come first and the call end with 129.
    let dir = project(20);
    let mut harno = start(dir.path(), Some(libc::SIGHUP));
    pid_in(dir.path(), "child");
    // SAFETY: signals to the harno this test started.
    unsafe {
        libc::kill(harno.id() as i32, libc::SIGHUP);
        libc::kill(harno.id() as i32, libc::SIGTERM);
    }
    assert_eq!(harno.wait().unwrap().code(), Some(143));
    assert_eq!(exits(dir.path()), serde_json::json!([143]));
}

#[test]
fn a_timeout_stops_the_command_and_all_it_started() {
    let dir = project(1);
    let mut harno = start(dir.path(), None);
    let (child, grandchild) = (pid_in(dir.path(), "child"), pid_in(dir.path(), "grandchild"));
    assert_eq!(harno.wait().unwrap().code(), Some(124));
    gone(child);
    gone(grandchild);
    assert_eq!(exits(dir.path()), serde_json::json!([124]));
}

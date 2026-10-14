//! Stopping a call. Every command runs in a process group of its own, so
//! that what it starts goes with it: a timeout stops the whole group, and
//! SIGTERM, SIGINT and SIGHUP to harno go on to the group of every command
//! running, after which the call ends the way it ends when a command fails,
//! with its line. A signal that finds no command running ends harno as it
//! would without the handler, and one ignored when harno started, as under
//! `nohup`, stays ignored. SIGKILL reaches no handler: its commands run on.

use std::process::{Command, ExitStatus};
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering::SeqCst};

/// The groups of the commands running now, 0 a free slot.
#[cfg(unix)]
static GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];

/// The signal that stopped the call, 0 while none has.
#[cfg(unix)]
static STOPPED: AtomicI32 = AtomicI32::new(0);

/// Start `cmd` in a process group of its own, with the handler in place.
pub fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                // SAFETY: `forward` only touches atomics and calls kill,
                // signal and raise, which are async-signal-safe.
                unsafe {
                    if libc::signal(sig, forward as *const () as libc::sighandler_t) == libc::SIG_IGN {
                        libc::signal(sig, libc::SIG_IGN);
                    }
                }
            }
        });
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Command `pid` runs, the leader of its group: a signal now reaches it.
pub fn hold(pid: u32) {
    #[cfg(unix)]
    for slot in &GROUPS {
        if slot.compare_exchange(0, pid as i32, SeqCst, SeqCst).is_ok() {
            return;
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Command `pid` ended: no signal goes to its group any more.
pub fn release(pid: u32) {
    #[cfg(unix)]
    for slot in &GROUPS {
        let _ = slot.compare_exchange(pid as i32, 0, SeqCst, SeqCst);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Stop the group of command `pid`: it and all it started.
pub fn kill_group(pid: u32) {
    // SAFETY: kill has no preconditions; a group that is gone answers ESRCH.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

/// Whether a signal stopped the call.
pub fn stopped() -> bool {
    #[cfg(unix)]
    let stopped = STOPPED.load(SeqCst) != 0;
    #[cfg(not(unix))]
    let stopped = false;
    stopped
}

/// The exit code of a command: 128 and the number of the signal that ended
/// it, the way a shell says it, else its own.
pub fn code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    status.code().unwrap_or(1)
}

/// The handler: the signal goes on to every group held, and the call ends
/// once they did; with none held, harno ends as it would without it.
#[cfg(unix)]
extern "C" fn forward(sig: libc::c_int) {
    STOPPED.store(sig, SeqCst);
    let mut sent = false;
    for slot in &GROUPS {
        let group = slot.load(SeqCst);
        if group > 0 {
            // SAFETY: kill is async-signal-safe.
            unsafe { libc::kill(-group, sig) };
            sent = true;
        }
    }
    if !sent {
        // SAFETY: signal and raise are async-signal-safe.
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }
}

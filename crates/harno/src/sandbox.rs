//! The sandbox of `harno run`: with `run.sandbox.enabled` a command writes
//! inside the project and nowhere else.
//!
//! macOS wraps the program in `sandbox-exec`, with a profile written from the
//! config; Linux builds a Landlock ruleset before the fork and applies it in
//! the child, between the fork and the program. Where no sandbox can start —
//! another platform, a kernel without Landlock, an outer sandbox that refuses
//! a sandbox of its own — harno enforces nothing: the commands run and every
//! call ends with the line [`notice`] writes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::{Config, EVERYWHERE};

/// What puts a command in a sandbox on macOS.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Devices a program writes to whatever else it may touch: without them a
/// command cannot write to the pipe it was handed.
const DEVICES: &[&str] = &["/dev/null", "/dev/zero", "/dev/tty", "/dev/fd", "/dev/stdout", "/dev/stderr", "/dev/dtracehelper"];

/// Where the programs themselves live: with reading closed these stay open,
/// or nothing would start.
const SYSTEM: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/opt", "/proc", "/sys", "/dev", "/var", "/tmp", "/run"];

/// What the commands of a call may write, and what they may read while
/// reading is closed. Every path is absolute and resolved: a rule about
/// /tmp says nothing about /private/tmp.
#[derive(Debug, Clone)]
pub struct Sandbox {
    pub project: PathBuf,
    /// `allow.write`: what a command may write beside the project.
    pub write: Vec<PathBuf>,
    /// `allow.read`: the paths a command may read, beside the project and the
    /// system directories. Empty while `read_all`.
    pub read: Vec<PathBuf>,
    /// `allow.read` named every path: nothing is closed but `deny_read`.
    pub read_all: bool,
    /// `deny.read`: what a command may not read; an entry of `read` wins
    /// over one here.
    pub deny_read: Vec<PathBuf>,
}

impl Sandbox {
    /// The sandbox of a config, when it asks for one and one can start. A
    /// sandbox that cannot start is no sandbox: wrapping the command in it
    /// would only make the command fail, so the command runs as it is and
    /// [`unavailable`] is what the call says.
    pub fn of(config: &Config, project: &Path) -> Option<Sandbox> {
        let s = &config.run.sandbox;
        if !s.enabled || unavailable().is_some() {
            return None;
        }
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        Some(Sandbox::new(project, &s.allow.write, &s.allow.read, &s.deny.read, &home))
    }

    pub fn new(project: &Path, write: &[String], read: &[String], deny_read: &[String], home: &Path) -> Sandbox {
        let paths = |list: &[String]| list.iter().filter(|p| p.as_str() != EVERYWHERE).map(|p| resolve(&expand(p, home))).collect();
        Sandbox {
            project: resolve(project),
            write: paths(write),
            read: paths(read),
            read_all: read.iter().any(|p| p == EVERYWHERE),
            deny_read: paths(deny_read),
        }
    }

    /// The project, the paths of `run.sandbox.write` and the devices. A path
    /// that is not there yet would be a rule about nothing.
    fn writable(&self) -> Vec<PathBuf> {
        std::iter::once(self.project.clone())
            .chain(self.write.iter().cloned())
            .chain(DEVICES.iter().map(PathBuf::from))
            .filter(|p| p.exists())
            .collect()
    }

    /// What stays readable while reading is narrowed at all. A path under a
    /// denied one is left out: a deny beats an allow, so naming it would say
    /// nothing on macOS and the wrong thing on Linux, which has no deny.
    fn readable(&self) -> Vec<PathBuf> {
        std::iter::once(self.project.clone())
            .chain(self.read.iter().cloned())
            .chain(SYSTEM.iter().map(PathBuf::from))
            .filter(|p| p.exists() && !self.deny_read.iter().any(|d| p.starts_with(d)))
            .collect()
    }

    /// The Seatbelt profile of macOS: everything is allowed, writing is not,
    /// and the project, the paths of the config and the devices are written
    /// back in. The last rule that matches wins, so the allows follow the deny.
    pub fn profile(&self) -> String {
        let mut p = String::from("(version 1)\n(allow default)\n(deny file-write*)\n");
        for dir in self.writable() {
            p.push_str(&format!("(allow file-write* (subpath {}))\n", lisp(&dir)));
        }
        // A narrower allow.read closes the root and opens what it names. The
        // last rule that matches wins, so deny.read is written last: a deny
        // beats every allow, whatever the allow names.
        if !self.read_all {
            p.push_str(&format!("(deny file-read* (subpath {}))\n", lisp(Path::new("/"))));
        }
        if !self.read_all || !self.deny_read.is_empty() {
            for dir in self.readable() {
                p.push_str(&format!("(allow file-read* (subpath {}))\n", lisp(&dir)));
            }
        }
        for dir in &self.deny_read {
            p.push_str(&format!("(deny file-read* (subpath {}))\n", lisp(dir)));
        }
        p
    }

    /// The program and the arguments that run `program` in the sandbox. On
    /// macOS the sandbox is a program of its own; elsewhere the command stays
    /// as it is and [`Sandbox::restrict`] does the work.
    pub fn command(&self, program: &str, args: &[String]) -> (String, Vec<String>) {
        if !cfg!(target_os = "macos") {
            return (program.to_string(), args.to_vec());
        }
        let mut wrapped = vec!["-p".to_string(), self.profile(), program.to_string()];
        wrapped.extend(args.iter().cloned());
        (SANDBOX_EXEC.to_string(), wrapped)
    }

    /// Put the ruleset on `cmd`. It is built here and applied in the child,
    /// before the program starts: what this process may touch does not change.
    #[cfg(target_os = "linux")]
    pub fn restrict(&self, cmd: &mut Command) -> Result<(), String> {
        use landlock::{path_beneath_rules, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI};
        use std::os::unix::process::CommandExt;

        let abi = ABI::V1;
        // Landlock has no deny: a narrowed read, whether allow.read names the
        // paths or deny.read closes one, becomes an allow of the system
        // directories, the project and allow.read.
        let closed = !self.read_all || !self.deny_read.is_empty();
        let mut handled = AccessFs::from_write(abi);
        if closed {
            handled |= AccessFs::from_read(abi);
        }
        let say = |e: landlock::RulesetError| e.to_string();
        // /dev/stdout and /dev/stderr lead to the streams of harno itself. On a pipe or
        // a socket, which is no path, Landlock refuses the rule (EBADFD), and the
        // command keeps the stream it inherits anyway: a path that does not resolve
        // to one is left out.
        let paths = |v: Vec<PathBuf>| v.into_iter().filter(|p| p.canonicalize().is_ok()).collect::<Vec<_>>();
        let mut ruleset = Ruleset::default().handle_access(handled).map_err(say)?.create().map_err(say)?;
        ruleset = ruleset.add_rules(path_beneath_rules(paths(self.writable()), AccessFs::from_write(abi))).map_err(say)?;
        if closed {
            ruleset = ruleset.add_rules(path_beneath_rules(paths(self.readable()), AccessFs::from_read(abi))).map_err(say)?;
        }
        // A pre_exec closure is FnMut and the ruleset is applied once: harno
        // spawns each command once.
        let mut once = Some(ruleset);
        unsafe {
            cmd.pre_exec(move || match once.take() {
                Some(r) => r.restrict_self().map(|_| ()).map_err(|e| std::io::Error::other(e.to_string())),
                None => Ok(()),
            });
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn restrict(&self, _cmd: &mut Command) -> Result<(), String> {
        Ok(())
    }
}

/// Why no sandbox is running, when none can be. The answer holds for the
/// whole process, so it is taken once.
pub fn unavailable() -> Option<&'static str> {
    static WHY: OnceLock<Option<String>> = OnceLock::new();
    WHY.get_or_init(probe).as_deref()
}

/// The line that ends every call while `run.sandbox` is on and no sandbox of
/// its own can start, given why. macOS refuses a sandbox inside another one
/// with `sandbox_apply: Operation not permitted`: the outer one then holds the
/// commands, and the line says so and asks nothing of the reader. Anything
/// else leaves them unguarded, and that is a warning.
pub fn notice(why: &str) -> String {
    if why.contains("sandbox_apply: Operation not permitted") {
        format!("note: run.sandbox steps aside, an outer sandbox holds this process — {why}")
    } else {
        format!("warning: run.sandbox is on and no sandbox is running — {why}")
    }
}

/// The one way to know is to start one: an outer sandbox refuses a sandbox of
/// its own, and that refusal shows nowhere else.
#[cfg(target_os = "macos")]
fn probe() -> Option<String> {
    match Command::new(SANDBOX_EXEC).args(["-p", "(version 1)(allow default)", "/usr/bin/true"]).output() {
        Ok(o) if o.status.success() => None,
        Ok(o) => Some(
            line_of(&String::from_utf8_lossy(&o.stderr))
                .unwrap_or_else(|| format!("sandbox-exec exited {}", o.status.code().unwrap_or(1))),
        ),
        Err(e) => Some(format!("sandbox-exec did not start ({e})")),
    }
}

/// The kernel answers with the Landlock ABI it offers, the way the landlock
/// crate asks it (flag 1, LANDLOCK_CREATE_RULESET_VERSION), which that crate
/// keeps private; below 1 there is none.
#[cfg(target_os = "linux")]
fn probe() -> Option<String> {
    let abi = unsafe { libc::syscall(libc::SYS_landlock_create_ruleset, std::ptr::null::<libc::c_void>(), 0usize, 1u32) };
    (abi < 1).then(|| "the kernel has no Landlock".to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn probe() -> Option<String> {
    Some(format!("no sandbox on {}", std::env::consts::OS))
}

/// The first line that says something.
#[cfg(target_os = "macos")]
fn line_of(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.to_string())
}

/// A `~` is the home directory; every other path is taken as it is written.
fn expand(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

/// The path a rule takes: the symlinks of its directories resolved, or the
/// path as it is written when it is not there.
fn resolve(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A path as a string of a Seatbelt profile.
fn lisp(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

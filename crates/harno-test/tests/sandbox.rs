//! `run.sandbox`: where a command may write, and what a call says while no
//! sandbox is running.

use harno::sandbox::{notice, unavailable, Sandbox};
use harno_test::Project;

/// A project whose commands run in the sandbox.
fn project() -> Project {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [touch]\n  sandbox:\n    enabled: true\n");
    p
}

/// The line a call ends with while no sandbox can start here.
fn warning() -> Option<String> {
    unavailable().map(|why| notice(why) + "\n")
}

#[test]
fn an_outer_sandbox_is_a_note_and_no_sandbox_a_warning() {
    let outer = notice("sandbox-exec: sandbox_apply: Operation not permitted");
    assert_eq!(outer, "note: run.sandbox steps aside, an outer sandbox holds this process — sandbox-exec: sandbox_apply: Operation not permitted");
    let none = notice("the kernel has no Landlock");
    assert_eq!(none, "warning: run.sandbox is on and no sandbox is running — the kernel has no Landlock");
}

#[test]
fn the_profile_opens_the_project_and_the_written_paths_and_nothing_else() {
    let p = Project::new();
    let home = p.path("home");
    std::fs::create_dir_all(home.join("cargo")).unwrap();
    let s = Sandbox::new(p.root(), &["~/cargo".to_string()], &["*".to_string()], &[], &home);
    let text = s.profile();
    assert!(text.starts_with("(version 1)\n(allow default)\n(deny file-write*)\n"), "{text}");
    let open = |path: std::path::PathBuf| format!("(allow file-write* (subpath \"{}\"))", path.canonicalize().unwrap().display());
    assert!(text.contains(&open(p.root().to_path_buf())), "{text}");
    assert!(text.contains(&open(home.join("cargo"))), "{text}");
    assert!(!text.contains("file-read"), "{text}");
}

#[test]
fn a_denied_path_beats_an_allow_inside_it() {
    let p = Project::new();
    let home = p.path("home");
    std::fs::create_dir_all(home.join("nvm")).unwrap();
    let s = Sandbox::new(p.root(), &[], &["*".to_string(), "~/nvm".to_string()], &["~".to_string()], &home);
    let text = s.profile();
    let home = home.canonicalize().unwrap();
    assert!(text.contains(&format!("(deny file-read* (subpath \"{}\"))", home.display())), "{text}");
    assert!(!text.contains(&format!("(allow file-read* (subpath \"{}\"))", home.join("nvm").display())), "{text}");
    assert!(text.contains(&format!("(allow file-read* (subpath \"{}\"))", p.root().canonicalize().unwrap().display())), "{text}");
    // The last rule that matches wins, so every deny is written after the allows.
    assert!(text.rfind("(deny file-read*") > text.rfind("(allow file-read*"), "{text}");
}

#[test]
fn a_star_with_nothing_denied_leaves_reading_open() {
    let p = Project::new();
    let s = Sandbox::new(p.root(), &[], &["*".to_string()], &[], &p.path("home"));
    assert!(!s.profile().contains("file-read"), "{}", s.profile());
}

#[test]
fn a_read_list_without_a_star_is_the_whole_of_what_may_be_read() {
    let p = Project::new();
    let home = p.path("home");
    std::fs::create_dir_all(home.join("nvm")).unwrap();
    let s = Sandbox::new(p.root(), &[], &["~/nvm".to_string()], &[], &home);
    let text = s.profile();
    assert!(text.contains("(deny file-read* (subpath \"/\"))"), "{text}");
    assert!(text.contains(&format!("(allow file-read* (subpath \"{}\"))", home.canonicalize().unwrap().join("nvm").display())), "{text}");
    assert!(text.contains("(allow file-read* (subpath \"/usr\"))"), "{text}");
}

#[test]
fn a_command_writes_inside_the_project() {
    let p = project();
    p.harno(&["run", "touch inside.txt"]).ok();
    assert!(p.exists("inside.txt"));
}

#[test]
fn a_command_does_not_write_outside_the_project() {
    let p = project();
    let outside = std::env::temp_dir().join(format!("harno-escape-{}", std::process::id()));
    let _ = std::fs::remove_file(&outside);
    let command = format!("touch {}", outside.display());
    let c = p.harno(&["run", &command]);
    match warning() {
        // No sandbox here: the call says so and the command ran as it always does.
        Some(w) => assert!(c.out.ends_with(&w), "{}", c.out),
        None => {
            assert_ne!(c.code, 0, "{}", c.out);
            assert!(!outside.exists(), "{}", c.out);
        }
    }
    let _ = std::fs::remove_file(&outside);
}

#[test]
fn every_call_carries_the_warning_while_no_sandbox_runs() {
    let p = project();
    let out = p.harno(&["log", "ls"]).out;
    match warning() {
        Some(w) => assert!(out.ends_with(&w), "{out}"),
        None => assert!(!out.contains("run.sandbox"), "{out}"),
    }
}

#[test]
fn a_call_of_a_project_without_the_sandbox_says_nothing_about_it() {
    let p = Project::new();
    let out = p.harno(&["log", "ls"]).out;
    assert!(!out.contains("run.sandbox"), "{out}");
}

//! `harno run`: the door every command goes through.

use harno_test::{Project, SESSION};

fn project() -> Project {
    let p = Project::new();
    p.config(
        "version: 1
run:
  timeout: 2
  allow: [echo, printf, false, sh -c, ./scripts/*.sh, ls, sleep]
  presets:
    gate: {seq: [\"echo build\", \"echo test\"]}
    quick: {par: [\"echo a\", \"echo b\"]}
    danger: {seq: [\"touch made-by-preset\"]}
",
    );
    p
}

#[test]
fn seq_runs_in_order_and_stops_at_the_first_failure() {
    let p = project();
    let c = p.harno(&["run", "--seq", "echo one", "sh -c 'echo broke; exit 7'", "echo never"]);
    assert_eq!(c.code, 7, "the failed command's own exit code");
    assert!(c.out.starts_with("--- 1: echo one\none\n--- 2: "), "{}", c.out);
    assert!(c.out.contains("\none\n--- 2: sh -c 'echo broke; exit 7' --- FAIL exit 7 · "), "{}", c.out);
    assert!(c.out.contains("\nbroke\n"), "{}", c.out);
    assert!(!c.out.contains("never"), "{}", c.out);
    assert!(c.out.ends_with("\n\nstopped: 2/3 ran — 2: exit 7\n"), "{}", c.out);
}

#[test]
fn seq_that_passes() {
    let p = project();
    let c = p.harno(&["run", "--seq", "echo one", "echo two"]);
    assert_eq!(c.code, 0);
    assert!(c.out.ends_with("\n\n2/2 ran\n"), "{}", c.out);
}

#[test]
fn a_call_of_one_command_gives_back_its_output_alone() {
    let p = project();
    assert_eq!(p.harno(&["run", "echo one"]).out, "one\n", "no header and no summary: the caller wrote the command");
    let c = p.harno(&["run", "sleep 0"]);
    assert!(c.out.starts_with("ok · "), "an output that says nothing still says that it ran: {}", c.out);
    let c = p.harno(&["run", "false"]);
    assert_eq!(c.code, 1);
    assert!(c.out.starts_with("FAIL exit 1 · "), "{}", c.out);
    let c = p.harno(&["run", "ls no-such-file"]);
    assert!(c.out.contains("\n\nFAIL exit "), "a failure keeps what it wrote: {}", c.out);
}

#[test]
fn a_long_command_is_cut_in_a_header() {
    let p = project();
    let long = format!("echo {}", "x".repeat(80));
    let c = p.harno(&["run", "--seq", &long, "echo two"]);
    let head = c.out.lines().next().unwrap();
    assert!(head.starts_with("--- 1: echo x"), "{head}");
    assert!(head.ends_with(" … 85 chars"), "{head}");
}

#[test]
fn a_block_that_passed_at_once_is_its_command_alone() {
    let p = project();
    // The footer counts what passed; a failure and a wait say so in the header.
    let c = p.harno(&["run", "--seq", "echo one", "sleep 1.1", "sh -c 'exit 3'"]);
    assert!(c.out.starts_with("--- 1: echo one\none\n--- 2: sleep 1.1 · 1."), "{}", c.out);
    assert!(c.out.contains("\n--- 3: sh -c 'exit 3' --- FAIL exit 3 · "), "{}", c.out);
}

#[test]
fn par_runs_everything_and_reports_every_result() {
    let p = project();
    let c = p.harno(&["run", "--par", "sh -c 'sleep 0.3; exit 3'", "echo fast", "false"]);
    assert_eq!(c.code, 3, "first non-zero in command order: {}", c.out);
    assert!(c.out.contains("--- 2: echo fast\n"), "{}", c.out);
    assert!(c.out.contains("--- 3: false --- FAIL exit 1"), "{}", c.out);
    assert!(c.out.contains("--- 1: sh -c 'sleep 0.3; exit 3' --- FAIL exit 3"), "{}", c.out);
    // Printed in the order they finish: the slow one last.
    let slow = c.out.find("--- 1:").unwrap();
    let fast = c.out.find("--- 2:").unwrap();
    assert!(fast < slow, "{}", c.out);
    // Every failure and its code in one line.
    assert!(c.out.ends_with("\n\n3 ran, 2 failed — 1: exit 3, 3: exit 1\n"), "{}", c.out);
}

#[test]
fn par_is_parallel() {
    let p = project();
    let started = std::time::Instant::now();
    p.harno(&["run", "--par", "sleep 0.5", "sleep 0.5", "sleep 0.5"]).ok();
    assert!(started.elapsed().as_secs_f64() < 1.2);
}

#[test]
fn a_command_not_in_the_allowlist_is_refused_with_the_raw_form() {
    let p = project();
    let c = p.harno(&["run", "rm -rf target"]);
    assert_eq!(c.out, "refused: 'rm' is not in run.allow\ninstead: harno run --raw \"rm -rf target\" --why \"<reason>\"\n");
    assert_eq!(c.code, 1);
}

#[test]
fn the_words_of_one_allowed_command_given_apart_are_told_to_go_in_quotes() {
    let p = project();
    p.config("version: 1\nrun:\n  allow: [echo]\n");
    let c = p.harno(&["run", "echo", "hi", "there"]);
    assert_eq!(c.out, "refused: 'echo hi there' is one command, given as 3 words\ninstead: harno run \"echo hi there\"\n");
    // Words that are commands of their own still run one by one, and --seq keeps them apart.
    p.harno(&["run", "echo", "echo"]).ok();
    let c = p.harno(&["run", "--seq", "echo", "hi"]);
    assert!(c.refused().starts_with("refused: 'hi' is not in run.allow"), "{}", c.out);
    // Words that make no allowed command keep the refusal of the word.
    let c = p.harno(&["run", "rm", "echo"]);
    assert!(c.refused().starts_with("refused: 'rm' is not in run.allow"), "{}", c.out);
}

#[test]
fn allowlist_is_a_word_prefix() {
    let p = project();
    p.config("version: 1\nrun:\n  allow: [echo hi]\n");
    p.harno(&["run", "echo hi there"]).ok();
    p.harno(&["run", "echo hi"]).ok();
    let c = p.harno(&["run", "echo hix"]);
    assert!(c.refused().starts_with("refused: 'echo hix' is not in run.allow"), "{}", c.out);
    p.harno(&["run", "echo"]).refused();
}

#[test]
fn nothing_runs_when_one_command_of_a_chain_is_refused() {
    let p = project();
    let c = p.harno(&["run", "--seq", "sh -c 'touch made'", "rm made"]);
    c.refused();
    assert!(!p.exists("made"));
}

#[test]
fn raw_runs_anything_and_logs_why() {
    let p = project();
    let c = p.harno(&["run", "--raw", "touch raw-made", "--why", "clean rebuild"]);
    assert_eq!(c.code, 0, "{}", c.out);
    assert!(p.exists("raw-made"));
    let call = p.calls().pop().unwrap();
    assert_eq!(call["verb"], "run");
    assert_eq!(call["args"], serde_json::json!(["--raw", "touch raw-made"]));
    assert_eq!(call["why"], "clean rebuild");
    assert_eq!(call["cmd"], serde_json::json!(["touch raw-made"]));
    assert_eq!(call["exit"], serde_json::json!([0]));
}

#[test]
fn raw_needs_a_reason() {
    let p = project();
    let c = p.harno(&["run", "--raw", "touch x"]);
    c.refused();
    assert_eq!(c.instead().unwrap(), "harno run --raw \"touch x\" --why \"<reason>\"");
    assert!(!p.exists("x"));
}

#[test]
fn presets_need_no_allow_entry() {
    let p = project();
    let c = p.harno(&["run", "gate"]);
    assert_eq!(c.code, 0, "{}", c.out);
    assert!(c.out.contains("--- 1: echo build\n"), "{}", c.out);
    assert!(c.out.contains("--- 2: echo test\n"), "{}", c.out);
    p.harno(&["run", "danger"]).ok();
    assert!(p.exists("made-by-preset"));
    let c = p.harno(&["run", "quick"]);
    assert!(c.out.ends_with("2 ran, 0 failed\n"), "{}", c.out);
}

#[test]
fn seq_and_par_take_presets() {
    let p = project();
    // A preset in --seq runs its own commands, numbered with the rest.
    let c = p.harno(&["run", "--seq", "gate", "echo after", "quick"]);
    assert_eq!(c.code, 0, "{}", c.out);
    assert!(c.out.contains("--- 1: echo build\n"), "{}", c.out);
    assert!(c.out.contains("--- 2: echo test\n"), "{}", c.out);
    assert!(c.out.contains("--- 3: echo after\n"), "{}", c.out);
    assert!(c.out.ends_with("\n\n5/5 ran\n"), "{}", c.out);
    // A failure before a preset stops it.
    let c = p.harno(&["run", "--seq", "sh -c 'exit 3'", "danger"]);
    assert_eq!(c.code, 3, "{}", c.out);
    assert!(!p.exists("made-by-preset"));
    assert!(c.out.ends_with("stopped: 1/2 ran — 1: exit 3\n"), "{}", c.out);
    // In --par a seq preset keeps its own order.
    let c = p.harno(&["run", "--par", "gate", "echo x"]);
    assert!(c.out.ends_with("3 ran, 0 failed\n"), "{}", c.out);
    assert!(c.out.find("--- 1: echo build").unwrap() < c.out.find("--- 2: echo test").unwrap(), "{}", c.out);
}

#[test]
fn no_shell_interpretation() {
    let p = project();
    let c = p.harno(&["run", "echo a && echo b | cat $HOME; x > out.txt"]);
    assert_eq!(c.code, 0);
    assert!(c.out.starts_with("a && echo b | cat $HOME; x > out.txt\n"), "{}", c.out);
    assert!(!p.exists("out.txt"));
}

#[test]
fn quotes_group_words_and_unquoted_globs_expand() {
    let p = project();
    p.write("docs/a.md", "");
    p.write("docs/b.md", "");
    let c = p.harno(&["run", "printf '%s|' \"two words\" docs/*.md '*.md'"]);
    assert_eq!(c.out, "two words|docs/a.md|docs/b.md|*.md|\n");
}

#[test]
fn a_script_path_is_a_command() {
    let p = project();
    // macOS checks a freshly written executable on its first run, the suspected
    // cause of this test outlasting the 2 s of project() under the load of the
    // whole suite; the script is what is tested here, not the timeout.
    p.config("version: 1\nrun:\n  allow: [{command: ./scripts/*.sh, timeout: 60}]\n");
    p.write("scripts/gate.sh", "#!/bin/sh\necho from script\nexit 4\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p.path("scripts/gate.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let c = p.harno(&["run", "./scripts/gate.sh"]);
    assert_eq!(c.code, 4);
    assert!(c.out.contains("from script"), "{}", c.out);
}

#[test]
fn several_commands_without_a_flag_run_in_sequence() {
    let p = project();
    let c = p.harno(&["run", "echo a", "false", "echo never"]);
    assert_eq!(c.code, 1);
    assert!(c.out.contains("--- 1: echo a\n"), "{}", c.out);
    assert!(!c.out.contains("never"), "{}", c.out);
    assert!(c.out.ends_with("stopped: 2/3 ran — 2: exit 1\n"), "{}", c.out);
}

#[test]
fn par_json_lists_every_exit_code() {
    let p = project();
    let v = p.harno(&["run", "--par", "echo a", "sh -c 'exit 3'", "sh -c 'exit 5'", "--json"]).json();
    assert_eq!(v["exits"], serde_json::json!([0, 3, 5]));
    assert_eq!(v["exit"], 3);
}

/// A project whose commands go step by step (`Project::turns`), under a
/// timeout, so that a test that breaks stops instead of waiting forever.
fn stepwise() -> Project {
    let p = Project::new();
    p.config("version: 1\nrun:\n  timeout: 20\n  allow: [sh scripts/turn.sh, sh -c, echo, false, seq]\n  filters:\n    seq: { script: tail -n 1 }\n");
    p.turns();
    p
}

/// The raw file of call `n`.
fn raw(n: u32) -> String {
    format!(".harno/log/harno/{SESSION}/{n:04}.raw")
}

#[test]
fn a_timeout_of_zero_is_none() {
    let p = Project::new();
    // An entry of 0 beats run.timeout, and 0 is no limit.
    let Ok(config) = harno::config::Config::parse("version: 1\nrun:\n  timeout: 1\n  allow: [{command: sleep, timeout: 0}]\n") else { panic!("config") };
    assert_eq!(harno::run::timeout_of(&config, "sleep 5"), std::time::Duration::ZERO);
    p.config("version: 1\nrun:\n  timeout: 0\n  allow: [sleep]\n");
    let c = p.harno(&["run", "sleep 0.3"]);
    assert_eq!(c.code, 0, "{}", c.out);
    // A run.timeout of 0 holds for a filter script as well: it runs to its end.
    p.config("version: 1\nrun:\n  timeout: 0\n  allow: [seq]\n  filters:\n    seq: { script: tail -n 1 }\n");
    let c = p.harno(&["run", "seq 1 300"]);
    assert!(c.ok().contains("300") && !c.out.contains("\n150\n") && !c.out.contains("filter script"), "{}", c.out);
}

#[test]
fn a_command_writes_into_the_raw_file_of_the_call_as_it_runs() {
    let p = stepwise();
    std::thread::scope(|s| {
        let call = s.spawn(|| p.harno(&["run", "sh scripts/turn.sh +one +two @stop"]));
        // One command: its lines alone, while it still runs.
        p.wait_for(&raw(1), "one\ntwo\n");
        assert!(!call.is_finished());
        p.write("stop", "");
        assert!(call.join().unwrap().ok().starts_with("one\ntwo\n"));
    });
    // The caller read all of it, so the raw file went with the end of the call.
    assert!(!p.exists(&raw(1)));
}

#[test]
fn several_commands_write_under_their_headers_as_they_take_turns() {
    let p = stepwise();
    let (a, b) = ("sh scripts/turn.sh +a b +c @stop", "sh scripts/turn.sh a +b");
    std::thread::scope(|s| {
        let call = s.spawn(|| p.harno(&["run", "--par", a, b]));
        // 1 writes a, 2 waits for it and writes b, 1 waits for b and writes c:
        // a line comes under the header of its command, again after the other wrote.
        p.wait_for(&raw(1), &format!("--- 1: {a}\na\n--- 2: {b}\nb\n--- 1: {a}\nc\n"));
        p.write("stop", "");
        let c = call.join().unwrap();
        assert!(c.ok().ends_with("\n2 ran, 0 failed\n"), "{}", c.out);
    });
    assert!(!p.exists(&raw(1)));
}

#[test]
fn a_watched_call_shows_each_line_as_it_comes_unfiltered_and_not_again() {
    let p = stepwise();
    let all: String = (1..=300).map(|i| format!("{i}\n")).collect();
    // Not watched, the filter cuts; watched, the terminal gets every line, and
    // the call ends with how the command ended, the raw file keeping the rest.
    assert!(!p.harno(&["run", "seq 1 300"]).ok().contains("\n150\n"));
    let (c, seen) = p.harno_watched(&["run", "seq 1 300"]);
    assert_eq!(seen, all);
    assert!(c.ok().starts_with("ok · ") && c.out.ends_with("s\n") && c.out.lines().count() == 1, "{}", c.out);
    assert!(p.read(&raw(2)).starts_with(&all));
    // Several commands: a header each, and at the end the headers and the summary.
    let (c, seen) = p.harno_watched(&["run", "--seq", "echo a", "echo b"]);
    assert_eq!(seen, "--- 1: echo a\na\n--- 2: echo b\nb\n");
    assert_eq!(c.ok(), "--- 1: echo a\n--- 2: echo b\n\n2/2 ran\n");
    assert_eq!(p.read(&raw(3)), "--- 1: echo a\na\n--- 2: echo b\nb\n\n2/2 ran\n");
    // A failure says so at the end, as it would unwatched.
    let (c, seen) = p.harno_watched(&["run", "--seq", "echo a", "false"]);
    assert_eq!(seen, "--- 1: echo a\na\n");
    assert_eq!(c.code, 1);
    assert!(c.out.starts_with("--- 1: echo a\n--- 2: false --- FAIL exit 1 · ") && c.out.ends_with("s\n\nstopped: 2/2 ran — 2: exit 1\n"), "{}", c.out);
    // --json is one value for a program to read: nothing goes to the terminal.
    let (c, seen) = p.harno_watched(&["run", "--json", "echo a"]);
    assert_eq!(seen, "");
    assert_eq!(c.json()["commands"][0]["output"], "a\n");
}

#[test]
fn what_a_process_left_behind_writes_after_the_call_reaches_nothing() {
    let p = stepwise();
    // The command ends at once; the process it leaves holds its output open
    // (the call gives it two seconds) and writes only once the call is over.
    let late = "sh -c '(i=0; until [ -e stop ] || [ $i -ge 2000 ]; do sleep 0.01; i=$((i+1)); done; echo late; touch done) & echo now'";
    let (c, seen) = p.harno_watched(&["run", late]);
    c.ok();
    assert_eq!(seen, "now\n");
    let kept = p.read(&raw(1));
    assert!(kept.starts_with("now\n"), "{kept}");
    p.write("stop", "");
    p.wait_for("done", "");
    // The late line reached the pipe the call read; give its reader the time to take it.
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(p.read(&raw(1)), kept);
}

#[test]
fn a_timeout_stops_the_command() {
    let p = project();
    let started = std::time::Instant::now();
    let c = p.harno(&["run", "sleep 10"]);
    assert_eq!(c.code, 124);
    assert!(c.out.starts_with("FAIL timeout · "), "{}", c.out);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn an_allow_entry_can_carry_its_own_timeout() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  timeout: 60\n  allow: [sleep, {command: sleep 10, timeout: 1}]\n");
    let started = std::time::Instant::now();
    let c = p.harno(&["run", "sleep 10"]);
    assert_eq!(c.code, 124, "the longest entry that matches gives the timeout: {}", c.out);
    assert!(started.elapsed().as_secs() < 5);
    p.config("version: 1\nrun:\n  allow: [{command: sleep, wait: 1}]\n");
    p.harno(&["run", "sleep 0"]).refused();
}

#[test]
fn a_missing_program() {
    let p = project();
    p.config("version: 1\nrun:\n  allow: [no-such-program-xyz]\n");
    let c = p.harno(&["run", "no-such-program-xyz"]);
    assert_eq!(c.code, 127);
    assert!(c.out.contains("FAIL not found"), "{}", c.out);
}

#[test]
fn stdout_and_stderr_arrive_together() {
    let p = project();
    let c = p.harno(&["run", "sh -c 'echo out; echo err >&2'"]);
    assert_eq!(c.out, "out\nerr\n");
}

#[test]
fn run_json() {
    let p = project();
    let v = p.harno(&["run", "--seq", "echo hi", "--json"]).json();
    assert_eq!(v["commands"][0]["output"], "hi\n");
    assert_eq!(v["exit"], 0);
}

#[test]
fn json_carries_the_warnings_as_a_field_so_the_output_stays_one_json_value() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo]\nwarn:\n  output: 1\n");
    let c = p.harno(&["run", "echo hi", "--json"]);
    assert!(c.out.starts_with("{"), "{}", c.out);
    let v = c.json();
    assert_eq!(v["exit"], 0);
    let warnings: Vec<&str> = v["warnings"].as_array().unwrap().iter().map(|w| w.as_str().unwrap()).collect();
    assert_eq!(warnings.len(), 1, "{}", c.out);
    assert!(warnings[0].contains("output ") && warnings[0].contains(" > "), "{}", c.out);
    assert!(!warnings[0].starts_with("warning: "), "{}", c.out);
    // The text form is the one that puts a warning in front of the output.
    let c = p.harno(&["run", "echo hi"]);
    assert!(c.out.starts_with("warning: "), "{}", c.out);
}

#[test]
fn run_refuses_what_it_cannot_run_before_it_runs_anything() {
    let p = project();
    let cases: [(&[&str], &str); 8] = [
        (&["run"], "run needs a command"),
        (&["run", "--seq", "--par", "echo a"], "--seq and --par exclude each other"),
        (&["run", "--raw", "echo a", "echo b", "--why", "two"], "--raw takes exactly one command"),
        (&["run", "--raw", "--seq", "echo a", "--why", "seq"], "--raw takes exactly one command"),
        (&["run", "--why", "no raw", "echo a"], "--why goes with --raw"),
        (&["run", "echo", "--", "a"], "arguments after -- go to a preset"),
        (&["run", "echo \"a"], "unclosed \" in"),
        (&["run", ""], "empty command"),
    ];
    for (args, reason) in cases {
        let c = p.harno(args);
        assert!(c.refused().contains(reason), "{args:?}: {}", c.out);
        assert!(!c.out.contains("--- 1:"), "{args:?} ran: {}", c.out);
    }
}

fn nested() -> Project {
    let p = Project::new();
    p.config(
        "version: 1
run:
  timeout: 2
  allow: [echo]
  presets:
    say: {seq: [\"echo {args}\"]}
    two: {seq: [\"echo one\", \"echo two\"]}
    both: {seq: [two, \"say -- three\", \"echo four\"]}
    pass: {seq: [\"say -- {args}\"]}
    fan: {par: [\"echo a\", say]}
    lost: {par: [two, \"echo x\"]}
    loop: {seq: [again]}
    again: {seq: [loop]}
    hidden: {seq: [\"touch made-by-preset\"]}
    outer: {seq: [hidden]}
    cat: {seq: [\"cat {args}\"]}
    wrap: {seq: [\"cat -- f.txt\"]}
",
    );
    p
}

#[test]
fn a_preset_item_can_name_another_preset() {
    let p = nested();
    let c = p.harno(&["run", "both"]);
    assert!(c.out.starts_with("--- 1: echo one\n"), "{}", c.out);
    assert!(c.out.contains("\n--- 2: echo two\n"), "{}", c.out);
    assert!(c.out.contains("\n--- 3: echo 'three'\n"), "{}", c.out);
    assert!(c.out.contains("\n--- 4: echo four\n"), "{}", c.out);
    assert!(c.out.ends_with("\n\n4/4 ran\n"), "{}", c.out);
    let c = p.harno(&["run", "pass", "--", "hi"]);
    assert_eq!(c.out, "hi\n", "the argument reached the command");
    // A preset inside a preset skips run.allow too: that is what presets are for.
    p.harno(&["run", "outer"]).ok();
    assert!(p.exists("made-by-preset"));
}

#[test]
fn a_nested_preset_runs_the_way_of_the_preset_around_it() {
    let p = nested();
    let c = p.harno(&["run", "fan"]);
    assert!(c.out.ends_with("2 ran, 0 failed\n"), "{}", c.out);
    let c = p.harno(&["run", "lost"]);
    assert!(c.refused().contains("preset lost runs in parallel; preset two inside it would lose its order"), "{}", c.out);
    let c = p.harno(&["run", "loop"]);
    assert!(c.refused().contains("preset loop > again > loop includes itself"), "{}", c.out);
}

#[test]
fn an_item_that_names_its_own_preset_is_the_program_of_that_name() {
    let p = nested();
    p.write("f.txt", "hi\n");
    // cat: {seq: ["cat {args}"]} runs the program cat, not the preset again.
    assert_eq!(p.harno(&["run", "cat", "f.txt"]).ok(), "hi\n");
    // Named from another preset it is still the preset, which runs the program.
    assert_eq!(p.harno(&["run", "wrap"]).ok(), "hi\n");
}

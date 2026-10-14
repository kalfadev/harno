//! `run.filters`: what a command's output looks like before the agent reads
//! it, and how the raw output stays reachable.
//!
//! Outputs here are long lines: a filter that would not make the output
//! shorter than the raw output (its note counted) shows the raw output.

use harno_test::{Project, SESSION};

const PAD: &str = "--------------------------------------------------";

/// A command that prints one padded line per word.
fn lines(words: &[&str]) -> String {
    let args: Vec<String> = words.iter().map(|w| format!("'{w} {PAD}'")).collect();
    format!("printf '%s\\n' {}", args.join(" "))
}

/// The same through `sh -c`, ending with `exit`.
fn sh(words: &[&str], exit: i32) -> String {
    format!("sh -c 'for w in {}; do echo \"$w {PAD}\"; done; exit {exit}'", words.join(" "))
}

/// `seq` with every number padded.
fn seq(from: u32, to: u32) -> String {
    format!("seq -f '%g {PAD}' {from} {to}")
}

/// A project whose `run.filters` is `filters` (YAML under `filters:`).
fn project(filters: &str) -> Project {
    let p = Project::new();
    p.config(&format!(
        "version: 1
run:
  timeout: 5
  allow: [printf, sh -c, seq, echo, false]
  presets:
    gate: {{seq: [{:?}, {:?}]}}
  filters:
{filters}
",
        lines(&["noise", "keep"]),
        seq(1, 3)
    ));
    // `check` has nothing to fix.
    p.write("docs/README.md", "# Docs {a00001}\n");
    p
}

/// The number of the call in the note of a filtered output.
#[track_caller]
fn raw_call(out: &str) -> String {
    let at = out.find(": harno log raw ").unwrap_or_else(|| panic!("no note in\n{out}"));
    out[at + ": harno log raw ".len()..].lines().next().unwrap_or_default().to_string()
}

/// The first word of each line of block `k`; a note stays whole. A call of one
/// command has no header: its whole output is block 1.
fn nth(out: &str, k: usize) -> Vec<String> {
    let lines = out.lines();
    let body: Vec<&str> = if out.contains("--- 1: ") {
        lines.skip_while(|l| !l.starts_with(&format!("--- {k}: "))).skip(1).take_while(|l| !l.starts_with("--- ") && !l.is_empty()).collect()
    } else {
        lines.skip_while(|l| l.starts_with("warning: ")).take_while(|l| !l.is_empty()).collect()
    };
    body.iter().map(|l| if l.starts_with('…') { l.to_string() } else { l.split(' ').next().unwrap().to_string() }).collect()
}

fn block(out: &str) -> Vec<String> {
    nth(out, 1)
}

fn five() -> String {
    lines(&["one", "two", "three", "four", "five"])
}

#[test]
fn drop_removes_lines_and_says_how_to_get_them_back() {
    let p = project("    printf: { drop: ['^t'] }");
    let c = p.harno(&["run", &five()]);
    assert_eq!(c.code, 0, "{}", c.out);
    let n = raw_call(&c.out);
    assert_eq!(block(&c.out), ["one", "four", "five", &format!("… 2 lines: harno log raw {n}")]);
    // What the command wrote is kept whole, and any later call can read it.
    let raw = format!("one {PAD}\ntwo {PAD}\nthree {PAD}\nfour {PAD}\nfive {PAD}\n");
    assert!(p.harno(&["log", "raw", &n]).ok().contains(&raw), "the call keeps what the command wrote");
}

#[test]
fn keep_keeps_only_matching_lines() {
    let p = project("    printf: { keep: ['^f'] }");
    let c = p.harno(&["run", &five()]);
    assert_eq!(block(&c.out)[..2], ["four", "five"]);
    assert!(block(&c.out)[2].starts_with("… 3 lines:"), "{}", c.out);
}

#[test]
fn dedupe_writes_a_repeated_neighbour_once() {
    let p = project("    printf: { dedupe: true }");
    let c = p.harno(&["run", &lines(&["warn", "warn", "warn", "end", "warn"])]);
    // Only neighbours: the last `warn` is another place in the output.
    let out: Vec<&str> = c.out.lines().collect();
    assert_eq!(out[0], format!("warn {PAD} (×3)"), "{}", c.out);
    assert_eq!(block(&c.out)[1..3], ["end", "warn"], "{}", c.out);
    assert!(block(&c.out)[3].starts_with("… 2 lines:"), "{}", c.out);
}

#[test]
fn head_and_tail_cut_the_middle_and_the_note_sits_in_the_cut() {
    let p = project("    seq: { head: 2, tail: 2 }");
    let c = p.harno(&["run", &seq(1, 10)]);
    let n = raw_call(&c.out);
    assert_eq!(block(&c.out), ["1", "2", &format!("… 6 lines: harno log raw {n}"), "9", "10"]);
}

#[test]
fn head_alone_and_tail_alone() {
    let p = project("    seq: { head: 2 }\n    printf: { tail: 2 }");
    let c = p.harno(&["run", &seq(1, 5)]);
    assert_eq!(block(&c.out)[..2], ["1", "2"]);
    assert!(block(&c.out)[2].starts_with("… 3 lines:"), "{}", c.out);
    let c = p.harno(&["run", &five()]);
    assert!(block(&c.out)[0].starts_with("… 3 lines:"), "{}", c.out);
    assert_eq!(block(&c.out)[1..], ["four", "five"]);
}

#[test]
fn width_cuts_a_long_line_and_counts_its_characters() {
    let p = project("    printf: { width: 10 }");
    let long = "x".repeat(400);
    let c = p.harno(&["run", &format!("printf '%s\\nshort\\n' {long}")]);
    let n = p.calls().pop().unwrap()["n"].as_u64().unwrap();
    let out: Vec<&str> = c.out.lines().collect();
    assert_eq!(out[0], format!("{} … 400 chars", "x".repeat(10)), "{}", c.out);
    assert_eq!(out[1], "short");
    assert_eq!(out[2], format!("… 1 long line cut: harno log raw {n}"), "{}", c.out);
    assert!(p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.raw")).contains(&long), "the line is whole in the log");
}

#[test]
fn width_runs_after_the_fields_that_read_a_line() {
    // drop reads the line the command wrote, not the line the reader sees.
    let p = project("    printf: { width: 5, drop: [end$] }");
    let long = "a".repeat(200);
    let c = p.harno(&["run", &format!("printf '%send\\nkeep\\n' {long}")]);
    let out: Vec<&str> = c.out.lines().collect();
    assert_eq!(out[0], "keep", "the line drop read was the whole line: {}", c.out);
    assert!(out[1].starts_with("… 1 line:"), "{}", c.out);
}

#[test]
fn on_empty_speaks_when_nothing_is_left() {
    let p = project("    seq: { drop: ['.'], on_empty: 'seq: ok' }");
    let c = p.harno(&["run", &seq(1, 30)]);
    assert_eq!(block(&c.out)[0], "seq:", "{}", c.out);
    assert_eq!(c.out, "seq: ok\n", "the text stands alone: the log keeps what it replaced");
    assert!(!p.read(&format!(".harno/log/harno/{SESSION}/{:04}.raw", p.calls().pop().unwrap()["n"].as_u64().unwrap())).is_empty());
}

#[test]
fn a_command_that_wrote_nothing_speaks_through_on_empty() {
    let p = project("    printf: { on_empty: no match }");
    let c = p.harno(&["run", "printf %s"]);
    assert_eq!(c.out, "no match\n", "{}", c.out);
}

#[test]
fn nothing_left_without_on_empty_shows_the_raw_output() {
    let p = project("    printf: { drop: ['.'] }");
    let c = p.harno(&["run", &five()]);
    assert_eq!(block(&c.out), ["one", "two", "three", "four", "five"]);
    assert!(!c.out.contains("harno log raw"), "{}", c.out);
    assert!(!p.exists(".harno/log/raw"), "nothing to keep");
}

#[test]
fn colour_codes_and_carriage_returns_are_cleaned() {
    let p = project("    printf: { drop: ['^zzz'] }");
    let c = p.harno(&["run", "printf '\\033[1;31merror\\033[0m here\\n 10%%\\r 50%%\\r100%%\\n'"]);
    let out: Vec<&str> = c.out.lines().collect();
    assert_eq!(out[0..2], ["error here", "100%"], "{}", c.out);
    // The text changed, so what the command wrote is kept, but no line was removed.
    assert!(!c.out.contains("harno log raw"), "{}", c.out);
    let n = p.calls().pop().unwrap()["n"].as_u64().unwrap();
    assert!(p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.raw")).contains("\u{1b}[1;31merror"));
}

#[test]
fn fields_run_in_the_fixed_order_whatever_order_they_are_written_in() {
    // head is written first, but drop runs before it.
    let p = project("    printf: { head: 2, drop: ['^t'] }");
    let c = p.harno(&["run", &five()]);
    assert_eq!(block(&c.out)[..2], ["one", "four"], "{}", c.out);
}

#[test]
fn replace_rewrites_each_line_before_drop_reads_it() {
    // The pairs run in order, each on what the one before it left.
    let p = project("    printf: { drop: ['^three!$'], replace: [{ pattern: ' -+$', with: '' }, { pattern: '^(t\\w+)', with: '$1!' }] }");
    let c = p.harno(&["run", &five()]);
    let n = raw_call(&c.out);
    assert_eq!(block(&c.out), ["one", "two!", "four", "five", &format!("… 1 line: harno log raw {n}")]);
}

#[test]
fn replace_passes_down_from_star_and_on_fail_sets_its_own() {
    let p = project("    \"*\": { replace: [{ pattern: ' -+$', with: '' }] }\n    sh -c:\n      drop: ['^noise']\n      on_fail: { replace: [{ pattern: '^broke', with: B }] }");
    // seq: `*` alone; the padding goes and no line does, so there is no note.
    let c = p.harno(&["run", &seq(1, 3)]);
    assert_eq!(c.out, "1\n2\n3\n");
    // sh -c: its own drop, and the replace of `*`.
    let c = p.harno(&["run", &sh(&["noise", "a"], 0)]);
    assert!(c.out.starts_with("a\n… 1 line: harno log raw "), "{}", c.out);
    // A failure: the replace of on_fail takes the place of the one of `*`, so the padding stays.
    let c = p.harno(&["run", &sh(&["broke"], 1)]);
    assert!(c.out.starts_with(&format!("B {PAD}\n")), "{}", c.out);
}

#[test]
fn a_note_counts_the_lines_removed_and_the_lines_cut() {
    let p = project("    printf: { drop: ['^x'], width: 10 }");
    let long = "y".repeat(400);
    let c = p.harno(&["run", &format!("printf 'x1\\nx2\\n%s\\n' {long}")]);
    let n = raw_call(&c.out);
    assert_eq!(c.out.lines().last(), Some(format!("… 2 lines, 1 cut: harno log raw {n}").as_str()), "{}", c.out);
}

#[test]
fn a_filter_of_one_stream_passes_the_other_whole() {
    let both = format!("sh -c 'echo noise {PAD}; echo noise {PAD} >&2; echo kept; echo err >&2'");
    // stdout: its noise goes; stderr, noise and all, follows whole.
    let p = project("    sh -c: { reads: stdout, drop: ['^noise'] }");
    let c = p.harno(&["run", &both]);
    let n = raw_call(&c.out);
    assert_eq!(c.out, format!("kept\n… 1 line: harno log raw {n}\nnoise {PAD}\nerr\n"));
    // The log keeps what both streams wrote.
    assert_eq!(p.harno(&["log", "raw", &n]).ok().matches("noise").count(), 2);
    // stderr: stdout whole first, then what the filter left of stderr.
    let p = project("    sh -c: { reads: stderr, drop: ['^noise'] }");
    let c = p.harno(&["run", &both]);
    let n = raw_call(&c.out);
    assert_eq!(c.out, format!("noise {PAD}\nkept\nerr\n… 1 line: harno log raw {n}\n"));
    // on_fail names its own stream, and the command runs apart for it alone.
    let p = project("    sh -c: { drop: ['^noise'], on_fail: { reads: stdout, drop: ['^noise'] } }");
    let c = p.harno(&["run", &format!("{} ; exit 1'", both.trim_end_matches('\''))]);
    assert!(c.out.starts_with("kept\n… 1 line: harno log raw ") && c.out.contains(&format!("\nnoise {PAD}\nerr\n")), "{}", c.out);
}

#[test]
fn a_filter_test_gives_the_other_stream_as_stderr() {
    let p = project("    sh -c:\n      reads: stderr\n      drop: ['^noise']\n      tests: [{ input: \"out\\n\", stderr: \"noise\\nerr\\n\", expected: \"out\\nerr\\n\" }]");
    let c = p.harno(&["tool", "test"]);
    assert!(c.ok().contains("1 filter test"), "{}", c.out);
}

#[test]
fn a_filter_that_would_make_the_output_longer_shows_the_raw_output() {
    let p = project("    printf: { drop: ['^two'] }");
    // Removing one short line costs more than the note that says so.
    let c = p.harno(&["run", "printf 'one\\ntwo\\n'"]);
    assert_eq!(c.out, "one\ntwo\n");
    assert!(!p.exists(".harno/log/raw"));
}

#[test]
fn every_command_gets_star_and_its_own_filter_overrides_field_by_field() {
    let p = project(
        "    \"*\": { head: 1, drop: ['^x'] }
    seq: { head: 3 }
    printf: { drop: ['^o'] }",
    );
    // sh: only `*`.
    let c = p.harno(&["run", &sh(&["x", "a", "b"], 0)]);
    assert_eq!(block(&c.out)[0], "a", "{}", c.out);
    assert!(block(&c.out)[1].starts_with("… 2 lines:"), "{}", c.out);
    // seq: its head, `*`'s drop.
    let c = p.harno(&["run", &seq(1, 5)]);
    assert_eq!(block(&c.out)[..3], ["1", "2", "3"]);
    // printf: its drop replaces `*`'s list; `*`'s head still applies.
    let c = p.harno(&["run", &lines(&["one", "x", "y"])]);
    assert_eq!(block(&c.out)[0], "x", "{}", c.out);
    assert!(block(&c.out)[1].starts_with("… 2 lines:"), "{}", c.out);
}

#[test]
fn zero_or_false_switches_a_star_field_off() {
    let p = project("    \"*\": { head: 1, dedupe: true }\n    printf: { head: 0, dedupe: false }");
    let c = p.harno(&["run", &lines(&["a", "a", "b"])]);
    assert_eq!(block(&c.out), ["a", "a", "b"]);
}

#[test]
fn the_longest_matching_command_wins() {
    let p = project("    sh: { drop: ['^a'] }\n    sh -c: { drop: ['^b'] }");
    let c = p.harno(&["run", &sh(&["a", "b"], 0)]);
    assert_eq!(block(&c.out)[0], "a", "{}", c.out);
}

#[test]
fn on_fail_replaces_the_filter_when_the_command_fails() {
    let p = project(
        "    sh -c:
      keep: ['^summary']
      on_fail: { drop: ['^noise'] }",
    );
    let c = p.harno(&["run", &sh(&["noise", "failures", "summary"], 0)]);
    assert_eq!(block(&c.out)[0], "summary", "{}", c.out);
    let c = p.harno(&["run", &sh(&["noise", "failures", "broke", "summary"], 3)]);
    assert_eq!(c.code, 3, "the exit code is the command's");
    assert_eq!(block(&c.out)[..3], ["failures", "broke", "summary"], "{}", c.out);
    assert!(c.out.ends_with("\n\nFAIL exit 3 · 0.0s\n"), "{}", c.out);
}

#[test]
fn without_on_fail_the_filter_applies_to_failures_too() {
    let p = project("    sh -c: { drop: ['^noise'] }");
    let c = p.harno(&["run", &sh(&["noise", "broke"], 1)]);
    assert_eq!(block(&c.out)[0], "broke");
}

#[test]
fn on_fail_takes_what_it_leaves_out_from_star() {
    let p = project("    \"*\": { drop: ['^noise'] }\n    sh -c: { keep: ['^zzz'], on_fail: { head: 1 } }");
    let c = p.harno(&["run", &sh(&["noise", "a", "b"], 1)]);
    assert_eq!(block(&c.out)[0], "a", "{}", c.out);
    assert!(block(&c.out)[1].starts_with("… 2 lines:"), "{}", c.out);
}

#[test]
fn star_on_fail_is_for_commands_without_a_filter() {
    let p = project("    \"*\": { on_fail: { keep: ['^error'] } }");
    let c = p.harno(&["run", &sh(&["noise", "error"], 1)]);
    assert_eq!(block(&c.out)[0], "error");
    let c = p.harno(&["run", &sh(&["noise", "error"], 0)]);
    assert_eq!(block(&c.out), ["noise", "error"], "a pass is not filtered");
}

fn executable(p: &Project, rel: &str, text: &str) {
    p.write(rel, text);
    std::process::Command::new("chmod").args(["+x", &p.path(rel).to_string_lossy()]).status().unwrap();
}

#[test]
fn a_script_filters_with_the_exit_code_in_harno_exit() {
    let p = project("    sh -c: { script: ./f.sh }");
    executable(&p, "f.sh", "#!/bin/sh\necho \"exit $HARNO_EXIT, $(wc -l | tr -d ' ') lines\"\n");
    let c = p.harno(&["run", &sh(&["a", "b", "c"], 4)]);
    let out: Vec<&str> = c.out.lines().collect();
    assert_eq!(out[0], "exit 4, 3 lines", "{}", c.out);
    assert!(out[1].starts_with("… 2 lines: harno log raw "), "{}", c.out);
    assert_eq!(c.code, 4);
}

#[test]
fn a_script_with_arguments() {
    let p = project("    seq: { script: 'tail -n 1' }");
    let c = p.harno(&["run", &seq(1, 5)]);
    assert_eq!(block(&c.out)[0], "5", "{}", c.out);
}

#[test]
fn a_failing_or_silent_script_shows_the_raw_output_with_a_warning() {
    let p = project("    printf: { script: 'sh -c \"exit 9\"' }\n    seq: { script: 'sh -c \"cat >/dev/null\"' }\n    sh -c: { script: ./missing.sh }");
    let c = p.harno(&["run", &five()]);
    assert!(c.out.starts_with("warning: filter script sh -c \"exit 9\" failed (exit 9) — output unfiltered\n"), "{}", c.out);
    assert_eq!(block(&c.out), ["one", "two", "three", "four", "five"]);
    let c = p.harno(&["run", &seq(1, 3)]);
    assert!(c.out.starts_with("warning: filter script sh -c \"cat >/dev/null\" printed nothing — output unfiltered\n"), "{}", c.out);
    let c = p.harno(&["run", &sh(&["a"], 0)]);
    assert!(c.out.starts_with("warning: filter script ./missing.sh did not start"), "{}", c.out);
    assert_eq!(block(&c.out), ["a"]);
}

#[test]
fn a_script_that_takes_too_long_is_stopped() {
    let p = project("    seq: { script: 'sleep 30' }");
    p.write(".harno/config.yml", &p.read(".harno/config.yml").replace("timeout: 5", "timeout: 1"));
    let started = std::time::Instant::now();
    let c = p.harno(&["run", &seq(1, 2)]);
    assert!(started.elapsed().as_secs() < 10);
    assert!(c.out.contains("timed out after 1s — output unfiltered"), "{}", c.out);
    assert_eq!(block(&c.out), ["1", "2"]);
}

#[test]
fn a_filter_in_a_file() {
    let p = project("    printf: .harno/tools/mine.yml");
    p.write(".harno/tools/mine.yml", "keep: ['^two']\n");
    let c = p.harno(&["run", &five()]);
    assert_eq!(block(&c.out)[0], "two", "{}", c.out);
}

#[test]
fn a_tool_folder_runs_its_script_in_the_folder() {
    let p = project("    printf: .harno/tools/mine");
    p.write(".harno/tools/mine/tool.yml", "filter:\n  script: sh keep.sh\n  tests:\n    - input: \"one\\ntwo\\n\"\n      expected: \"two\\n\"\n");
    p.write(".harno/tools/mine/keep.sh", "grep two\n");
    let c = p.harno(&["run", &five()]);
    assert_eq!(block(&c.out)[0], "two", "{}", c.out);
    assert_eq!(p.harno(&["tool", "test"]).ok(), "1 filter test passed\n");
}

#[test]
fn a_broken_filter_refuses_the_config() {
    let cases = [
        ("    printf: { drop: ['('] }", "filter 'printf': bad regex \"(\""),
        ("    printf: { dorp: ['x'] }", "unknown field `dorp`"),
        ("    printf: { script: ./x.sh, head: 3 }", "a filter is either a script or fields, not both"),
        ("    printf: { on_fail: { on_fail: {} } }", "on_fail takes filter fields or a script"),
        ("    printf: { on_fail: { keep: ['['] } }", "bad regex \"[\""),
        ("    printf: .harno/tools/none.yml", "filter 'printf': cannot read .harno/tools/none.yml"),
        ("    printf: { head: -1 }", "filter 'printf'"),
        // Fields that were tried and left out.
        ("    printf: { keep_from: '^x' }", "unknown field `keep_from`"),
        ("    printf: { replace: [{ pattern: '(', with: '' }] }", "filter 'printf': bad regex \"(\""),
        ("    printf: { replace: [{ pattern: x }] }", "missing field `with`"),
        ("    printf: { reads: both }", "reads is stdout or stderr, not \"both\""),
        ("    printf: { stream: stdout }", "unknown field `stream`"),
    ];
    for (yaml, why) in cases {
        let p = project(yaml);
        let c = p.harno(&["run", "echo hi"]);
        assert!(c.refused().contains(why), "{yaml}: {}", c.out);
        // Every verb: the config is read before anything else.
        p.harno(&["tool", "ls"]).refused();
    }
}

#[test]
fn each_command_of_a_run_has_its_own_filter() {
    let p = project("    seq: { head: 1 }\n    printf: { drop: ['^noise'] }");
    for mode in ["--seq", "--par"] {
        let c = p.harno(&["run", mode, &seq(1, 3), &lines(&["noise", "kept"])]);
        let (a, b) = (nth(&c.out, 1), nth(&c.out, 2));
        assert!(a[0] == "1" && a[1].starts_with("… 2 lines:"), "{}", c.out);
        assert!(b[0] == "kept" && b[1].starts_with("… 1 line:"), "{}", c.out);
        // One call, one raw output: both notes name it, and it holds both commands.
        let call = p.calls().pop().unwrap();
        assert_eq!(raw_call(&a[1]), call["n"].to_string());
        assert_eq!(raw_call(&b[1]), call["n"].to_string());
        let n = call["n"].as_u64().unwrap();
        let raw = p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.raw"));
        assert!(raw.contains("\n3 ") && raw.contains("\nnoise "), "{raw}");
    }
}

#[test]
fn a_preset_s_commands_are_each_filtered_by_their_own_filter() {
    let p = project("    printf: { drop: ['^noise'] }\n    seq: { tail: 1 }\n    gate: { drop: ['.'] }");
    let c = p.harno(&["run", "gate"]);
    let (a, b) = (nth(&c.out, 1), nth(&c.out, 2));
    assert!(a[0] == "keep" && a[1].starts_with("… 1 line:"), "{}", c.out);
    assert!(b[0].starts_with("… 2 lines:") && b[1] == "3", "the preset's name matches no filter: {}", c.out);
}

#[test]
fn run_raw_is_filtered_too() {
    let p = project("    cat: { head: 1 }");
    p.write("a.txt", &format!("{PAD}\n{PAD}\n{PAD}\n"));
    let c = p.harno(&["run", "--raw", "cat a.txt", "--why", "read the file"]);
    assert!(c.out.contains("… 2 lines:"), "{}", c.out);
}

#[test]
fn the_log_keeps_what_the_agent_read_and_what_the_command_wrote() {
    let p = project("    seq: { head: 1 }");
    let c = p.harno(&["run", &seq(1, 100)]);
    let call = p.calls().pop().unwrap();
    let n = call["n"].as_u64().unwrap();
    assert_eq!(raw_call(&c.out), n.to_string());
    assert!(call["output"].as_u64() > call["filtered"].as_u64(), "{call}");
    assert!(call["lines"].as_u64() > call["kept"].as_u64(), "{call}");
    // `.out` is what the agent read, `.raw` what the command wrote.
    assert_eq!(p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.out")), c.out);
    assert!(p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.raw")).contains("\n100 "), "the whole output is kept");
    // Reading it back is a call like any other.
    let raw = p.harno(&["log", "raw", &n.to_string()]).ok().to_string();
    let call = p.calls().pop().unwrap();
    assert_eq!(call["verb"], "log");
    assert_eq!(call["args"], serde_json::json!(["raw", n.to_string()]));
    let m = call["n"].as_u64().unwrap();
    assert_eq!(p.read(&format!(".harno/log/harno/{SESSION}/{m:04}.out")), raw);
}

#[test]
fn an_unfiltered_call_keeps_no_raw_output() {
    let p = project("    seq: { head: 10 }");
    p.harno(&["run", &seq(1, 3)]).ok();
    let call = p.calls().pop().unwrap();
    let n = call["n"].as_u64().unwrap();
    assert_eq!(call["output"], call["filtered"]);
    assert!(!p.exists(&format!(".harno/log/harno/{SESSION}/{n:04}.raw")));
}

#[test]
fn log_raw_refuses_what_it_cannot_read() {
    let p = project("    seq: { head: 1 }");
    let c = p.harno(&["log", "raw", "9"]);
    assert!(c.refused().contains("no raw output for call 9"), "{}", c.out);
    let c = p.harno(&["log", "raw", "--session", "../../x", "1"]);
    assert!(c.refused().contains("no raw output"), "{}", c.out);
    let c = p.harno(&["log", "--something"]);
    assert!(c.refused().contains("log takes ls [<from> [<to>]], raw <n> [--session <id>], stats [<session>], analytics [<session>] or fails [<session>]"), "{}", c.out);
}

#[test]
fn a_long_raw_output_comes_with_the_size_warning() {
    let p = project("    seq: { head: 1 }");
    p.write(".harno/config.yml", &(p.read(".harno/config.yml") + "warn:\n  output: 50\n"));
    let n = raw_call(&p.harno(&["run", "seq 1 100"]).out);
    let out = p.harno(&["log", "raw", &n]).ok().to_string();
    assert!(out.contains(" — the whole output, on purpose\n"), "{out}");
    assert!(out.trim_end().ends_with("\n100"), "the raw output is whole: {out}");
}

#[test]
fn json_output_says_which_commands_a_filter_cut() {
    let p = project("    seq: { head: 1 }");
    let j = p.harno(&["run", &seq(1, 3), "--json"]).json();
    let cmd = &j["commands"][0];
    assert_eq!(cmd["filtered"], true);
    let n = p.calls().pop().unwrap()["n"].as_u64().unwrap();
    assert!(cmd["output"].as_str().unwrap().ends_with(&format!("harno log raw {n}\n")), "{j}");
    let j = p.harno(&["log", "raw", &n.to_string(), "--json"]).json();
    assert!(j["output"].as_str().unwrap().contains(&format!("1 {PAD}\n2 {PAD}\n3 {PAD}\n")), "{j}");
}

#[test]
fn check_runs_every_filter_s_tests() {
    let p = project(
        "    seq:
      keep: ['^3$']
      tests:
        - input: \"1\\n3\\n\"
          expected: \"3\\n\"
    sh -c:
      drop: ['^a']
      on_fail: { drop: ['^b'] }
      tests:
        - input: \"a\\nb\\n\"
          expected: \"b\\n\"
        - exit: 1
          input: \"a\\nb\\n\"
          expected: \"a\\n\"",
    );
    assert_eq!(p.harno(&["tool", "test"]).ok(), "3 filter tests passed\n");

    p.write(".harno/config.yml", &p.read(".harno/config.yml").replace("expected: \"3\\n\"", "expected: \"1\\n\""));
    let c = p.harno(&["tool", "test"]);
    assert_eq!(c.code, 1);
    assert_eq!(c.out, "filter seq: test 1 failed\n  expected:\n    1\n  got:\n    3\n\n1 of 3 filter tests failed\n");
    let j = p.harno(&["tool", "test", "--json"]).json();
    assert_eq!(j["failed"][0]["filter"], "seq");
    assert_eq!(j["ran"], 3);
}

#[test]
fn a_filter_test_checks_what_the_filter_makes_of_its_input() {
    // Nothing left and no on_empty: the input, as run would show it.
    let p = project("    seq:\n      drop: ['.']\n      tests: [{input: \"1\\n\", expected: \"1\\n\"}]");
    assert_eq!(p.harno(&["tool", "test"]).ok(), "1 filter test passed\n");
    // A short example is filtered even where a short real output would be
    // shown whole: the length rule is not the filter's.
    let p = project("    seq:\n      drop: ['^1$']\n      tests: [{input: \"1\\n2\\n\", expected: \"2\\n\"}]");
    assert_eq!(p.harno(&["tool", "test"]).ok(), "1 filter test passed\n");
}

#[test]
fn script_filters_are_tested_too() {
    let p = project("    seq:\n      script: 'sh -c \"tail -n 1\"'\n      tests: [{input: \"1\\n2\\n\", expected: \"2\\n\"}]");
    assert_eq!(p.harno(&["tool", "test"]).ok(), "1 filter test passed\n");
}

// Built-in filters

#[test]
fn tool_ls_lists_the_built_in_tools_and_the_project_s() {
    let p = Project::new();
    let out = p.harno(&["tool", "ls"]).ok().to_string();
    assert!(out.starts_with("built-in (harno tool add <name>)\n"), "{out}");
    for name in ["cargo-build-filter", "cargo-test-filter", "cargo-clippy-filter", "go-test-filter", "pytest-filter", "claude-code"] {
        assert!(out.lines().any(|l| l.trim_start().starts_with(name)), "{name}: {out}");
    }
    assert!(out.contains("filter of cargo test"), "{out}");
    assert!(out.contains("\n.harno/tools/\n  (none)\n"), "{out}");
    p.harno(&["tool", "add", "cargo-test-filter"]).ok();
    let out = p.harno(&["tool", "ls"]).ok().to_string();
    assert!(out.lines().any(|l| l.contains("cargo-test-filter") && l.ends_with("added")), "{out}");
    assert!(out.contains("\n.harno/tools/\n  cargo-test-filter\n"), "{out}");
}

#[test]
fn tool_add_copies_the_folder_and_writes_the_lines_its_manifest_declares() {
    let p = Project::new();
    let c = p.harno(&["tool", "add", "cargo-test-filter"]);
    assert_eq!(
        c.ok(),
        "added .harno/tools/cargo-test-filter/tool.yml\n.harno/config.yml: run.filters takes cargo test\n"
    );
    assert!(p.read(".harno/tools/cargo-test-filter/tool.yml").contains("command: cargo test"));
    // The line goes under run.filters, and what the config holds already stays.
    let cfg = p.read(".harno/config.yml");
    assert!(cfg.contains("    cargo test: .harno/tools/cargo-test-filter\n"), "{cfg}");
    assert!(cfg.contains("    \"*\": { dedupe: true, head: 100, tail: 100 }\n"), "{cfg}");
}

#[test]
fn an_exit_the_filter_calls_ok_is_no_failure() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [grep, echo]\n  filters:\n    grep: { on_empty: no match, ok: [1] }\n");
    let c = p.harno(&["run", "--seq", "grep x /dev/null", "echo after"]);
    let out = c.ok().to_string();
    assert!(out.contains("no match") && out.contains("after"), "{out}");
    // The code itself is said, it just does not break the call.
    assert!(out.contains("ok, exit 1") && out.contains("2/2 ran"), "{out}");
    // Without ok the same exit stops the chain, as every other failure does.
    p.config("version: 1\nrun:\n  allow: [grep, echo]\n  filters:\n    grep: { on_empty: no match }\n");
    let c = p.harno(&["run", "--seq", "grep x /dev/null", "echo after"]);
    assert!(c.out.contains("stopped: 1/2 ran") && !c.out.contains("after"), "{}", c.out);
    // A code that says nothing: 0 is no failure to begin with, and on_fail is
    // the filter of a call that broke, so ok would never be read there.
    p.config("version: 1\nrun:\n  allow: [grep]\n  filters:\n    grep: { ok: [0] }\n");
    assert!(p.harno(&["run", "grep x /dev/null"]).refused().contains("0 is never one of them"));
    p.config("version: 1\nrun:\n  allow: [grep]\n  filters:\n    grep: { on_fail: { ok: [1] } }\n");
    assert!(p.harno(&["run", "grep x /dev/null"]).refused().contains("not on_fail, tests, command or ok"));
}

#[test]
fn tool_add_refuses_what_it_cannot_do() {
    let p = Project::new();
    let c = p.harno(&["tool", "add", "nope"]);
    assert!(c.refused().contains("no built-in tool 'nope'"));
    assert_eq!(c.instead().as_deref(), Some("harno tool ls"));
    p.harno(&["tool", "add", "pytest-filter"]).ok();
    let c = p.harno(&["tool", "add", "pytest-filter"]);
    assert_eq!(c.instead().as_deref(), Some("harno tool update pytest-filter"), "{}", c.out);
}

#[test]
fn tool_remove_takes_out_the_folder_and_leaves_the_config() {
    let p = Project::new();
    p.harno(&["tool", "add", "cargo-build-filter"]).ok();
    p.config("version: 1\nrun:\n  filters:\n    cargo build: .harno/tools/cargo-build-filter\n");
    let c = p.harno(&["tool", "remove", "cargo-build-filter"]);
    assert!(c.refused().contains("run.filters in .harno/config.yml names .harno/tools/cargo-build-filter"), "{}", c.out);
    p.config("version: 1\n");
    assert_eq!(p.harno(&["tool", "remove", "cargo-build-filter"]).ok(), "removed .harno/tools/cargo-build-filter/\n");
    assert!(!p.exists(".harno/tools/cargo-build-filter"));
    let c = p.harno(&["tool", "remove", "cargo-build-filter"]);
    assert!(c.refused().contains("no tool 'cargo-build-filter' in this project"), "{}", c.out);
}

#[test]
fn tool_update_puts_back_the_binary_s_version_and_shows_the_change() {
    let p = Project::new();
    p.harno(&["tool", "add", "cargo-build-filter"]).ok();
    p.harno(&["tool", "add", "go-test-filter"]).ok();
    let original = p.read(".harno/tools/cargo-build-filter/tool.yml");
    p.write(".harno/tools/cargo-build-filter/tool.yml", &original.replace("on_empty: 'cargo build: ok'", "on_empty: 'fine'"));
    let out = p.harno(&["tool", "update"]).ok().to_string();
    assert!(out.contains("cargo-build-filter: updated .harno/tools/cargo-build-filter/tool.yml\n  -   on_empty: 'fine'\n  +   on_empty: 'cargo build: ok'\n"), "{out}");
    assert!(out.contains("go-test-filter: up to date\n"), "{out}");
    assert_eq!(p.read(".harno/tools/cargo-build-filter/tool.yml"), original);
    assert_eq!(p.harno(&["tool", "update", "cargo-build-filter"]).ok(), "cargo-build-filter: up to date\n");
    let c = p.harno(&["tool", "update", "pytest-filter"]);
    assert_eq!(c.instead().as_deref(), Some("harno tool add pytest-filter"), "{}", c.out);
}

#[test]
fn tool_update_keeps_the_config_block_of_the_project() {
    let p = Project::new();
    p.harno(&["tool", "add", "claude-code"]).ok();
    let manifest = ".harno/tools/claude-code/tool.yml";
    let original = p.read(manifest);
    // The settings of the project are its own; everything else is the binary.
    let mine = original.replace("strict: false", "strict: true").replace("# Claude Code:", "# Mine:");
    p.write(manifest, &mine);
    let out = p.harno(&["tool", "update", "claude-code"]).ok().to_string();
    let after = p.read(manifest);
    assert!(after.contains("strict: true"), "the config block of the project stays: {after}");
    assert!(after.starts_with("# Claude Code:"), "the rest comes back: {after}");
    assert!(out.contains("# Mine:"), "the change is shown: {out}");
    assert_eq!(p.harno(&["tool", "update", "claude-code"]).ok(), "claude-code: up to date\n");
}

#[test]
fn every_built_in_filter_passes_its_own_tests() {
    const ALL: [&str; 8] = ["cargo-build-filter", "cargo-test-filter", "cargo-clippy-filter", "go-test-filter", "pytest-filter", "rg-filter", "bat-filter", "cargo-nextest-filter"];
    // With the `*` init writes, and without any `*`.
    for bare in [false, true] {
        let p = Project::new();
        let mut filters = String::new();
        for name in ALL {
            p.harno(&["tool", "add", name]).ok();
            let yml = p.read(&format!(".harno/tools/{name}/tool.yml"));
            let command = yml.lines().find_map(|l| l.trim().strip_prefix("command: ")).unwrap().to_string();
            filters.push_str(&format!("    {command}: .harno/tools/{name}\n"));
        }
        let star = if bare { "" } else { "    \"*\": { dedupe: true, head: 100, tail: 100 }\n" };
        p.config(&format!("version: 1\nrun:\n  filters:\n{star}{filters}"));
        let out = p.harno(&["tool", "test"]).ok().to_string();
        assert!(out.ends_with(" filter tests passed\n") && !out.starts_with("0 "), "{out}");
    }
}

#[test]
fn the_agent_uses_tool_verbs_like_any_other_call() {
    let p = Project::new();
    p.harno(&["tool", "ls"]).ok();
    let call = p.calls().pop().unwrap();
    assert_eq!(call["verb"], "tool");
    assert_eq!(call["args"], serde_json::json!(["ls"]));
}

// Preset arguments

fn preset_project() -> Project {
    let p = Project::new();
    p.config(
        "version: 1
run:
  allow: [echo]
  presets:
    say: {seq: [\"printf '%s|' {args}\", \"echo done\"]}
    fixed: {seq: [\"echo fixed\"]}
",
    );
    p
}

#[test]
fn preset_arguments_take_the_place_of_args() {
    let p = preset_project();
    let c = p.harno(&["run", "say", "--", "a", "b c", "it's", "$HOME", "*"]);
    assert!(c.ok().contains("a|b c|it's|$HOME|*|"), "each argument one word, not expanded: {}", c.out);
    assert!(c.out.contains("--- 1: printf '%s|' 'a' 'b c' 'it'\\''s' '$HOME' '*'\n"), "{}", c.out);
}

#[test]
fn json_after_a_lone_preset_is_the_preset_s() {
    let p = preset_project();
    let c = p.harno(&["run", "say", "a", "--json"]);
    assert!(c.ok().contains("\na|--json|\n"), "the command gets it: {}", c.out);
    let c = p.harno(&["run", "--json", "say", "a"]);
    assert!(c.json()["commands"].is_array() || c.json().is_object(), "before the preset it is harno's: {}", c.out);
}

#[test]
fn a_lone_preset_takes_every_word_after_it() {
    let p = preset_project();
    // Without --: flags too are the preset's, and a second name is an argument, not a command.
    let c = p.harno(&["run", "say", "a", "--why", "fixed"]);
    assert!(c.ok().contains("--- 1: printf '%s|' 'a' '--why' 'fixed'\n"), "{}", c.out);
    assert!(c.out.contains("\na|--why|fixed|\n"), "{}", c.out);
    let c = p.harno(&["run", "--seq", "say", "fixed"]);
    assert!(c.ok().contains("--- 3: echo fixed\n"), "{}", c.out);
}

#[test]
fn a_preset_without_arguments_drops_args() {
    let p = preset_project();
    let c = p.harno(&["run", "say"]);
    assert!(c.ok().contains("--- 1: printf '%s|'\n"), "{}", c.out);
    let c = p.harno(&["run", "say", "--"]);
    assert!(c.ok().contains("--- 1: printf '%s|'\n"), "{}", c.out);
}

#[test]
fn arguments_need_a_preset_that_takes_them() {
    let p = preset_project();
    let c = p.harno(&["run", "fixed", "--", "x"]);
    assert!(c.refused().contains("preset fixed takes no arguments; it has no {args}"), "{}", c.out);
    let c = p.harno(&["run", "echo", "--", "x"]);
    assert!(c.refused().contains("arguments after -- go to a preset's {args}"), "{}", c.out);
    assert_eq!(c.instead().as_deref(), Some("harno run \"echo x\""));
}

#[test]
fn args_inside_a_word_takes_one_plain_argument() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  presets:\n    calls: {seq: [\"echo '{args}(' x{args}\"]}\n    shell: {seq: [\"sh -c 'echo {args} x'\"]}\n");
    let c = p.harno(&["run", "calls", "a::b_1.c/d-e"]);
    assert!(c.ok().contains("a::b_1.c/d-e( xa::b_1.c/d-e"), "{}", c.out);
    let c = p.harno(&["run", "shell", "ok"]);
    assert!(c.ok().contains("ok x"), "{}", c.out);
    // Inside quotes an argument would reach the shell as code: refused before anything runs.
    let c = p.harno(&["run", "shell", "a;touch${IFS}pwned"]);
    assert!(c.refused().contains("{args} inside a word takes only letters, digits and _ . : / -"), "{}", c.out);
    let c = p.harno(&["run", "calls", "a", "b"]);
    assert!(c.refused().contains("{args} inside a word takes one argument, not 2"), "{}", c.out);
    p.harno(&["run", "calls"]).refused();
}

#[test]
fn a_default_argument_gives_way_to_the_flag_the_caller_writes() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  presets:\n    log: {seq: [\"printf '%s|' {args}\"], defaults: {\"-n\": \"20\", \"--oneline\": \"\"}}\n    bare: {seq: [\"echo x\"], defaults: {\"-n\": \"1\"}}\n");
    let run = |args: &[&str]| p.harno(&[&["run", "log", "--"][..], args].concat()).ok().to_string();
    // The flags go in front of the caller's arguments; an empty value is a flag alone.
    assert!(run(&[]).contains("--oneline|-n|20|"), "{}", run(&[]));
    assert!(run(&["src/"]).contains("--oneline|-n|20|src/|"));
    // A flag the caller writes, in either form, takes the place of its default alone.
    assert!(run(&["-n", "5"]).contains("--oneline|-n|5|"));
    assert!(run(&["-n5"]).contains("--oneline|-n5|"));
    let both = run(&["--oneline", "-n", "1"]);
    assert!(both.contains("--oneline|-n|1|") && !both.contains("|20|"), "{both}");
    // The help shows them, and a preset with nowhere to put them is refused.
    let help = p.harno(&["help"]).ok().to_string();
    assert!(help.contains("{args}; defaults --oneline -n 20\n"), "{help}");
    let c = p.harno(&["run", "bare"]);
    assert!(c.refused().contains("preset bare has defaults but no {args} to put them in"), "{}", c.out);
}

#[test]
fn presets_in_seq_and_par_take_their_arguments_after_dashes() {
    let p = preset_project();
    let c = p.harno(&["run", "--seq", "say -- a 'b c'", "fixed"]);
    assert!(c.ok().contains("--- 1: printf '%s|' 'a' 'b c'\n"), "{}", c.out);
    assert!(c.out.contains("\na|b c|\n"), "{}", c.out);
    assert!(c.out.contains("--- 3: echo fixed\n"), "{}", c.out);
    let c = p.harno(&["run", "--seq", "say a"]);
    assert!(c.refused().contains("preset say takes its arguments after --"), "{}", c.out);
    let c = p.harno(&["run", "--par", "fixed -- x"]);
    assert!(c.refused().contains("preset fixed takes no arguments"), "{}", c.out);
}

#[test]
fn allowed_commands_take_arguments_without_a_preset() {
    let p = preset_project();
    assert_eq!(p.harno(&["run", "echo one two"]).ok(), "one two\n");
}

#[test]
fn tool_says_what_it_takes() {
    let p = Project::new();
    let wrong: [&[&str]; 5] = [&["tool"], &["tool", "frob"], &["tool", "ls", "extra"], &["tool", "add"], &["tool", "remove"]];
    for args in wrong {
        assert_eq!(
            p.harno(args).refused(),
            "refused: tool takes ls, add <name>…, remove <name>, update [<name>] or test\ninstead: harno tool ls\n",
            "{args:?}"
        );
    }
}

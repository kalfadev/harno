//! The log of the gate: one line per call, the texts of the call beside it.

use harno_test::{Project, SESSION};

/// A project with the preset `say`, whose calls stand for any call.
fn project() -> Project {
    let p = Project::new();
    p.config("version: 1\nrun:\n  presets:\n    say: { seq: [\"echo {args}\"] }\n");
    p
}

#[test]
fn one_line_per_call_in_a_file_of_its_own() {
    let p = project();
    p.harno(&["run", "say", "--", "a00001", "--depth", "2"]).ok();
    p.harno(&["nosuch", "--info"]).refused();
    let calls = p.calls();
    for (i, c) in calls.iter().enumerate() {
        assert_eq!(c["n"], i as u64 + 1);
        assert!(c["ts"].as_str().unwrap().ends_with('Z'));
    }

    // What the agent asked for, and what ran for it.
    assert_eq!(calls[0]["verb"], "run");
    assert_eq!(calls[0]["args"], serde_json::json!(["say", "--", "a00001", "--depth", "2"]));
    assert_eq!(calls[0]["cmd"], serde_json::json!(["echo 'a00001' '--depth' '2'"]));
    assert_eq!(calls[0]["exit"], serde_json::json!([0]));
    assert_eq!(calls[0]["cwd"], ".");
    assert!(calls[0]["ms"].is_u64());

    // A refused call carries its reason alone: nothing ran.
    assert_eq!(calls[1]["refused"], "unknown verb 'nosuch'");
    assert!(calls[1].get("cmd").is_none() && calls[1].get("exit").is_none(), "{}", calls[1]);

    // Key order: the shared skeleton first.
    let keys: Vec<String> = calls[0].as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["n", "ts", "ms", "cwd", "verb", "args", "cmd", "exit", "failed", "output", "filtered", "lines", "kept"]);
}

#[test]
fn each_call_keeps_what_the_caller_read_and_counts_it() {
    let p = project();
    let out = p.harno(&["run", "say", "--", "a00013"]).ok().to_string();
    assert!(out.contains("a00013\n"), "{out}");
    let call = p.calls().pop().unwrap();
    let n = call["n"].as_u64().unwrap();
    assert_eq!(p.read(&format!(".harno/log/harno/{SESSION}/{n:04}.out")), out);
    assert_eq!(call["filtered"].as_u64().unwrap() as usize, out.chars().count());
    assert_eq!(call["kept"].as_u64().unwrap() as usize, out.lines().count());
    // No filter cut anything, so what the commands wrote is what was read.
    assert_eq!(call["output"], call["filtered"]);
    assert_eq!(call["lines"], call["kept"]);
    assert!(!p.exists(&format!(".harno/log/harno/{SESSION}/{n:04}.raw")));
}

#[test]
fn a_call_logs_its_commands_and_every_exit_code() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo, false]\n");
    p.harno(&["run", "--par", "echo a", "false"]);
    let call = p.calls().pop().unwrap();
    assert_eq!(call["cmd"], serde_json::json!(["echo a", "false"]));
    assert_eq!(call["exit"], serde_json::json!([0, 1]));
    assert_eq!(call["failed"], serde_json::json!([false, true]));
}

#[test]
fn without_a_session_the_calls_of_a_day_go_to_one_file_and_warn_once() {
    let mut p = project();
    p.vars.remove("HARNO_SESSION");
    let c = p.harno(&["run", "say", "--", "a00013"]);
    assert!(c.out.starts_with("warning: no session hook — calls log to session manual-"), "{}", c.out);
    // Only the call that starts the log says so; every call of the day goes to the same file.
    let c = p.harno(&["run", "say", "--", "a00013"]);
    assert!(!c.out.contains("warning"), "{}", c.out);
    let dir = p.path(".harno/log/harno");
    let name = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with("manual-") && n.ends_with(".jsonl"))
        .unwrap_or_else(|| panic!("no manual session in {dir:?}"));
    let text = std::fs::read_to_string(dir.join(&name)).unwrap();
    assert_eq!(text.matches("\"verb\":\"run\"").count(), 2);
    // The warning is part of what went into the context, so it is in .out.
    let session = name.strip_suffix(".jsonl").unwrap();
    let out = p.read(&format!(".harno/log/harno/{session}/0001.out"));
    assert!(out.starts_with("warning: no session hook"), "{out}");
}

#[test]
fn the_log_directory_is_gitignored_when_it_is_born() {
    let p = Project::bare();
    p.write(".gitignore", "/target");
    p.harno(&["init"]).ok();
    p.harno(&["tool", "ls"]);
    assert_eq!(p.read(".gitignore"), "/target\n.harno/log/\n");
    p.harno(&["tool", "ls"]);
    assert_eq!(p.read(".gitignore"), "/target\n.harno/log/\n");
}

#[test]
fn log_ls_prints_the_calls_of_the_session_one_line_each() {
    let p = project();
    p.harno(&["run", "say", "--", "a00013"]).ok();
    p.harno(&["nosuch"]).refused();
    let out = p.harno(&["log", "ls"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n")), "{out}");
    // The session, the names of the columns, then a call a line.
    let rows: Vec<&str> = out.lines().skip(2).collect();
    assert_eq!(rows.len(), 2, "{out}");
    assert!(rows[0].contains("  ok  ") && rows[0].ends_with("run say -- a00013"), "{out}");
    assert!(rows[1].contains("  refused  ") && rows[1].contains("unknown verb"), "{out}");
    // A range takes the calls it names, and --json carries the lines themselves.
    let one = p.harno(&["log", "ls", "2", "2"]).ok().to_string();
    assert_eq!(one.lines().count(), 3, "{one}");
    let v = p.harno(&["log", "ls", "--json"]).json();
    assert_eq!(v["calls"][0]["verb"], "run");
    assert_eq!(v["session"], SESSION);
    let c = p.harno(&["log", "ls", "later"]);
    assert!(c.refused().contains("log ls takes <from> [<to>], not later"), "{}", c.out);
}

#[test]
fn a_call_that_runs_is_in_log_ls_and_what_it_wrote_in_log_raw() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  timeout: 20\n  allow: [sh scripts/turn.sh]\n");
    p.turns();
    let (raw, run) = (format!(".harno/log/harno/{SESSION}/0001.raw"), format!(".harno/log/harno/{SESSION}/0001.run"));
    std::thread::scope(|s| {
        let server = s.spawn(|| p.harno(&["run", "sh scripts/turn.sh +ready @stop"]));
        p.wait_for(&raw, "ready\n");
        // No line yet: log ls has it running since it began, log raw what it wrote so far.
        let ls = p.harno(&["log", "ls"]).ok().to_string();
        let row = ls.lines().find(|l| l.starts_with("   1  ")).unwrap_or_else(|| panic!("{ls}"));
        assert!(row.contains(&format!("  {:>6}  {:<15}  running   run ", "", "")), "{ls}");
        let v = p.harno(&["log", "ls", "--json"]).json();
        let began = &v["calls"][0];
        assert_eq!(began["n"], 1, "{v}");
        assert_eq!(began["state"], "running", "{v}");
        assert_eq!(began["verb"], "run", "{v}");
        assert_eq!(began["cwd"], ".", "{v}");
        assert_eq!(began["args"], serde_json::json!(["sh scripts/turn.sh +ready @stop"]), "{v}");
        assert_eq!(p.harno(&["log", "raw", "1"]).ok(), "ready\n");
        assert!(!server.is_finished());
        p.write("stop", "");
        assert!(server.join().unwrap().ok().starts_with("ready\n"));
    });
    // It ended: its line is written, and neither file stays, since the caller read all of it.
    assert!(!p.exists(&raw) && !p.exists(&run));
    let call = p.calls().into_iter().find(|c| c["n"] == 1).unwrap();
    assert_eq!(call["exit"], serde_json::json!([0]));
    let ls = p.harno(&["log", "ls"]).ok().to_string();
    assert!(ls.lines().any(|l| l.starts_with("   1  ") && l.contains("  ok  ")), "{ls}");
    // A refused call of run leaves neither behind.
    p.harno(&["run", "nosuch"]).refused();
    assert!(!p.exists(&format!(".harno/log/harno/{SESSION}/0006.raw")) && !p.exists(&format!(".harno/log/harno/{SESSION}/0006.run")));
}

#[test]
fn a_call_that_died_before_its_line_is_listed_as_died() {
    let p = project();
    p.harno(&["run", "say", "--", "x"]).ok();
    let began = |n: u32| format!(r#"{{"n":{n},"ts":"2026-10-01T10:00:00.000Z","verb":"run","args":["page-demo"],"cwd":"."}}"#);
    // Nothing holds its lock: the process ended before it wrote its line.
    p.write(&format!(".harno/log/harno/{SESSION}/0009.run"), &began(9));
    // One left beside a call that has its line counts for nothing.
    p.write(&format!(".harno/log/harno/{SESSION}/0001.run"), &began(1));
    let out = p.harno(&["log", "ls"]).ok().to_string();
    let rows: Vec<&str> = out.lines().skip(2).collect();
    assert_eq!(rows.len(), 2, "{out}");
    assert!(rows[0].contains("  ok  "), "{out}");
    assert_eq!(rows[1], format!("{:>4}  {:<8}  {:>6}  {:<15}  {:<8}  {}", 9, "10:00:00", "", "", "died", "run page-demo"));
    let v = p.harno(&["log", "ls", "--json"]).json();
    let died: Vec<&serde_json::Value> = v["calls"].as_array().unwrap().iter().filter(|c| c.get("state").is_some()).collect();
    assert_eq!(died.len(), 1, "{v}");
    assert_eq!((died[0]["n"].as_u64(), died[0]["state"].as_str()), (Some(9), Some("died")), "{v}");
}

#[test]
fn stats_count_the_calls_the_characters_and_the_failures() {
    let p = project();
    p.harno(&["run", "say", "--", "a00013"]).ok();
    p.harno(&["nosuch"]).refused();
    let out = p.harno(&["log", "stats"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n  started   ")), "{out}");
    assert!(out.contains("\n  calls     2, 1 failed, 0 raw reads\n"), "{out}");
    assert!(out.contains("\n  returned "), "{out}");
}

#[test]
fn an_exit_its_filter_calls_ok_is_logged_as_no_failure_and_counted_as_none() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [\"false\"]\n  filters:\n    \"false\": { ok: [1] }\n");
    p.harno(&["run", "false"]).ok();
    let call = p.calls().pop().unwrap();
    assert_eq!(call["exit"], serde_json::json!([1]));
    assert_eq!(call["failed"], serde_json::json!([false]));
    // ls, stats and fails read what the call judged, whatever the config says later.
    p.config("version: 1\nrun:\n  allow: [\"false\"]\n");
    let ls = p.harno(&["log", "ls"]).ok().to_string();
    assert!(ls.contains("  ok  ") && !ls.contains("exit 1"), "{ls}");
    let v = p.harno(&["log", "stats", "--json"]).json();
    assert_eq!(v["sessions"][0]["failed"], 0, "{v}");
    let v = p.harno(&["log", "fails", "--json"]).json();
    assert_eq!(v["failed"], 0, "{v}");
    // A line written before the field has its exit codes alone: one that is not 0 failed.
    p.write(".harno/log/harno/old.jsonl", "{\"n\":1,\"ts\":\"2026-09-01T00:00:00.000Z\",\"verb\":\"run\",\"args\":[\"false\"],\"cmd\":[\"false\"],\"exit\":[1]}\n");
    let v = p.harno(&["log", "stats", "old", "--json"]).json();
    assert_eq!(v["sessions"][0]["failed"], 1, "{v}");
    let v = p.harno(&["log", "fails", "old", "--json"]).json();
    assert_eq!(v["failed"], 1, "{v}");
}

#[test]
fn stats_count_a_raw_read() {
    let p = project();
    p.harno(&["log", "raw", "7"]).refused();
    let out = p.harno(&["log", "stats"]).ok().to_string();
    assert!(out.contains(", 1 raw read\n"), "{out}");
}

#[test]
fn stats_are_the_newest_session_alone_and_all_is_every_one() {
    let mut p = project();
    p.harno(&["run", "say", "--", "a00013"]).ok();
    p.vars.insert("HARNO_SESSION".into(), "0196aaaa-0000-7000-8000-000000000002".into());
    p.harno(&["run", "say", "--", "a00014"]).ok();
    let out = p.harno(&["log", "stats"]).ok().to_string();
    assert_eq!(out.matches("  calls ").count(), 1, "{out}");
    assert!(out.contains("\n1 more session — harno log stats --all\n"), "{out}");
    // Every session is a row under the names of the columns, and the whole log one more.
    let every = p.harno(&["log", "stats", "--all"]).ok().to_string();
    assert_eq!(every.lines().count(), 4, "{every}");
    assert!(every.starts_with("session ") && every.lines().last().is_some_and(|l| l.starts_with("total ")), "{every}");
    assert!(!every.contains("more session"), "{every}");
    let v = p.harno(&["log", "stats", "--all", "--json"]).json();
    assert_eq!(v["total"]["calls"], 4, "{v}");
}

#[test]
fn without_a_session_it_is_the_one_asking_and_all_puts_the_one_used_last_first() {
    let mut p = project();
    p.harno(&["run", "say", "--", "a00013"]).ok();
    p.vars.insert("HARNO_SESSION".into(), "0196aaaa-0000-7000-8000-000000000002".into());
    p.harno(&["run", "say", "--", "a00014"]).ok();
    p.vars.insert("HARNO_SESSION".into(), SESSION.into());
    // The first session asks: it is its own, though the other started later.
    let out = p.harno(&["log", "stats"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n")), "{out}");
    assert!(out.ends_with("\n1 more session — harno log stats --all\n"), "{out}");
    let v = p.harno(&["log", "analytics", "--json"]).json();
    assert_eq!(v["sessions"], serde_json::json!([SESSION]), "{v}");
    // --all: the session used last comes first, not the one started last.
    let v = p.harno(&["log", "stats", "--all", "--json"]).json();
    assert_eq!(v["sessions"][0]["session"], SESSION, "{v}");
    let v = p.harno(&["log", "analytics", "--all", "--json"]).json();
    assert_eq!(v["sessions"][0], SESSION, "{v}");
    // A session with no call yet gets the one used last.
    p.vars.insert("HARNO_SESSION".into(), "0196aaaa-0000-7000-8000-000000000003".into());
    let out = p.harno(&["log", "stats"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n")), "{out}");
}

#[test]
fn stats_json_carries_the_same_numbers_and_an_unknown_session_is_refused() {
    let p = project();
    p.harno(&["run", "say", "--", "a00013"]).ok();
    let v = p.harno(&["log", "stats", "--json"]).json();
    assert_eq!(v["sessions"][0]["session"], SESSION);
    assert_eq!(v["sessions"][0]["calls"], 1);
    assert_eq!(v["sessions"][0]["failed"], 0);
    let c = p.harno(&["log", "stats", "nosuch"]);
    assert!(c.refused().contains("no session nosuch"), "{}", c.out);
}

#[test]
fn analytics_ranks_the_commands_without_a_filter_of_their_own() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo, printf]\n  filters:\n    printf: { head: 5 }\n");
    p.harno(&["run", "echo one"]).ok();
    p.harno(&["run", "--seq", "echo two three", "printf four"]).ok();
    let out = p.harno(&["log", "analytics"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n")), "{out}");
    let (without, with) = out.split_once("with a filter of their own").unwrap_or((&out, ""));
    assert!(without.contains("%  echo\n") && !without.contains("printf") && with.contains("%  printf\n"), "{out}");
    // A lone command is its whole output; in a chain, its block without the header and the footer.
    let v = p.harno(&["log", "analytics", "--json"]).json();
    assert_eq!(v["commands"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(v["commands"][0]["command"], "echo");
    assert_eq!(v["commands"][0]["runs"], 2);
    assert_eq!(v["commands"][0]["chars"], 14);
    let c = p.harno(&["log", "analytics", "nosuch"]);
    assert!(c.refused().contains("no session nosuch"), "{}", c.out);
}

#[test]
fn analytics_names_the_preset_that_ran_a_command_and_all_is_every_session() {
    let mut p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo, printf]\n  presets:\n    say: { seq: [\"echo {args}\"] }\n    shout: { seq: [\"echo {args}\"] }\n    two: { seq: [\"say -- a\", \"printf b\"] }\n    x: { seq: [\"sh x.sh\"] }\n");
    p.write("x.sh", "echo hi\n");
    p.harno(&["run", "shout", "--", "hi"]).ok();
    p.harno(&["run", "two"]).ok();
    p.harno(&["run", "echo hey"]).ok();
    p.harno(&["run", "--par", "x", "echo you"]).ok();
    let out = p.harno(&["log", "analytics"]).ok().to_string();
    // Of two presets that run the same command, the one the call named; a
    // preset inside another is the inner one; an item of --par names its own.
    for name in ["preset shout", "preset say", "preset two", "preset x"] {
        assert!(out.contains(&format!("%  {name}\n")), "{name}: {out}");
    }
    // A command typed by hand is itself, in a chain or alone.
    assert!(out.contains("%  echo\n"), "{out}");
    let v = p.harno(&["log", "analytics", "--json"]).json();
    let rows = v["commands"].as_array().cloned().unwrap_or_default();
    assert!(rows.iter().any(|r| r["preset"] == "shout" && r["runs"] == 1 && r["chars"] == 3), "{v}");
    assert!(rows.iter().any(|r| r["command"] == "echo" && r["runs"] == 2), "{v}");
    // A script typed by hand is its interpreter and its path, whole however long.
    let long = format!("{}.sh", "d".repeat(80));
    p.write(&long, "echo hi\n");
    p.harno(&["run", "--raw", &format!("sh {long}"), "--why", "a long name"]).ok();
    let out = p.harno(&["log", "analytics"]).ok().to_string();
    assert!(out.contains(&format!("%  sh {long}\n")), "{out}");
    p.vars.insert("HARNO_SESSION".into(), "0196aaaa-0000-7000-8000-000000000002".into());
    p.harno(&["run", "echo hi"]).ok();
    let newest = p.harno(&["log", "analytics"]).ok().to_string();
    assert!(newest.contains("%  echo\n") && !newest.contains("preset x"), "{newest}");
    assert!(newest.ends_with("\n1 more session — harno log analytics --all\n"), "{newest}");
    let every = p.harno(&["log", "analytics", "--all"]).ok().to_string();
    assert!(every.starts_with("2 sessions\n") && every.contains("preset x") && every.contains("%  echo\n"), "{every}");
    let v = p.harno(&["log", "analytics", "--all", "--json"]).json();
    assert_eq!(v["sessions"].as_array().map(Vec::len), Some(2), "{v}");
    assert_eq!(v["more"], 0);
    let c = p.harno(&["log", "analytics", "--x"]);
    assert!(c.refused().contains("log analytics takes a session or --all, not --x"), "{}", c.out);
}

#[test]
fn analytics_says_when_there_is_nothing_to_rank() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo]\n  filters:\n    echo: { head: 5 }\n");
    assert_eq!(p.harno(&["log", "analytics"]).ok(), "no sessions in .harno/log\n");
    p.harno(&["run", "echo hi"]).ok();
    let out = p.harno(&["log", "analytics"]).ok().to_string();
    let top = format!("session {SESSION}\n\nwithout a filter of their own: none\n\nwith a filter of their own");
    assert!(out.starts_with(&top) && out.ends_with("100%  echo\n"), "{out}");
}

#[test]
fn analytics_lists_the_commands_with_a_filter_and_what_it_kept() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [seq]\n  presets:\n    count: { seq: [\"seq 1 {args}\"] }\n  filters:\n    seq: { head: 1 }\n");
    let shown = p.harno(&["run", "count", "--", "1000"]).ok().chars().count() as u64;
    // Cutting ten short lines would cost more than the note: the raw output is shown, all of it kept.
    p.harno(&["run", "seq 1 10"]).ok();
    let v = p.harno(&["log", "analytics", "--json"]).json();
    assert_eq!(v["commands"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(v["filtered"][0]["preset"], "count", "{v}");
    assert_eq!(v["filtered"][0]["chars"], shown, "{v}");
    assert_eq!(v["filtered"][0]["raw"], 3893, "{v}");
    assert_eq!(v["filtered"][1]["command"], "seq", "{v}");
    assert_eq!(v["filtered"][1]["raw"], v["filtered"][1]["chars"], "{v}");
    let out = p.harno(&["log", "analytics"]).ok().to_string();
    // Sizes read the way a reader takes them in: 3893 characters are 3.9k.
    let pct = shown * 100 / 3893;
    assert!(out.contains(&format!("     3.9k  {shown:>8}  {:>4}  preset count\n", format!("{pct}%"))), "{out}");
    assert!(out.contains("100%  seq\n") && out.contains("   runs   written     shown  kept  name\n"), "{out}");
}

#[test]
fn fails_count_the_commands_by_name_and_exit_and_the_refusals_by_reason() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [sh, \"false\"]\n  filters:\n    sh: { on_fail: { head: 1 } }\n    \"false\": { ok: [1] }\n");
    p.harno(&["run", "sh -c \"echo ok\""]).ok();
    // An exit code the ok of its filter allows is no failure.
    p.harno(&["run", "false"]).ok();
    assert_eq!(p.harno(&["run", "sh -c \"echo cut; seq 1 50; exit 5\""]).code, 5);
    assert_eq!(p.harno(&["run", "sh -c \"echo first; exit 3\""]).code, 3);
    assert_eq!(p.harno(&["run", "sh -c \"echo second; exit 3\""]).code, 3);
    assert_eq!(p.harno(&["run", "--par", "sh -c \"echo fine\"", "sh -c \"echo third; exit 4\""]).code, 4);
    p.harno(&["run", "rm x"]).refused();
    p.harno(&["run", "mv x y"]).refused();
    let out = p.harno(&["log", "fails"]).ok().to_string();
    assert!(out.starts_with(&format!("session {SESSION}\n6 of 8 calls failed\n")), "{out}");
    assert!(!out.contains("false"), "{out}");
    // The latest of a name and an exit code gives its last line, what harno adds left out.
    assert!(out.contains("\n      2     3  sh  second\n      1     4  sh  third\n      1     5  sh  cut\n"), "{out}");
    // Two refusals for one reason are one line, the latest whole after it.
    assert!(out.contains("\n      2  '…' is not in run.allow — latest: 'mv' is not in run.allow\n"), "{out}");
    let v = p.harno(&["log", "fails", "--json"]).json();
    assert_eq!(v["failed"], 6, "{v}");
    assert_eq!(v["commands"][1]["exit"], 4, "{v}");
    assert_eq!(v["refused"][0]["reason"], "'mv' is not in run.allow", "{v}");
    assert_eq!(v["refused"][0]["shape"], "'…' is not in run.allow", "{v}");
    assert!(p.harno(&["log", "fails", "nosuch"]).refused().contains("no session nosuch"));
}

#[test]
fn shown_per_command_cuts_a_call_at_its_headers() {
    use harno::run::shown_per_command as per;
    // A lone command is the whole text.
    assert_eq!(per("one\n", 1), [4]);
    // Several: each block without its header; the footer is no command's.
    assert_eq!(per("--- 1: echo a --- ok · 0.0s\na\n--- 2: echo bb --- ok · 0.0s\nbb\n\n2/2 ran\n", 2), [2, 3]);
    // The text of each block, cut the same way.
    assert_eq!(harno::run::text_per_command("--- 1: echo a --- ok · 0.0s\na\n--- 2: echo bb --- ok · 0.0s\nbb\n\n2/2 ran\n", 2), ["a\n", "bb\n"]);
    // A chain that stopped: the header stays, and so does the footer.
    assert_eq!(per("--- 1: sh -c x --- FAIL exit 1 · 0.0s\nx\n\nstopped: 1/2 ran — 1: exit 1\n", 1), [2]);
    // A header number no command has is a line of output.
    assert_eq!(per("--- 1: a --- ok · 0.0s\n--- 7: x\n--- 2: b --- ok · 0.0s\nb\n\n2/2 ran\n", 2), [9, 2]);
    // A lone command whose output holds the headers of another call.
    assert_eq!(per("--- 2: x --- ok\nout\n", 1), [20]);
    // Headers the call cannot map give nothing rather than a wrong count.
    assert_eq!(per("x\n", 2), [0, 0]);
    // An older log wrote a command of several lines whole in its header, up to the status.
    assert_eq!(per("--- 1: python3 x.py '@@ plan\nline of the plan' --- ok · 0.1s\ndone\n--- 2: b --- FAIL exit 1 · 2.0s\nb\n\n1 ran, 1 failed — 2: exit 1\n", 2), [5, 2]);
    // A block that passed at once is its command alone, one that took a while ends with its seconds.
    assert_eq!(per("--- 1: echo a\na\n--- 2: sleep 2 · 2.0s\n--- 3: echo bb\nbb\n\n3/3 ran\n", 3), [2, 0, 3]);
}

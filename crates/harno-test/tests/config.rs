//! `harno init` and `.harno/config.yml`.

use harno_test::Project;


#[test]
fn init_writes_every_field_with_its_default_and_nothing_else() {
    let p = Project::bare();
    let c = p.harno(&["init"]);
    assert_eq!(c.out, "created .harno/config.yml\n");
    assert_eq!(
        p.read(".harno/config.yml"),
        "version: 1\nrun:\n  timeout: 900\n  allow: []\n  presets: {}\n  filters:\n    \"*\": { dedupe: true, head: 100, tail: 100 }\n  sandbox:\n    enabled: false\n    allow:\n      # outside the project: what a command may write, and what it may read\n      # ([\"*\"] is every path)\n      write: []\n      read: [\"*\"]\n    deny:\n      # paths closed to reads; a path here beats every allow\n      read: []\ndeny:\n  calls: []\nwarn:\n  output: 20000\n"
    );
    let entries: Vec<_> = std::fs::read_dir(p.root()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(entries, [std::ffi::OsString::from(".harno")]);
    let inside: Vec<_> = std::fs::read_dir(p.path(".harno")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(inside, [std::ffi::OsString::from("config.yml")]);
}

#[test]
fn init_does_not_overwrite() {
    let p = Project::new();
    p.config("version: 1\nwarn:\n  output: 10\n");
    p.harno(&["init"]).refused();
    assert_eq!(p.read(".harno/config.yml"), "version: 1\nwarn:\n  output: 10\n");
}

#[test]
fn without_a_config_every_verb_points_to_init() {
    let p = Project::bare();
    let c = p.harno(&["tool", "ls"]);
    c.refused();
    assert_eq!(c.instead().unwrap(), "harno init");
}

#[test]
fn an_unknown_version_stops_harno() {
    let p = Project::new();
    p.config("version: 2\n");
    let c = p.harno(&["tool", "ls"]);
    assert_eq!(c.out, "refused: .harno/config.yml has version 2; this harno reads version 1\n");
    assert_ne!(c.code, 0);
}

#[test]
fn an_unknown_field_is_refused_rather_than_ignored() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  alow: [cargo]\n");
    assert!(p.harno(&["tool", "ls"]).refused().contains("alow"));
    // The fields of the markdown command live in its own tool.yml now.
    p.config("version: 1\nroots: [docs/]\n");
    assert!(p.harno(&["tool", "ls"]).refused().contains("roots"));
}

#[test]
fn the_project_is_found_from_a_subdirectory_and_commands_run_at_its_root() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [ls]\n");
    std::fs::create_dir_all(p.path("docs/deep")).unwrap();
    let c = p.harno_in(&p.path("docs/deep"), &["run", "ls"]);
    assert!(c.ok().contains("docs\n"), "{}", c.out);
}

#[test]
fn help_lists_the_verbs() {
    let p = Project::bare();
    let out = p.harno(&["help"]).ok().to_string();
    for verb in ["init", "run", "log", "tool"] {
        assert!(out.contains(&format!("harno {verb}")), "{verb}");
    }
}

#[test]
fn an_unknown_verb_or_option_is_refused() {
    let p = Project::new();
    assert!(p.harno(&["frobnicate"]).refused().contains("unknown verb 'frobnicate'"));
    // A command is no verb: it runs through a preset that names its program.
    p.tool("echo", "# A preset calls it: echo {args}\n");
    assert!(p.harno(&["echo", "hi"]).refused().contains("unknown verb 'echo'"));
    // The page for the human is the mdmap command's: harno run mdmap serve.
    assert!(p.harno(&["serve"]).refused().contains("unknown verb 'serve'"));
    assert!(p.harno(&["run", "--deep"]).refused().contains("unknown option --deep for run"));
}

#[test]
fn a_tool_is_added_with_the_presets_and_the_filter_line_its_manifest_declares() {
    let p = Project::new();
    assert!(p.harno(&["tool", "ls"]).ok().contains("claude-code"));
    let out = p.harno(&["tool", "add", "claude-code"]).ok().to_string();
    assert!(out.contains("added .harno/tools/claude-code/hook.py\n"), "{out}");
    assert!(out.contains("added .harno/tools/claude-code/tool.yml\n"), "{out}");
    assert!(out.contains(".harno/config.yml: run.presets takes claude-code, session, transcript\n"), "{out}");
    let cfg = p.read(".harno/config.yml");
    assert!(cfg.contains("    claude-code: { seq: [\"python3 .harno/tools/claude-code/install.py {args}\"] }\n"), "{cfg}");
    // The hooks of the harness are a call of the tool, never of add.
    assert!(!p.exists(".claude/settings.json"), "add writes the config and nothing else");
    assert!(p.harno(&["tool", "add", "claude-code"]).refused().contains(".harno/tools/claude-code/ already exists"));
    assert!(p.harno(&["tool", "update", "claude-code"]).ok().contains("claude-code: up to date"));
    // An update brings the files back and keeps the config block of the project.
    let yml = p.read(".harno/tools/claude-code/tool.yml");
    p.write(".harno/tools/claude-code/tool.yml", &yml.replace("strict: true", "strict: false").replace("# Claude Code:", "# Claude  Code:"));
    let out = p.harno(&["tool", "update", "claude-code"]).ok().to_string();
    assert!(out.contains("  - # Claude  Code:") && !out.contains("strict"), "{out}");
    assert!(p.read(".harno/tools/claude-code/tool.yml").contains("strict: false"));
    assert!(!p.exists(".claude/settings.json"), "an update wires nothing either");
    let out = p.harno(&["tool", "remove", "claude-code"]).ok().to_string();
    assert!(out.ends_with("removed .harno/tools/claude-code/\n"), "{out}");
    assert!(p.harno(&["tool", "add", "cursor"]).refused().contains("no built-in tool 'cursor'"));
}

#[test]
fn tool_add_takes_several_names_and_checks_them_all_first() {
    let p = Project::new();
    let c = p.harno(&["tool", "add", "cargo-build-filter", "cursor"]);
    assert!(c.refused().contains("no built-in tool 'cursor'"), "{}", c.out);
    assert!(!p.exists(".harno/tools/cargo-build-filter"), "a wrong name adds nothing");
    let out = p.harno(&["tool", "add", "cargo-build-filter", "pytest-filter"]).ok().to_string();
    assert!(out.contains("run.filters takes cargo build\n") && out.contains("added .harno/tools/pytest-filter/tool.yml\n"), "{out}");
    assert!(p.harno(&["tool", "add", "sym", "pytest-filter"]).refused().contains(".harno/tools/pytest-filter/ already exists"));
    assert!(!p.exists(".harno/tools/sym"), "a tool the project has adds nothing either");
    assert!(p.harno(&["tool", "add", "sym", "sym"]).refused().contains("tool add names sym twice"));
}

#[test]
fn a_tool_that_carries_a_filter_writes_the_run_filters_line() {
    let p = Project::new();
    let out = p.harno(&["tool", "add", "cargo-build-filter"]).ok().to_string();
    assert!(out.contains(".harno/config.yml: run.filters takes cargo build\n"), "{out}");
    assert!(p.read(".harno/config.yml").contains("    cargo build: .harno/tools/cargo-build-filter\n"));
    assert!(p.harno(&["tool", "test"]).ok().contains("filter test"));
}

#[test]
fn a_preset_the_config_already_has_is_left_as_it_is() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  presets:\n    session: { seq: [\"echo mine\"] }\n");
    let out = p.harno(&["tool", "add", "claude-code"]).ok().to_string();
    assert!(out.contains("run.presets takes claude-code, transcript\n"), "{out}");
    assert!(out.contains("run.presets has session already, left as it is\n"), "{out}");
    assert!(p.harno(&["run", "session"]).ok().contains("mine"));
}

#[test]
fn deny_calls_is_the_humans_to_change() {
    let p = Project::new();
    // `*` stands for any one argument.
    p.config("version: 1\nrun:\n  allow: [echo]\ndeny:\n  calls: [tool add *, run --raw]\n");
    let c = p.harno(&["tool", "add", "sym"]);
    assert!(c.refused().starts_with("refused: 'tool add *' is in deny.calls"), "{}", c.out);
    assert_eq!(c.instead(), None);
    p.harno(&["tool", "ls"]).ok();
    p.harno(&["run", "--raw", "echo x", "--why", "test"]).refused();
    // Without the denied flag the call may be allowed, and the refusal says how.
    p.config("version: 1\nrun:\n  allow: [echo]\ndeny:\n  calls: [harno run --seq]\n");
    let c = p.harno(&["run", "--seq", "echo hi"]);
    assert_eq!(c.out, "refused: 'harno run --seq' is in deny.calls in .harno/config.yml\ninstead: harno run \"echo hi\"\n");
    // Emptied, every call works again.
    p.config("version: 1\nrun:\n  allow: [echo]\ndeny:\n  calls: []\n");
    assert_eq!(p.harno(&["run", "--seq", "echo hi"]).ok(), "hi\n");
}

#[test]
fn a_long_command_output_starts_with_advice_to_filter_it() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [seq]\nwarn:\n  output: 100\n");
    let out = p.harno(&["run", "seq 1 200"]).ok().to_string();
    assert!(out.starts_with("warning: output ") && out.lines().next().unwrap().ends_with(" — narrow the command or filter it in run.filters"), "{out}");
    assert!(out.contains("\n200\n"), "{out}");
}

#[test]
fn a_zero_turns_the_warning_off() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo]\nwarn:\n  output: 0\n");
    assert!(!p.harno(&["run", &format!("echo {}", "long ".repeat(5000))]).ok().contains("warning"));
}

#[test]
fn help_in_a_project_lists_its_presets_and_allow() {
    let p = Project::new();
    p.config("version: 1\nrun:\n  allow: [echo, {command: sleep, timeout: 5}]\n  presets:\n    hi: { seq: [\"echo hi {args}\"] }\n    both: { par: [echo a, echo b] }\n");
    let out = p.harno(&["help"]).ok().to_string();
    let want = "\npresets of this project: harno run <preset> [<arg>…], the arguments where {args} is\n  both  par: echo a, echo b\n  hi    echo hi {args}\n\nrun.allow of this project: harno run \"<command>\", matched on its first words\n  echo, sleep (5s)\n";
    assert!(out.ends_with(want), "{out}");
}

#[test]
fn no_verb_and_the_help_flags_print_the_usage_too() {
    let p = Project::bare();
    let usage = p.harno(&["help"]).ok().to_string();
    assert!(usage.starts_with("usage: harno <verb> [args]\n"), "{usage}");
    let spellings: [&[&str]; 3] = [&[], &["-h"], &["--help"]];
    for args in spellings {
        assert_eq!(p.harno(args).ok(), usage, "{args:?}");
    }
}

#[test]
fn help_verbose_prints_the_manual_with_the_built_in_tools() {
    let p = Project::bare();
    let out = p.harno(&["help", "--verbose"]).ok().to_string();
    for part in ["WHAT IT IS", "CONFIG: .harno/config.yml", "\nRUN\n", "\nFILTERS\n", "LOG: .harno/log/", "\nTOOLS\n", "\nOUTPUT\n"] {
        assert!(out.contains(part), "{part}: {out}");
    }
    assert!(out.contains("Built in: analytics, bat-filter, cargo-build-filter, cargo-clippy-filter, cargo-nextest-filter, cargo-test-filter, claude-code, commit-only, edit, go-test-filter, implement, pytest-filter, rg-filter, sym, text."), "{out}");
    assert!(p.harno(&["help", "--nope"]).refused().starts_with("refused: help takes --verbose\ninstead: harno help --verbose\n"));
}

#[test]
fn a_verb_given_more_arguments_than_it_takes_is_refused() {
    let p = Project::new();
    let c = p.harno(&["tool", "ls", "extra"]);
    assert_eq!(c.refused(), "refused: tool takes ls, add <name>…, remove <name>, update [<name>] or test\ninstead: harno tool ls\n");
    let c = p.harno(&["log", "raw"]);
    assert!(c.refused().starts_with("refused: log takes ls"), "{}", c.out);
}

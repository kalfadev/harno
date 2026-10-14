# Guide {7bffaa}

Using harno: building it, every verb, filtering command output, the session log.

## Install and start a project {7a3c54}

Install harno with the installer of the latest release, `curl -LsSf https://github.com/kalfadev/harno/releases/latest/download/harno-installer.sh | sh`, or from a checkout, `cargo install --path crates/harno --locked`; both put it into `~/.cargo/bin`, and `harno help` shows it works. The release holds harno alone, for macOS, Linux and Windows (`harno-installer.ps1` on Windows); a tool written in a compiled language, such as analytics, is built from the repository, and harno itself downloads nothing. The docs command, `mdmap`, is a program of its own repository, [kalfadev/mdmap](https://github.com/kalfadev/mdmap): its installer, `curl -LsSf https://github.com/kalfadev/mdmap/releases/latest/download/mdmap-installer.sh | sh`, puts it into `~/.cargo/bin`, and the preset `mdmap: { seq: ["mdmap {args}"] }` in `run.presets` calls it. harno needs nothing else at run time: the outside commands in **Working on harno** serve the presets of this repository, not harno.

Start a project in the directory that holds the docs, usually the repository root:

1. `harno init` writes `.harno/config.yml`, every field with its default, and nothing else. It refuses when the file exists.
2. Install mdmap as above and add its preset to `run.presets`. Write `mdmap.yml` at the root of the project: `roots`, the folders that hold the markdown (`[docs/]` by default), and `ignore`, what mdmap must not touch. `ignore` globs are relative to the root (`drafts/**`, not `docs/drafts/`); one that matches nothing is silent, so look at the tree in the next step. `harno tool ls` lists the tools a project can add.
3. `harno run mdmap ls`. The first call gives every heading an id (such as `{7a3c54}` at the end of the heading line) and says so in an `autofix:` line. `harno run mdmap check` reports what is left, `harno run mdmap check --fix` repairs what it can.
4. Commit the config and the ids. Ids never change: links, `CLAUDE.md` and the log name headings by them. The first call that logs adds `.harno/log/` to `.gitignore`.

Then read with `harno run mdmap ls`, `show`, `search` and write with `add`, `update`, `remove` (**The docs: harno mdmap**). `run.allow` and `run.presets` start empty. `run.allow` lists the commands the agent may start by hand, `harno run "<command>"`. A preset is a named list of commands, `harno run <preset>`; its commands skip `run.allow` on purpose, so a command the agent may run only as written (an install, a commit) goes into a preset and not into `run.allow`. A preset item may name another preset. Without harness hooks the calls of a day log to the session `manual-<date>`; to wire an agent, see **Claude Code setup: Your project, step by step**.

To work on harno itself: `cargo build --release`, `cargo test --workspace`. The binaries are `target/release/harno` and `target/release/analytics`; in this repository the installed copies (`harno run install`: harno in `~/.cargo/bin`, analytics in its tool folder) are the ones in use, and mdmap is the copy its own repository installs. Behaviour tests live in the `harno-test` crate (`crates/harno-test/tests/`) and in the tests of the tools (`tools/analytics/tests/`); they drive harno and analytics in-process against temporary projects.

## Use {9caf9b}

Every verb and flag of the core, as `harno help` prints it; the docs verbs belong to the mdmap command (**The docs: harno mdmap**). Every verb takes `--json`. `deny.calls` in the config lists calls harno refuses (none by default; the leading `harno` may be left out); `warn.output` sets the size past which the output starts with advice to narrow a call.

<!-- verify: usage -->

```
usage: harno <verb> [args]

run
  harno run "<command>"
  harno run --seq "<command>" "<preset> [-- <arg>…]"…
  harno run --par "<command>" "<preset> [-- <arg>…]"…
  harno run <preset> [<arg>…]        every word after a lone preset is its argument
  harno run --raw "<command>" --why "<reason>"
log
  harno log ls [<from> [<to>]]       the calls of the session, one line each; the last 20
  harno log raw <n> [--session <id>]  the whole output a filter shortened
  harno log stats [<session>] [--all]   what a session of calls adds up to: calls, characters
  harno log analytics [<session>] [--all]  the commands with no filter of their own, most shown first
  harno log fails [<session>] [--all]      the commands that failed and the calls refused, most first
tools
  harno tool ls                      the built-in tools and the project's, each a folder in .harno/tools/ with a tool.yml
  harno tool add <name>…             copy built-in tools into .harno/tools/, with the presets and the filter line each tool.yml declares
  harno tool remove <name>
  harno tool update [<name>]
  harno tool test                    the tests of every filter in run.filters
  a tool runs through a preset that names its program: harno run <preset> <arg>…
project
  harno init

Every verb takes --json.
harno help, -h, --help or no verb prints this; in a project, then the presets and run.allow of its config.
harno help --verbose prints the manual: what harno is, the config, run, filters, the log, tools.
A call works in the nearest directory upward with .harno/config.yml; harno init writes one.
```

## Filtering command output {386bad}

`run.filters` decides what the output of a command looks like before the agent reads it:

```yaml
run:
  filters:
    "*": { dedupe: true, head: 100, tail: 100 }   # every command
    cargo test: .harno/tools/cargo-test         # a filter folder: its tool.yml
    make: { drop: ['^make\['], on_empty: 'make: ok' }
```

A command filter overrides `*` field by field. The fields run in one fixed order: colour codes removed, `replace`, `drop`, `keep`, `dedupe`, `head` / `tail`, `width`, `on_empty`; `replace: [{pattern: <regex>, with: <text>}…]` rewrites every match in a line, pair by pair, `$1` the first group, so the line fields read the short form; a line longer than `width` keeps its first characters and counts the rest, since no line field can reach inside one long line. `reads: stdout` or `reads: stderr` gives the fields, or the script, that stream alone and passes the other whole, stdout first; without it the two are one text in the order they were written. It is for a script that parses structured stdout, JSON a stray stderr line would break; an error is known by the exit code, which `on_fail` reads, not by the stream. `on_fail` is the filter for a failing command; `script: <command>` hands the output to a program instead; in a filter folder it runs in the folder, beside the scripts it names. `ok: [<exit>…]` says which exit codes of that command are no failure of the call, so `rg` answering `no match` with exit 1 does not stop a chain. `tests` are examples `harno tool test` runs; for a filter of one stream a test gives stdout as `input` and stderr as `stderr`. Where lines were removed the output says how to get them back:

```
test result: FAILED. 40 passed; 2 failed
… 312 lines: harno log raw 7
```

A filter never loses output: what the command wrote is kept beside the line of its call, `.harno/log/harno/<session>/<n>.raw`, and harno shows the raw output when a filter would make it longer, leaves nothing, or its script fails. An `on_empty` text is the exception at both ends: a command that wrote nothing keeps it, and it stands alone, without the note. The binary carries filters and tools for common commands (`harno tool ls`); `harno tool add <name>` copies a folder into `.harno/tools/` and writes into the config what its `tool.yml` declares, the presets it needs and the `run.filters` line of the filter it carries, and `harno tool update` brings the copies up to date. A tool written in a compiled language is built from a checkout of its repository, as its `tool.yml` says.

## The log {b405b4}

harno writes one line for every call in `.harno/log/harno/<session>.jsonl`, and the hooks of a harness write the words of their session beside it, for Claude Code in `.harno/log/claude-code/<session>.jsonl`; harno adds `.harno/log/` to `.gitignore`.

The line of a call carries `n`, `ts`, `ms`, `cwd`, `verb` and `args`, and for a call that ran commands `cmd`, `exit` and `failed` (whether each exit code failed the call, as the filter of its command judged it when it ran: an exit its `ok` names did not), `output` and `lines` (what the commands wrote) and `filtered` and `kept` (what the caller read). A line written before `failed` counts every exit code but 0 as a failure. A refused call carries `refused` alone: nothing ran. Beside the file, `<session>/<n>.out` is what the caller read and `<n>.raw` what the commands wrote before a filter cut it, kept only when the two differ; `harno log raw <n>` reads it, of this session or of the one `--session <id>` names, a call logged like any other. While a call of `run` runs, `<n>.raw` fills line by line as its commands write, and `<n>.run` says how the call began, so a call that has not ended is read like one that has: a server the agent started in the background, and its address. A human in a terminal sees the lines as they come, unfiltered, and the call ends with the headers of its commands and the summary.

A line of the log of the harness has `n`, `ts` and `type`:

| type | from → to | where it comes from |
|---|---|---|
| `session` | — | `session-start`, `session-end` hooks |
| `prompt` | human → agent | `prompt` hook |
| `tool` | agent → another tool | `tool` hook |
| `subagent` | agent → subagent | `tool` hook, when the tool hands work to a subagent |
| `response` | agent → human | `response` hook |

A line counts what it holds, `chars`, and the text sits beside it: `<session>/<n>.out` is what a tool or subagent returned or what was written, `<session>/<n>.in` the input of a tool. A tool call that went through harno keeps no text of its own: `harno` names the lines of the gate log it claims, and the gate has what they ran and returned.
A `tool` line also names its API turn, `turn` (the id of the model response that asked for the call), and a `response` line counts the turns since the prompt, `turns`; both come from the transcript, when the harness has one.
The first `tool` line of a turn also carries what the agent wrote in it besides its calls, as `said`.
`harno log ls [<from> [<to>]] [--session <id>]` prints the calls of a session in number order, one line each under the names of the columns: the number, the time, how long it took, what came back (`3.5k of 39.3k` where a filter cut what the commands wrote), how it ended (`ok`, the first failing `exit`, or `refused` with the reason after the call), and last the call as the agent wrote it, on one line and cut. A call without its line yet has the time it began and `running`, or `died` when its process ended before it wrote one. Without a range it is the last 20. Sizes read `950`, `9.7k`, `4.4M` in `ls`, `stats` and `analytics` alike; `--json` carries the exact numbers. The words of a session read back the same way, through the tool that wrote them: `harno run session [-- <from n> [<to n>]]` for Claude Code.
`harno log stats [<session>] [--all]` adds a session of calls up: how many calls and how many failed, characters returned with their median and p90, and what the filters cut. Without a session it is the session of the call, the one `log ls` reads, and before that session has a call the one used last; a line names how many others the log holds. `--all` is every session as a row of the same numbers, the one used last first, so a session that started early and is still at work comes before one that started later, and under them the whole log as one, `total`; `--json` carries the same numbers, `total` with them. A call counts when its line is written, so the call asking is not in its own numbers.
`harno log analytics [<session>] [--all]` ranks the commands that have no filter of their own by the characters they gave the caller, most first, so the first line is the next filter or preset to write; a second list ranks the commands a filter of their own names the same way. Every line says how much of what the commands wrote reached the caller, `kept`: what a filter cut is in the raw output of the call, cut at the same headers. A command counts under the preset that ran it, `preset <name>`: of the presets the call named and those they include, the one whose command fits it with the most text of its own, so of two presets that run the same command the one the call named wins and a preset inside another is the inner one. A command typed by hand counts under the `run.allow` entry that lets it through, else its first word, with its script when that word is an interpreter running a `.py`, `.sh`, `.js`, `.mjs` or `.rb` file. The name ends the line and is never cut. A call of several commands is cut at their headers, and the headers and the footer count for none. The presets and filters are those of the config today, and the sessions are chosen as `stats` chooses them.
`harno log fails [<session>] [--all]` lists what failed, most first: the commands by the name they count under in `analytics` and their exit code, each with the last line the latest of them wrote, the lines harno adds to an output left out (the footer of a lone command, the note of a cut, the sandbox line), then the refused calls by their reason with what it quotes left out, `…`, so two commands refused as not in `run.allow` are one line, with the latest reason whole after it. The first line counts the calls that failed, a command or a refusal, out of all. Which command failed is what its call wrote, `failed`, so `ls`, `stats`, `fails` and the page of `analytics` count the same calls whatever the config says later; the sessions are chosen as `stats` chooses them.
The `subagent` line is written from the hook payload as the Claude Code docs describe it, and no real session has logged one, since this repository denies `Agent` and `Task`. The owner left it unverified: the log is there to read calls and tool calls, and a subagent is rare.

## The docs: harno mdmap {86f22e}

mdmap is optional: harno runs without it, and this repository uses it for its own docs. It is a program of its own repository, [kalfadev/mdmap](https://github.com/kalfadev/mdmap), installed into `~/.cargo/bin` and called through the preset `mdmap: { seq: ["mdmap {args}"] }`. It reads markdown as one tree: a folder and a file are named by their path, and every heading by an id that never changes (this one `{86f22e}`). Every verb takes an id or a path; `harno run mdmap help` lists them. Its settings are `mdmap.yml` at the root of the project. What harno uses of it, the preset, its filters and `implement`, and the log that `mdmap stats` reads, are a contract between the two repositories: a change on either side is a task in both.

## Manual: harno help --verbose {e987ea}

The manual the binary carries, as `harno help --verbose` prints it outside a project; in a project the presets and `run.allow` of its config follow, as after `harno help`. Someone with only the binary learns every feature from it; `refresh` keeps this copy equal to the binary.

<!-- verify: manual -->

```
harno: the harness an agent works through

WHAT IT IS
  harno sits between a coding agent and the machine. The agent runs every command
  through harno, and harno
    - runs only what .harno/config.yml allows (run.allow, run.presets),
    - filters the output before the agent reads it; the whole output stays one call away,
    - logs every call, prompt, tool call and answer of a session, verbatim, in .harno/log/,
    - adds tools: folders copied from the ones built in, a program, a filter, hook scripts.
  harno calls no model. A call works in the nearest directory upward that holds
  .harno/config.yml.

START
  harno init                      write .harno/config.yml, every field with its default
  harno tool add claude-code      the hooks that wire Claude Code to harno (TOOLS)
  harno help                      the verbs, then the presets and run.allow of the project

CONFIG: .harno/config.yml (an unknown field is refused)
  version: 1
  run:
    timeout: 900                  seconds a command may run; 0 is no limit
    allow: []                     what harno run "<command>" may start, matched on its first words;
                                  an entry is a command or {command: <words>, timeout: <seconds>},
                                  and the longest match gives the timeout
    presets: {}                   <name>: {seq: [...]} or {par: [...]}; an item is a command or
                                  another preset; {args} takes the words given after the preset,
                                  and inside a word one word of letters, digits and _ . : / -;
                                  preset commands skip run.allow, on purpose; an item
                                  named like its own preset is that program;
                                  defaults: {"-n": "20"} puts -n 20 before the arguments unless
                                  the caller writes -n itself (-n 5, -n5); "" is a flag alone
    filters:                      <command>: a filter (FILTERS); "*" is every command
      "*": { dedupe: true, head: 100, tail: 100 }
    sandbox:                      the OS sandbox every command of a call runs in
      enabled: false              true: a command writes in the project and nowhere else
      allow:                      what a command may touch outside the project; ~ is the home
        write: []                 paths it may write beside the project
        read: ["*"]               paths it may read; ["*"] is every path, and a narrower list
                                  is the whole of what may be read
      deny:
        read: []                  paths it may not read; a path here beats every allow, the
                                  project included. Empty denies nothing
  deny:
    calls: []                     calls harno refuses: a verb and the arguments that must all be
                                  present, such as "run mdmap ls --body"; * is any one argument
  warn:
    output: 20000                 characters; a longer output starts with advice; 0 is off

RUN
  harno run "<command>"                          one command of run.allow
  harno run <preset> [<arg>…]                    every word after a lone preset is its argument, --json too
  harno run --seq "<a>" "<preset> -- <args>"…    in order; stops at the first failure
  harno run --par "<a>" "<b>"…                   at once; every exit code is reported
  harno run --raw "<command>" --why "<reason>"   anything else; the reason is logged
  A command is an argument vector, not a shell line: &&, |, ;, $VAR are plain words;
  quotes group words; an unquoted *, ? or [ expands to the matching paths.
  A call of one command gives back what the command wrote and nothing else; a failure, an
  empty output or a wait of a second or more adds one line, "FAIL exit 1 · 0.2s". Several
  commands each print a block under "--- <n>: <command>", 60 characters of the command,
  and a failure or a wait ends that line the same way, " --- FAIL exit 1 · 0.2s" or
  " · 1.3s"; the call ends with a summary: "3/3 ran", "stopped: 2/3 ran — 2: exit 1",
  "3 ran, 1 failed — 3: exit 1".
  harno exits with the first failing exit code, in command order.
  While a command runs, each line it writes goes to the raw file of the call, and to the
  terminal when a human watches (stdout is one, no --json): such a call is not filtered and
  ends with the headers and the summary alone. A command that runs until it is stopped, a
  server, takes timeout 0 and starts in the background of the harness: harno log ls shows
  it running and harno log raw <n> what it wrote so far.
  Every command runs in a process group of its own: a timeout stops it and all it started,
  and SIGTERM, SIGINT or SIGHUP to harno go on to them, the call ending with its line and
  exit 128 + the signal; a signal harno started with ignored stays ignored (nohup).
  With run.sandbox.enabled every command, --raw and a preset alike, runs in the sandbox of
  the platform: sandbox-exec on macOS, Landlock on Linux. Where none can start the commands
  run without one and every call ends with a warning saying why.

FILTERS
  A value of run.filters is inline fields, a tool folder (the filter of its tool.yml) or a filter file.
  The fields apply in this order, whatever order they are written in:
    colour codes and carriage returns are removed
    replace: [{pattern, with}…]   every match in a line becomes with, $1 its first group
    drop: [<regex>…]              remove the matching lines
    keep: [<regex>…]              keep only the matching lines
    dedupe: true                  a repeated neighbour line once, with (×N)
    head: <n>, tail: <n>          keep the first n and the last n lines; 0 does not cut
    width: <n>                    a longer line keeps its first n characters and counts the rest
    on_empty: "<text>"            printed when nothing is left
    ok: [<exit>…]                 exit codes of the command that are no failure of the call
    reads: stdout|stderr          the fields read that stream alone and the other passes whole,
                                  stdout first; the command then runs with the two apart
  on_fail: {…}                    the filter of a failing command
  script: <command>               the output goes to a program on stdin, the exit code in
                                  HARNO_EXIT; what it prints is the output
  tests: [{input, stderr, expected, exit}]  examples that harno tool test checks
  A command filter overrides "*" field by field; the longest matching command wins.
  Where lines are cut the output says "… <n> lines: harno log raw <call>";
  an on_empty text stands alone, and what it replaced stays in the log.
  The raw output is shown instead when a filter would make it longer or empty, or its
  script fails. A command that wrote nothing keeps its on_empty text: there is nothing
  for it to be worse than, and a call that gives back nothing at all says nothing.

LOG: .harno/log/ (added to .gitignore)
  harno/<session>.jsonl holds one line per call that went through harno, with
    n, ts, ms, cwd, verb, args, and for a call that ran commands cmd, exit and
    failed (whether each exit code failed the call, as its filter judged it),
    output and lines (what the commands wrote) and filtered and kept (what the
    caller read). A refused call carries refused alone: nothing ran. Beside the
    file, <session>/<n>.out is what the caller read and <n>.raw what the
    commands wrote before a filter cut it; while a call of run runs, <n>.raw
    fills as its commands write and <n>.run says how the call began. The
    session comes from HARNO_SESSION;
    without it the calls of a day go to the session manual-<date>, and the first
    call of that log warns.
  <harness>/<session>.jsonl holds the words of a session, written by the hooks of that
    harness in its own words: for Claude Code claude-code/<session>.jsonl, the prompt,
    every tool call and the answer, and for a call that went through harno the lines of
    the gate log it claims. Long text sits beside it: <session>/<n>.out and <n>.in.
  harno log ls [<from> [<to>]] [--session <id>]  the calls of a session, one line each; the last
                                  20 without a range; a call without its line yet is running
                                  or died
  harno log raw <n> [--session <id>]  the whole output a filter shortened
  harno log stats [<session>] [--all]   what a session of calls adds up to: calls, failures,
                                  characters, what the filters cut
  harno log analytics [<session>] [--all]  the commands with no filter of their own, most
                                  characters shown first: the next filter to write
  harno log fails [<session>] [--all]  the commands that failed by name and exit code, most
                                  first, with the last line of the latest; the refused calls

TOOLS
  A tool is a folder a project copies from the ones built into harno:
    harno tool ls | add <name>… | remove <name> | update [<name>] | test
  .harno/tools/<name>/tool.yml is what harno reads of it, every field optional:
    presets: {…}                  the run.presets the tool needs, written as the config takes them
    filter: {…}                   an output filter (FILTERS) run.filters names the folder of
    config: {…}                   settings harno leaves to the tool itself
  add copies the folder and writes what the manifest declares into .harno/config.yml: the
  presets of its presets: block as they are written, comments and all, and the run.filters
  line of the filter it carries. A key the config already has is left as it is, and a
  manifest that declares neither leaves the config untouched. remove takes the folder out
  and leaves the config as it is; update brings the
  files back to what the binary carries, shows each change and keeps the config: block of
  the project. harno ships no program of a tool: one written in a compiled language is
  built as its tool.yml says. A tool is not a verb: its call runs through its preset,
  filtered and logged like any other.
  Built in: analytics, bat-filter, cargo-build-filter, cargo-clippy-filter, cargo-nextest-filter, cargo-test-filter, claude-code, commit-only, edit, go-test-filter, implement, pytest-filter, rg-filter, sym, text.
  mdmap, the docs as one tree, is no built-in tool but a program of its own repository:
    install curl -LsSf https://github.com/kalfadev/mdmap/releases/latest/download/mdmap-installer.sh | sh,
            or cargo install --git https://github.com/kalfadev/mdmap --locked; into ~/.cargo/bin
    preset  mdmap: { seq: ["mdmap {args}"] }
    config  mdmap.yml at the root of the project; mdmap reads it itself
    verbs   harno run mdmap help
  analytics, the log of the project on one local read-only page:
    program built from a harno checkout with
            cargo install --path <harno checkout>/tools/analytics --locked --root .harno/tools/analytics
    preset  analytics: { seq: [".harno/tools/analytics/bin/analytics {args}"] }
    call    harno run analytics [--port N] [--no-open] [--background | --stop]
  claude-code, the hooks that write the log of a Claude Code session beside the log of the
  gate, and the short view of it, harno run session. The hooks reach
  .claude/settings.json through a preset, which needs python3; other hooks there stay:
    claude-code: { seq: ["python3 .harno/tools/claude-code/install.py {args}"] }
    harno run claude-code install | remove
    strict: true      a Bash hook refuses every command that is not harno (needs shfmt);
                      after a change run harno run claude-code install again
  The first line of CLAUDE.md that starts with "$ " is the start command: its output goes
  into the first prompt of a session.
  The other built-in tools are scripts; their tool.yml says how a preset calls them.

OUTPUT
  Every verb takes --json. A refusal is "refused: <reason>", then "instead: <command>" when
  there is a next call, and exits 1. A long output starts with advice to narrow the call;
  it is never cut.
```

## Working on harno {592c5c}

The `harno` on PATH is an installed copy (`~/.cargo/bin/`) and analytics is built into `.harno/tools/analytics/bin/`, not `target/`: a broken build cannot lock the agent out. After a change passes `ci`, `harno run install` puts both in use. `mdmap` on PATH is the copy its own repository (`../mdmap`) installs into `~/.cargo/bin`, with `harno run install` there. `harno run doctor` checks that every outside tool is present: cargo-modules, cargo-public-api, cargo-nextest, cargo-machete, cargo-llvm-cov, cargo-insta, ast-grep, ripgrep, bat, tokei, difftastic, shfmt, mcptools, jq, typos, graphify, python3. Install: `cargo install --locked cargo-modules cargo-public-api cargo-nextest cargo-machete cargo-llvm-cov cargo-insta` (cargo-llvm-cov also needs `rustup component add llvm-tools-preview`), `brew install ast-grep ripgrep bat tokei difftastic shfmt mcptools jq typos-cli`, `uv tool install graphifyy`. Serena is called over MCP with `mcptools`: the `serena` preset starts `serena start-mcp-server` for one tool, about 2 s; `.serena/project.yml` names the language. The `areas` preset checks that every source file is in an area; `_typos.toml` says what `typos` must not call a typo. The tools are in `.harno/tools/`, copies of those in the repository: `sym` (items by name over ast-grep), `text` (a literal text), `commit-only`, `edit` (replace, lines, write, doc), `implement`, `analytics` (the log page), the filters `bat-filter`, `cargo-build-filter`, `cargo-clippy-filter`, `cargo-nextest-filter`, `cargo-test-filter` and `rg-filter`, and the Claude Code harness `claude-code` with `strict: true`: `hook.py` (the log of the session), `session.py` (that log in short lines), `only-harno.py` (the Bash hook, over `shfmt --to-json`), `transcript.py` (the transcript reader), `install.py` (the hooks in `settings.json`) and `tool_config.py` (reads `tool.yml`). `.claude/` holds `settings.json`, whose hooks `install.py` writes, and `skills/harness-engineer`, the skill CLAUDE.md names: while a task runs, the agent turns a failed call, a blind search or a slow step into a fix of the harness. A tool keeps its tests in the repository, in its `tests/` folder, and `ci` runs them; the page check is `tools/analytics/tests/page.mjs`. The `targets` step of `ci` lints harno for Linux and Windows, which a mac never compiles otherwise; it needs `rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu`.

A release is cargo-dist (`dist-workspace.toml`, `.github/workflows/release.yml`): pushing a tag such as `v0.1.0` builds harno for macOS, Linux and Windows on GitHub and publishes the archives, `harno-installer.sh` and `harno-installer.ps1`. `dist plan` shows what a release holds; after a change of `dist-workspace.toml`, `dist generate` writes the workflow again. The version is `version` in `[workspace.package]` of `Cargo.toml`, and the tag must match it. `.github/workflows/test.yml` runs the tests on Linux and macOS for every push to master and every pull request, and on Windows builds harno and runs a command through it, since the tests start `sh`. `master` takes pull requests only, and `linux`, `macos` and `windows` must pass before one merges.

# harno

harno sits between an AI coding agent and your machine. The agent runs every
command through harno, and harno decides what may run, trims the output before
the agent reads it, and writes down everything that happened.

It is not an agent and it calls no model. It is a small Rust CLI that you put
under one. Today it comes with hooks for [Claude Code](https://claude.com/claude-code).

## Why

Watch an agent work for an hour and you see the same waste again and again:

- It runs the tests and reads 400 lines to learn that all 151 passed.
- It opens a 900-line file to find one function.
- A command fails, it guesses, it tries again. Every guess is another turn.
- Afterwards it is hard to tell what it actually ran.

Each of these costs time, money and context window. harno is built to cut them,
and to show you where they still happen.

## What it does

**One allow list.** The agent can run only what `.harno/config.yml` allows.
Anything else is refused, and the refusal says what to do instead:

```
$ harno run "rm -rf target"
refused: 'rm' is not in run.allow
instead: harno run --raw "rm -rf target" --why "<reason>"
```

There is a way out (`--raw`), but it needs a reason, and the reason is logged.

**Presets.** Give a command, or a list of commands, a name once. The agent
calls it by name. Several commands can run in one call, in order or in
parallel, so work that took ten turns can take one.

```yaml
run:
  allow:
    - cargo test
    - git status
  presets:
    test: { seq: ["cargo nextest run --workspace"] }
    check: { par: ["cargo clippy --workspace --all-targets", "typos"] }
  filters:
    cargo nextest: .harno/tools/cargo-nextest-filter
```

**Filtered output.** A filter drops the noise before the agent sees it. A
passing test run comes back as:

```
$ harno run test
     Summary [   3.668s] 151 tests run: 151 passed, 0 skipped
… 5 lines: harno log raw 26
```

Nothing is thrown away: the last line tells the agent how to read the full
output if it needs it. A filter is a few lines of YAML (`drop`, `keep`, `head`,
`tail`, `dedupe`, …) and carries its own tests, which `harno tool test` runs.
Filters for cargo build, test, clippy and nextest, go test, pytest, rg and bat
come built in.

**A full log.** Every call goes to `.harno/log/`: the command, the exit code,
the time it took, what the agent saw and what the filter cut. With the Claude
Code hooks, the prompts, every tool call and the answers are logged beside it.
`harno run analytics` opens all of it on a local page, so you can scroll
through a session and see where the turns went.

**Optional: docs the agent can walk.** harno runs fine on its own. This
repository also uses [mdmap](https://github.com/kalfadev/mdmap), a separate
tool that reads markdown as one tree of headings with ids that never change, so
the agent opens one section (`harno run mdmap show 7a3c54`) instead of a whole
file. You need it only to work on harno itself; any project can call it
through a preset the same way.

## Quick start

macOS and Linux:

```
curl -LsSf https://github.com/kalfadev/harno/releases/latest/download/harno-installer.sh | sh
```

Windows (PowerShell):

```
powershell -ExecutionPolicy Bypass -c "irm https://github.com/kalfadev/harno/releases/latest/download/harno-installer.ps1 | iex"
```

Or download the binary for your system from the
[releases page](https://github.com/kalfadev/harno/releases). With a Rust
toolchain: `cargo install --git https://github.com/kalfadev/harno harno --locked`.
Then `harno help` shows it works.

Then, in your project:

```
harno init
```

This writes `.harno/config.yml`. It starts closed: `run.allow` and
`run.presets` are empty, so every command is refused until you add it. Open the
file and add what the agent may run, like the example above:

```yaml
run:
  allow:
    - cargo test
    - git status
  presets:
    test: { seq: ["cargo test"] }
```

Now:

```
harno run "cargo test"           # a command from run.allow
harno run test                   # a preset
harno log ls                     # what ran in this session
```

Built-in tools (filters, the Claude Code hooks, the log page and more) are
listed by `harno tool ls`. `harno tool add <name>` copies one into
`.harno/tools/` and adds its preset or filter line to the config:

```
harno tool add cargo-test-filter     # cargo test output, filtered
harno tool add analytics             # the log page
```

The log page is a compiled program that is not in the release, so build it
once (you need Rust):

```
cargo install --git https://github.com/kalfadev/harno analytics --root .harno/tools/analytics --locked
harno run analytics
```

## With Claude Code

`harno tool add claude-code` adds hooks that log the whole session, and an
optional strict mode in which Claude Code may run nothing but harno. The steps
are in [Claude Code setup](docs/claude-code-setup.md).

This repository is built that way. `CLAUDE.md`, `.claude/settings.json` and
`.harno/config.yml` are a real, daily-used setup you can copy from. The
`harness-engineer` skill in `.claude/skills/` keeps the agent improving that
setup as it works: a failed call, a blind search or a slow step becomes a
preset, a filter or a line of docs, so the next task goes faster.

## Docs

- [Overview](docs/overview.md): what happens in one call, and where the code lives
- [Guide](docs/guide.md): install, config, filters, the log, the full manual
- [Claude Code setup](docs/claude-code-setup.md)
- [Code map](docs/code-map.md): for contributors

## Status

Early, version 0.1. The config format may still change. Built for macOS, Linux
and Windows. It is used daily on macOS, where the tests run at every commit; on
GitHub they run on Linux at every push, and on Windows the CI builds harno and
runs a command through it.
The Claude Code hooks are Python scripts, run as `python3`, or `python` on
Windows.

## License

MIT

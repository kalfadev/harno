# Overview {3e57e7}

What harno is, the parts it is made of, how one call flows through them, and where each part lives. Read this first; the **Guide** has every verb and field, `harno help --verbose` the same manual in the binary.

## What harno is {ccc13e}

An agent spends turns and context: a failed call, a long output read whole, a file opened to find one line. harno cuts both, and shows what the agent did. It is not an agent and calls no model: it sits under one (Claude Code, through the claude-code tool), between the agent and the machine, and does four jobs.

- **A gate.** Only what `.harno/config.yml` allows runs: a command of `run.allow` or a named preset. Anything else takes `harno run --raw "<command>" --why "<reason>"`, and the reason is logged.
- **Filtered output.** A filter cuts what the agent does not need before it reads it; the whole output stays one call away, `harno log raw <n>`.
- **A verbatim log.** Every call with what it ran and what came back, in `.harno/log/harno/`, and beside it the prompt, every tool call with its output and the answer, grouped by API turn; `harno run analytics` shows it on a local page.
- **Tools.** Folders a project copies from the ones built into harno and changes as it likes: a program a preset runs, an output filter, the hook scripts that wire an agent.

The docs command, mdmap, is a program of its own repository that a preset calls: it reads markdown as one tree of folders, files and headings with ids, so the agent opens one heading, not a file, and sees sizes before it reads. An agent or an agent framework that owns the model loop can sit on top of harno: its shell tool runs through harno, which a harness tool wires in.

## Concepts {863932}

- **Project**: the nearest directory upward with `.harno/config.yml`; `harno init` writes one, every field with its default.
- **run.allow**: the commands the agent may start by hand, matched on their first words; an entry may carry its own timeout.
- **Preset**: a named list of commands, run in sequence (`seq`) or at once (`par`); `{args}` takes the words after its name. A preset runs without `run.allow`, so a command the agent may run only as written goes into a preset.
- **Filter**: what the output of a command looks like before the agent reads it; fields (`drop`, `keep`, `dedupe`, `head`, `tail`, `on_empty`, `on_fail`) or a script. Every cut is recoverable.
- **Session and log**: every call belongs to a session (`HARNO_SESSION`, set by the harness); the calls go to `.harno/log/harno/<session>.jsonl`, and the hooks of the harness write the words of the session beside them, `.harno/log/<harness>/<session>.jsonl`.
- **Tool**: a folder copied from the binary into `.harno/tools/<name>/`, with a `tool.yml`: the `presets` that run its program, the `filter` a `run.filters` line may name, and `config`, which the tool reads itself; the hook scripts that wire an agent are files of its folder. `harno tool add` copies the folder and writes its presets and its `run.filters` line into the config.
- **Corpus**: the markdown under the `roots` of mdmap; a folder and a file are named by their path, a heading by its six-hex id, which never changes.

## One call {9d9e6e}

What happens when the agent runs `harno run test`:

```
agent ── Bash: harno run test ──► harno (cli::main)
  1. find the project: the nearest .harno/config.yml upward
  2. load the config, refuse a call deny.calls lists
  3. pick the session (HARNO_SESSION, else "manual-<date>"), take the number of the call
  4. run: split each command into words, check run.allow or expand the preset,
     run the commands (--seq or --par), each under its timeout
  5. filter each output (run.filters); what the commands wrote is kept whole,
     .harno/log/harno/<session>/<n>.raw
  6. put warnings first, write the line of the call, print, exit with the first failure
```

The harness adds what harno cannot see itself:

```
Claude Code hooks ──► hook.py of the tool ──► .harno/log/claude-code/<session>.jsonl
  (session start and end, prompt, tool call, answer)
every harno call ─────────────────────────────────────────────► .harno/log/harno/<session>.jsonl
harno run analytics   ──► the log on a local read-only page
```

A program such as mdmap is only what a preset names: `harno run mdmap show <id>` runs `mdmap show <id>` from PATH through the same six steps.

## Where things live {06fa67}

| path | what |
|---|---|
| `crates/harno/` | the core: `cli`, `config`, `run`, `filter`, `tool`, and `harno-log` beside it |
| `crates/harno-test/` | behaviour tests of the core, in-process against temporary projects |
| `tools/<name>/` | the built-in tools; `tools/analytics/` is a Rust crate, the others are scripts, and `tools/claude-code/` holds the hook scripts and their tests |
| `.harno/config.yml` | the config of a project |
| `.harno/tools/` | the tools a project added |
| `.harno/log/` | the calls of the gate, the words of a session, the state of the analytics server; gitignored |
| `docs/` | this corpus |

**Code map** has every source file by area.

## Where new behavior goes {698580}

| goal | where |
|---|---|
| let the agent run a command | `run.allow`, or a preset when it must run only as written |
| give a slow command more time | `{command: <words>, timeout: <seconds>}` in `run.allow` |
| shorten an output | a filter in `run.filters`; a tool folder with its scripts under `.harno/tools/` |
| a program for the agent | a tool: a folder with a `tool.yml` and its program, and a preset that names it |
| wire another agent | a tool of hook scripts that write `.harno/log/<harness>/<session>.jsonl`, and a preset that installs them |
| read or write docs | the mdmap verbs, `harno run mdmap help` |
| a core verb or config field | **Code map: Entry** |
| a log event type | **Code map: The log** |

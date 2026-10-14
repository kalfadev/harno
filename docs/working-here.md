# Working here {2b5616}

Every action goes through harno: Claude Code's own tools are denied, and a hook refuses a Bash command that does not start with `harno`. The aim is the fewest turns and the least context: ask narrow questions, batch everything that does not depend on an earlier answer.

## Rules {47dc05}

Docs (`docs/`) are read and written only as nodes of mdmap: `ls`, `show <id>|<path>`, `search "<text>"`, `add`, `update`, `move`, `remove`, all through `harno run mdmap`, or `doc` for a text inside a body. Find a text with `search`, never `rg` over `docs/`: it gives the id `show` takes. A long argument is one single-quoted string with real newlines, a quote inside written `'\''`; no heredoc, no `$(…)`. The code is not rustfmt-formatted: match its long-line style by hand. Code, comments and docs in English; talk to the owner in the language of the owner, short and plain, with examples. The owner decides design points: ask with concrete options, and record the answer and why in the doc of that part. Docs say what is, never what was: the commit log keeps the past. Commit when sure the work is done, never push. A feature or field only when a real need shows up. The log is verbatim: never filter or reinterpret what the agent did.

## Presets {7cf629}

The presets of `.harno/config.yml` (the config is the source; this table says when to use which). Call `harno run <preset> <args>` (a `--` before the arguments may stay), or put `"<preset> -- <args>"` as an item of `--seq`/`--par`.

| need | preset |
|---|---|
| crate tree, private items too | `modules -- harno [--focus-on harno::run]` |
| crate dependencies | `deps [-- -p harno]` |
| public API with signatures | `api -- harno` |
| file sizes | `sizes -- crates/harno/src` |
| items of a file, with spans | `outline -- <file>` |
| where a name is defined | `def -- <name> [Type::method]…` |
| an item with its doc comments, numbered | `body -- <name>… [--in <file>]` |
| exact lines | `show -- <file> -r 40:90` |
| every mention | `refs -- <name> crates/` |
| a literal text, signs and all, whole-word at its ends | `text -- <words…> [<path>…]` |
| every call of a function, its definition too | `calls -- <name>` |
| a Rust function or method, whole, from ast-grep | `fn -- <name>` |
| what depends on it, its neighbours | `affected -- "name()"`, `explain -- "name()"` |
| language server: callers, implementations, rename | `serena -- <tool> -p '{"name_path": "exec"}'`; `serena-tools` lists every tool with its parameters |
| the docs of a library, from context7 | `context7-search -- nextjs`, `context7 -- /vercel/next.js routing` |
| change text that occurs once | `replace -- <file> <old> <new>` |
| replace lines a-b (several: bottom up) | `lines -- <file> <a>-<b> <text>` |
| create or overwrite a file | `write -- <file> <text>` |
| change a text in the body of a doc node; its lines are found when it runs | `doc -- <id> <old> <new>` |
| structural rewrite | `rewrite -- -p 'foo($A)' -r 'bar($A)' -U crates/` |
| build, test, lint, spelling | `build`, `test`, `clippy`, `ci`, `machete`, `spell` |
| lines no test runs (own build, about 15 s) | `cover` |
| an output change failed a snapshot test: take the new text | `accept` |
| the log and docs page in a browser, first load | `page "<js expression>"` |
| a whole change: edits, docs, checks, commit | `implement -- '<plan>'` (sections: **Workflow**) |
| the edits of a plan, no checks | `apply -- '<plan>'` |
| the calls of the session, the words of the session, the transcript, in short lines | `harno log ls [<from> [<to>]]`, `session [-- <from n> [<to n>]]`, `transcript [-- <from hh:mm> [<to>]]` |
| git | `status`, `diff [-- <path>]`, `diffstat`, `log [-- -n 100]` (20 unless `-n` is given) |
| the verify blocks of the docs follow the code (before a commit) | `refresh` |
| stage everything and commit (last item of a `--seq`) | `commit -- 'message'` |
| commit the paths of this task alone, while another session works in the tree | `commit-only -- '<message>' <path>…` |
| put a changed harno in use (after `ci`) | `install` |
| the hooks and permissions of Claude Code, outside the sandbox | `claude-code install`, `claude-code remove` |
| check the outside tools | `doctor` |
| the core usage, as outside a project | `usage` |

Anything else in `run.allow` runs as `harno run "<command>"`; outside it, `harno run --raw "<command>" --why "<reason>"`.

## Workflow {ab19c9}

A task in three calls: discovery, the change, the answer. Before the first call, list every question the task has: what to read, what to change, how to prove it.

1. Discovery, one call, every read in it: `harno run mdmap show <area ids>… && harno run --par "body -- <name>…" "show -- <file> -r 1:12" …`; in `--par` a failed read does not stop the others, `&&` does. The area ids from **Code map**, the names guessed from the task and **Recipes**. Add `show -- <file> -r 1:12` for every file that gets new code (its `use` lines), the sample corpus when tests assert text, and for an output change `rg -n -F -e "<old text>" crates tools`: every test that asserts it goes into the change.
2. The change, one call: `harno run implement -- '<plan>'` applies every edit, then runs test, clippy, install, refresh, ci and the commit. Plan sections: `@@ replace <file>` (the old text, a line `@@ with`, the new text), `@@ lines <file> <a>-<b>`, `@@ write <file>`, `@@ doc <id>` (the old text of a doc node, a line `@@ with`, the new text; its lines are found when it runs), `@@ harno run mdmap <verb> <args>`, `@@ commit` (the message; it runs after ci passes). The old text is copied from the read; a new line is a replace of the line it goes next to; `lines` only for a long block, bottom up. A new source file goes into the verify line of its area in the same plan. Put the commit in the plan when the work is done, docs with it.
3. The answer. A file section that does not fit writes nothing, and one already applied is skipped: send the fixed plan whole again. A failed harno section stops the rest; the output names those that ran: send the plan without them. A failed check: the shown lines say why; when not, the filter is wrong — fix it in `.harno/config.yml`.

Tasks that come back: **Recipes** `{298a0c}`, one discovery call each. The mistakes that cost a call: **Call rules** `{eee4af}`, which every session reads. A mistake is fixed in the harness where it can be: a preset, a clearer refusal, a docs line where the task looks; a line goes into **Call rules** only when it refuses or loses a whole call and no harness change can prevent it.
Usage: `harno help`, `harno run mdmap help`. `--raw` is the last resort. `ci` runs every check of this repository; `install` puts harno and analytics in use; `refresh` is `areas` and `mdmap verify --fix`.

## Recipes {298a0c}

Discovery for tasks that come back, one call each. Show this node in discovery when the task matches one.

A task on mdmap itself, a verb, a flag or a setting, belongs to its own repository (`../mdmap`), which has its recipes.
A new core verb or a config field: discovery is `harno run mdmap show d78638 551eb9 && harno run --par "body -- USAGE MANUAL dispatch --in crates/harno/src/cli.rs" "outline -- crates/harno/src/config.rs" "show -- crates/harno/src/cli.rs -r 1:20" "outline -- crates/harno-test/tests/config.rs"`. The change: `USAGE` and `MANUAL`, the case in `dispatch`, a `<verb>_verb` that parses its flags; a config field also in its struct in config.rs with `#[serde(default)]`, since an unknown field is refused, and in `DEFAULT_CONFIG`; the test in `crates/harno-test/tests/<area>.rs` and its area verify line. **Guide: Use** `{9caf9b}` and **Manual: harno help --verbose** `{e987ea}` are verify-locked and follow at `refresh`.

A preset, an allowed command or a filter: discovery is `harno run mdmap show ddf01d 0e740b && harno run --par "show -- .harno/config.yml" "outline -- crates/harno/src/filter.rs"`. The change: the block in `run.presets`, `run.allow` or `run.filters` of `.harno/config.yml`, with the comment above it that shows its arguments, and the row in **Presets** `{7cf629}`; a filter that belongs to a tool is the `filter` of its `tool.yml`, with its cases, and `harno tool test` runs them. A config edit reaches the next call only.

A new tool or a harness hook: discovery is `harno run mdmap show 0e740b 331d7f && harno run --par "show -- tools/claude-code/tool.yml" "outline -- crates/harno/src/tool.rs" "show -- tools/claude-code/install.py"`. The change: a folder `tools/<name>/` with a `tool.yml`, then `harno tool add <name>` and a preset that names its program; a tool written in Rust is a crate of the workspace and a line of the `install` preset. A hook script of claude-code also needs its event in `install.py`, a test in `tools/claude-code/tests/`, and `tool update claude-code` to reach `.harno/tools/`; what it wires reaches `.claude/settings.json` only when the human runs `harno run claude-code install` outside the sandbox. Tests: tests/config.rs for the files of a tool, tests/filter.rs for the verbs.

A roadmap item: a section of **Roadmap** `{d80c9a}`, written with `harno run mdmap add d80c9a --title <t> --body <text>` and removed when it is built; the roadmap of mdmap is in its own repository.

## Call rules {eee4af}

What refuses or loses a whole call; every session reads this node.

- Not harno is refused: a pipe, `cd`, `cat`, `ls`, plain `git`, a variable, a brace, a command substitution, inside `--raw` as well. Instead a preset, `--raw "<one command>" --why "<reason>"`, or a scratchpad script as `--raw "sh <path>"`. Two harno calls join with `&&`, never a pipe: the hook allows the chain, but the second call then reads the repository, not the output of the first.
- One `--raw` per call, one command in it, always with `--why`. A `--seq` or `--par` item is a preset or an allowed command, never a harno verb and never `--raw`; mdmap there is `mdmap -- <verb>`.
- A long argument is one single-quoted string with real newlines; no heredoc, no command substitution. A single quote inside it is closed, escaped and reopened, four characters in a row: quote, backslash, quote, quote.
- A read that comes back empty is no failure: `rg` with no match and `body --` with a name it cannot find answer `no match` and `not found` with exit 1, which the `ok` of their filter allows, in `--seq` as well. What still breaks a call is a path that is not there, `rg` exit 2, and `~` is no path: harno starts a command without a shell, so a home path is written out.
- A plan is one transaction: a refused section writes nothing, so send it whole again. The old text is copied from the read, every `@@ lines` of a file goes bottom up, and the plan is literal text.
- Keep one output small: `ls <path> --depth 3`, then `show <ids>`, never a whole file. Past `warn.output` harno opens the output with advice to narrow the call, and past the cap of the harness the output goes to a file, which costs another call to read it back.
- A path outside the sandbox read list is refused before the command runs and a retry changes nothing: everything under the home but `.claude/projects` and this repository is gone.
- Every git write is one `--raw` with `--why`: `git commit -a -m "<message>"`, and `git add <path>` first for a path git does not track yet.
- An allowed command with its arguments is one quoted argument: `harno run "rg -n x src"`; given apart, the words are refused.
- A config edit reaches the next call only: an `implement` that changes `.harno/config.yml` runs its checks with the config it started with, so a check the edit adds or removes counts from the next call.

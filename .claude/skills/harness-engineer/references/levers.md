# Levers

Techniques that remove waste, grouped by what they save. They are examples,
not a checklist. Take one only when the evidence of this project points to it,
check that it fits the project (its tools, versions and style), and measure
before and after. The examples cover several ecosystems; the idea is what
carries over.

- [Speed](#speed): checker before build, scope to the change, safe caches,
  parallel work, build and link cost, slow tests
- [Calls](#calls)
- [Context](#context)
- [Docs](#docs)

## Speed

### Check instead of build while working

Most questions in the middle of a task are "does it compile and pass lint". A
checker answers that in seconds, without code generation or linking. Give the
project a checker-only step for the middle of a task, and keep the build and
the tests for the proof at the end.

| ecosystem | checker |
|---|---|
| Rust | `cargo check`, `cargo clippy` |
| TypeScript / JS | `tsc --noEmit`, eslint |
| Python | ruff, pyright or mypy |
| Go | `go vet`, staticcheck (the compiler is fast enough to be one) |
| Java / Kotlin | compile only (`gradle compileKotlin` / `compileJava`), detekt, ktlint |
| Dart / Flutter | `dart analyze` |
| C / C++ | `clang-tidy`, `-fsyntax-only` |

A build is needed only for something that runs: tests, a served app, a release.

### Scope to the change

Map every changed file to an area of the project (a package, the web app, the
docs, the tooling) and run the checks of those areas only. A change to a root
manifest or lock file runs every check of that language. Read the change from
git, so the same tree always gives the same checks. Take the areas from the
layout and presets of this project; never copy them from another one.

```bash
#!/usr/bin/env bash
# checks [quick]: the checks of what changed against HEAD, and nothing else.
set -o pipefail
files=$( { git diff --name-only HEAD --; git ls-files --others --exclude-standard; } | sort -u )
if [ -z "$files" ]; then echo "nothing changed against HEAD: nothing to check"; exit 0; fi
has() { printf "%s\n" "$files" | grep -Eq "$1"; }
failed=""
step() { label=$1; shift; echo "== $label"; "$@" || failed="$failed $label"; }

# One line per area: the paths that put it in the change.
a=""; b=""
has "<paths of area a>" && a=1
has "<paths of area b>" && b=1

# The checkers, each to its end, so every error shows at once.
[ -n "$a" ] && step <name> harno run <checker preset of a>
[ -n "$b" ] && step <name> harno run <checker preset of b>
if [ -n "$failed" ]; then echo "failed:$failed"; exit 1; fi
if [ "$1" = quick ]; then echo "the checkers of the change pass"; exit 0; fi

# Tests and builds, only when every checker passed.
[ -n "$a" ] && step <name> harno run <test preset of a>
if [ -n "$failed" ]; then echo "failed:$failed"; exit 1; fi
echo "the checks of the change pass"
```

Pitfalls met in practice:

- `in` is a shell keyword and cannot name a function.
- macOS ships bash 3.2, where `set -u` fails on an empty array.
- A step over the changed files must skip the deleted ones (`[ -f "$f" ]`).
- A script that prints the already filtered output of presets needs
  `{ head: 0, tail: 0 }` as its own filter, or it gets cut twice.

Keep a full check (`ci`) that runs everything, whatever changed.

Finer scoping inside one area, where the tool offers it: `vitest --changed`,
`jest --changedSince`, `nx affected`, `go test` over the changed packages,
`cargo nextest -E "rdeps(<crate>)"`. Selection by coverage (pytest-testmon,
cargo-difftests) remembers state from earlier runs: weigh it against
determinism.

The order of the proving step: checks, `git add -A`, docs generators,
`git add -A`, commit. A failed check stops before any test or commit.

### Safe caches

Reuse only what the tool keys on content it fully tracks:

- Rust: incremental compilation (on in dev), sccache for a shared compile cache.
- TypeScript: `tsc --incremental --tsBuildInfoFile <cache dir>`; eslint
  `--cache --cache-strategy content`.
- Python: ruff and mypy keep their own caches; dmypy runs as a daemon.
- Go: the build cache, on by default.
- Gradle: the build cache, the configuration cache, the daemon.
- C / C++: ccache or sccache.

A cache of test results is safe only when the tool hashes every input. The
network, the clock and files the tool does not track make a remembered pass
stale.

### Parallel work

- Tests: cargo nextest; pytest-xdist; vitest workers as threads when the suite
  passes that way (leave `isolate: false` alone: files leak state); Go runs
  packages in parallel, with `t.Parallel()` inside; Gradle `--parallel` and
  JUnit parallel execution; `ctest -j`.
- Parallel tests must share no state. A fixed temp path, a fixed port or a
  global has to become per test first.
- Reads: one `harno run --par` with every independent read.

### Build and link cost

- Rust: one test binary per crate (`autotests = false`, then
  `[[test]] name = "all" path = "tests/all.rs"` with a `mod` line per file; the
  files stay where they are), so a library change links once.
  `[profile.dev] debug = "line-tables-only"` and
  `[profile.dev.package."*"] debug = false`. A faster linker where the default
  is slow (mold or lld on Linux).
- C / C++: precompiled headers, unity builds, lld or mold.
- JVM: never `clean` out of habit; keep the daemon warm.
- Everywhere: a slowness that goes away after a clean build was stale build
  state, not the code. Measure again after a clean build before chasing it.

### Slow tests

List the slowest first: `cargo nextest run --status-level pass` sorted,
`pytest --durations=20`, vitest with a JSON reporter, `go test -json` sorted by
elapsed time, Gradle `--profile`. Then, for each:

- **A CPU-heavy dependency in an unoptimized build** (password hashing, crypto,
  compression): optimize that dependency alone in dev (Rust
  `[profile.dev.package.<name>] opt-level = 3`; C/C++ build the library with
  `-O2`).
- **A fixed wait** (a lockout window, a timeout against a server that never
  answers, a sleep): shorten it to what the rule it proves needs, with margin
  for a loaded machine. A wait that is itself the proof (two calls side by side,
  one sleeping) stays.
- **Polling for something that never comes:** a bug in the test. Wait for the
  event that does come.
- **Time-dependent code:** a fake clock where the code takes a clock you can
  swap (tokio `time::pause`, jest or vitest fake timers, freezegun, an injected
  clock in Go). Real sockets and processes do not follow a fake clock.
- **Setup per test** (a database, a process, a server): set it up once and
  isolate each test (a transaction rolled back, a fresh schema, a temp dir with
  the pid in its name).
- **Doc tests or generators with nothing to do:** keep them in `ci`, out of the
  proof of each change.
- **Flaky tests:** make the runner name a test that passed only on retry
  (nextest `--final-status-level flaky`, the rerun report of
  pytest-rerunfailures), so it gets fixed rather than hidden.

### Measuring

`took` in `harno log ls` is per call, and `--seq` prints the time of each item.
Compare the same step on the same machine, warm, twice. Say when a number is
cold.

## Calls

- **Never save a call at the cost of time.** Joining steps, skipping a quick
  checker or running every check to avoid a decision each saves a call and
  loses minutes. The minutes win.
- **A repeated `--raw` or scratchpad script becomes a preset.** A fixed
  command, `{args}` for what varies, and output that depends only on the
  repository and the arguments. Add it to the docs that list the presets in the
  same change.
- **A blind search becomes a deterministic preset.** Where a session hunted
  with `rg`, listings or reads for something that has a name (a definition, its
  callers, the test of a function, the file of a module), the next session asks
  a preset that answers from a syntax tree, a language server, the build tool or
  the docs. When it finds nothing, it names the next place to ask, so the
  follow-up is not a guess. Examples: definitions and callers through a
  language server (serena `find_symbol` and `find_referencing_symbols`,
  `gopls references`, tsserver, pyright), the tests that name a function
  through the syntax tree, the module that owns a path from a module map.
- **A failed call becomes a fix at its source:** a preset, a clearer refusal
  with the right form after `instead:`, a docs line where the next session
  looks. Only a mistake no harness change can prevent becomes a rule in the
  docs, with the exact error and what avoids it.
- **A discovery that took several rounds becomes a recipe in the docs:** the
  one call that does it, with its names and paths.
- **A fact that took a tool call goes into the docs:** which file, which test.
  When the fact is the output of a command, a generated block keeps it current
  (see [Docs](#docs)).
- **One call per round of reads,** under the output warning. A call whose
  output was saved to a file cost a call and delivered nothing.
- **The start context is paid in every session.** Keep the rules, the workflow
  and pointers to the rest in it; everything else is one read away. A hook that
  prints the start context is capped (Claude Code keeps about 10,000 characters
  of the output of a hook and saves the rest to a file). Past that cap,
  `CLAUDE.md` names what to read and the first call reads it.

## Context

Filters follow the rules in SKILL.md. Their fields run in a fixed order: colour
codes and carriage returns removed, then `replace`, `drop`, `keep`, `dedupe`,
`head` and `tail`, `width`, `on_empty`. For a test runner, for example:

```yaml
run:
  filters:
    cargo nextest:
      drop: ["^\\s*Starting \\d+ tests", "^\\s*(Compiling|Finished) ", "^\\s*$"]
      tests:
        - input: "   Compiling x v0.1.0\n    Summary [ 2.4s] 21 tests run: 21 passed\n"
          expected: "    Summary [ 2.4s] 21 tests run: 21 passed\n"
        - exit: 100
          input: "  FAIL [ 0.009s] x a_test\n    Summary [ 2.3s] 21 tests run: 20 passed, 1 failed\n"
          expected: "  FAIL [ 0.009s] x a_test\n    Summary [ 2.3s] 21 tests run: 20 passed, 1 failed\n"
    pytest:
      drop: ["^\\S+\\.py \\.+\\s*\\[\\s*\\d+%\\]$", "^=+ test session starts =+$"]
    typos: { on_empty: "no typos", tests: [{ input: "", expected: "no typos\n" }] }
    rg: { width: 400, on_empty: "no match" }
```

A read that comes back huge gets a narrower preset (a line range, a filtered
command) or a smaller docs section, never a blind `head`.

## Docs

The docs are where a session finds its way without searching. Every fact a
session had to dig up belongs there, one read away.

- **Generated facts stay current.** A folder map, a file list, the usage of a
  program, the outline of a module: keep them as the output of a command,
  written into the docs by a generator and checked in `ci`, so a stale block
  fails the build. The command must be deterministic. A command that names a
  package, a binary or a path goes stale when those are renamed, and `|| true`
  hides that: read the blocks again after a rename.
- **Split big sections.** A smaller section is a cheaper read. Split where a
  session needs one part and not the rest.
- **Prune what nobody reads.** A big section no session opens is dead weight,
  or a sign it is not linked from where sessions look (the start context, the
  code map, the recipes). A section every session opens belongs in the start
  context or should be smaller.

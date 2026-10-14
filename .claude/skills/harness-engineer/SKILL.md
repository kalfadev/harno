---
name: harness-engineer
description: Improve the harness of a project that works through harno (a .harno/config.yml at the root) while doing any task in it. Notice where the harness wastes time, model calls or context - a failed or refused call, a blind search, a --raw call, a slow build, test or check, an output too big, a fact that had to be dug up - and fix it in the project right then with a preset, a filter, a faster check or a line of docs. Priorities, in order - never prove less, finish sooner, use fewer calls, send less text to the model. Use it throughout any task in a harno project, and whenever the user asks to speed up the dev loop, cut calls or noise, tune presets, filters, checks, builds or tests, or review how the harness performs, even if they do not say "harness".
---

# Harness engineer

The harness is everything between the agent and the code: the presets and
filters in `.harno/config.yml`, the steps that prove a change (checks, tests,
builds), and the docs a session reads to find its way.

When a task is slow, takes ten calls where three would do, guesses, or reads
40 KB to learn one fact, the waste is in the harness. This skill finds that
waste and removes it, so the next task is faster. It is not a checklist: look
at what real tasks cost, fix what is worth fixing, measure, and move on.

## Priorities

In this order. When two conflict, the higher one wins.

1. **Never weaken proof.** Dropping a check or a test, a filter that hides a
   failure, a wait cut below what its test needs: none of these is an
   optimization. Speed bought with a wrong answer costs more later.
2. **Time to a finished task.** End to end, as the user waits: tool time, model
   time between calls, and every retry after a failed or blind call.
3. **Model calls.** Each call is a round trip. But never save a call by making
   the task slower.
4. **Context.** Text that reaches the model and carries nothing. Every later
   turn reads it again, and a big output hides the line that mattered.

Traps seen in practice:

- Folding the checks into the write step to save a call. A failed proof then
  costs a second write. Fewer calls come from better reads, presets and docs,
  never from proving less or later.
- Running every check to avoid deciding which ones apply: one call less,
  minutes slower. Run the checks of what changed.
- Skipping a checker that takes seconds, then paying for a failed test run
  that takes minutes.
- Chaining two slow steps with `--seq` when they could run side by side with
  `--par`.
- Trimming the start context so far that every task needs an extra read.
- Polishing characters while the log shows minutes lost to slow checks or
  failures.

## When to act

**While working, the moment you notice it.** This is the main mode. Fix a
harness problem when it shows up:

- a call failed or was refused because of a preset, a quoting rule or a doc
- a search guessed its way to the answer
- a `--raw` call or a scratchpad script did what a preset should do
- a step was much slower than it should be
- an output was too big to read
- a fact had to be dug up

Put the fix into the next change of the task, so it is proved and committed
with it. If no change is coming, or the task needs the fix now, make it a small
change of its own and carry on.

A fix that needs a design decision goes to the user as a question with options
and a recommendation; the task goes on meanwhile. A fix that costs more than it
saves over the next several tasks waits, noted for the user.

**When the user asks** for speed, fewer calls, less noise or a review of the
harness: run the review below, all of it.

The rules of the project (`CLAUDE.md`, its docs) come first. Make each change
the way the project makes any change.

## How to think

1. **Measure whole tasks.** Where did the minutes go: tool time, model turns,
   failures and their retries, rounds of searching? Read it off the log before
   changing anything. A guess is not a finding.
2. **Failures and blind calls are the most expensive calls.** Each costs its
   own time, a turn to read it and a retry, and they come in chains. Fix each
   at its source: a preset, a docs line, a clearer error.
3. **Do only the work the change needs.** Check what changed, not everything.
   Fast checkers (types, lint) before slow tests. Build only when something has
   to run.
4. **Never redo work.** Use caches that the tool keys on content it fully
   tracks. Never trust a remembered result that could be stale.
5. **Do independent work together.** Tests in parallel, independent reads in
   one call, as long as the joined call is not slower than its parts.
6. **Show the model only what informs.** Filter noise you are sure of, narrow
   reads that come back huge, and write facts down where one read finds them.
7. **Turn each lesson into the harness.** A failed call becomes a fix at its
   source. A blind search becomes a preset. A discovery that took several
   rounds becomes a recipe in the docs. A repeated `--raw` becomes a preset.
8. **Prove every change and keep the numbers.** Before and after, on the same
   machine.

`references/levers.md` lists concrete techniques, with examples in several
languages. Read it when a finding needs one, and pick what fits this project.

## Presets: a name in, a determined answer out

A preset lets the agent ask a named question and get the answer in one call.
Every preset this skill writes:

- **Takes a named input and answers from a source of truth.** The input is a
  symbol, a path, a test name or a docs heading; the answer comes from a syntax
  tree, a language server, the build tool or the docs. The same tree and the
  same arguments give the same output.
- **Does not hide a regex hunt behind a name.** A preset that greps the code
  for a definition is still guessing. Text search is for literal text only,
  such as an error message.
- **Says what to do next when the answer is empty.** "not found: X" names
  where it looked and which preset answers the nearby question. A wrong form
  prints the right one.
- **Answers the whole question in one call:** a definition with its doc
  comment, a caller with its line.
- **Has stable output:** sorted, no timestamps, no absolute paths that differ
  per machine, and small enough to stay under the output warning.

A blind search in the log is a finding even when it worked: `rg` for a name a
language server knows, a listing to see what exists, three reads to find one
function, a scratchpad script the next task will need again. Fix it with a
preset like the above, or a docs line that sends the next session straight to
the answer.

## The review

### 1. Collect the evidence in one round

Send independent reads together. `harno log` is a verb of its own, not an item
of `harno run`, so it is a separate tool call in the same message.

- **The timeline of recent tasks.** `.harno/log/harno/<session>.jsonl` has one
  line per call, with its start, duration, exit code and output size. Split
  each session into tasks (at each commit, or where the user spoke) and list
  per task: minutes, calls, failed calls, tool seconds by preset, and model
  seconds (the gaps between calls). A read-only scratchpad script does this;
  after the first review, make it a preset. This is the first table of the
  review.
- `harno log stats --all`: calls, failures and characters per session, with
  median and p90.
- `harno log analytics --all`: commands ranked by characters shown, the ones
  without a filter first.
- `harno log fails --all`: the commands that failed and the calls that were
  refused, most frequent first.
- For every failed call: why it failed (`harno log raw <n> --session <id>`) and
  what the retry cost.
- Blind searches: reads and listings that did not lead to the answer.
- Timings of the slow steps: the build, the tests (slowest first), the checks.
- Your own transcript: failures, refusals, outputs saved to a file for being
  too big, `--raw` calls, repeated reads.

`references/evidence.md` explains each source and which pattern points to
which fix.

### 2. Pick

Rank the findings by the time they cost per task, then calls, then characters.
Take every certain finding whose saving over the next several tasks beats its
cost; a review rarely ends with one fix. Send design decisions to the user.
Stop when what is left is a decision (asked), unproven (noted), or costs more
than it saves (noted).

### 3. Change

One change with every edit and the docs that describe it: the table of
presets, a fix at its source for each failed call, a recipe, a status number, and
the decision of the user with its reason.

### 4. Prove

- A new preset is run on a name it must find and on one it must not, and both
  answers are the expected ones. Its script gets a test when the project tests
  its scripts.
- A filter has `tests:` with a passing and a failing input, and
  `harno tool test` passes.
- A change to the proving steps is tried twice: a harmless edit runs the right
  checks, and a deliberate error stops before the commit. Both are undone.
- A speed change is measured before and after on the same machine, warm. The
  numbers go into the commit message.
- A change aimed at calls says how many calls a typical task takes before and
  after, counted from the log, not guessed.

### 5. Report

To the user, in their language, short: a before and after table (minutes and
calls per task first, then characters), what changed, what was looked at and
left alone and why, what is not proven, and open questions with options and a
recommendation.

## When a filter is safe

A filter decides what the model will not see unless it asks again. A hidden
failure costs a wrong conclusion and the calls to find it. Filter only when all
of these hold:

- The dropped lines match a pattern seen in several raw outputs
  (`harno log raw <n>`), and never carry an error, a failure, a panic, a useful
  warning, a file:line or a summary.
- A failing run keeps its report: `on_fail` keeps more, or the patterns cannot
  match failure text.
- The filter has `tests:` with a passing and a failing input.
- It cuts the noise of a command, never a read the agent asked for (a file, a
  search result).

Prefer `drop` of a known noise pattern over `head` and `tail`, which can cut the
one line that mattered. When in doubt, leave it unfiltered and note it.

## Determinism

The output of a preset depends only on the repository and its arguments: never
on the time, the order of a listing, or a file an earlier call left behind.

- Read what changed from git (`git diff --name-only HEAD`,
  `git ls-files --others --exclude-standard`), not from a stamp of the last run.
- A cache is fine when the tool keys it on content it fully tracks (incremental
  compilation, a content-keyed lint cache, ccache, sccache). A remembered
  result that depends on the network, the clock or untracked files is not.
- A value passed from one call to the next is an argument, not a file.
- A generator that lists tracked files runs after `git add -A`, or a new file
  shows up one commit late.

## Guardrails

- The user decides design points. Never widen a task on your own.
- Keep a preset that sessions still run (`harno log analytics --all`) or that a
  doc still names.
- `--raw` is never the way around a missing preset.
- A shortened wait must hold under load. A test that passes only on retry after
  your change is a regression.
- A review that only trimmed characters, while the log shows minutes or calls
  lost elsewhere, is not finished.
- Match the repository: its tools, versions, comment style, line length and
  docs language.

## Sources

The installed `harno` binary is the truth: `harno help --verbose` prints the
whole manual.

# Reading the evidence

Where to look in the log, and which fix each pattern points to.

## The timeline of a task

`.harno/log/harno/<session>.jsonl` has one JSON line per call: `n`, `ts` (the
start, UTC), `ms` (tool time), `verb`, `args`, `cmd`, `exit`, `output` (what the
commands wrote) and `filtered` (what the agent was shown). With the Claude Code
hooks, `.harno/log/claude-code/<session>.jsonl` holds the prompts and answers
of the same session.

A read-only scratchpad script turns these lines into a table per task. After
the first review, make it a preset. The table shows what no other view does:

- **Minutes per task.** Split a session at each commit, or where the user
  spoke, and take the first and last `ts`.
- **Tool time against model time.** The sum of `ms` is the time the tools took;
  the rest of the span is the model. Many short calls with long gaps mean too
  many calls. Few calls with long `ms` mean slow steps.
- **Tool seconds by preset.** Which step eats the time across tasks: a check
  run five times in one task, a build nobody needed.
- **Failure chains.** A call with a non-zero `exit` and the calls after it until
  the same step passed. Their count and seconds are what the failure cost.
- **Blind searches.** Runs of `rg`, listings and reads before the first write of
  a task, most of them not followed by a read of what they found. Their count
  per task is the discovery the harness left to guessing.

Compare tasks of the same kind (a bug fix, a docs change, a feature) before and
after a change. One task is an anecdote.

## harno log ls

One line per call: `n`, time, `took`, `shown`, `result`, the call. Look for:

| pattern | what to do |
|---|---|
| `took` over a few seconds on a check or test step | speed: scope it to what changed, cache, split, run in parallel |
| `3.5k of 39.3k` on a command read many times | context: is the cut right, is a narrower preset missing |
| `exit N` | calls: why it failed; a fix at the source (preset, refusal, doc) |
| `refused` | calls: a missing preset or `run.allow` entry, or a wrong form |
| the same read twice | calls: a docs line, or a recipe |
| several reads in a row that did not depend on each other | calls: they belonged in one `--par` |
| `--raw` | calls: a preset that should exist; the `--why` says what for |
| a scratchpad script written again in a later session | calls: a preset that should exist |
| a search whose result was not used, a pattern tried twice | a blind search: a deterministic preset, or a docs line that points to the answer |
| a long gap between two quick calls | time: the model read a big output, or planned what a doc could have said |

`harno log raw <n>` shows the whole output of call `n` when a filter cut it.

## harno log stats [--all]

Calls, failures, characters returned with median and p90, and what the filters
cut, per session. A rising p90 means reads are getting wider. Many failures
mean a rule or a preset is misleading.

## harno log analytics [--all]

Commands ranked by characters shown to the model:

1. Commands without a filter of their own. The first line is the next filter or
   preset to write, if its output is noise.
2. Commands with a filter, with `kept`: the share of what the command wrote
   that reached the model. A low `kept` on a command that often fails needs a
   look: is the failure still visible? A high `kept` on a big command is a
   candidate for a filter.

Across `--all` sessions it also lists presets no session ran: candidates to
remove, once no doc names them.

## harno log fails [--all]

The commands that failed, by name and exit code, most frequent first, with the
last line of the latest failure; then the refused calls. The top of each list
is the first fix to make.

## The transcript

What the log cannot show: an output saved to a file because it was too big (it
never reached the model and cost a call), a wrong guess, a read that answered
nothing, a question to the user that the docs could have answered. Each one is
a finding.

## Measuring time

`took` in `harno log ls` is per call, and `--seq` prints the time of each item.
For a before and after, run the same step twice on the same machine with a warm
cache and quote both numbers. A cold build is measured once and called cold.

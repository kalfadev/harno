#!/usr/bin/env node
// Cases for buildRows, the analytics page logic that puts the calls of the gate under the
// words of the session that made them: which tool call claims a call, a call no tool call
// ran, a prompt that reaches the agent after the answer. Runs the page code, cut out of
// page.html.
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../assets/page.html", import.meta.url), "utf8");
const helpers = html.split("\n").filter(l => /^const (toDate|time|SQ|quote|isTool|isRaw|brokeOf|failedRow|noTurn) = /.test(l)).join("\n");
const start = html.indexOf("// ── The session: the words"), end = html.indexOf("function turnText");
if (start < 0 || end < start) { console.log("failed: page.html has no session section"); process.exit(1); }
const buildRows = new Function(helpers + "\n" + html.slice(start, end) + "\nreturn buildRows;")();
const failedRow = new Function(helpers + "\nreturn failedRow;")();

const T = s => new Date(Date.UTC(2026, 0, 1) + s * 1000).toISOString();
const call = (n, t, verb, args, extra = {}) => ({ n, ts: T(t), ms: 100, verb, args, cmd: ["x"], exit: [0], output: 10, filtered: 10, ...extra });
const calls = [
  call(1, 0.1, "run", ["mdmap", "ls"]),
  call(2, 5, "run", ["test"]),
  call(3, 10, "run", ["--seq", "x"], { exit: [1] }),
  call(4, 15, "run", ["curl", "https://x"], { refused: "run.allow does not list curl", exit: null }),
  call(5, 45, "run", ["build"]),
];
const events = [
  { n: 1, type: "prompt", ts: T(0), text: "first" },
  { n: 2, type: "tool", ts: T(6), tool: "Bash", summary: "harno run test", turn: "mA", harno: [2], said: "Trying." },
  { n: 3, type: "tool", ts: T(11), tool: "Bash", ok: false, summary: "harno run --seq x", turn: "mB", harno: [3] },
  { n: 4, type: "tool", ts: T(16), tool: "Read", summary: "a.md", turn: "mB", chars: 12 },
  { n: 5, type: "response", ts: T(20), turns: 3, text: "done" },
  { n: 6, type: "prompt", ts: T(30), text: "second" },
  { n: 7, type: "tool", ts: T(36), tool: "Bash", summary: "cd docs && harno run mdmap ls", turn: "mD", harno: [] },
  { n: 8, type: "prompt", ts: T(38), text: "typed while the answer was written" },
  { n: 9, type: "response", ts: T(40), turns: 2, text: "done" },
];
const rows = buildRows(events, calls);
const c = n => rows.find(r => r.kind === "call" && r.ev.n === n) || { ev: {} };
const e = n => rows.find(r => r.kind !== "call" && r.kind !== "note" && r.ev.n === n) || { ev: {} };
const checks = [
  ["a tool call claims the calls it made", c(2).via && c(2).via.n === 2 && c(2).tn === 1],
  ["a Bash call that is only harno is not a row of its own", !rows.some(r => r.kind === "tool" && r.ev.n === 2)],
  ["a failed Bash call keeps the call it made", c(3).via && c(3).via.n === 3 && failedRow(c(3))],
  ["a call no tool call ran stands on its own", c(1).tk === "loose" && !c(1).via],
  ["a refused call is a failed one", failedRow(c(4)) && c(4).ev.refused],
  ["an exit its filter calls ok is no failure", !failedRow({ kind: "call", ev: { exit: [1], failed: [false] } })],
  ["a Bash call that runs harno late stays a row", e(7).warn === true],
  ["what the agent wrote in a turn becomes a note", rows.some(r => r.kind === "note" && r.ev.n === 2)],
  ["a prompt typed during the answer goes after it", rows.indexOf(e(8)) > rows.indexOf(e(9))],
  ["a prompt counts the turns its answer logged", e(1).stats && e(1).stats.api === 3],
  ["a call after the last event comes last", rows[rows.length - 1].ev.n === 5 && rows[rows.length - 1].kind === "call"],
];
for (const [name, ok] of checks) if (!ok) console.log("failed:", name);
const good = checks.filter(x => x[1]).length;
console.log(`${good}/${checks.length} ok`);
process.exit(good === checks.length ? 0 : 1);

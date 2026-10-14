#!/usr/bin/env node
// Cases for keptLines, the analytics page logic that says which lines of a raw output
// reached the agent. Runs the page code, cut out of page.html.
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../assets/page.html", import.meta.url), "utf8");
const start = html.indexOf("// ── What the agent read, and what the command wrote ──"), end = html.indexOf("function markKept");
if (start < 0 || end < start) { console.log("failed: page.html has no raw output section"); process.exit(1); }
const keptLines = new Function(html.slice(start, end) + "\nreturn keptLines;")();

const raw = "one\ntwo\nthree\n\nfour";
const out = "one\nthree\n… 2 lines: harno log raw 5\n";
const k = keptLines(raw, out);
const e = String.fromCharCode(27);
const colour = keptLines(e + "[32m" + "one" + e + "[0m  ", "one");
const checks = [
  ["a line of the output is kept", k.marks[0] === true && k.marks[2] === true],
  ["a line the filter dropped is marked", k.marks[1] === false && k.marks[4] === false],
  ["an empty line counts for neither", k.marks[3] === null && k.total === 4],
  ["the counts say how much reached the agent", k.kept === 2],
  ["colour codes and trailing spaces do not hide a line", colour.kept === 1],
];
for (const [name, ok] of checks) if (!ok) console.log("failed:", name);
console.log(`${checks.filter(c => c[1]).length}/${checks.length} ok`);
process.exit(checks.every(c => c[1]) ? 0 : 1);

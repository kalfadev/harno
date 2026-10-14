#!/usr/bin/env node
// The analytics page in a real browser, in one call: `harno run page "<js expression>"` for
// the session log of `harno run analytics`. Starts its own server in the foreground on a free
// port and headless Chrome with a fresh profile (a first load, nothing cached), waits for the
// page, prints what the expression returns (JSON unless a string; it may be async), and stops
// both. The page keeps its state in top-level `S`, the session rows.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import net from "node:net";

const expr = process.argv.slice(2).join(" ") || "document.title";
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const sleep = ms => new Promise(r => setTimeout(r, ms));
const free = () => new Promise(r => { const s = net.createServer().listen(0, () => { const p = s.address().port; s.close(() => r(p)); }); });
const port = await free(), debug = await free();
const profile = mkdtempSync(join(tmpdir(), "harno-page-"));
const serve = spawn(".harno/tools/analytics/bin/analytics", ["--no-open", "--port", String(port)], { stdio: "ignore" });
await sleep(300);
const chrome = spawn(CHROME, ["--headless=new", "--window-size=1600,1000", `--remote-debugging-port=${debug}`, `--user-data-dir=${profile}`, `http://127.0.0.1:${port}/`], { stdio: "ignore" });
let code = 1;
try {
  let page;
  for (let i = 0; i < 50 && !page; i++) {
    await sleep(200);
    try { page = (await (await fetch(`http://127.0.0.1:${debug}/json`)).json()).find(t => t.type === "page"); } catch { /* not up yet */ }
  }
  if (!page) throw new Error("Chrome did not open the page");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
  let id = 0;
  const call = (method, params) => new Promise(r => {
    const n = ++id;
    ws.addEventListener("message", function on(m) { const d = JSON.parse(m.data); if (d.id === n) { ws.removeEventListener("message", on); r(d.result); } });
    ws.send(JSON.stringify({ id: n, method, params }));
  });
  const ready = "typeof S !== \"undefined\" && !!S.rows";
  for (let i = 0; i < 75; i++) {
    const r = await call("Runtime.evaluate", { expression: ready, returnByValue: true });
    if (r.result && r.result.value) break;
    await sleep(200);
  }
  const r = await call("Runtime.evaluate", { expression: `(async () => (${expr}))()`, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) console.log("error:", r.exceptionDetails.exception ? r.exceptionDetails.exception.description : r.exceptionDetails.text);
  else { const v = r.result.value; console.log(typeof v === "string" ? v : JSON.stringify(v)); code = 0; }
  ws.close();
} catch (e) {
  console.log("error:", e.message);
} finally {
  chrome.kill();
  serve.kill();
  await sleep(200);
  rmSync(profile, { recursive: true, force: true });
}
process.exit(code);

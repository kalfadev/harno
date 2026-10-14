#!/usr/bin/env node
// The page is one script tag, and a syntax error anywhere in it leaves the browser with
// the static HTML: no session list, no timeline, nothing in the console but the page.
// The other tests here run pieces cut out of the page, so none of them sees that.
// This one parses the whole script, and checks the marker the server fills with the log.
import { readFileSync } from "node:fs";
import vm from "node:vm";

const html = readFileSync(new URL("../assets/page.html", import.meta.url), "utf8");
let ok = 0, failed = 0;
const check = (name, run) => {
  try {
    run();
    ok += 1;
  } catch (e) {
    failed += 1;
    console.log("failed: " + name + ": " + e.message);
  }
};

check("the page carries one script", () => {
  const tags = html.match(/<script[^>]*>/g) || [];
  if (tags.length !== 1) throw new Error(tags.length + " script tags, not one");
});

check("the script parses", () => {
  const script = html.split(/<script[^>]*>/)[1].split("</script>")[0];
  new vm.Script(script, { filename: "page.html" });
});

check("the server has its marker to fill", () => {
  if (!html.includes("/*HARNO_DATA*/null")) throw new Error("the page holds no /*HARNO_DATA*/null for serve.rs");
});

console.log(ok + "/" + (ok + failed) + " ok");
process.exit(failed ? 1 : 0);

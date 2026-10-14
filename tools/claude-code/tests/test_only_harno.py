#!/usr/bin/env python3
"""Cases for only-harno.py: what must pass, what must be refused."""
import importlib.util
import json
import pathlib
import subprocess
import sys

HOOK = str(pathlib.Path(__file__).parent.parent / "only-harno.py")

spec = importlib.util.spec_from_file_location("g", pathlib.Path(__file__).parent.parent / "only-harno.py")
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)

PASS = [
    "harno ls",
    "harno ls && harno show a1",
    "harno run test 2>&1",
    "harno ls >/dev/null", "harno ls || harno lint", "harno ls & harno lint", "har\"no\" ls",
    "harno ls\nharno lint",
    "harno run write -- f 'fn a() -> bool {\n    a || b; c && d; $(x) `y` > z <<E\n}'",
    "harno run replace -- f 'it'\\''s' 'a | b'",
]
REFUSE = [
    "harno ls; cat x", "harno ls && cat x", "harno ls | head", "harno ls\ncat x", "harno ls & cat x",
    "cd docs && harno ls", "harno show \"$(cat secret)\"", "harno show `id`", "harno ls > out.txt",
    "(harno ls)", "ls", "harno show $HOME", "if true; then harno ls; fi", "harno ls <<<x", "{ harno ls; }", "harno ls 2>err", "FOO=1 harno ls", "cat <(harno ls)",
    "harno run write -- f \"$(cat <<'X'\nbody\nX\n)\"", "harno ls <<X\nhi\nX",
    "harno show \"`id`\"", "harno show \"\\\"$(id)\"", "harno show 'a'$(id)",
]
bad = [("should pass", c, g.why_not(c)) for c in PASS if g.why_not(c) is not None]
bad += [("should refuse", c, None) for c in REFUSE if g.why_not(c) is None]
# A hook that fails open lets the call through: input it cannot read exits 2 too.
broken = subprocess.run([sys.executable, HOOK], input="{", capture_output=True, text=True)
bad += [] if broken.returncode == 2 else [("should refuse", "hook input that is no JSON", broken.returncode)]
# A harno line is allowed by the hook itself, so Claude Code asks nothing.
ok = subprocess.run([sys.executable, HOOK], input=json.dumps({"tool_input": {"command": "harno ls"}}), capture_output=True, text=True)
decision = json.loads(ok.stdout or "{}").get("hookSpecificOutput", {}).get("permissionDecision")
bad += [] if ok.returncode == 0 and decision == "allow" else [("should allow without asking", "harno ls", (ok.returncode, ok.stdout))]
for b in bad:
    print(*b, sep=" | ")
print(f"{len(PASS) + len(REFUSE) + 2 - len(bad)}/{len(PASS) + len(REFUSE) + 2} ok")
sys.exit(1 if bad else 0)

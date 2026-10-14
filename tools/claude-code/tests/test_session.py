#!/usr/bin/env python3
"""Cases for session.py: the words of a session in short lines."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).parent.parent
TMP = pathlib.Path(tempfile.mkdtemp())
checks = []


def check(name, ok):
    checks.append((name, ok))


project = TMP / "p"
log = project / ".harno/log/claude-code"
(log / "s1").mkdir(parents=True)
(log / "s1.jsonl").write_text("\n".join([
    json.dumps({"n": 1, "ts": "2026-01-01T10:00:00.000Z", "type": "prompt", "chars": 5}),
    json.dumps({"n": 2, "ts": "2026-01-01T10:00:01.000Z", "type": "tool", "tool": "Bash", "summary": "harno run ci", "harno": [3, 4]}),
    json.dumps({"n": 3, "ts": "2026-01-01T10:00:02.000Z", "type": "tool", "tool": "Read", "summary": "a.md", "chars": 12, "ok": False}),
    json.dumps({"n": 4, "ts": "2026-01-01T10:00:03.000Z", "type": "response", "chars": 7, "turns": 2}),
]) + "\n")
(log / "s1" / "0001.out").write_text("do it")


def run(*args, session="s1"):
    env = dict(os.environ, HARNO_SESSION=session)
    return subprocess.run([sys.executable, str(HERE / "session.py"), *args], cwd=str(project), capture_output=True, text=True, env=env)


r = run()
lines = r.stdout.splitlines()
check("the session first, then one line per event", r.returncode == 0 and lines[0] == "session s1" and len(lines) == 5)
check("a prompt shows its size and its text", lines[1] == "1  prompt  10:00:00.000  5 chars  do it")
check("a call through the gate names the lines it claims", lines[2].endswith("Bash  harno 3,4  harno run ci"))
check("a failed tool says so", lines[3].endswith("Read FAIL  12 chars  a.md"))
check("the answer counts its turns", lines[4].endswith("turns 2  7 chars"))
check("a range takes the events it names", run("2", "2").stdout.splitlines()[1:] == lines[2:3])
r = run(session="nosuch")
check("without a log of that session it says so", r.returncode != 0 and "no session log" in r.stdout + r.stderr)

for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

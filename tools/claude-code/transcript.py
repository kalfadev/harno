#!/usr/bin/env python3
"""The Claude Code transcript in short lines: `harno run transcript -- [<from hh:mm[:ss]> [<to>]]`,
times in UTC as in the log. The session is $HARNO_SESSION, else the newest transcript of this
project. Prompts, queued prompts, assistant text and calls, failed results, stop hooks."""
import json
import os
import pathlib
import sys

d = pathlib.Path.home() / ".claude" / "projects" / str(pathlib.Path.cwd()).replace("/", "-")
sid = os.environ.get("HARNO_SESSION")
path = d / f"{sid}.jsonl" if sid else max(d.glob("*.jsonl"), key=lambda p: p.stat().st_mtime, default=None)
if not path or not path.exists():
    print("no transcript in", d)
    sys.exit(1)
lo = sys.argv[1] if len(sys.argv) > 1 else ""
hi = sys.argv[2] if len(sys.argv) > 2 else "~"


def cut(s, n):
    return " ".join(str(s).split())[:n]


print("transcript", path.name)
for raw in path.open(encoding="utf-8"):
    try:
        v = json.loads(raw)
    except ValueError:
        continue
    t = v.get("timestamp", "")[11:23]
    if not t or not (lo <= t <= hi) or v.get("isSidechain"):
        continue
    kind, m = v.get("type"), v.get("message") or {}
    c = m.get("content")
    if kind == "queue-operation":
        print(t, "queue", v.get("operation"), cut(v.get("content", ""), 60))
    elif kind == "system":
        print(t, "system", v.get("subtype", ""))
    elif kind == "user" and isinstance(c, str):
        print(t, "PROMPT", cut(c, 80))
    elif kind in ("user", "assistant") and isinstance(c, list):
        for b in c:
            bt = b.get("type") if isinstance(b, dict) else None
            if bt == "text":
                print(t, "assistant " + m.get("id", "")[-6:] + " text" if kind == "assistant" else "PROMPT", cut(b.get("text"), 70))
            elif bt == "tool_use":
                print(t, "assistant", m.get("id", "")[-6:], "call", cut((b.get("input") or {}).get("command") or b.get("name"), 70))
            elif bt == "tool_result" and b.get("is_error"):
                print(t, "result ERROR", cut(b.get("content"), 70))
            elif bt == "thinking":
                print(t, "assistant", m.get("id", "")[-6:], "thinking")

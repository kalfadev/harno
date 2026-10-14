#!/usr/bin/env python3
"""The words of a session in short lines: `harno run session -- [<from n> [<to n>]]`.

The session is $HARNO_SESSION, else the newest log in `.harno/log/claude-code/`.
A tool call that went through harno keeps no text of its own: it names the
lines of the gate log it claims, which `harno log ls` prints."""
import json
import os
import pathlib
import sys

LOG = ".harno/log/claude-code"


def cut(s, n):
    s = " ".join(str(s or "").split())
    return s if len(s) <= n else s[: n - 1] + "…"


def read(path):
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        return ""


def newest(directory):
    files = list(directory.glob("*.jsonl"))
    return max(files, key=lambda p: p.stat().st_mtime).stem if files else None


def parts(e, text):
    """What one event holds, in the order it reads."""
    kind = e.get("type")
    if kind == "session":
        return [e.get("event") or "", cut(e.get("source") or e.get("reason"), 40)]
    if kind == "prompt":
        return ["%s chars" % (e.get("chars") or 0), cut(text, 70)]
    if kind == "response":
        return ["turns %s" % (e.get("turns") or "?"), "%s chars" % (e.get("chars") or 0), cut(text, 50)]
    name = e.get("tool") or e.get("agent") or "?"
    if e.get("ok") is False:
        name += " FAIL"
    came = "harno %s" % ",".join(str(n) for n in e["harno"]) if e.get("harno") else "%s chars" % (e.get("chars") or 0)
    return [name, came, cut(e.get("summary"), 60)]


def main(argv):
    directory = pathlib.Path.cwd() / LOG
    numbers = [int(a) for a in argv if a.isdigit()]
    first = numbers[0] if numbers else 0
    last = numbers[1] if len(numbers) > 1 else 10 ** 9
    session = os.environ.get("HARNO_SESSION") or newest(directory)
    if not session or not (directory / ("%s.jsonl" % session)).is_file():
        sys.exit("no session log in %s" % directory)
    print("session", session)
    for raw in (directory / ("%s.jsonl" % session)).read_text(encoding="utf-8").splitlines():
        try:
            e = json.loads(raw)
        except ValueError:
            continue
        n = e.get("n") or 0
        if n < first or n > last:
            continue
        text = read(directory / session / ("%04d.out" % n))
        line = [str(n), e.get("type") or "?", (e.get("ts") or "")[11:23]] + parts(e, text)
        print("  ".join(p for p in line if p))


if __name__ == "__main__":
    main(sys.argv[1:])

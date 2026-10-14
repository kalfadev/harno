#!/usr/bin/env python3
"""Change a file without a shell.

    edit.py replace <path> <old> <new>    <old> must occur exactly once
    edit.py lines <path> <a>-<b> <text>   lines a..b become <text>; empty text deletes them
    edit.py write <path> <text>           create or overwrite the file
    edit.py doc <id> <old> <new>          <old> in the body of doc node <id> becomes <new>

Prefer `replace` with the old text copied from a read: a new line is a
replace of the line it goes next to. `lines` is for a long block; several
in one file go from the bottom up, so the numbers above stay true, and each
prints the lines it took out. Markdown under docs/ is refused: `doc` changes
it through mdmap update, with the lines of the old text found when it runs,
so an edit before it moves nothing. A text the shell split into more
arguments than its mode takes is refused before anything is written.
"""
import json
import os
import re
import subprocess
import sys

MODES = {"replace": 3, "lines": 3, "write": 2, "doc": 3}
# A project that keeps its own mdmap in a tool folder; else the one on PATH.
OWN_MDMAP = ".harno/tools/mdmap/bin/mdmap"


def refuse(msg):
    sys.exit(f"refused: {msg}")


def usage():
    sys.exit(__doc__.strip())


def mdmap():
    return OWN_MDMAP if os.path.isfile(OWN_MDMAP) else "mdmap"


def node_span(node):
    """The file of a doc node, its lines, and the first and last line of its body."""
    r = subprocess.run([mdmap(), "show", node, "--info", "--json"], capture_output=True, text=True)
    try:
        # mdmap may open its answer with a warning line, the size of a node past warn.body
        path, head = json.loads(r.stdout[r.stdout.find("{"):])["nodes"][0]["path"].rsplit(":", 1)
        head = int(head)
    except (ValueError, KeyError, IndexError):
        raise ValueError("no doc node %s: %s" % (node, (r.stderr or r.stdout).strip()[:120]))
    rows = open(path).read().split("\n")
    last, fence = len(rows), False
    for i in range(head, len(rows)):
        if rows[i].lstrip().startswith("```"):
            fence = not fence
        elif not fence and re.match(r"#{1,6} ", rows[i]):
            last = i
            break
    return path, rows, head + 1, last


def doc_lines(node, old, new):
    """The lines a-b of the body of a doc node that hold the old text, and what they
    read with the new text in its place; None when the new text is there already."""
    if not old:
        raise ValueError("the old text is empty")
    path, rows, first, last = node_span(node)
    body = "\n".join(rows[first - 1:last])
    if new and new != old and new in body and (old not in body or old in new):
        return None
    n = body.count(old)
    if n != 1:
        raise ValueError(f"the old text occurs {n} times in the body of {node}, {path}:{first}-{last}")
    start = body.index(old)
    end = start + len(old)
    a = first + body[:start].count("\n")
    left = body.rfind("\n", 0, start) + 1
    right = body.find("\n", end)
    right = len(body) if right < 0 else right
    return a, a + old.count("\n"), body[left:start] + new + body[end:right]


def doc(node, old, new):
    """The text in the body of a doc node changed through mdmap update; the exit code."""
    try:
        at = doc_lines(node, old, new)
    except ValueError as e:
        print(f"refused: {e}", file=sys.stderr)
        return 1
    if at is None:
        print(f"{node}: already applied")
        return 0
    a, b, text = at
    return subprocess.run([mdmap(), "update", node, "--lines", f"{a}-{b}", "--body", text]).returncode


def main(argv):
    if not argv or argv[0] not in MODES or len(argv) - 1 < MODES[argv[0]]:
        usage()
    mode = argv[0]
    if len(argv) - 1 > MODES[mode]:
        refuse(f"{mode} takes {MODES[mode]} arguments and the shell gave {len(argv) - 1}: a single quote inside a text "
               "ends the string around it, write each as quote, backslash, quote, quote; nothing was written")
    if mode == "doc":
        sys.exit(doc(*argv[1:]))
    path = argv[1]
    if os.path.normpath(path).startswith("docs" + os.sep) and path.endswith(".md"):
        refuse("markdown under docs/ is written with doc or harno run mdmap add / update / remove")
    if mode == "write":
        text = argv[2] if argv[2].endswith("\n") else argv[2] + "\n"
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        existed = os.path.exists(path)
        with open(path, "w") as f:
            f.write(text)
        print(("overwrote" if existed else "created") + f" {path}: {text.count(chr(10))} lines")
        return
    text = open(path).read()
    if mode == "lines":
        try:
            a, b = (int(n) for n in argv[2].split("-"))
        except ValueError:
            refuse(f"{argv[2]!r} is not a line range a-b")
        lines = text.split("\n")
        if text.endswith("\n"):
            lines.pop()
        if not 1 <= a <= b <= len(lines):
            refuse(f"lines {a}-{b} outside {path} (1-{len(lines)})")
        new = argv[3].split("\n") if argv[3] else []
        if new and new[-1] == "":
            new.pop()
        old = lines[a - 1:b]
        lines[a - 1:b] = new
        with open(path, "w") as f:
            f.write("\n".join(lines) + "\n")
        print(f"{path}:{a}-{b} replaced ({b - a + 1} lines -> {len(new)})")
        # What went out, so a wrong line number shows in this output, not in the build.
        for line in old[:3]:
            print(f"  was: {line}")
        if len(old) > 3:
            print(f"  was: … {len(old) - 3} more")
        return
    old, new = argv[2], argv[3]
    if not old:
        refuse("<old> is empty")
    count = text.count(old)
    if count != 1:
        at = [str(text[:i].count("\n") + 1) for i in _all(text, old)]
        refuse(f"<old> occurs {count} times in {path}" + (" (lines " + ", ".join(at) + ")" if at else ""))
    i = text.index(old)
    with open(path, "w") as f:
        f.write(text.replace(old, new, 1))
    first = text[:i].count("\n") + 1
    print(f"{path}:{first}-{first + new.count(chr(10))} replaced ({old.count(chr(10)) + 1} lines -> {new.count(chr(10)) + 1})")


def _all(text, old):
    i = text.find(old)
    while i != -1:
        yield i
        i = text.find(old, i + 1)


if __name__ == "__main__":
    main(sys.argv[1:])

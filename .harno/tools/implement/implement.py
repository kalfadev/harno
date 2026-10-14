#!/usr/bin/env python3
"""A whole change in one argument: `harno run implement -- <plan>` applies the
plan, then runs test, clippy, install, refresh, ci and, when the plan asks, the
commit. A plan is sections, each opened by a line that starts with `@@ `:

    @@ replace <path>        the old text, a line `@@ with`, the new text
    @@ lines <path> <a>-<b>  the new text of lines a..b (several: bottom up)
    @@ write <path>          the whole file
    @@ doc <id>              the old text in the body of doc node <id>, a line `@@ with`,
                             the new text; its lines are found when it runs (edit.py doc)
    @@ harno run mdmap <verb> <args>  a harno run mdmap update, add, remove or move; the
                             text under it, if any, is its last argument
    @@ commit                the message; the commit runs after ci passes

The file sections are checked and applied in memory first: one that does not
fit writes nothing. A section already applied is skipped (a replace whose new
text is there and old text is not, lines that already read as the new text),
so after a failure the fixed plan is sent again whole. The harno sections run
after the files, in order, and are not skipped: when one fails, the output
names those that ran. The header is split like a shell line (quote a title
with spaces); trailing blank lines of a section are dropped. Markdown under
docs/ is written only through doc and harno sections; another file there is a
file section like any other. A plan the shell split into several arguments, at
a single quote inside it, is refused before anything is written.

    implement.py --commit    commit with the message the last plan left
"""
import os
import pathlib
import shlex
import subprocess
import sys

MSG = pathlib.Path("target/implement-commit.txt")
WITH = "@@ with"


class Bad(Exception):
    pass


def sections(plan):
    out, head, body = [], None, []
    for line in plan.split("\n"):
        if line.startswith("@@ ") and line != WITH:
            if head is not None:
                out.append((head, "\n".join(body).rstrip("\n")))
            head, body = line[3:].strip(), []
        elif head is not None:
            body.append(line)
        elif line.strip():
            raise Bad("a plan starts with a line @@ <op>")
    if head is not None:
        out.append((head, "\n".join(body).rstrip("\n")))
    return out


def no_docs(path):
    if os.path.normpath(path).startswith("docs" + os.sep) and path.endswith(".md"):
        raise Bad("markdown under docs/ is written with a doc or harno section")


def edit_tool():
    """edit.py of the edit tool beside this one, which finds and changes a text of a doc node."""
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "edit"))
    try:
        import edit
    except ImportError:
        raise Bad("a doc section needs the edit tool: harno tool add edit")
    return edit


def load(files, path):
    no_docs(path)
    if path not in files:
        if not os.path.isfile(path):
            raise Bad(f"{path} does not exist")
        files[path] = pathlib.Path(path).read_text()
    return files[path]


def replace(files, path, old, new):
    text = load(files, path)
    if not old:
        raise Bad("the old text is empty")
    if new and new != old and new in text and (old not in text or old in new):
        return f"{path}: already applied"
    n = text.count(old)
    if n != 1:
        raise Bad(f"the old text occurs {n} times in {path}")
    first = text[:text.index(old)].count("\n") + 1
    files[path] = text.replace(old, new, 1)
    return f"{path}:{first} replaced ({old.count(chr(10)) + 1} lines -> {new.count(chr(10)) + 1})"


def lines(files, path, span, body):
    text = load(files, path)
    try:
        a, b = (int(x) for x in span.split("-"))
    except ValueError:
        raise Bad(f"{span} is not a line range a-b")
    rows = text.split("\n")
    if text.endswith("\n"):
        rows.pop()
    if not 1 <= a <= b <= len(rows):
        raise Bad(f"lines {a}-{b} outside {path} (1-{len(rows)})")
    new = body.split("\n") if body else []
    if rows[a - 1:b] == new:
        return f"{path}:{a}-{b}: already applied"
    was = rows[a - 1]
    rows[a - 1:b] = new
    files[path] = "\n".join(rows) + "\n"
    return f"{path}:{a}-{b} replaced ({b - a + 1} lines -> {len(new)}), was: {was[:60]}"


def write(files, path, body):
    no_docs(path)
    files[path] = body + "\n"
    return f"{path}: written, {body.count(chr(10)) + 1} lines"


def check(plan):
    """The file sections applied in memory, and the harno sections for later."""
    files, later = {}, []
    for i, (head, body) in enumerate(plan, 1):
        try:
            w = shlex.split(head)
            op = w[0] if w else ""
            if op == "replace" and len(w) == 2:
                part = body.split("\n")
                if WITH not in part:
                    raise Bad(f"no line {WITH}")
                k = part.index(WITH)
                print(i, replace(files, w[1], "\n".join(part[:k]), "\n".join(part[k + 1:])))
            elif op == "lines" and len(w) == 3:
                print(i, lines(files, w[1], w[2], body))
            elif op == "write" and len(w) == 2:
                print(i, write(files, w[1], body))
            elif op == "doc" and len(w) == 2:
                part = body.split("\n")
                if WITH not in part:
                    raise Bad(f"no line {WITH}")
                k = part.index(WITH)
                old, new = "\n".join(part[:k]), "\n".join(part[k + 1:])
                at = edit_tool().doc_lines(w[1], old, new)
                print(i, f"{w[1]}: already applied" if at is None else f"{w[1]}: lines {at[0]}-{at[1]}")
                if at is not None:
                    later.append((i, w, (old, new)))
            elif (op == "harno" and len(w) >= 5 and w[1:3] == ["run", "mdmap"] and w[3] in ("update", "add", "remove", "move")) or (op == "commit" and body.strip()):
                later.append((i, w, body))
            else:
                raise Bad("not a plan section")
        except (Bad, ValueError) as e:
            raise Bad(f"section {i} (@@ {head}): {e}")
    return files, later


def run(cmd):
    return subprocess.run(cmd).returncode


def main(argv):
    if argv == ["--commit"]:
        if MSG.exists():
            if run(["git", "add", "-A"]) or run(["git", "commit", "-F", str(MSG)]):
                sys.exit(1)
            MSG.unlink()
        return
    if len(argv) > 1 and argv[0].startswith("@@ "):
        sys.exit(f"refused: the plan came as {len(argv)} arguments, split after {argv[0][-60:]!r}: a single quote inside a "
                 "plan ends the shell string around it, write each as quote, backslash, quote, quote; nothing was written")
    if len(argv) != 1:
        sys.exit(__doc__.strip())
    MSG.unlink(missing_ok=True)
    try:
        plan = sections(argv[0])
        if not plan:
            raise Bad("the plan is empty")
        files, later = check(plan)
    except Bad as e:
        sys.exit(f"refused: {e}; nothing was written")
    for path, text in files.items():
        p = pathlib.Path(path)
        if not p.exists() or p.read_text() != text:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text)
    # What the sections printed comes before what the commands below them write.
    sys.stdout.flush()
    for i, w, body in later:
        if w[0] == "commit":
            MSG.parent.mkdir(parents=True, exist_ok=True)
            MSG.write_text(body.strip() + "\n")
            print(i, "commit after ci:", body.strip().splitlines()[0])
        elif edit_tool().doc(w[1], *body) if w[0] == "doc" else run(w + ([body] if body else [])):
            ran = ", ".join(str(j) for j, x, _ in later if j < i and x[0] in ("harno", "doc")) or "none"
            sys.exit(f"section {i} failed; the files are written; harno and doc sections that ran: {ran}")


if __name__ == "__main__":
    main(sys.argv[1:])

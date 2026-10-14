#!/usr/bin/env python3
"""Cases for implement.py: a plan is checked whole before a file changes, applies its
sections in order, and skips what is already applied, so it can be sent again."""
import pathlib
import subprocess
import sys
import tempfile

TOOL = pathlib.Path(__file__).resolve().parents[1] / "implement.py"
PLAN = ["@@ replace a.txt", "two", "@@ with", "TWO", "@@ lines a.txt 3-3", "3",
        "@@ write \"b/c d.txt\"", "new file", "", "@@ commit", "the message", ""]


def run(cwd, *lines):
    return subprocess.run([sys.executable, str(TOOL), "\n".join(lines)], cwd=cwd, capture_output=True, text=True)


checks = []
with tempfile.TemporaryDirectory() as tmp:
    d = pathlib.Path(tmp)
    a = d / "a.txt"
    a.write_text("one\ntwo\nthree\n")
    r = run(d, *PLAN)
    checks.append(("a plan applies every section", r.returncode == 0 and a.read_text() == "one\nTWO\n3\n"))
    checks.append(("write takes a quoted path, blank lines at the end dropped", (d / "b" / "c d.txt").read_text() == "new file\n"))
    checks.append(("commit leaves its message for after ci", (d / "target" / "implement-commit.txt").read_text() == "the message\n"))
    r = run(d, *PLAN)
    checks.append(("a plan sent again skips what is applied", r.returncode == 0 and a.read_text() == "one\nTWO\n3\n" and "already applied" in r.stdout))
    r = run(d, "@@ replace a.txt", "one", "@@ with", "ONE", "@@ replace a.txt", "missing", "@@ with", "x", "@@ write e.txt", "e")
    checks.append(("a section that does not fit writes nothing", r.returncode != 0 and a.read_text() == "one\nTWO\n3\n" and not (d / "e.txt").exists()))
    checks.append(("the refusal names the section", "section 2" in r.stderr))
    checks.append(("a new plan drops the old message", not (d / "target" / "implement-commit.txt").exists()))
    checks.append(("docs are written through harno only", run(d, "@@ write docs/x.md", "x").returncode != 0))
    checks.append(("an unknown section is refused", run(d, "@@ rm a.txt").returncode != 0))
    r = run(d, "@@ harno update a00001 --title x")
    checks.append(("a doc edit is a harno run mdmap section", r.returncode != 0 and "not a plan section" in r.stderr))
    r = run(d, "@@ harno run mdmap update a00001", "text", "@@ replace a.txt", "missing", "@@ with", "x")
    checks.append(("a harno run mdmap section passes the check and waits for the files", r.returncode != 0 and "section 2" in r.stderr and "section 1" not in r.stderr))
    r = subprocess.run([sys.executable, str(TOOL), "@@ write s.txt\nit", "s split"], cwd=d, capture_output=True, text=True)
    checks.append(("a plan the shell split is refused whole", r.returncode != 0 and "single quote" in r.stderr and not (d / "s.txt").exists()))
    checks.append(("a file under docs/ that is no markdown is a file section", run(d, "@@ write docs/book.toml", "x").returncode == 0 and (d / "docs" / "book.toml").read_text() == "x\n"))
    (d / "mdmap.yml").write_text("roots: [docs/]\nignore: []\n")
    page = d / "docs" / "a.md"
    page.write_text("# A {aaa111}\n\n## B {bbb222}\n\nold line\nkeep\n\n## C {ccc333}\n\nold line\n")
    r = run(d, "@@ doc bbb222", "old line", "@@ with", "new line")
    checks.append(("a doc section changes the text in the body of its node", r.returncode == 0 and "new line\nkeep" in page.read_text() and page.read_text().count("old line") == 1))
    r = run(d, "@@ doc bbb222", "missing", "@@ with", "x", "@@ write f.txt", "f")
    checks.append(("a doc section whose old text is not there writes nothing", r.returncode != 0 and not (d / "f.txt").exists()))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

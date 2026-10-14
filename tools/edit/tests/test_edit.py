#!/usr/bin/env python3
"""Cases for edit.py: a text the shell split is refused, markdown under docs/
goes through mdmap, and doc changes a text in the body of one doc node."""
import pathlib
import subprocess
import sys
import tempfile

TOOL = pathlib.Path(__file__).resolve().parents[1] / "edit.py"


def run(cwd, *args):
    return subprocess.run([sys.executable, str(TOOL), *args], cwd=cwd, capture_output=True, text=True)


checks = []
with tempfile.TemporaryDirectory() as tmp:
    d = pathlib.Path(tmp)
    r = run(d, "write", "a.txt", "it", "s split")
    checks.append(("a text the shell split is refused, nothing written", r.returncode != 0 and "single quote" in r.stderr and not (d / "a.txt").exists()))
    checks.append(("markdown under docs/ is refused", run(d, "write", "docs/x.md", "x").returncode != 0 and not (d / "docs" / "x.md").exists()))
    checks.append(("another file under docs/ is written", run(d, "write", "docs/book.toml", "x").returncode == 0 and (d / "docs" / "book.toml").read_text() == "x\n"))
    (d / "mdmap.yml").write_text("roots: [docs/]\nignore: []\nwarn: {body: 5}\n")
    page = d / "docs" / "a.md"
    page.write_text("# A {aaa111}\n\n## B {bbb222}\n\nold line\nkeep\n\n## C {ccc333}\n\nold line\n")
    r = run(d, "doc", "bbb222", "old line", "new line")
    checks.append(("doc changes the text in the body of its node alone", r.returncode == 0 and "new line\nkeep" in page.read_text() and page.read_text().count("old line") == 1))
    r = run(d, "doc", "bbb222", "old line", "new line")
    checks.append(("doc sent again is already applied", r.returncode == 0 and "already applied" in r.stdout))
    r = run(d, "doc", "bbb222", "missing", "x")
    checks.append(("an old text that is not there is refused", r.returncode != 0 and "occurs 0 times" in r.stderr))
    checks.append(("a node that is not there is refused", run(d, "doc", "fff999", "a", "b").returncode != 0))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

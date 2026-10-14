#!/usr/bin/env python3
"""Cases for commit-only.sh: the named paths make the commit, every other change stays as it was."""
import os
import pathlib
import subprocess
import sys
import tempfile

TOOL = pathlib.Path(__file__).resolve().parents[1] / "commit-only.sh"
ENV = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t",
           GIT_CONFIG_GLOBAL="/dev/null")


def git(d, *args):
    return subprocess.run(["git", *args], cwd=d, capture_output=True, text=True, env=ENV).stdout


def tool(d, *args):
    return subprocess.run(["bash", str(TOOL), *args], cwd=d, capture_output=True, text=True, env=ENV)


checks = []
with tempfile.TemporaryDirectory() as tmp:
    d = pathlib.Path(tmp)
    git(d, "init", "-q")
    for f in ("a.txt", "b.txt", "gone.txt"):
        (d / f).write_text("one\n")
    git(d, "add", "-A")
    git(d, "commit", "-q", "-m", "base")
    (d / "a.txt").write_text("mine\n")
    (d / "b.txt").write_text("theirs\n")
    git(d, "add", "b.txt")
    (d / "new.txt").write_text("new\n")
    (d / "gone.txt").unlink()
    r = tool(d, "my change", "a.txt", "new.txt", "gone.txt")
    files = git(d, "show", "--name-status", "--format=%s", "HEAD")
    checks.append(("the named paths, new, changed and deleted, make the commit", r.returncode == 0 and "my change" in files
                   and "M\ta.txt" in files and "A\tnew.txt" in files and "D\tgone.txt" in files and "b.txt" not in files))
    checks.append(("another change stays staged as it was", git(d, "diff", "--cached", "--name-only") == "b.txt\n"))
    checks.append(("a message without a path is refused", tool(d, "only a message").returncode == 2))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

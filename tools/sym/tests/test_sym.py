#!/usr/bin/env python3
"""Cases for sym.py: a name in Rust, Python and TypeScript, a method by its owner, outline."""
import pathlib
import subprocess
import sys
import tempfile

TOOL = pathlib.Path(__file__).resolve().parents[1] / "sym.py"


def run(cwd, *args):
    r = subprocess.run([sys.executable, str(TOOL), *args], cwd=cwd, capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr


checks = []
with tempfile.TemporaryDirectory() as tmp:
    d = pathlib.Path(tmp)
    (d / "a.rs").write_text("/// Loads it.\nfn load() {}\nstruct Graph;\nimpl Graph {\n    fn load(&self) {}\n}\n")
    (d / "b.py").write_text("class Graph:\n    @property\n    def load(self):\n        return 1\n\n\ndef load():\n    pass\n")
    (d / "c.ts").write_text("export class Graph {\n  load() {}\n}\nexport function load() {}\n")
    code, out = run(d, "def", "load")
    checks.append(("def finds a name in every language", code == 0 and all(f in out for f in ("a.rs:2-2", "b.py:7-8", "c.ts:4-4"))))
    code, out = run(d, "def", "Graph::load")
    checks.append(("a method by its owner, impl or class", code == 0 and all(f in out for f in ("a.rs:5-5", "b.py:3-4", "c.ts:2-2")) and "b.py:7" not in out))
    code, out = run(d, "body", "load", "--in", "a.rs")
    checks.append(("body takes the doc comment above", "/// Loads it." in out))
    code, out = run(d, "body", "load", "--in", "b.py")
    checks.append(("body takes the decorator of a method", "@property" in out))
    code, out = run(d, "outline", "b.py")
    checks.append(("outline lists a class and its methods", code == 0 and "class Graph" in out and "def load(self)" in out))
    code, out = run(d, "def", "nothing")
    checks.append(("a name that is not there is exit 1", code == 1 and "not found: nothing" in out))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

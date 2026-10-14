#!/usr/bin/env python3
"""Cases for sandbox.py: the sandbox block goes into .claude/settings.json and
out again, install.py writes it when tool.yml asks, and the rest stays."""
import json
import os
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).parent.parent
TMP = pathlib.Path(tempfile.mkdtemp())
SETTINGS = TMP / ".claude" / "settings.json"
checks = []


def run(script, *args, sandbox=False, write="~/.cargo", read=""):
    yml = TMP / "tool.yml"
    yml.write_text("config:\n  strict: false\n  sandbox: %s\n  sandbox_write: %s\n  sandbox_read: \"%s\"\n" % ("true" if sandbox else "false", write, read))
    env = dict(os.environ, TOOL_YML=str(yml))
    return subprocess.run([sys.executable, str(HERE / script), *args], cwd=TMP, capture_output=True, text=True, env=env)


def settings():
    return json.loads(SETTINGS.read_text())


SETTINGS.parent.mkdir()
SETTINGS.write_text(json.dumps({"model": "x"}))
r = run("sandbox.py")
s = settings()
checks.append(("the block turns the sandbox on", r.returncode == 0 and s["sandbox"]["enabled"] is True))
checks.append(("a sandboxed command is not asked about", s["sandbox"]["autoAllowBashIfSandboxed"] is True))
checks.append(("nothing takes a command out of the sandbox", s["sandbox"]["allowUnsandboxedCommands"] is False))
checks.append(("no unix socket reaches a daemon outside", s["sandbox"]["network"]["allowAllUnixSockets"] is False))
checks.append(("the paths of tool.yml stay writable", s["sandbox"]["filesystem"]["allowWrite"] == ["~/.cargo"]))
checks.append(("the rest of the file stays", s["model"] == "x"))
checks.append(("a sandbox that cannot start fails the session", s["sandbox"]["failIfUnavailable"] is True))
checks.append(("reading is open while no path is named", "denyRead" not in s["sandbox"]["filesystem"]))
r = run("sandbox.py", read="~/.nvm")
s = settings()
checks.append(("a named read path closes the home", s["sandbox"]["filesystem"]["denyRead"] == ["~/"]))
checks.append(("the project and the named paths stay readable", s["sandbox"]["filesystem"]["allowRead"] == [".", "~/.nvm"]))
checks.append(("remove takes the block out and leaves the rest", run("sandbox.py", "--remove").returncode == 0 and "sandbox" not in settings() and settings()["model"] == "x"))
checks.append(("install writes it when tool.yml asks", run("install.py", sandbox=True).returncode == 0 and settings()["sandbox"]["enabled"] is True))
checks.append(("install takes it out when it does not", run("install.py").returncode == 0 and "sandbox" not in settings()))
checks.append(("removing the harness takes it out too", run("install.py", "remove", sandbox=True).returncode == 0 and "sandbox" not in settings()))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

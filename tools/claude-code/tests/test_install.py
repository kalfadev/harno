#!/usr/bin/env python3
"""Cases for install.py: the hooks go into .claude/settings.json and out again,
and everything else there stays."""
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


def install(*args, strict=False, deny=""):
    yml = TMP / "tool.yml"
    yml.write_text("config:\n  strict: %s\n  deny_tools: \"%s\"\n" % ("true" if strict else "false", deny))
    env = dict(os.environ, TOOL_YML=str(yml))
    return subprocess.run([sys.executable, str(HERE / "install.py"), *args], cwd=TMP, capture_output=True, text=True, env=env)


def settings():
    return json.loads(SETTINGS.read_text())


def commands(s, event):
    return [h["command"] for g in s.get("hooks", {}).get(event, []) for h in g["hooks"]]


SETTINGS.parent.mkdir()
mine = {"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]}, "model": "x"}
SETTINGS.write_text(json.dumps(mine))
r = install()
s = settings()
checks.append(("add writes the hooks", r.returncode == 0 and any("hook.py\" session-start" in c for c in commands(s, "SessionStart"))))
checks.append(("a prompt runs the log hook alone", [c.split("/")[-1] for c in commands(s, "UserPromptSubmit")] == ["hook.py\" prompt"]))
checks.append(("other hooks and settings stay", commands(s, "Stop")[0] == "say done" and s["model"] == "x"))
checks.append(("not strict: no PreToolUse, no permission", "PreToolUse" not in s["hooks"] and "permissions" not in s))
install()
checks.append(("add twice writes each hook once", settings() == s))
install(strict=True)
s = settings()
checks.append(("strict adds only-harno and the permission", any("only-harno.py" in c for c in commands(s, "PreToolUse")) and s["permissions"]["allow"] == ["Bash(harno:*)"]))
install(strict=True, deny="Read, mcp__x__*")
checks.append(("deny_tools goes into permissions.deny", settings()["permissions"]["deny"] == ["Read", "mcp__x__*"]))
install(strict=True, deny="Read, Write")
checks.append(("the deny list only grows", settings()["permissions"]["deny"] == ["Read", "mcp__x__*", "Write"]))
install("remove")
s = settings()
checks.append(("remove takes out only ours", s["hooks"] == mine["hooks"] and s["model"] == "x"))
SETTINGS.unlink()
checks.append(("remove leaves no settings file where there was none", install("remove").returncode == 0 and not SETTINGS.exists()))
for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

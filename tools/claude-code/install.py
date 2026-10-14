#!/usr/bin/env python3
"""The hooks of this harness in .claude/settings.json: `install.py install`
writes them and `install.py remove` takes them out. `harno tool add` copies
the files and writes nothing, so a preset of the project calls this script:
`harno run claude-code install`, outside the sandbox. A hook is ours when its command runs a script of
.harno/tools/claude-code/; every other hook and setting stays as it is.
With `strict: true` in tool.yml, a PreToolUse hook refuses every Bash call
that is not harno and answers a harno line with a PreToolUse allow, so it runs
without a question; `Bash(harno:*)` stays in the allow list as the plain case.
The tools of
`deny_tools` go into `permissions.deny`, and with `sandbox: true` it also writes
the sandbox block (sandbox.py)."""
import json
import os
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from tool_config import config  # noqa: E402
from sandbox import apply as sandbox  # noqa: E402

HERE = ".harno/tools/claude-code"
SETTINGS = pathlib.Path(".claude/settings.json")


# Python on Windows answers to python; python3 is the name elsewhere.
PYTHON = "python" if os.name == "nt" else "python3"


def cmd(script, *args):
    return " ".join(["%s \"$CLAUDE_PROJECT_DIR/%s/%s\"" % (PYTHON, HERE, script), *args])


def hooks(strict):
    h = [("SessionStart", None, cmd("hook.py", "session-start")),
         ("UserPromptSubmit", None, cmd("hook.py", "prompt")),
         ("PostToolUse", None, cmd("hook.py", "tool")),
         ("Stop", None, cmd("hook.py", "response")),
         ("SessionEnd", None, cmd("hook.py", "session-end"))]
    if strict:
        h.insert(0, ("PreToolUse", "Bash", cmd("only-harno.py")))
    return h


def ours(h):
    return HERE + "/" in h.get("command", "")


def strip(settings):
    all_hooks = settings.get("hooks", {})
    for event in list(all_hooks):
        groups = []
        for g in all_hooks[event]:
            g = dict(g, hooks=[h for h in g.get("hooks", []) if not ours(h)])
            if g["hooks"]:
                groups.append(g)
        if groups:
            all_hooks[event] = groups
        else:
            del all_hooks[event]
    if "hooks" in settings and not all_hooks:
        del settings["hooks"]


def main(argv):
    remove = "remove" in argv
    if remove and not SETTINGS.exists():
        # Nothing was ever written: a remove that writes an empty settings file
        # leaves more behind than it takes out.
        print("nothing of this harness is in %s" % SETTINGS)
        return
    settings = json.loads(SETTINGS.read_text()) if SETTINGS.exists() else {}
    strip(settings)
    if not remove:
        strict = config().get("strict") is True
        all_hooks = settings.setdefault("hooks", {})
        for event, matcher, command in hooks(strict):
            groups = all_hooks.setdefault(event, [])
            g = next((g for g in groups if g.get("matcher") == matcher and all(ours(h) for h in g["hooks"])), None)
            if g is None:
                g = {"matcher": matcher, "hooks": []} if matcher else {"hooks": []}
                groups.append(g)
            g["hooks"].append({"type": "command", "command": command})
        if strict:
            # Only a line that starts with the word harno goes through unasked,
            # in both shapes of the same prefix rule; Bash whole would let a line
            # the hook refuses run without a question ever reaching the human.
            allow = settings.setdefault("permissions", {}).setdefault("allow", [])
            allow[:] = [a for a in allow if a not in ("Bash", "Bash(harno *)")]
            if "Bash(harno:*)" not in allow:
                allow.append("Bash(harno:*)")
        # The deny list only grows: what the project refuses is written, what is
        # already refused stays, and nothing here takes a deny out again.
        refused = [t.strip() for t in (config().get("deny_tools") or "").split(",") if t.strip()]
        if refused:
            deny = settings.setdefault("permissions", {}).setdefault("deny", [])
            deny.extend(t for t in refused if t not in deny)
    sandbox(settings, not remove and config().get("sandbox") is True)
    SETTINGS.parent.mkdir(exist_ok=True)
    SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    print(("took the hooks out of %s" if remove else "wrote the hooks into %s; start a new Claude Code session") % SETTINGS)


if __name__ == "__main__":
    main(sys.argv[1:])

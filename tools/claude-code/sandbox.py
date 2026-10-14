#!/usr/bin/env python3
"""The sandbox of this harness in .claude/settings.json: `sandbox.py` writes the
block, `sandbox.py --remove` takes it out, and `install.py` does both from the
`sandbox` value of tool.yml. A sandboxed command writes inside the project
and nowhere else; outside it the sandbox refuses, and the way out is one
permission the human answers. The paths outside the project a tool needs anyway
(`cargo install` writes to ~/.cargo) are `sandbox_write` in tool.yml, comma
separated. `sandbox_read` closes the home to reads and names what stays
readable there (the programs the presets run, the git config); empty, reading is
open. Nothing takes a command out of the sandbox, and no unix socket reaches a
daemon outside it. Everything else in the settings file stays as it is."""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from tool_config import config  # noqa: E402

SETTINGS = pathlib.Path(".claude/settings.json")


def paths(key):
    """A comma separated list of paths from tool.yml."""
    return [p.strip() for p in (config().get(key) or "").split(",") if p.strip()]


def apply(settings, on):
    """Put the sandbox block into `settings`, or take it out."""
    if not on:
        settings.pop("sandbox", None)
        return
    filesystem = {"allowWrite": paths("sandbox_write")}
    readable = paths("sandbox_read")
    if readable:
        # Reading is open everywhere until a path is named: then the home is
        # closed and only the project and the named paths stay readable.
        filesystem["denyRead"] = ["~/"]
        filesystem["allowRead"] = ["."] + readable
    settings["sandbox"] = {
        "enabled": True,
        # A sandboxed command needs no question: the sandbox already says what
        # it may touch.
        "autoAllowBashIfSandboxed": True,
        # And nothing takes a command out of it: with Bash(harno *) allowed and no
        # question asked, dangerouslyDisableSandbox would be a way out unwatched.
        "allowUnsandboxedCommands": False,
        # A sandbox that cannot start is a sandbox that is not there: say so
        # instead of running the call without one.
        "failIfUnavailable": True,
        "filesystem": filesystem,
        # The network is not what this sandbox is for, and the 127.0.0.1 the page of
        # serve and its tests bind and call is allowLocalBinding. A unix socket is
        # not that: it reaches a daemon outside the sandbox, so it stays closed.
        "network": {"allowedDomains": ["*"], "allowLocalBinding": True, "allowAllUnixSockets": False},
    }


def main(argv):
    remove = "--remove" in argv
    settings = json.loads(SETTINGS.read_text()) if SETTINGS.exists() else {}
    apply(settings, not remove)
    SETTINGS.parent.mkdir(exist_ok=True)
    SETTINGS.write_text(json.dumps(settings, indent=2) + "\n")
    print(("took the sandbox out of %s" if remove else "wrote the sandbox into %s; start a new Claude Code session") % SETTINGS)


if __name__ == "__main__":
    main(sys.argv[1:])

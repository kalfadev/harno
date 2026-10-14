#!/usr/bin/env python3
"""PreToolUse hook for Bash: every command goes through harno.

shfmt parses the line into a bash syntax tree (`shfmt --to-json`) and the
hook walks it. Every command in the tree must be a harno call: `harno a &&
harno b` passes, `harno ls; cat x` does not. A word is literal text only:
plain, single-quoted, or double-quoted without any expansion. Refused too:
command and process substitution, parameter expansion, heredocs, subshells,
variable assignments, and redirection other than `2>&1` and `>/dev/null`.
A long argument is a single-quoted string, newlines included; a quote inside
it is written '\''.

A line that is only harno calls is answered with a PreToolUse `allow`, so it
runs without a question; exit 2 blocks the call and hands stderr to the agent;
without shfmt, and on every error of the hook itself, the call is refused: any
other exit lets it through. The way out stays inside harno, where the log sees it:
harno run --raw "<command>" --why "<reason>".
"""
import json
import os
import shutil
import subprocess
import sys

# A hook may run with a shorter PATH than the shell: look where brew puts shfmt too.
SHFMT = shutil.which("shfmt") or next((p for p in ("/opt/homebrew/bin/shfmt", "/usr/local/bin/shfmt") if os.path.exists(p)), "shfmt")

PARTS = {"CmdSubst": "command substitution", "ProcSubst": "process substitution", "ParamExp": "parameter expansion",
         "ArithmExp": "arithmetic expansion", "ExtGlob": "an extended glob", "BraceExp": "brace expansion"}


def why_not(cmd):
    """None when the line is only harno calls, else the reason."""
    try:
        out = subprocess.run([SHFMT, "-ln", "bash", "--to-json"], input=cmd, capture_output=True, text=True)
    except FileNotFoundError:
        return "no shfmt on PATH to read the line (brew install shfmt)"
    if out.returncode != 0:
        return f"unparsable: {out.stderr.strip()}"
    return stmts(json.loads(out.stdout).get("Stmts", []))


def stmts(items):
    for s in items:
        reason = redirs(s.get("Redirs", [])) or command(s.get("Cmd"))
        if reason:
            return reason
    return None


def command(c):
    if c is None:
        return "a redirection without a command"
    kind = c["Type"]
    if kind == "BinaryCmd":
        return stmts([c["X"], c["Y"]])
    if kind == "Subshell":
        return "a subshell"
    if kind != "CallExpr":
        return f"a shell {kind}"
    if c.get("Assigns"):
        return "a variable assignment"
    args = c.get("Args", [])
    for w in args:
        reason = word(w)
        if reason:
            return reason
    name = text(args[0]) if args else ""
    return None if name == "harno" else f"`{name}` is not harno"


def literals(w):
    """The parts of a word, double quotes opened."""
    for p in w.get("Parts", []):
        yield from p.get("Parts", []) if p["Type"] == "DblQuoted" else [p]


def word(w):
    for p in literals(w):
        if p["Type"] not in ("Lit", "SglQuoted"):
            return PARTS.get(p["Type"], p["Type"])
    return None


def text(w):
    """The value of a word that word() let through."""
    return "".join(p.get("Value", "") for p in literals(w))


def redirs(items):
    for r in items:
        op, target = r["Op"], r.get("Word", {})
        if op.startswith("<<"):
            return "a here-string" if op == "<<<" else "a heredoc"
        reason = word(target)
        if reason:
            return reason
        to = text(target)
        if not (op == ">&" and to.isdigit() or op in (">", ">>") and to == "/dev/null"):
            return f"redirection {op} {to}".strip()
    return None


def main():
    try:
        command = json.load(sys.stdin).get("tool_input", {}).get("command", "")
        reason = why_not(command)
    except Exception as e:
        # Every exit but 2 lets the call through, and a sandboxed Bash call is not
        # asked about: a line the hook cannot read is refused, not let through.
        reason = "the hook could not read the line (%s: %s)" % (type(e).__name__, e)
    if reason is None:
        # The hook is the gate, so it answers for the permission too: a
        # PreToolUse allow runs the line without a question. A pattern alone
        # would not, since Claude Code holds a command line back from one when
        # its text is long, many-lined or looks like code, which every plan of
        # implement and every doc body is.
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow",
                                                 "permissionDecisionReason": "every command of this project goes through harno"}}))
        sys.exit(0)
    print(f"refused: {reason}. Every command goes through harno: harno run <preset> -- <args>, "
          "harno run \"<command>\" for run.allow, or harno run --raw \"<command>\" --why \"<reason>\". "
          "A long argument is a single-quoted string.",
          file=sys.stderr)
    sys.exit(2)


if __name__ == "__main__":
    main()

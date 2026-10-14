#!/usr/bin/env python3
"""Claude Code hooks into a log of their own: `hook.py <action>` reads the
JSON a Claude Code hook gets on stdin, reads the transcript where the hook
input falls short, and writes the log of this session. Everything the log
says about Claude Code is decided here: the hook input field names, the
transcript format, the tools that start subagents, `CLAUDE_ENV_FILE`.

    action         Claude Code hook   event
    session-start  SessionStart       session start; HARNO_SESSION into $CLAUDE_ENV_FILE
    prompt         UserPromptSubmit   prompt
    tool           PostToolUse        tool or subagent, after the calls that failed before it
    response       Stop               response, after the calls that failed before it
    session-end    SessionEnd         session end

The log of this harness is `.harno/log/claude-code/<session>.jsonl`, beside
the log of the gate, `.harno/log/harno/<session>.jsonl`, which harno writes
itself. Outside a harno project it does nothing."""
import datetime
import json
import os
import pathlib
import shlex
import sys
import time

ACTIONS = {"session-start": "SessionStart", "prompt": "UserPromptSubmit", "tool": "PostToolUse",
           "response": "Stop", "session-end": "SessionEnd"}
SUBAGENT_TOOLS = ("Task", "Agent")
SUMMARY_KEYS = ("command", "file_path", "path", "pattern", "url", "query", "description", "prompt")
LOG = ".harno/log"
HARNESS = "claude-code"


class Bad(Exception):
    pass


def truncate(s, n):
    return s if len(s) <= n else s[:n - 1].rstrip() + "…"


def blocks(v):
    c = (v.get("message") or {}).get("content")
    return [b for b in c if isinstance(b, dict)] if isinstance(c, list) else []


def is_assistant(v):
    return v.get("type") == "assistant"


def message_id(v):
    return (v.get("message") or {}).get("id") or v.get("requestId")


def read_lines(transcript):
    try:
        text = pathlib.Path(transcript).read_text(encoding="utf-8")
    except OSError:
        return None
    out = []
    for raw in text.splitlines():
        try:
            out.append(json.loads(raw))
        except ValueError:
            pass
    return out


def since_prompt(transcript):
    """The transcript lines after the last prompt the human typed. A user line
    whose content is only tool results is not a prompt; a subagent line
    (isSidechain) is not the agent's own."""
    lines = read_lines(transcript)
    if lines is None:
        return None
    lines = [v for v in lines if v.get("isSidechain") is not True]

    def is_prompt(v):
        c = (v.get("message") or {}).get("content")
        return v.get("type") == "user" and (isinstance(c, str) or any(b.get("type") == "text" for b in blocks(v)))

    start = max((i for i, v in enumerate(lines) if is_prompt(v)), default=-1) + 1
    return lines[start:]


def wait_for_answer(transcript):
    """Claude Code may write the answer after the Stop hook starts: wait up to a
    second for the agent's last line to hold no tool call."""
    for _ in range(20):
        lines = since_prompt(transcript) or []
        last = next((v for v in reversed(lines) if is_assistant(v)), None)
        if last is not None and not any(b.get("type") == "tool_use" for b in blocks(last)):
            return
        time.sleep(0.05)


def wait_for(transcript, call_id):
    """Claude Code may write a call's line after its PostToolUse hook starts."""
    for _ in range(20):
        try:
            if call_id in pathlib.Path(transcript).read_text(encoding="utf-8"):
                return
        except OSError:
            pass
        time.sleep(0.05)


def last_response(transcript):
    parts = [b.get("text") for v in since_prompt(transcript) or [] if is_assistant(v)
             for b in blocks(v) if b.get("type") == "text" and isinstance(b.get("text"), str)]
    return "\n\n".join(parts) if parts else None


def turns(transcript):
    """The API turns since the prompt: distinct assistant messages; a message
    comes as one transcript line per block."""
    ids = {message_id(v) for v in since_prompt(transcript) or [] if is_assistant(v) and message_id(v)}
    return len(ids) or None


def written_from(lines, mid):
    """What message `mid` holds besides its tool calls: its text and thinking
    blocks. Claude Code keeps the text shown before a call as a thinking block."""
    said = []
    for v in lines:
        if not is_assistant(v) or message_id(v) != mid:
            continue
        for b in blocks(v):
            t = b.get("text") if b.get("type") == "text" else b.get("thinking") if b.get("type") == "thinking" else None
            if isinstance(t, str) and t.strip():
                said.append(t)
    return mid, ("\n\n".join(said) if said else None)


def turn_of(transcript, call_id, name, args):
    """The API turn of a call: the assistant message that asked for it, by the
    tool use id, or without one the last call with the same name and input."""
    lines = read_lines(transcript)
    if lines is None:
        return None, None
    for v in reversed(lines):
        if not is_assistant(v):
            continue
        asked = any(b.get("type") == "tool_use" and (b.get("id") == call_id if call_id else b.get("name") == name and b.get("input") == args)
                    for b in blocks(v))
        if asked:
            mid = message_id(v)
            return written_from(lines, mid) if mid else (None, None)
    return None, None


def summary(args):
    """What the call did, from the input field that says the most, as the agent
    wrote it; cut at 200 characters. The whole input goes into `in`."""
    if isinstance(args, dict):
        for k in SUMMARY_KEYS:
            if isinstance(args.get(k), str):
                return truncate(args[k], 200)
    return truncate(json.dumps(args, separators=(",", ":"), ensure_ascii=False), 200)


def response_text(r):
    if isinstance(r, str):
        return r
    if r is None:
        return ""
    if isinstance(r, dict) and ("stdout" in r or "stderr" in r):
        out, err = r.get("stdout") or "", r.get("stderr") or ""
        return out if not err else err if not out else f"{out}\n{err}"
    if isinstance(r, dict) and isinstance(r.get("content"), list):
        return "\n\n".join(b["text"] for b in r["content"] if isinstance(b, dict) and isinstance(b.get("text"), str))
    return json.dumps(r, indent=2, ensure_ascii=False)


def result_text(c):
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return "\n\n".join(b["text"] for b in c if isinstance(b, dict) and isinstance(b.get("text"), str))
    return ""


def call_event(name, args, output, ok, turn, said):
    if name in SUBAGENT_TOOLS:
        e = {"type": "subagent", "agent": (args or {}).get("subagent_type") or "subagent"}
    else:
        e = {"type": "tool", "tool": name}
    e["summary"] = summary(args)
    e["in"] = args
    e["out"] = output
    if not ok:
        e["ok"] = False
    if turn:
        e["turn"] = turn
    if said:
        e["said"] = said
    return e


def failed_before(transcript, current):
    """The calls that failed just before the call in `current` (a PostToolUse
    input), or before the end when there is none, back to the last call that
    did not fail. PostToolUse fires only for a call that succeeded; a call a
    PreToolUse hook refused is a failed one."""
    lines = since_prompt(transcript)
    if lines is None:
        return []
    uses, results = [], {}
    for v in lines:
        for b in blocks(v):
            if b.get("type") == "tool_use" and is_assistant(v):
                uses.append((v, b))
            elif b.get("type") == "tool_result" and isinstance(b.get("tool_use_id"), str):
                results[b["tool_use_id"]] = (b.get("is_error") is True, result_text(b.get("content")))
    end = len(uses)
    if current is not None:
        cid, name, args = current.get("tool_use_id"), current.get("tool_name"), current.get("tool_input")
        found = [k for k, (_, b) in enumerate(uses) if (b.get("id") == cid if cid else b.get("name") == name and b.get("input") == args)]
        if not found:
            return []
        end = found[-1]
    failed = []
    for v, b in reversed(uses[:end]):
        err = results.get(b.get("id"))
        if not err or not err[0]:
            break
        turn, said = written_from(lines, message_id(v)) if message_id(v) else (None, None)
        failed.append(call_event(b.get("name") or "?", b.get("input"), err[1], False, turn, said))
    failed.reverse()
    return failed


def events(action, data):
    """The events for one hook call, in the order they go into the log."""
    if action not in ACTIONS:
        raise Bad("unknown action %r; actions: %s" % (action, ", ".join(ACTIONS)))
    name, session = data.get("hook_event_name"), data.get("session_id")
    if not name:
        raise Bad("the hook input has no hook_event_name")
    if not session:
        raise Bad("the hook input has no session_id")
    if name not in ACTIONS.values():
        raise Bad("harno does not log Claude Code's %s event" % name)
    if ACTIONS[action] != name:
        raise Bad("'%s' was called for a %s event; attach it to the %s hook" % (action, name, ACTIONS[action]))
    t = data.get("transcript_path")
    out = []
    if name == "SessionStart":
        out.append({"type": "session", "event": "start", "source": data.get("source")})
    elif name == "SessionEnd":
        out.append({"type": "session", "event": "end", "reason": data.get("reason")})
    elif name == "UserPromptSubmit":
        out.append({"type": "prompt", "text": data.get("prompt") or ""})
    elif name == "Stop":
        if t:
            wait_for_answer(t)
            out += failed_before(t, None)
        text = data.get("last_assistant_message") or (last_response(t) if t else None) or ""
        out.append({"type": "response", "text": text, "turns": turns(t) if t else None})
    else:
        tool, args = data.get("tool_name") or "?", data.get("tool_input")
        turn = said = None
        if t:
            if data.get("tool_use_id"):
                wait_for(t, data["tool_use_id"])
            out += failed_before(t, data)
            turn, said = turn_of(t, data.get("tool_use_id"), tool, args)
        out.append(call_event(tool, args, response_text(data.get("tool_response")), True, turn, said))
    for e in out:
        e["session"] = session
        if data.get("cwd"):
            e["cwd"] = data["cwd"]
    return out


def project_of(cwd):
    """The harno project a hook call belongs to, or None."""
    p = pathlib.Path(cwd).resolve()
    return next((d for d in [p, *p.parents] if (d / ".harno" / "config.yml").is_file()), None)


def read_jsonl(path):
    out = []
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return out
    for raw in text.splitlines():
        try:
            out.append(json.loads(raw))
        except ValueError:
            pass
    return out


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def holds(command, words):
    """The command holds every word, in order: a call as the agent wrote it."""
    at = 0
    for w in words:
        i = command.find(w, at)
        if i < 0:
            return False
        at = i + len(w)
    return True


def plain(command):
    """The command as the shell hands it on, its quotes and escapes taken out: an
    argument with a quote in it reads there as harno logged it."""
    try:
        return " ".join(shlex.split(command))
    except ValueError:
        return command


def claims(project, session, command, taken, after):
    """The lines of the gate log a shell command produced: the unclaimed lines
    written since the last line of this log, whose call the command holds word
    by word, in order, as the agent wrote it or as the shell hands it on. A call
    a hook made is claimed by nobody: no command of the agent holds it."""
    if not command or "harno" not in command:
        return []
    shown = plain(command)
    out = []
    for line in read_jsonl(project / LOG / "harno" / ("%s.jsonl" % session)):
        n = line.get("n")
        if n is None or n in taken or (after and (line.get("ts") or "") <= after):
            continue
        words = ["harno", line.get("verb") or ""] + [a for a in line.get("args") or [] if isinstance(a, str)]
        if holds(command, words) or holds(shown, words):
            out.append(n)
            taken.add(n)
    return out


def line_of(e, ctx):
    """One event as a line of this log, with the text that goes beside it. A
    tool call that went through harno keeps no text of its own: the gate
    already has it, so the line names the gate lines it claims."""
    kind = e.get("type")
    if kind == "session":
        return {k: e[k] for k in ("type", "event", "source", "reason") if e.get(k) is not None}, []
    if kind in ("prompt", "response"):
        text = e.get("text") or ""
        if not text.strip():
            return None, []
        m = {"type": kind, "chars": len(text)}
        if e.get("turns") is not None:
            m["turns"] = e["turns"]
        return m, [("out", text)]
    m = {"type": kind, "tool" if kind == "tool" else "agent": e.get("tool") or e.get("agent") or "?"}
    m["summary"] = e.get("summary") or ""
    if e.get("ok") is False:
        m["ok"] = False
    turn = e.get("turn")
    if turn:
        m["turn"] = turn
        # What the agent wrote in a turn goes with its first call only.
        if turn not in ctx["turns"] and e.get("said"):
            m["said"] = e["said"]
        ctx["turns"].add(turn)
    args = e.get("in") if isinstance(e.get("in"), dict) else {}
    mine = claims(ctx["project"], ctx["session"], args.get("command"), ctx["taken"], ctx["after"])
    if mine:
        m["harno"] = mine
        return m, []
    out = e.get("out") or ""
    m["chars"] = len(out)
    return m, [("in", json.dumps(e.get("in"), indent=2, ensure_ascii=False)), ("out", out)]


def record(project, session, evs):
    """The log of this harness: `.harno/log/claude-code/<session>.jsonl`, one
    line per event, the bulky text beside it as `<session>/<n>.in` and
    `<n>.out`."""
    directory = project / LOG / HARNESS
    (directory / session).mkdir(parents=True, exist_ok=True)
    path = directory / ("%s.jsonl" % session)
    written = read_jsonl(path)
    ctx = {
        "project": project,
        "session": session,
        "taken": {n for line in written for n in line.get("harno") or []},
        "turns": {line.get("turn") for line in written if line.get("turn")},
        # A call of this hook call was written after the line before it.
        "after": (written[-1].get("ts") or "") if written else "",
    }
    n = len(written)
    with open(path, "a", encoding="utf-8") as f:
        for e in evs:
            m, files = line_of(e, ctx)
            if m is None:
                continue
            n += 1
            line = {"n": n, "ts": now()}
            line.update(m)
            f.write(json.dumps(line, ensure_ascii=False) + "\n")
            for ext, text in files:
                (directory / session / ("%04d.%s" % (n, ext))).write_text(text, encoding="utf-8")


def main(argv):
    action = argv[1] if len(argv) > 1 else ""
    try:
        data = json.load(sys.stdin)
    except ValueError as e:
        sys.exit("refused: the hook input is not JSON: %s" % e)
    try:
        evs = events(action, data)
    except Bad as e:
        sys.exit("refused: %s" % e)
    project = project_of(data.get("cwd") or os.getcwd())
    if project is None:
        return
    record(project, data["session_id"], evs)
    if action == "session-start":
        env = os.environ.get("CLAUDE_ENV_FILE")
        if not env:
            sys.exit("refused: CLAUDE_ENV_FILE is not set; attach session-start to the SessionStart hook")
        with open(env, "a") as f:
            f.write("export HARNO_SESSION=%s\n" % data["session_id"])


if __name__ == "__main__":
    main(sys.argv)

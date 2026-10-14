#!/usr/bin/env python3
"""Cases for hook.py: what Claude Code sends becomes the log of a session.
The JSON here is what Claude Code sends; the transcript lines are its format."""
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import threading
import time

HERE = pathlib.Path(__file__).parent.parent
spec = importlib.util.spec_from_file_location("hook", HERE / "hook.py")
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)
TMP = pathlib.Path(tempfile.mkdtemp())
EVENTS = {"session-start": "SessionStart", "prompt": "UserPromptSubmit", "tool": "PostToolUse", "response": "Stop", "session-end": "SessionEnd"}
checks = []


def transcript(*lines):
    p = TMP / ("t%d.jsonl" % len(list(TMP.iterdir())))
    p.write_text("\n".join(lines))
    return str(p)


def events(action, **fields):
    data = dict(session_id="s1", hook_event_name=EVENTS.get(action, action))
    data.update(fields)
    return hook.events(action, data)


def refused(action, text, **fields):
    try:
        events(action, **fields)
    except hook.Bad as e:
        return text in str(e)
    return False


def check(name, ok):
    checks.append((name, ok))


# Plain events.
e = events("session-start", source="startup")
check("session start", e == [{"type": "session", "event": "start", "source": "startup", "session": "s1"}])
check("session end", events("session-end", reason="logout")[0]["reason"] == "logout")
check("prompt", events("prompt", prompt="add a run section", cwd="/p") == [{"type": "prompt", "text": "add a run section", "session": "s1", "cwd": "/p"}])

# A tool: input, output, summary.
e = events("tool", tool_name="Read", tool_input={"file_path": "/repo/docs/arch.md"}, tool_response="# Architecture\n...")[-1]
check("a tool with its input and output", (e["type"], e["tool"], e["summary"], e["in"], e["out"]) == ("tool", "Read", "/repo/docs/arch.md", {"file_path": "/repo/docs/arch.md"}, "# Architecture\n..."))
e = events("tool", tool_name="Bash", tool_input={"command": "cargo test"}, tool_response={"stdout": "ok", "stderr": "warn"})[-1]
check("bash output is stdout and stderr", e["out"] == "ok\nwarn")
cmds = ["harno show a00013", "cd docs && harno ls --body", "cargo   build\n  && ./target/release/harno ls"]
check("every command exactly as written", [events("tool", tool_name="Bash", tool_input={"command": c}, tool_response={"stdout": ""})[-1]["summary"] for c in cmds] == cmds)
e = events("tool", tool_name="Task", tool_input={"description": "Find callers", "prompt": "Find every caller", "subagent_type": "Explore"},
           tool_response={"content": [{"type": "text", "text": "Two callers."}]})[-1]
check("a subagent task is its own kind", (e["type"], e["agent"], e["summary"], e["out"]) == ("subagent", "Explore", "Find callers", "Two callers."))

# The API turn of a call and what the agent wrote in it.
t = transcript(
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "thinking", "thinking": "Which dir?"}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "Listing."}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m2", "content": [{"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "ls"}}]}}),
)
call = lambda **f: events("tool", tool_name="Bash", tool_input={"command": "ls"}, tool_response="x", transcript_path=t, **f)[-1]
e = call(tool_use_id="t1")
check("a call names its turn and what was written in it", (e.get("turn"), e.get("said")) == ("m1", "Which dir?\n\nListing."))
check("without the id: the last call with the same input", call()["turn"] == "m2")
check("no message asked for it: no turn", "turn" not in call(tool_use_id="t9"))

# Calls that failed before, from the transcript.
lines = [
    json.dumps({"type": "user", "message": {"role": "user", "content": "go"}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}}),
    json.dumps({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": "a.md"}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "Trying."}, {"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "cat x"}}]}}),
    json.dumps({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t2", "is_error": True, "content": "refused: cat is not harno"}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m3", "content": [{"type": "tool_use", "id": "t3", "name": "Bash", "input": {"command": "harno ls"}}]}}),
]
t = transcript(*lines)
evs = events("tool", tool_name="Bash", tool_use_id="t3", tool_input={"command": "harno ls"}, tool_response="ok", transcript_path=t)
f = evs[0]
check("a failed call comes first", len(evs) == 2 and (f["summary"], f["ok"], f["turn"], f["said"], f["out"]) == ("cat x", False, "m2", "Trying.", "refused: cat is not harno") and "ok" not in evs[1])
lines += [
    json.dumps({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t3", "content": "ok"}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m4", "content": [{"type": "tool_use", "id": "t4", "name": "Bash", "input": {"command": "rm x"}}]}}),
    json.dumps({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t4", "is_error": True, "content": [{"type": "text", "text": "denied"}]}]}}),
]
t = transcript(*lines)
evs = events("response", last_assistant_message="Done.", transcript_path=t)
check("a failed last call comes with the answer", [(e["type"], e.get("summary")) for e in evs] == [("tool", "rm x"), ("response", None)])

# The response.
t = transcript(
    json.dumps({"type": "user", "message": {"role": "user", "content": "first prompt"}}),
    json.dumps({"type": "assistant", "message": {"id": "m0", "content": [{"type": "text", "text": "old answer"}]}}),
    json.dumps({"type": "user", "message": {"role": "user", "content": "second prompt"}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "Looking."}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t2", "name": "Bash", "input": {}}]}}),
    json.dumps({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
    json.dumps({"type": "assistant", "isSidechain": True, "message": {"id": "s1", "content": [{"type": "text", "text": "sub"}]}}),
    json.dumps({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "Done."}]}}),
)
e = events("response", transcript_path=t)[-1]
check("the answer comes from the transcript", e["text"] == "Looking.\n\nDone.")
check("two turns: one message in two lines, the subagent line is none", e["turns"] == 2)
check("the message given wins", events("response", last_assistant_message="Short.", transcript_path=t)[-1]["text"] == "Short.")
t = transcript(
    json.dumps({"type": "user", "message": {"role": "user", "content": "a prompt"}}),
    json.dumps({"type": "assistant", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {}}]}}),
    json.dumps({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}}),
    "",
)


def late():
    time.sleep(0.2)
    with open(t, "a") as f:
        f.write(json.dumps({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "Done."}]}}))


threading.Thread(target=late).start()
check("the hook waits for the answer line", events("response", last_assistant_message="Done.", transcript_path=t)[-1]["turns"] == 2)

# Refusals.
check("a hook on the wrong event", refused("prompt", "'prompt' was called for a Stop event; attach it to the UserPromptSubmit hook", hook_event_name="Stop"))
check("an event harno does not log", refused("tool", "does not log Claude Code's Notification event", hook_event_name="Notification"))
check("an unknown action", refused("notify", "actions: session-start, prompt, tool, response, session-end"))
check("no session id", refused("prompt", "no session_id", session_id=""))

# The script: outside a project nothing, session-start writes the env file.
project = TMP / "project"
(project / ".harno").mkdir(parents=True)
(project / ".harno" / "config.yml").write_text("version: 1\n")
env_file = TMP / "env"


def script(action, cwd, **fields):
    data = dict(session_id="abc-123", hook_event_name=EVENTS[action], cwd=str(cwd), **fields)
    env = dict(os.environ, CLAUDE_ENV_FILE=str(env_file))
    return subprocess.run([sys.executable, str(HERE / "hook.py"), action], input=json.dumps(data), capture_output=True, text=True, env=env)


r = script("session-start", project, source="startup")
check("session-start writes the env file and prints nothing",
      r.returncode == 0 and r.stdout == "" and env_file.read_text() == "export HARNO_SESSION=abc-123\n")
r = script("prompt", TMP, prompt="hello")
check("outside a project nothing happens", r.returncode == 0 and not (TMP / ".harno").exists())

# The log of this harness, beside the log of the gate.
gate = project / ".harno/log/harno"
gate.mkdir(parents=True)
script("prompt", project, prompt="do it")
# The calls of the gate are written between the prompt and the tool call that made them.
now = hook.now()
(gate / "abc-123.jsonl").write_text(
    json.dumps({"n": 1, "ts": now, "cwd": ".", "verb": "run", "args": ["ci"]}) + "\n"
    + json.dumps({"n": 2, "ts": now, "cwd": ".", "verb": "run", "args": ["mdmap", "ls"]}) + "\n")
script("tool", project, tool_name="Bash", tool_input={"command": "harno run ci"}, tool_response={"stdout": "18/18 ran"})
script("tool", project, tool_name="Read", tool_input={"file_path": "/p/a.md"}, tool_response="# A")
mine = project / ".harno/log/claude-code"
own = [json.loads(l) for l in (mine / "abc-123.jsonl").read_text().splitlines()]
check("the harness numbers the lines of its own log", [e["n"] for e in own] == [1, 2, 3, 4] and [e["type"] for e in own] == ["session", "prompt", "tool", "tool"])
check("a prompt keeps its size, its text beside the line", own[1]["chars"] == 5 and (mine / "abc-123/0002.out").read_text() == "do it")
check("a call through the gate claims its line and keeps no text of its own",
      own[2].get("harno") == [1] and "chars" not in own[2] and not (mine / "abc-123/0003.out").exists())
check("another tool keeps what it returned", own[3]["chars"] == 3 and (mine / "abc-123/0004.out").read_text() == "# A")
# An argument with a quote in it reaches harno without the quoting the agent wrote around it.
time.sleep(0.01)
with open(gate / "abc-123.jsonl", "a") as f:
    f.write(json.dumps({"n": 3, "ts": hook.now(), "cwd": ".", "verb": "run", "args": ["write", "--", "a.txt", "it" + chr(39) + "s \"so\""]}) + "\n")
quoted = "harno run write -- a.txt " + chr(39) + "it" + chr(39) + "\\" + chr(39) + chr(39) + "s \"so\"" + chr(39)
script("tool", project, tool_name="Bash", tool_input={"command": quoted}, tool_response={"stdout": "created"})
own = [json.loads(l) for l in (mine / "abc-123.jsonl").read_text().splitlines()]
check("a call with a quote inside an argument claims its line", own[4].get("harno") == [3])

for name, ok in checks:
    if not ok:
        print("failed:", name)
print(f"{sum(ok for _, ok in checks)}/{len(checks)} ok")
sys.exit(0 if all(ok for _, ok in checks) else 1)

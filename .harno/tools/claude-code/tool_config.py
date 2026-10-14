"""The `config` of the tool.yml next to this file ($TOOL_YML names
another), read without a YAML library: one `key: value` per line, the value
true, false or a string."""
import os
import pathlib


def config():
    path = os.environ.get("TOOL_YML") or pathlib.Path(__file__).with_name("tool.yml")
    out, inside = {}, False
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line.startswith(" "):
            inside = line.split("#")[0].strip() == "config:"
            continue
        if inside and ":" in line:
            key, value = line.split(":", 1)
            value = value.split(" #")[0].strip().strip("\"'")
            out[key.strip()] = {"true": True, "false": False}.get(value, value)
    return out

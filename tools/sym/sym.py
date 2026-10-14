#!/usr/bin/env python3
"""Items of source files by name, through ast-grep. Glue only: ast-grep does the parsing.

    sym.py def <name>...        file:start-end and the signature line
    sym.py body <name>...       the item with its doc comments and attributes
    ... --in <path>             look only in that file or folder
    sym.py outline <path>...    every item in the files, with its span

A name is `load`, or `Graph::load` for a method of one impl, trait or class. Each
language is one scan, and ast-grep reads a file only in the language of its
extension: Rust, TypeScript, TSX, JavaScript and Python. Without --in, def and body
look under `roots` of the config of tool.yml, comma separated.
"""
import json
import pathlib
import subprocess
import sys


def kinds(names):
    return [{"kind": k} for k in names]


RUST = ["function_item", "function_signature_item", "struct_item", "enum_item",
        "trait_item", "type_item", "const_item", "static_item", "macro_definition", "mod_item"]
JS = ["function_declaration", "generator_function_declaration", "class_declaration", "method_definition"]
TS = JS + ["abstract_class_declaration", "interface_declaration", "type_alias_declaration", "enum_declaration"]
# A `const Name = …` at the top of a file, which is how components and hooks are written.
TOP = [{"kind": "variable_declarator", "inside": {"kind": "lexical_declaration",
        "inside": {"any": [{"kind": "program"}, {"kind": "export_statement"}]}}}]
CLASS = [("class_declaration", "name", "$"), ("abstract_class_declaration", "name", "$")]
C_DOCS = ("/**", "*", "//", "@")
# The items, what outline adds to them, what a method sits in, the lines of its docs, the width of a level.
LANGS = {
    "rust": (kinds(RUST), kinds(["impl_item"]), [("impl_item", "type", r"\b"), ("trait_item", "name", "$")],
             ("///", "#[", "//!"), 4),
    "typescript": (kinds(TS) + TOP, [], CLASS, C_DOCS, 2),
    "tsx": (kinds(TS) + TOP, [], CLASS, C_DOCS, 2),
    "javascript": (kinds(JS) + TOP, [], CLASS[:1], C_DOCS, 2),
    "python": (kinds(["function_definition", "class_definition"]), [], [("class_definition", "name", "$")],
               ("#", "@"), 4),
}
QUOTES = "\"" + chr(39)


def roots():
    """Where def and body look without --in: `roots` of the config of the tool.yml beside
    this file, comma separated; the whole project when it names none."""
    path = pathlib.Path(__file__).with_name("tool.yml")
    inside = False
    for line in path.read_text().splitlines() if path.exists() else []:
        if not line.startswith(" "):
            inside = line.split("#")[0].strip() == "config:"
        elif inside and line.strip().startswith("roots:"):
            value = line.split(":", 1)[1].split(" #")[0].strip().strip(QUOTES)
            return [r.strip() for r in value.split(",") if r.strip()] or ["."]
    return ["."]


def rule(lang, name=None):
    items, outline, owners = LANGS[lang][:3]
    r = {"any": items + ([] if name else outline)}
    if name:
        owner, _, item = name.rpartition("::")
        r = {"all": [r, {"has": {"field": "name", "regex": f"^{item}$"}}]}
        if owner:
            r["all"].append({"inside": {"stopBy": "end", "any": [
                {"kind": k, "has": {"field": f, "regex": f"^{owner}{end}"}} for k, f, end in owners]}})
    return json.dumps({"id": "sym", "language": lang, "rule": r})


def scan(r, paths):
    out = subprocess.run(["ast-grep", "scan", "--inline-rules", r, "--json=stream", *paths],
                         capture_output=True, text=True)
    if out.returncode not in (0, 1):
        sys.exit(out.stderr.strip() or f"ast-grep exited {out.returncode}")
    return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


def span(m):
    return m["range"]["start"]["line"] + 1, m["range"]["end"]["line"] + 1


def signature(m):
    return m["lines"].split("\n")[0].strip().rstrip("{").strip()


def with_docs(m):
    """Start line moved up over doc comments and attributes."""
    start, end = span(m)
    lines = open(m["file"]).read().split("\n")
    while start > 1 and lines[start - 2].strip().startswith(LANGS[m["lang"]][3]):
        start -= 1
    return start, end, lines


def main(argv):
    if len(argv) < 2 or argv[0] not in ("def", "body", "outline"):
        sys.exit(__doc__.strip())
    mode, args = argv[0], argv[1:]
    paths = roots()
    if "--in" in args:
        i = args.index("--in")
        paths, args = args[i + 1:i + 2], args[:i] + args[i + 2:]
    missing, last = [], None
    for name in ([None] if mode == "outline" else args):
        found = [dict(m, lang=lang) for lang in LANGS
                 for m in scan(rule(lang, name), args if mode == "outline" else paths)]
        if not found:
            # outline takes files, not a name: it says which files came back empty.
            missing.append(name or " ".join(args))
            continue
        found.sort(key=lambda m: (m["file"], span(m)[0]))
        for m in found:
            a, b = span(m)
            file = m["file"]
            if mode == "body":
                start, end, lines = with_docs(m)
                print(f"── {file}:{start}-{end}")
                for n in range(start, end + 1):
                    print(f"{n:>5} {lines[n - 1]}")
            elif mode == "outline":
                if file != last:
                    last = file
                    print(last)
                first = m["lines"].split("\n")[0]
                indent = "  " * ((len(first) - len(first.lstrip())) // LANGS[m["lang"]][4])
                print(f"  {a}-{b}  {indent}{signature(m)}")
            else:
                print(f"{file}:{a}-{b}  {signature(m)}")
    if missing:
        sys.exit("not found: " + ", ".join(missing))


if __name__ == "__main__":
    main(sys.argv[1:])

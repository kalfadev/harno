#!/usr/bin/env python3
"""A literal text and every place it is mentioned (harno tool add text).

    text.py [--type <t>] [--in <path>]... [<rg flag>...] <word or phrase...> [<path>...]

The trailing arguments that name an existing path are where to look; the words before
them are the text, joined by one space, so `text -- fn reconnect crates/` needs no
quotes. The text is literal, every sign escaped, and whole-word at each end that is a
word character: `foo(` and `Name {` are found as they read, never parsed as a regex,
and `foo(` still matches `foo(x`, which rg -w would miss. With no path given, the --in
paths are searched.
A trailing word with a slash that names no path is refused, since it was meant as one.
"""
import os
import re
import subprocess
import sys

VALUED = {"-m", "--max-count", "-A", "-B", "-C", "-g", "--glob", "-t", "-T"}
SIGNS = set("\\.+*?()|[]{}^$")


def pattern(text):
    body = "".join("\\" + c if c in SIGNS else c for c in text)
    edge = lambda c: r"\b" if re.match(r"\w", c) else ""
    return edge(text[0]) + body + edge(text[-1])


def split(argv):
    kinds, default, flags, rest = [], [], [], []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a in ("--type", "--in") and i + 1 < len(argv):
            (kinds if a == "--type" else default).append(argv[i + 1])
            i += 2
        elif not rest and a.startswith("-") and len(a) > 1:
            flags.append(a)
            if a in VALUED and i + 1 < len(argv):
                flags.append(argv[i + 1])
                i += 1
            i += 1
        else:
            rest.append(a)
            i += 1
    paths = []
    while len(rest) > 1 and os.path.exists(rest[-1]):
        paths.insert(0, rest.pop())
    return kinds, flags, rest, paths or default


def main(argv):
    kinds, flags, words, paths = split(argv)
    if not words:
        sys.exit("usage: <word or phrase> [<path>...]")
    for w in words[1:]:
        if "/" in w:
            # exit 2, as rg for a missing path: 1 is "no match", which the filter lets pass
            print(f"no path {w}: a trailing word with a slash is taken for a path; name one that is there", file=sys.stderr)
            sys.exit(2)
    cmd = ["rg", "-n", "--no-heading", *flags, *(f"--type={k}" for k in kinds), "-e", pattern(" ".join(words)), *paths]
    sys.exit(subprocess.run(cmd).returncode)


if __name__ == "__main__":
    main(sys.argv[1:])

#!/usr/bin/env python3
"""Moves the inline test modules of Rust source files into files of their own.

usage: scripts/dev/move_inline_tests.py [--replace] <file.rs>...

An inline module is `#[cfg(test)]`, any further attributes, and `mod name {`
at column zero, closed by the first `}` at column zero that is code. Its body,
dedented one level, becomes `tests/<stem>_<name>.rs` beside the file, and the
module stays declared in place through `#[path]`, so `use super::*` keeps
working. A line that starts inside a string literal or a block comment is
data, not code: it is neither dedented nor taken for the module's end, so a
raw string's indentation and a `}` at the start of one of its lines survive
the move.

A target file that already exists with other content is an error unless
`--replace` is given, which rewrites it from the inline module. Pass it only
to redo this script's own earlier output: a file with that name may belong to
another module of the same source file.
"""
import pathlib
import re
import sys

MOD = re.compile(r"^mod ([A-Za-z_][A-Za-z0-9_]*) \{$")
RAW_START = re.compile(r'b?r(#*)"')
CHAR_LITERAL = re.compile(r"'(\\u\{[0-9A-Fa-f]+\}|\\.|[^\\'])'")


def scan(line: str, state):
    """The literal or comment `line` ends inside, starting from `state`:
    None for code, ("str",), ("raw", hashes) or ("block", depth)."""
    i = 0
    n = len(line)
    while i < n:
        if state is None:
            if line.startswith("//", i):
                return None
            if line.startswith("/*", i):
                state = ("block", 1)
                i += 2
                continue
            raw = RAW_START.match(line, i)
            if raw and (i == 0 or not (line[i - 1].isalnum() or line[i - 1] == "_")):
                state = ("raw", raw.group(1))
                i = raw.end()
                continue
            if line[i] == '"':
                state = ("str",)
                i += 1
                continue
            if line[i] == "'":
                char = CHAR_LITERAL.match(line, i)
                i = char.end() if char else i + 1
                continue
            i += 1
        elif state[0] == "str":
            if line[i] == "\\":
                i += 2
                continue
            if line[i] == '"':
                state = None
            i += 1
        elif state[0] == "raw":
            close = '"' + state[1]
            end = line.find(close, i)
            if end < 0:
                return state
            state = None
            i = end + len(close)
        else:
            if line.startswith("/*", i):
                state = ("block", state[1] + 1)
                i += 2
            elif line.startswith("*/", i):
                state = None if state[1] == 1 else ("block", state[1] - 1)
                i += 2
            else:
                i += 1
    return state


def starts_in_literal(lines: list[str]) -> list[bool]:
    inside = []
    state = None
    for line in lines:
        inside.append(state is not None)
        state = scan(line, state)
    return inside


def stem_for(path: pathlib.Path) -> str:
    stem = path.stem
    if stem in ("lib", "mod"):
        crate = path.parent.parent.name if path.parent.name == "src" else path.parent.name
        stem = crate.replace("-", "_")
        if stem.startswith("cranpose_"):
            stem = stem[len("cranpose_"):]
    return stem


def split_out(path: pathlib.Path, replace: bool) -> int:
    lines = path.read_text().split("\n")
    literal = starts_in_literal(lines)
    out = []
    moved = 0
    i = 0
    while i < len(lines):
        if lines[i] != "#[cfg(test)]":
            out.append(lines[i])
            i += 1
            continue
        j = i + 1
        attributes = []
        while j < len(lines) and lines[j].startswith("#[") and not lines[j].startswith("#[path"):
            attributes.append(lines[j])
            j += 1
        match = MOD.match(lines[j]) if j < len(lines) else None
        if match is None:
            out.append(lines[i])
            i += 1
            continue
        name = match.group(1)
        end = j + 1
        while end < len(lines) and (lines[end] != "}" or literal[end]):
            end += 1
        if end >= len(lines):
            raise SystemExit(f"{path}: module {name} never closes")
        body = [
            line[4:] if line.startswith("    ") and not literal[k] else line
            for k, line in enumerate(lines[j + 1:end], start=j + 1)
        ]
        while body and body[-1] == "":
            body.pop()
        target_name = f"{stem_for(path)}_{name}.rs" if name != "tests" else f"{stem_for(path)}_tests.rs"
        target = path.parent / "tests" / target_name
        content = "\n".join(body) + "\n"
        if target.exists() and target.read_text() != content and not replace:
            raise SystemExit(f"{target} exists with other content; pass --replace to rewrite it")
        target.parent.mkdir(exist_ok=True)
        target.write_text(content)
        out.append("#[cfg(test)]")
        out.extend(attributes)
        out.append(f'#[path = "tests/{target_name}"]')
        out.append(f"mod {name};")
        moved += 1
        i = end + 1
    if moved:
        path.write_text("\n".join(out))
    return moved


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    replace = "--replace" in argv
    total = 0
    for name in [arg for arg in argv[1:] if arg != "--replace"]:
        path = pathlib.Path(name)
        moved = split_out(path, replace)
        total += moved
        print(f"{path}: {moved} module(s) moved")
    print(f"{total} module(s) moved")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

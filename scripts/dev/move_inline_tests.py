#!/usr/bin/env python3
"""Moves the inline test modules of Rust source files into files of their own.

usage: scripts/dev/move_inline_tests.py [--replace] <file.rs>...

An inline module is `#[cfg(test)]`, any further attributes, and `mod name {`
at column zero, closed by the first `}` at column zero. Its body, dedented
one level, becomes `tests/<stem>_<name>.rs` beside the file, and the module
stays declared in place through `#[path]`, so `use super::*` keeps working.
A target file that already exists with other content is an error unless
`--replace` is given, which rewrites it from the inline module.
"""
import pathlib
import re
import sys

MOD = re.compile(r"^mod ([A-Za-z_][A-Za-z0-9_]*) \{$")


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
        while end < len(lines) and lines[end] != "}":
            end += 1
        if end >= len(lines):
            raise SystemExit(f"{path}: module {name} never closes")
        body = [line[4:] if line.startswith("    ") else line for line in lines[j + 1:end]]
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

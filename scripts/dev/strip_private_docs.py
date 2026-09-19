#!/usr/bin/env python3
"""Removes every comment that is not documentation of a public API.

usage: scripts/dev/strip_private_docs.py <file.rs>...

Kept: a `///` block that documents an item declared `pub` (not `pub(crate)`
or `pub(super)`), a variant or member of a `pub enum` or `pub trait`, or an
exported macro, in a crate under `crates/`, and `//!` module docs in such a
crate outside its test files. Removed: every other `///` block, every `//!`
in applications and test files, and every plain `//` line. Run `cargo fmt`
afterwards to fold the blank lines that are left.
"""
import pathlib
import sys

DOC = "///"
INNER = "//!"
PUBLIC_BLOCKS = ("pub enum ", "pub trait ", "pub unsafe trait ")


def published(path: pathlib.Path) -> bool:
    parts = path.parts
    return "crates" in parts and "tests" not in parts


def indentation(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def documents_public_item(lines: list[str], index: int, public_depths: list[int]) -> bool:
    exported = False
    while index < len(lines):
        stripped = lines[index].strip()
        if stripped.startswith("#["):
            exported = exported or stripped.startswith("#[macro_export]")
            index += 1
            continue
        if stripped.startswith("pub "):
            return True
        if exported and stripped.startswith("macro_rules!"):
            return True
        return bool(public_depths) and indentation(lines[index]) > public_depths[-1]
    return False


def track_public_block(line: str, public_depths: list[int]) -> None:
    stripped = line.strip()
    depth = indentation(line)
    if public_depths and stripped == "}" and depth == public_depths[-1]:
        public_depths.pop()
    elif stripped.startswith(PUBLIC_BLOCKS) and stripped.endswith("{"):
        public_depths.append(depth)


def strip(path: pathlib.Path) -> int:
    lines = path.read_text().split("\n")
    keep_public_docs = published(path)
    out = []
    removed = 0
    public_depths: list[int] = []
    i = 0
    while i < len(lines):
        stripped = lines[i].strip()
        track_public_block(lines[i], public_depths)
        if stripped.startswith(DOC):
            end = i
            while end < len(lines) and lines[end].strip().startswith(DOC):
                end += 1
            if keep_public_docs and documents_public_item(lines, end, public_depths):
                out.extend(lines[i:end])
            else:
                removed += end - i
            i = end
            continue
        if stripped.startswith(INNER):
            if keep_public_docs:
                out.append(lines[i])
            else:
                removed += 1
            i += 1
            continue
        if stripped.startswith("//"):
            removed += 1
            i += 1
            continue
        out.append(lines[i])
        i += 1
    if removed:
        path.write_text("\n".join(out))
    return removed


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    total = 0
    for name in argv[1:]:
        path = pathlib.Path(name)
        removed = strip(path)
        total += removed
        print(f"{path}: {removed} line(s) removed")
    print(f"{total} line(s) removed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

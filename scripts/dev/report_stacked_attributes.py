#!/usr/bin/env python3
"""Reports attributes stacked on one item, and what they landed on.

    report_stacked_attributes.py <file.rs> [<file.rs> ...]

Deleting items whose attributes sat on lines of their own leaves the
attributes behind, stacked above whatever item came next. They compile, and
the survivor silently takes over the item below it — a `#[cfg]` meant for a
deleted struct quietly excluding the enum that followed, which only some
target's build will notice.

This only reports: whether the attribute belongs on the item below is a
question about the item, not one a script can answer. It prints the file, the
line the run starts on, how many copies there are, and the first line of what
they now apply to. It exits 1 when it finds any, so a gate can run it.
"""

import re
import sys

ATTRIBUTE = re.compile(r"^\s*#!?\[")


def attribute_blocks(lines):
    index = 0
    while index < len(lines):
        if not ATTRIBUTE.match(lines[index]):
            index += 1
            continue
        depth = 0
        start = index
        while index < len(lines):
            depth += lines[index].count("[") - lines[index].count("]")
            index += 1
            if depth <= 0:
                break
        yield start, index, "".join(lines[start:index])


def item_after(lines, index):
    while index < len(lines):
        line = lines[index].strip()
        if line and not ATTRIBUTE.match(lines[index]):
            return line
        index += 1
    return "<end of file>"


def runs(lines):
    blocks = list(attribute_blocks(lines))
    found = []
    repeats = 1
    for earlier, later in zip(blocks, blocks[1:]):
        if earlier[1] == later[0] and earlier[2] == later[2]:
            repeats += 1
            continue
        if repeats > 1:
            found.append((earlier[0] - (repeats - 1), repeats, earlier[1]))
        repeats = 1
    if repeats > 1 and blocks:
        last = blocks[-1]
        found.append((last[0] - (repeats - 1), repeats, last[1]))
    return [
        (start + 1, count, item_after(lines, end)) for start, count, end in found
    ]


def main(paths):
    stacked = 0
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            lines = handle.readlines()
        for line, count, item in runs(lines):
            stacked += 1
            print(f"{path}:{line}: {count} copies of the same attribute, now on: {item}")
    if stacked:
        print(f"{stacked} stacked attribute run(s); check what each one landed on")
        return 1
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(main(sys.argv[1:]))

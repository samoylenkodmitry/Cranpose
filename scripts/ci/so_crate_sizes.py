#!/usr/bin/env python3
"""Per-crate size of an unstripped native library, as a Markdown report.

Usage: so_crate_sizes.py LIBRARY --llvm-bin DIR [--top N]

Sums the sizes `llvm-nm` reports for the library's code and data symbols by
the crate each demangled path starts with. A generic instantiation counts
toward the crate its path names first: `core::ptr::drop_in_place<T>` is
core's, whatever `T` is, and `<cranpose_ui::X as core::fmt::Debug>::fmt` is
cranpose_ui's. Symbols with no Rust path (the C runtime, C dependencies) are
counted together. The report opens with the library stripped, as an APK
carries it, section by section: much of `.rodata` (literals, embedded assets)
has no symbol. `DIR` holds the NDK's `llvm-nm`, `llvm-strip` and `llvm-size`.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter

# Symbol types that take space in the file: code, read-only data and data,
# local or global, and weak definitions. Zero-initialised data (b, B) does not.
SIZED_TYPES = set("tTwWrRdDvV")

# Rust's legacy mangling escapes, left in place by llvm-nm's C++ demangler.
LEGACY_ESCAPES = {
    "SP": "@",
    "BP": "*",
    "RF": "&",
    "LT": "<",
    "GT": ">",
    "LP": "(",
    "RP": ")",
    "C": ",",
}
LEGACY_ESCAPE = re.compile(r"\$(SP|BP|RF|LT|GT|LP|RP|C|u[0-9a-fA-F]+)\$")
LEADING_NOISE = re.compile(r"^(?:[<&*(\[ ]|_(?=<)|mut |const |dyn |impl )+")
CRATE = re.compile(r"^([A-Za-z_][A-Za-z0-9_]*)(?:\[[0-9a-f]+\])?::")
NON_RUST = "(no Rust path)"


def decode_legacy(name):
    """Undoes the `$LT$`-style escapes and `..` separators of legacy mangling."""

    def escape(match):
        code = match.group(1)
        if code in LEGACY_ESCAPES:
            return LEGACY_ESCAPES[code]
        return chr(int(code[1:], 16))

    return LEGACY_ESCAPE.sub(escape, name).replace("..", "::")


def crate_of(name):
    """The crate a demangled symbol's path starts with; for an impl on a type
    with no path, such as `<[u8] as core::fmt::Debug>::fmt`, the trait's."""
    path = decode_legacy(name)
    for candidate in (path, path.partition(" as ")[2]):
        match = CRATE.match(LEADING_NOISE.sub("", candidate))
        if match:
            return match.group(1)
    return NON_RUST


def crate_sizes(nm_lines):
    """Bytes per crate, from `llvm-nm --print-size` lines."""
    sizes = Counter()
    for line in nm_lines:
        fields = line.split(maxsplit=3)
        if len(fields) != 4 or fields[2] not in SIZED_TYPES:
            continue
        sizes[crate_of(fields[3])] += int(fields[1], 16)
    return sizes


def read_symbols(nm, library):
    result = subprocess.run(
        [nm, "--print-size", "--defined-only", "--demangle", library],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.splitlines()


def stripped_sections(llvm_bin, library):
    """The stripped library's size and its sections of at least 1% of it,
    largest first, from `llvm-size -A`."""
    with tempfile.TemporaryDirectory() as scratch:
        stripped = os.path.join(scratch, "stripped.so")
        strip = os.path.join(llvm_bin, "llvm-strip")
        subprocess.run([strip, "--strip-all", "-o", stripped, library], check=True)
        size = os.path.getsize(stripped)
        listing = subprocess.run(
            [os.path.join(llvm_bin, "llvm-size"), "-A", stripped],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    sections = []
    for line in listing.splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[0].startswith(".") and fields[1].isdigit():
            sections.append((fields[0], int(fields[1])))
    sections.sort(key=lambda section: -section[1])
    return size, [(name, bytes_) for name, bytes_ in sections if bytes_ * 100 >= size]


def kilobytes(size):
    return f"{size / 1024:,.0f}"


def report(library, sizes, top, stripped):
    total = sum(sizes.values())
    shipped, sections = stripped
    listed = ", ".join(f"`{name}` {kilobytes(size)}" for name, size in sections)
    lines = [
        f"### `{os.path.basename(library)}` by crate",
        "",
        f"Stripped, as an APK carries it: **{kilobytes(shipped)} KB** ({listed} KB).",
        f"Named code and data symbols: {kilobytes(total)} KB.",
    ]
    lines += ["", "| crate | KB | share |", "| --- | ---: | ---: |"]
    ranked = sizes.most_common()
    for crate, size in ranked[:top]:
        lines.append(f"| {crate} | {kilobytes(size)} | {100 * size / total:.1f}% |")
    rest = ranked[top:]
    if rest:
        size = sum(size for _, size in rest)
        lines.append(
            f"| {len(rest)} more crates | {kilobytes(size)} | {100 * size / total:.1f}% |"
        )
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("library")
    parser.add_argument("--llvm-bin", required=True)
    parser.add_argument("--top", type=int, default=25)
    args = parser.parse_args()
    sizes = crate_sizes(read_symbols(os.path.join(args.llvm_bin, "llvm-nm"), args.library))
    if not sizes:
        sys.exit(f"{args.library} has no sized symbols: is it stripped?")
    stripped = stripped_sections(args.llvm_bin, args.library)
    print(report(args.library, sizes, args.top, stripped))


if __name__ == "__main__":
    main()

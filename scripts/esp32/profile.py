#!/usr/bin/env python3
"""Prints the flat profile of a log that scripts/esp32/sampler wrote: the
share of samples of each function, hottest first, and with `--crates` the
share and code size of each crate.

Usage: profile.py ELF LOG [--top N] [--crates]
"""

import argparse
import bisect
import glob
import pathlib
import re
import subprocess
import sys


def find_nm():
    pattern = str(pathlib.Path.home() / ".espressif/tools/xtensa-esp-elf/*/xtensa-esp-elf/bin/xtensa-esp-elf-nm")
    found = sorted(glob.glob(pattern))
    if not found:
        sys.exit(f"no Xtensa nm at {pattern}")
    return found[-1]


def functions(elf, demangle):
    """The functions of `elf` in address order: starts, sizes and names."""
    flags = ["-n", "-S", "--defined-only"] + (["-C"] if demangle else [])
    out = subprocess.run([find_nm(), *flags, str(elf)], capture_output=True, text=True, check=True).stdout
    starts, sizes, names = [], [], []
    for line in out.splitlines():
        parts = line.split(" ", 3)
        if len(parts) == 4 and parts[2] in "tTwW":
            starts.append(int(parts[0], 16))
            sizes.append(int(parts[1], 16))
            names.append(parts[3])
    return starts, sizes, names


def samples(log, starts, sizes):
    """The samples of each function index, from the `SPC` lines of `log`."""
    hits = {}
    for line in open(log, errors="replace"):
        match = re.search(r"SPC ([0-9a-f]{8}) (\d+)", line)
        if match:
            pc = int(match.group(1), 16)
            index = bisect.bisect_right(starts, pc) - 1
            if index >= 0 and pc < starts[index] + sizes[index]:
                hits[index] = hits.get(index, 0) + int(match.group(2))
    return hits


def crate(name):
    match = re.match(r"<*([a-z_][a-z0-9_]*)::", name)
    return match.group(1) if match else "C code"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf")
    parser.add_argument("log")
    parser.add_argument("--top", type=int, default=40)
    parser.add_argument("--crates", action="store_true")
    args = parser.parse_args()
    starts, sizes, names = functions(args.elf, demangle=True)
    hits = samples(args.log, starts, sizes)
    idle = {i for i in hits if names[i] in ("esp_cpu_wait_for_intr", "cpu_ll_waiti")}
    total = sum(hits.values())
    busy = total - sum(hits[i] for i in idle)
    if not busy:
        sys.exit(f"{args.log} has no samples outside the idle wait")
    print(f"{total} samples, {100 * busy / total:.1f}% outside the idle wait")
    rows = sorted((i for i in hits if i not in idle), key=lambda i: -hits[i])
    for i in rows[: args.top]:
        name = re.sub(r"::h[0-9a-f]{16}$", "", names[i])
        print(f"{100 * hits[i] / busy:6.2f}% {sizes[i] / 1024:6.1f} KB  {name[:120]}")
    if args.crates:
        groups = {}
        for i in rows:
            group = groups.setdefault(crate(names[i]), [0, 0, 0])
            group[0] += hits[i]
            group[1] += sizes[i]
            group[2] += 1
        print()
        for name, (count, size, n) in sorted(groups.items(), key=lambda kv: -kv[1][0]):
            print(f"{100 * count / busy:6.2f}% {size / 1024:7.1f} KB in {n:3d} functions  {name}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Prints the flat profile of a log that scripts/esp32/sampler wrote: the
share of samples of each function, hottest first, and with `--crates` the
share and code size of each crate.

Samples in the chip's ROM, such as its soft float and `memcpy`, count under
the ROM's own function names (from Espressif's `esp-rom-elfs`) when the log
names its chip on an `MCU` line, as board_run.py and sim_profile.py write
it. Samples in no known function count as unknown.

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


def rom_functions(log):
    """The functions of the ROM of the chip `log` names, as `functions` gives
    them, each up to the next, with " (ROM)" after its name: none when the
    log names no chip or its ROM image is not installed."""
    match = re.search(r"^MCU (\S+)$", open(log, errors="replace").read(), re.M)
    if not match:
        return [], [], []
    pattern = str(pathlib.Path.home() / f".espressif/tools/esp-rom-elfs/*/{match.group(1)}_rev*_rom.elf")
    # The latest revision: a later chip runs the ROM of its revision.
    images = sorted(glob.glob(pattern), key=lambda path: int(re.search(r"_rev(\d+)_rom", path).group(1)))
    if not images:
        return [], [], []
    out = subprocess.run([find_nm(), "-n", "--defined-only", images[-1]],
                         capture_output=True, text=True, check=True).stdout
    found = []
    for line in out.splitlines():
        parts = line.split(" ", 2)
        if len(parts) == 3 and parts[1] in "tTwW":
            found.append((int(parts[0], 16), parts[2]))
    starts = [start for start, _ in found]
    sizes = [after - start for start, after in zip(starts, starts[1:] + starts[-1:])]
    return starts, sizes, [f"{name} (ROM)" for _, name in found]


def samples(log, starts, sizes):
    """The samples of each function index, from the `SPC` lines of `log`,
    and how many fell in none."""
    hits = {}
    unknown = 0
    for line in open(log, errors="replace"):
        match = re.search(r"SPC ([0-9a-f]{8}) (\d+)", line)
        if match:
            pc = int(match.group(1), 16)
            index = bisect.bisect_right(starts, pc) - 1
            if index >= 0 and pc < starts[index] + sizes[index]:
                hits[index] = hits.get(index, 0) + int(match.group(2))
            else:
                unknown += int(match.group(2))
    return hits, unknown


def crate(name):
    if name.endswith(" (ROM)"):
        return "ROM"
    match = re.match(r"<*([a-z_][a-z0-9_]*)::", name)
    return match.group(1) if match else "C code"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf")
    parser.add_argument("log")
    parser.add_argument("--top", type=int, default=40)
    parser.add_argument("--crates", action="store_true")
    args = parser.parse_args()
    app = functions(args.elf, demangle=True)
    rom = rom_functions(args.log)
    # ROM code lies below the app's code, so the two lists stay in order.
    starts, sizes, names = (rom_part + app_part for rom_part, app_part in zip(rom, app))
    hits, unknown = samples(args.log, starts, sizes)
    idle = {i for i in hits if names[i] in ("esp_cpu_wait_for_intr", "cpu_ll_waiti")}
    total = sum(hits.values()) + unknown
    busy = total - sum(hits[i] for i in idle)
    if not busy:
        sys.exit(f"{args.log} has no samples outside the idle wait")
    print(f"{total} samples, {100 * busy / total:.1f}% outside the idle wait, "
          f"{100 * unknown / busy:.2f}% of those in no known function")
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

#!/usr/bin/env python3
"""Compares revisions of the ESP32 demo on a board, every screen from one
image, over several placements of the code.

A screen's frame time on these chips moves by up to 0.1 ms with where the
linker puts code the order file leaves out, which any change shifts, so one
build per side measures luck. Each revision builds once, with the demo's
`screens-in-turn` feature, and is linked again for each placement: the
order file as it is, and with empty gaps a seed picks between its patterns
(the same gaps for every revision). The runs go placement by placement,
revision by revision; the report gives each screen's mean frame time and
instructions a frame, and each revision's mean, smallest and largest
difference from the first, placement by placement.

Builds run in one worktree for each board, `<repository>-esp-ab-<board>`
beside the main checkout, made on the first run: two worktrees of one commit
build different code, as cargo hashes a path crate's metadata from its path.
A build and its links stay under that worktree's target/esp32-ab, kept by
commit, so the base of the next comparison does not build again.

Run it in the Xtensa Rust environment (`. ~/export-esp-1.98.1.sh`).

Usage: board_ab.py --board e32r35t BASE REVISION [REVISION...]
"""

import argparse
import json
import os
import pathlib
import random
import re
import shutil
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "benchmarks/esp32"))
import board_run  # noqa: E402
import summary  # noqa: E402

ROOT = board_run.ROOT
# Each screen shows for 6 s; the boot takes about 1.5 s more.
SCREEN_SECONDS = 6
BOOT_SECONDS = 2
# The classic ESP32 runs out of memory on the dashboard and on the changing
# paths, so it shows the demo's four screens only. The ES3C35P shows four
# more: the changing paths, the deep moves, the still wall and the dashboard.
LARGE_SCREENS = {"es3c35p": True, "e32r35t": False}
GAPS = 6
# Below this many bytes; a cache way is 4 KB on the ES3C35P. The E32R35T
# maps 3,342,304 bytes of flash code, of which the demo leaves about 15 KB,
# so its gaps stay within a third of that.
LARGEST_GAP = {"es3c35p": 4096, "e32r35t": 2048}


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


def worktree(board):
    """The board's build worktree, made at the base on first use."""
    common = pathlib.Path(git("rev-parse", "--path-format=absolute", "--git-common-dir"))
    tree = common.parent.parent / f"{common.parent.name}-esp-ab-{board}"
    if not tree.exists():
        git("worktree", "add", "--detach", str(tree), "HEAD")
    return tree


def gapped(order, seed, out, largest):
    """`order` with an empty gap before its first pattern and `GAPS` more
    where `seed` picks, each a multiple of 4 bytes below `largest`."""
    lines = order.read_text().splitlines()
    patterns = [index for index, line in enumerate(lines) if line.lstrip().startswith("*(")]
    rng = random.Random(seed)
    places = sorted({patterns[0], *rng.sample(patterns[1:], GAPS)})
    sizes = {place: 4 * rng.randrange(largest // 4) for place in places}
    text = []
    for index, line in enumerate(lines):
        if index in sizes:
            text.append(f"  . = . + {sizes[index]};")
        text.append(line)
    out.write_text("\n".join(text) + "\n")


def relink(kept, order, elf):
    saved = json.loads((kept / "args.json").read_text())
    flag = "-Wl,--section-ordering-file,"
    arguments = [flag + str(order) if argument.startswith(flag) else argument for argument in saved["args"]]
    arguments[arguments.index("-o") + 1] = str(elf)
    linked = subprocess.run(["ldproxy", *arguments], cwd=saved["cwd"], capture_output=True, text=True)
    if linked.returncode:
        sys.exit(f"relink of {elf} failed:\n{linked.stdout[-2000:]}{linked.stderr[-2000:]}")


def build(tree, board, commit, placements):
    """The ELFs of `commit`, one a placement, built in `tree` unless kept."""
    out = tree / "target/esp32-ab" / f"{commit[:12]}-{board}"
    elves = [out / f"p{seed}.elf" for seed in range(placements)]
    if all(elf.exists() for elf in elves):
        return out, elves
    git("checkout", "--force", "--detach", commit, cwd=tree)
    out.mkdir(parents=True, exist_ok=True)
    mcu = board_run.BOARDS[board]["mcu"]
    perfstats = tree / "benchmarks/esp32/perfstats"
    window = out / "sdkconfig.window"
    window.write_text("CONFIG_PERFSTATS_WINDOW_MS=1000\n")
    features = [board, "screens-in-turn", *(["dashboard"] if LARGE_SCREENS[board] else [])]
    triple = f"xtensa-{mcu}-espidf".upper().replace("-", "_")
    environment = {f"CARGO_TARGET_{triple}_LINKER": str(pathlib.Path(__file__).with_name("keep_link.py")),
                   "KEEP_LINK_DIR": str(out / "link")}
    saved = {name: os.environ.get(name) for name in environment}
    os.environ.update(environment)
    try:
        # One log path for every build of the board keeps the components
        # file, and so esp-idf-sys's inputs, the same.
        elf, bootloader, partitions = board_run.build_rust(
            tree / "apps/esp32-demo", "esp32-demo", board, [perfstats],
            [perfstats / "sdkconfig.defaults", window], features,
            tree / "target/esp32-ab" / f"{board}.log", demo=tree / "apps/esp32-demo")
    finally:
        for name, value in saved.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
    shutil.copy(bootloader, out / "bootloader.bin")
    shutil.copy(partitions, out / "partitions.csv")
    order = tree / f"crates/cranpose-platform/esp-idf/components/cranpose_hot_code/{mcu}.order"
    for seed, target in enumerate(elves):
        if seed == 0:
            shutil.copy(elf, target)
            continue
        gaps = out / f"gaps{seed}.order"
        gapped(order, seed, gaps, LARGEST_GAP[board])
        relink(out / "link", gaps, target)
    return out, elves


def run(board, port, out, elf, seconds):
    log = elf.with_suffix(".log")
    board_run.flash(board, port, elf, out / "bootloader.bin", out / "partitions.csv", log)
    capture = board_run.capture_usb if board_run.BOARDS[board]["console"] == "usb" else board_run.capture_uart
    log.write_text(f"MCU {board_run.BOARDS[board]['mcu']}\n{capture(port, seconds)}")
    return log


def figures(log):
    """Frame time and instructions a frame by screen."""
    out = {}
    for line in summary.summarize(log):
        name, _, text = line.partition(": ")
        screen = name.rsplit(" ", 1)[-1]
        frame = re.search(r"frame ([\d.]+) ms", text)
        instructions = re.search(r"instructions (\d+)K", text)
        if frame:
            out[screen] = (float(frame.group(1)), int(instructions.group(1)) if instructions else None)
    return out


def report(results, names):
    lines = []
    screens = list(dict.fromkeys(screen for side in results.values() for run in side for screen in run))
    for screen in screens:
        base = [run.get(screen) for run in results[names[0]]]
        for name in names:
            got = [run.get(screen) for run in results[name]]
            frames = [value[0] for value in got if value]
            if not frames:
                continue
            instructions = sorted({value[1] for value in got if value and value[1] is not None})
            text = (f"{screen:9} {name:12} frame {sum(frames) / len(frames):6.3f} ms "
                    f"[{' '.join(f'{frame:.2f}' for frame in frames)}] "
                    f"instructions {'/'.join(map(str, instructions))}K")
            if name != names[0]:
                pairs = [mine[0] - theirs[0] for mine, theirs in zip(got, base) if mine and theirs]
                if pairs:
                    text += (f"  diff {sum(pairs) / len(pairs):+.3f} ms "
                             f"({min(pairs):+.2f}..{max(pairs):+.2f}, {len(pairs)} placements)")
            lines.append(text)
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--board", choices=board_run.BOARDS, required=True)
    parser.add_argument("--port", help="the serial port; found by name when omitted")
    parser.add_argument("--placements", type=int, default=4)
    parser.add_argument("revisions", nargs="+", help="the base first, then the revisions to compare with it")
    args = parser.parse_args()
    if len(args.revisions) < 2:
        sys.exit("give a base and at least one revision")
    port = args.port or board_run.one(board_run.BOARDS[args.board]["port"], "serial port")
    commits = [git("rev-parse", revision) for revision in args.revisions]
    tree = worktree(args.board)
    builds = {commit: build(tree, args.board, commit, args.placements) for commit in dict.fromkeys(commits)}
    screens = 4 + 4 * LARGE_SCREENS[args.board]
    seconds = screens * SCREEN_SECONDS + BOOT_SECONDS
    names = list(dict.fromkeys(args.revisions))
    if len(names) != len(args.revisions):
        sys.exit("a revision is given twice")
    results = {name: [] for name in names}
    for seed in range(args.placements):
        for name, commit in zip(names, commits):
            out, elves = builds[commit]
            results[name].append(figures(run(args.board, port, out, elves[seed], seconds)))
        print(f"placement {seed + 1} of {args.placements}:\n{report(results, names)}\n", flush=True)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Profiles an ESP32-S3 build of the demo in QEMU by its cache misses.

Runs the ELF in the QEMU that scripts/esp32/cache/setup.sh builds, with the
cache plugin cache.c counting the misses of core 0 for `--frames` frames
after `--skip`, and writes them as the sampler writes its samples: one
`SPC <address> <count>` line per code line that missed. hot_code.py and
profile.py read the log as they read a sampler's, so a list made from it
picks the functions with the most misses per byte: the code IRAM saves the
most waiting for. With `--cache data` the misses are those of the data
cache, at the instructions that made them: the code that waits for PSRAM
and for the flash's read-only data. A frame starts at the app shell's
update.

Usage: sim_profile.py ELF LOG [--cache instructions|data] [--bootloader BIN]
       [--skip N] [--frames N]
"""

import argparse
import pathlib
import re
import subprocess
import sys
import tempfile

from profile import find_nm

HERE = pathlib.Path(__file__).resolve().parent
PLUGINS = pathlib.Path.home() / ".local/esp-qemu-plugins"
FRAME = re.compile(r"^([0-9a-f]{8}) [tT] <cranpose_app_shell::AppShell<[^>]*>>::update_with_frame_time$")


def frame_address(elf):
    out = subprocess.run([find_nm(), "-C", str(elf)], capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        match = FRAME.match(line)
        if match:
            return int(match.group(1), 16)
    sys.exit(f"{elf} has no AppShell::update_with_frame_time to start frames at")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf", type=pathlib.Path)
    parser.add_argument("log", type=pathlib.Path)
    parser.add_argument("--cache", choices=["instructions", "data"], default="instructions")
    parser.add_argument("--bootloader", type=pathlib.Path)
    parser.add_argument("--partitions", type=pathlib.Path,
                        default=HERE.parent.parent / "apps/esp32-demo/partitions.csv")
    parser.add_argument("--skip", type=int, default=60)
    parser.add_argument("--frames", type=int, default=120)
    # Under the cache plugin a frame of a busy screen takes seconds of the
    # host's time, more while it builds something else: the timeout only
    # stops a run that hangs.
    parser.add_argument("--timeout", type=float, default=900.0)
    args = parser.parse_args()

    qemu = PLUGINS / "bin/qemu-system-xtensa"
    plugin = PLUGINS / "libcache.so"
    if not qemu.exists() or not plugin.exists():
        sys.exit("run scripts/esp32/cache/setup.sh first")
    frame = frame_address(args.elf)
    with tempfile.TemporaryDirectory() as work:
        out = pathlib.Path(work) / "cache.txt"
        command = [
            sys.executable, str(HERE / "qemu_run.py"), "--qemu", str(qemu), "--elf", str(args.elf),
            "--partitions", str(args.partitions), "--timeout", str(args.timeout),
            "--plugin", f"{plugin},kind={args.cache},frame={frame:#x},skip={args.skip},"
            f"frames={args.frames},out={out}",
        ]
        if args.bootloader:
            command += ["--bootloader", str(args.bootloader)]
        # QEMU now and then stays in the ROM at boot; a second run boots.
        for _ in range(3):
            subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
            if out.exists() and out.stat().st_size:
                break
        else:
            sys.exit(f"{args.elf} counted no frames in QEMU")
        frames, misses, lines = 0, 0, []
        for row in out.read_text().splitlines():
            fields = row.split()
            if fields[0] == "frame":
                frames += 1
                misses += int(fields[5])
            elif fields[0] == "line":
                lines.append(f"SPC {int(fields[1], 16):08x} {fields[3]}")
    # QEMU runs an ESP32-S3; profile.py names its ROM functions.
    args.log.write_text("\n".join(["MCU esp32s3", *lines]) + "\n")
    print(f"{args.log}: {frames} frames, {misses / max(frames, 1):.0f} misses a frame")


main()

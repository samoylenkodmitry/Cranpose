#!/usr/bin/env python3
"""Summarizes the frame statistics that a Meta Quest's runtime logs for an app, one line a second.

The runtime writes a `VrApi` line to logcat for each process that draws, with the frames of the
last second, the time that the app took for a frame (`App`), the share of the GPU's time that the
process used (`GPU%`), the GPU's clock and the time of the compositor's own pass (`TW`). Save the log
with `adb logcat -d -v time > all.log` and pass the app's process id, which every `cranpose` line of
the same log names in its parentheses.

    xr_vrapi_stats.py all.log 4242 [--skip 3]

The script skips the first `--skip` seconds (the start of the session, which draws fewer frames) and
the last second (partial), then prints the number of seconds, the mean of each value and the lowest
frame count. It uses the Python standard library only.
"""

import argparse
import re
import statistics

LINE = re.compile(
    r"FPS=(\d+)/(\d+).*?GPU=\d+/\d+,\d+/(\d+)MHz.*?TW=([\d.]+)ms,App=([\d.]+)ms.*?GPU%=([\d.]+)"
)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("log")
    parser.add_argument("pid")
    parser.add_argument("--skip", type=int, default=3)
    args = parser.parse_args()
    marker = f"({args.pid})"
    rows = []
    for line in open(args.log, errors="replace"):
        if "I/VrApi" not in line or marker not in line.replace(" ", ""):
            continue
        found = LINE.search(line)
        if found:
            rows.append(
                (
                    int(found[1]),
                    int(found[2]),
                    float(found[5]),
                    float(found[6]),
                    int(found[3]),
                    float(found[4]),
                )
            )
    rows = rows[args.skip : -1]
    if not rows:
        raise SystemExit(f"{args.log}: no VrApi line for process {args.pid}")
    mean = lambda column: statistics.mean(row[column] for row in rows)
    print(
        f"{len(rows)} s: frames {mean(0):.1f} of {rows[0][1]} (lowest {min(row[0] for row in rows)}), "
        f"app {mean(2):.2f} ms, GPU {100 * mean(3):.1f}%, GPU clock {mean(4):.0f} MHz, "
        f"compositor {mean(5):.2f} ms"
    )


if __name__ == "__main__":
    main()

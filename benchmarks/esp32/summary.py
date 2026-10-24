#!/usr/bin/env python3
"""Prints the per-frame figures of benchmark logs that scripts/esp32/board_run.py
saved with the perfstats component: frame time, instructions and stall
cycles a frame, time to the first frame and the internal RAM left.

Usage: summary.py LOG [LOG...]
"""

import re
import sys


def summarize(path):
    text = open(path, errors="replace").read()
    rates, busy = [], []
    for match in re.finditer(r"(\d+) frames in ([\d.]+) s: (?:update ([\d.]+) ms, draw and send ([\d.]+) ms|busy ([\d.]+) ms)", text):
        rates.append(int(match.group(1)) / float(match.group(2)))
        if match.group(5):
            busy.append(float(match.group(5)))
        else:
            busy.append(float(match.group(3)) + float(match.group(4)))
    if not rates:
        return f"{path}: no frame statistics"
    rate = sum(rates) / len(rates)
    counts = {}
    for match in re.finditer(r"PERF ([\d.]+) s (\S+) (\d+) (\S+) (\d+)", text):
        frames = rate * float(match.group(1))
        counts[match.group(2)] = int(match.group(3)) / frames
        counts[match.group(4)] = int(match.group(5)) / frames
    heap = re.findall(r"HEAP internal free (\d+) lowest (\d+)", text)
    first = re.search(r"I \((\d+)\) \S+: first frame after (\d+) ms", text)
    fields = [f"frame {sum(busy) / len(busy):.2f} ms at {rate:.0f} fps"]
    for name in ("instructions", "i-stall", "d-stall"):
        if name in counts:
            fields.append(f"{name} {counts[name] / 1000:.0f}K")
    if first:
        fields.append(f"first frame {first.group(1)} ms after reset ({first.group(2)} ms after start)")
    if heap:
        fields.append(f"internal RAM free {int(heap[-1][0]) // 1024} KB, lowest {int(heap[-1][1]) // 1024} KB")
    return f"{path}: " + ", ".join(fields)


for log in sys.argv[1:]:
    print(summarize(log))

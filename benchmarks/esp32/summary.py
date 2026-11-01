#!/usr/bin/env python3
"""Prints the per-frame figures of benchmark logs that scripts/esp32/board_run.py
saved with the perfstats component: frame time, the CPU time a frame takes
(the time core 0 did not idle), instructions and stall cycles a frame, the
bytes sent to the panel a frame, time to the first frame and the internal
RAM left.

A log of the demo's `screens-in-turn` feature gets a line for each screen,
from the windows that start a second or more after the screen does and end
before the next one starts.

Usage: summary.py LOG [LOG...]
"""

import re
import sys

STAMP = re.compile(r"I \((\d+)\) ")
FRAMES = re.compile(r"(\d+) frames in ([\d.]+) s: (?:update ([\d.]+) ms, draw and send ([\d.]+) ms|busy ([\d.]+) ms)")
PERF = re.compile(r"PERF ([\d.]+) s (\S+) (\d+) (\S+) (\d+)(?: idle ([\d.]+) s)?")
SENT = re.compile(r"sent ([\d.]+) KB a frame")
HEAP = re.compile(r"HEAP internal free (\d+) lowest (\d+)")
FIRST = re.compile(r"I \((\d+)\) \S+: first frame after (\d+) ms")
SCREEN = re.compile(r"I \((\d+)\) \S+: screen (\S+)")
# The time a screen takes to compose and warm its caches, which its windows
# leave out.
WARM_UP = 1.0


def windows(lines):
    """The frame and counter windows of `lines`, each with the seconds at
    which it starts and ends when the log has timestamps."""
    frames, perf, heap = [], [], []
    for line in lines:
        stamp = STAMP.search(line)
        end = int(stamp.group(1)) / 1000 if stamp else None
        if match := FRAMES.search(line):
            seconds = float(match.group(2))
            if match.group(5):
                busy = float(match.group(5))
            else:
                busy = float(match.group(3)) + float(match.group(4))
            sent = SENT.search(line)
            frames.append((end, seconds, int(match.group(1)) / seconds, busy,
                           float(sent.group(1)) if sent else None))
        elif match := PERF.search(line):
            perf.append((end, float(match.group(1)), match))
        elif match := HEAP.search(line):
            heap.append((end, 0.0, match))
    return frames, perf, heap


def inside(window, start, stop):
    end, seconds = window[0], window[1]
    return end is not None and end - seconds >= start and end <= stop


def fields(frames, perf, heap):
    if not frames:
        return ["no frame statistics"]
    rate = sum(window[2] for window in frames) / len(frames)
    busy = sum(window[3] for window in frames) / len(frames)
    counts = {}
    cpu = []
    for _, seconds, match in perf:
        per_frame = rate * seconds
        counts.setdefault(match.group(2), []).append(int(match.group(3)) / per_frame)
        counts.setdefault(match.group(4), []).append(int(match.group(5)) / per_frame)
        if match.group(6):
            cpu.append((seconds - float(match.group(6))) * 1000 / per_frame)
    sent = [window[4] for window in frames if window[4] is not None]
    out = [f"frame {busy:.2f} ms at {rate:.1f} fps"]
    if cpu:
        out.append(f"CPU {sum(cpu) / len(cpu):.2f} ms")
    if sent:
        out.append(f"sent {sum(sent) / len(sent):.0f} KB")
    for name in ("instructions", "i-stall", "d-stall"):
        if name in counts:
            values = counts[name]
            out.append(f"{name} {sum(values) / len(values) / 1000:.0f}K")
    if heap:
        match = heap[-1][2]
        out.append(f"internal RAM free {int(match.group(1)) // 1024} KB, lowest {int(match.group(2)) // 1024} KB")
    return out


def summarize(path):
    lines = open(path, errors="replace").read().splitlines()
    frames, perf, heap = windows(lines)
    first = next((match for line in lines if (match := FIRST.search(line))), None)
    first_field = (f"first frame {first.group(1)} ms after reset ({first.group(2)} ms after start)"
                   if first else None)
    screens = [(int(m.group(1)) / 1000, m.group(2)) for line in lines if (m := SCREEN.search(line))]
    if not screens:
        out = fields(frames, perf, heap)
        if first_field and frames:
            out.insert(len(out) - (1 if heap else 0), first_field)
        return [f"{path}: " + ", ".join(out)]
    result = []
    for index, (start, name) in enumerate(screens):
        stop = screens[index + 1][0] if index + 1 < len(screens) else float("inf")
        begin = start + WARM_UP
        out = fields([w for w in frames if inside(w, begin, stop)],
                     [w for w in perf if inside(w, begin, stop)],
                     [w for w in heap if w[0] is not None and start <= w[0] <= stop])
        if index == 0 and first_field:
            out.append(first_field)
        result.append(f"{path} {name}: " + ", ".join(out))
    return result


if __name__ == "__main__":
    for log in sys.argv[1:]:
        print("\n".join(summarize(log)))

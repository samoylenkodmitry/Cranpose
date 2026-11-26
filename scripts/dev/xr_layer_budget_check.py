#!/usr/bin/env python3
"""Checks a compositor screenshot of the xr-demo build `XR_DEMO_PANELS=<count>`: every panel of the
grid shows in both eyes.

The build shows `count` square panels of saturated colors on black, with gaps of black between them.
A Quest's `adb exec-out screencap -p` holds both eyes side by side. The script finds the connected
regions of saturated pixels in each half, on every second pixel of every second row, and fails when
either half holds fewer than `count` regions of `--min-pixels` or more. The display's color
management shifts the hues, so the script counts regions and does not match colors.

The black gate runs first: a screenshot whose share of black pixels is under `--min-black`, or whose
share of pixels that are neither black nor saturated is over `--max-other`, may show the room
around the headset. The script then deletes the file unread and fails. It uses the Python standard
library only.

    xr_layer_budget_check.py shot.png 19 [--min-pixels 1500] [--min-black 0.4] [--max-other 0.15]
"""

import argparse
import colorsys
import os
import struct
import sys
import zlib


def read_png(path):
    """The width, height and rows of RGB(A) bytes of an 8-bit, non-interlaced PNG."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    at, chunks, header = 8, [], None
    while at < len(data):
        length, kind = struct.unpack(">I4s", data[at : at + 8])
        body = data[at + 8 : at + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            chunks.append(body)
        at += 12 + length
    width, height, depth, color, _, _, interlace = header
    channels = {2: 3, 6: 4}.get(color)
    if depth != 8 or channels is None or interlace:
        raise SystemExit(f"{path}: needs 8-bit RGB or RGBA without interlace")
    raw = zlib.decompress(b"".join(chunks))
    stride = width * channels
    rows, previous = [], bytearray(stride)
    for row in range(height):
        start = row * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            left = line[i - channels] if i >= channels else 0
            up = previous[i]
            corner = previous[i - channels] if i >= channels else 0
            if kind == 1:
                line[i] = (line[i] + left) & 255
            elif kind == 2:
                line[i] = (line[i] + up) & 255
            elif kind == 3:
                line[i] = (line[i] + (left + up) // 2) & 255
            elif kind == 4:
                guess = left + up - corner
                to_left, to_up, to_corner = abs(guess - left), abs(guess - up), abs(guess - corner)
                if to_left <= to_up and to_left <= to_corner:
                    near = left
                elif to_up <= to_corner:
                    near = up
                else:
                    near = corner
                line[i] = (line[i] + near) & 255
        rows.append(bytes(line))
        previous = line
    return width, height, channels, rows


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("png")
    parser.add_argument("count", type=int)
    parser.add_argument("--min-pixels", type=int, default=1500)
    parser.add_argument("--min-black", type=float, default=0.4)
    parser.add_argument("--max-other", type=float, default=0.15)
    args = parser.parse_args()
    width, height, channels, rows = read_png(args.png)
    # Every second pixel of every second row: a panel covers thousands of them
    columns, lines = width // 2, height // 2
    saturated = bytearray(columns * lines)
    black = other = 0
    for y in range(lines):
        row = rows[2 * y]
        for x in range(columns):
            at = 2 * x * channels
            r, g, b = row[at : at + 3]
            if max(r, g, b) <= 12:
                black += 1
                continue
            _, saturation, value = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
            if saturation < 0.5 or value < 0.3:
                other += 1
                continue
            saturated[y * columns + x] = 1
    total = columns * lines
    shares = (black / total, other / total)
    print(f"black {shares[0]:.3f}, other {shares[1]:.3f} of {width}x{height}")
    if shares[0] < args.min_black or shares[1] > args.max_other:
        os.remove(args.png)
        print(f"the gate failed: {args.png} may show the room and is deleted unread")
        return 1
    regions = [[], []]
    seen = bytearray(total)
    for start in range(total):
        if not saturated[start] or seen[start]:
            continue
        seen[start] = 1
        stack, area, left_sum = [start], 0, 0
        while stack:
            at = stack.pop()
            area += 1
            x, y = at % columns, at // columns
            left_sum += x
            for near, inside in (
                (at - 1, x > 0),
                (at + 1, x + 1 < columns),
                (at - columns, y > 0),
                (at + columns, y + 1 < lines),
            ):
                if inside and saturated[near] and not seen[near]:
                    seen[near] = 1
                    stack.append(near)
        if area >= args.min_pixels:
            eye = 0 if left_sum / area < columns / 2 else 1
            regions[eye].append(area)
    for name, areas in zip(("left", "right"), regions):
        print(f"{name} eye: {len(areas)} panels, areas {sorted(areas)}")
    shown = min(len(areas) for areas in regions)
    if shown < args.count:
        print(f"an eye shows {shown} of the {args.count} panels")
        return 1
    print(f"all {args.count} panels show in both eyes")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Checks the depth test of a headset from two screenshots of its compositor.

The xr-demo builds `XR_DEMO_VOLUME=through` and `XR_DEMO_VOLUME=alone` show a pink panel and a
striped orange and blue torus that pierces it. The panel hides the torus's far half and the torus's
near half covers the panel. The `alone` build shows the torus with no panel. Take each screenshot
with `adb exec-out screencap -p > shot.png`. The picture holds the left eye in its left half and
the right eye in its right half.

    xr_depth_check.py through.png alone.png [--low 0.55] [--high 0.85]

Each eye passes when the torus pixels of `through.png` are a share of those of `alone.png` between
`--low` and `--high`. On a Quest 3 the near half of the ring covers 0.68 of the ring's pixels. A
share near 0 means that the panel covers the whole torus: the compositor does not test depth, or
tests it the wrong way round. A share near 1 means that the torus covers the whole panel. A panel
200 mm nearer to the head than the ring gives 0.00, and one 200 mm farther gives 0.98. The script
uses the Python standard library only.
"""

import argparse
import struct
import sys
import zlib

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def read_png(path):
    """Returns (width, rows, bytes per pixel) of an 8-bit RGB or RGBA PNG, one bytes object per row."""
    data = open(path, "rb").read()
    if not data.startswith(PNG_SIGNATURE):
        sys.exit(f"{path}: not a PNG")
    position, idat = len(PNG_SIGNATURE), []
    while position < len(data):
        length, kind = struct.unpack(">I4s", data[position : position + 8])
        body = data[position + 8 : position + 8 + length]
        position += 12 + length
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or color not in (2, 6) or interlace:
                sys.exit(f"{path}: only 8-bit RGB or RGBA without interlacing")
        elif kind == b"IDAT":
            idat.append(body)
    pixel = 3 if color == 2 else 4
    stride = width * pixel
    raw = zlib.decompress(b"".join(idat))
    rows, previous = [], bytearray(stride)
    for y in range(height):
        kind = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        unfilter(kind, line, previous, pixel)
        rows.append(bytes(line))
        previous = line
    return width, rows, pixel


def unfilter(kind, line, previous, pixel):
    if kind == 0:
        return
    for i in range(len(line)):
        left = line[i - pixel] if i >= pixel else 0
        up = previous[i]
        corner = previous[i - pixel] if i >= pixel else 0
        if kind == 1:
            line[i] = (line[i] + left) & 255
        elif kind == 2:
            line[i] = (line[i] + up) & 255
        elif kind == 3:
            line[i] = (line[i] + ((left + up) >> 1)) & 255
        else:
            estimate = left + up - corner
            to_left, to_up, to_corner = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
            if to_left <= to_up and to_left <= to_corner:
                nearest = left
            elif to_up <= to_corner:
                nearest = up
            else:
                nearest = corner
            line[i] = (line[i] + nearest) & 255


def is_torus(r, g, b):
    """The torus's orange stripe has a hue of 15 to 50 degrees, and its blue stripe 190 to 250."""
    high, low = max(r, g, b), min(r, g, b)
    if high < 60 or high - low < 0.35 * high:
        return False
    if r == high and b == low:
        return 0.25 <= (g - b) / (r - b) <= 0.83
    if b == high and r == low:
        return 0.2 <= (g - r) / (b - r) <= 0.75
    return False


def torus_pixels(path):
    """Counts the torus's pixels in the left half and the right half of the picture."""
    width, rows, pixel = read_png(path)
    middle, counts = width // 2, [0, 0]
    background = bytes([0, 0, 0, 255][:pixel]) * width
    for row in rows:
        if row == background:
            continue
        for x in range(width):
            i = x * pixel
            if is_torus(row[i], row[i + 1], row[i + 2]):
                counts[x >= middle] += 1
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("through", help="screenshot of the build with the panel")
    parser.add_argument("alone", help="screenshot of the build without the panel")
    parser.add_argument("--low", type=float, default=0.55)
    parser.add_argument("--high", type=float, default=0.85)
    arguments = parser.parse_args()
    through = torus_pixels(arguments.through)
    alone = torus_pixels(arguments.alone)
    passed = True
    for name, behind, whole in zip(("left eye", "right eye"), through, alone):
        share = behind / whole if whole else 0.0
        eye_passed = whole > 0 and arguments.low <= share <= arguments.high
        passed = passed and eye_passed
        print(
            f"{name}: {behind} torus pixels with the panel, {whole} without, share {share:.2f}: "
            f"{'ok' if eye_passed else 'FAILED'}"
        )
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()

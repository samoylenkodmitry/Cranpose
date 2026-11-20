#!/usr/bin/env python3
"""Checks that a headset shows the xr-demo's glTF crate with its texture in both eyes.

The xr-demo build `XR_DEMO_VOLUME=model` shows only the crate of `assets/spinning_crate.glb`, a
cube turned to show three faces. Its texture has magenta and lime tiles in a white frame. Take the
screenshot with `adb exec-out screencap -p > shot.png`. The picture holds the left eye in its left
half and the right eye in its right half.

    xr_model_check.py shot.png [--least 200] [--later other.png --moved 10000]

The script counts the magenta pixels and the lime pixels in each eye. An eye passes when both
counts reach `--least`. With `--later`, a second screenshot of the build that turns the crate (the
full demo, not the `model` build), the script also counts the pixels that are in a different class
(magenta, lime, other) in the two shots. A crate that turns changes tens of thousands of them. A
crate that stands still changes about two thousand, because the headset moves a little. The check
passes when at least `--moved` pixels changed.

A lit face has the tile's hue at a lower brightness, so the classes test the hue and not the exact
color. The script prints the counts and exits with 1 when a check fails. It uses the Python
standard library only.
"""

import argparse
import sys

from xr_depth_check import read_png


def is_magenta(r, g, b):
    """The magenta tile (230, 30, 200) at any brightness: red and blue strong, green weak."""
    return min(r, b) > 50 and g < 0.45 * min(r, b) and 0.6 < r / b < 1.7


def is_lime(r, g, b):
    """The lime tile (120, 230, 30) at any brightness: green strong, red about half, blue weak."""
    return g > 60 and g > 1.4 * r and b < 0.35 * g and r > 0.25 * g


def classes(path):
    """Returns the class of each pixel, row by row: 1 magenta, 2 lime, 0 other, and the width."""
    width, rows, pixel = read_png(path)
    picture = []
    for row in rows:
        line = bytearray(width)
        for x in range(width):
            i = x * pixel
            r, g, b = row[i], row[i + 1], row[i + 2]
            line[x] = 1 if is_magenta(r, g, b) else 2 if is_lime(r, g, b) else 0
        picture.append(line)
    return width, picture


def count(picture, width):
    """Returns ((magenta, lime) of the left eye, (magenta, lime) of the right eye)."""
    middle, counts = width // 2, [[0, 0], [0, 0]]
    for line in picture:
        for x, kind in enumerate(line):
            if kind:
                counts[x >= middle][kind - 1] += 1
    return counts


def changed(first, second):
    """Counts the pixels in a crate class in one shot and a different class in the other."""
    return sum(
        1
        for a, b in zip(first, second)
        for x, y in zip(a, b)
        if x != y and (x or y)
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("shot", help="screenshot of the compositor")
    parser.add_argument("--least", type=int, default=200)
    parser.add_argument("--later", help="a second screenshot, taken a second after the first")
    parser.add_argument("--moved", type=int, default=10000)
    arguments = parser.parse_args()
    width, picture = classes(arguments.shot)
    passed = True
    for name, (magenta, lime) in zip(("left eye", "right eye"), count(picture, width)):
        eye_passed = magenta >= arguments.least and lime >= arguments.least
        passed = passed and eye_passed
        print(
            f"{name}: {magenta} magenta and {lime} lime pixels: {'ok' if eye_passed else 'FAILED'}"
        )
    if arguments.later:
        later_width, later = classes(arguments.later)
        if later_width != width or len(later) != len(picture):
            sys.exit("the two screenshots differ in size")
        moved = changed(picture, later)
        moved_passed = moved >= arguments.moved
        passed = passed and moved_passed
        print(f"{moved} crate pixels changed class: {'ok' if moved_passed else 'FAILED'}")
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()

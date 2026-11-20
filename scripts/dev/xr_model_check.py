#!/usr/bin/env python3
"""Checks that a headset shows the xr-demo's glTF crate with its texture in both eyes.

The xr-demo build `XR_DEMO_VOLUME=model` shows only the crate of `assets/spinning_crate.glb`, a
cube turned to show three faces. Its texture has magenta and lime tiles in a white frame. Take the
screenshot with `adb exec-out screencap -p > shot.png`. The picture holds the left eye in its left
half and the right eye in its right half.

    xr_model_check.py shot.png [--least 200]

The script counts the magenta pixels and the lime pixels in each eye. An eye passes when both
counts reach `--least`. A lit face has the tile's hue at a lower brightness, so the classes test
the hue and not the exact color. It prints the counts and exits with 1 when an eye fails. It uses
the Python standard library only.
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


def count(path):
    """Returns ((magenta, lime) of the left eye, (magenta, lime) of the right eye)."""
    width, rows, pixel = read_png(path)
    middle, counts = width // 2, [[0, 0], [0, 0]]
    for row in rows:
        for x in range(width):
            i = x * pixel
            r, g, b = row[i], row[i + 1], row[i + 2]
            if is_magenta(r, g, b):
                counts[x >= middle][0] += 1
            elif is_lime(r, g, b):
                counts[x >= middle][1] += 1
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("shot", help="screenshot of the compositor")
    parser.add_argument("--least", type=int, default=200)
    arguments = parser.parse_args()
    passed = True
    for name, (magenta, lime) in zip(("left eye", "right eye"), count(arguments.shot)):
        eye_passed = magenta >= arguments.least and lime >= arguments.least
        passed = passed and eye_passed
        print(
            f"{name}: {magenta} magenta and {lime} lime pixels: {'ok' if eye_passed else 'FAILED'}"
        )
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()

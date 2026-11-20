#!/usr/bin/env python3
"""Checks that a headset shows both controllers' models in both eyes at their grip poses.

The xr-demo builds `XR_DEMO_CONTROLLERS=only` and `XR_DEMO_CONTROLLERS=only,off` show nothing but
the controllers' models, and nothing at all, so the difference of their screenshots holds the
models alone. Both builds log the eyes' poses and fields of view and the controllers' grips once a
second (`xr demo: controllers:`). Take each screenshot with `adb exec-out screencap -p > shot.png`
while the controllers lie still, and save `adb logcat -d` of the run with the models.

    xr_controller_check.py models.png --without off.png --log logcat.txt [--least 150]
                           [--tolerance 0.08]

The picture holds the left eye in its left half and the right eye in its right half. The script marks
each pixel whose color differs between the two screenshots, joins the marked pixels into blobs, and
projects each grip's origin into each eye from the logged poses. An eye passes when a blob of at
least `--least` pixels lies within `--tolerance` of the eye's width from the projected point of each
grip that the log shows, and the left controller's blob lies left of the right controller's. It
prints each blob's box and its distance from the projection, and exits with 1 when a check fails. A grip
that projects outside an eye's picture is not checked in that eye.
It uses the Python standard library only. It has run on pictures that a script drew and on the
screenshots of a Meta Quest 3 that shows the demo's scripted controllers.
"""

import argparse
import math
import re
import sys

from xr_depth_check import read_png

FLOAT = r"[-+0-9.eE]+"
VECTOR = rf"\[?({FLOAT}),\s*({FLOAT}),\s*({FLOAT})\]?"


def last_log_line(path):
    """Returns the last `xr demo: controllers:` line of the saved log."""
    found = None
    for line in open(path, errors="replace"):
        if "xr demo: controllers:" in line:
            found = line
    if found is None:
        sys.exit(f"{path}: no 'xr demo: controllers:' line")
    return found


def numbers(text):
    return [float(value) for value in re.findall(FLOAT, text)]


def parse(line):
    """Returns the eyes and the grips of a log line: (position, orientation, fov) or None."""
    fields = {}
    for name in ("left_eye", "right_eye", "left_grip", "right_grip"):
        match = re.search(rf"{name} ([^;]*);", line)
        text = match.group(1).strip() if match else "none"
        fields[name] = None if text.startswith("none") else numbers(text.replace("fov", ""))
    return fields


def rotate_inverse(q, v):
    """Turns the vector v by the inverse of the quaternion q = (x, y, z, w)."""
    x, y, z, w = q
    x, y, z = -x, -y, -z
    tx, ty, tz = (
        2 * (y * v[2] - z * v[1]),
        2 * (z * v[0] - x * v[2]),
        2 * (x * v[1] - y * v[0]),
    )
    return (
        v[0] + w * tx + (y * tz - z * ty),
        v[1] + w * ty + (z * tx - x * tz),
        v[2] + w * tz + (x * ty - y * tx),
    )


def project(eye, point, width, height):
    """Returns the pixel at which the eye sees the point, in an image of its field of view."""
    position, orientation = eye[0:3], eye[3:7]
    left, right, up, down = eye[7:11]
    v = rotate_inverse(orientation, [point[i] - position[i] for i in range(3)])
    if v[2] >= 0:
        return None
    tx, ty = v[0] / -v[2], v[1] / -v[2]
    column = (tx - math.tan(left)) / (math.tan(right) - math.tan(left)) * width
    row = (math.tan(up) - ty) / (math.tan(up) - math.tan(down)) * height
    return column, row


def marked(first, second, threshold):
    """Returns a function that tells whether the pixel (x, y) differs between the screenshots."""
    (width, rows_a, pixel_a), (_, rows_b, pixel_b) = first, second

    def differs(x, y):
        a, b = rows_a[y], rows_b[y]
        return (
            abs(a[x * pixel_a] - b[x * pixel_b])
            + abs(a[x * pixel_a + 1] - b[x * pixel_b + 1])
            + abs(a[x * pixel_a + 2] - b[x * pixel_b + 2])
        ) > threshold

    return width, len(rows_a), differs


def blobs(differs, x0, x1, height, step, least):
    """Joins the marked pixels of the columns x0 to x1 into boxes of at least `least` pixels."""
    columns = range(x0, x1, step)
    rows = range(0, height, step)
    grid = {(x, y) for y in rows for x in columns if differs(x, y)}
    found, seen = [], set()
    for start in sorted(grid):
        if start in seen:
            continue
        stack, cells = [start], []
        seen.add(start)
        while stack:
            x, y = stack.pop()
            cells.append((x, y))
            for dx in (-step, 0, step):
                for dy in (-step, 0, step):
                    near = (x + dx, y + dy)
                    if near in grid and near not in seen:
                        seen.add(near)
                        stack.append(near)
        if len(cells) * step * step >= least:
            xs, ys = [c[0] for c in cells], [c[1] for c in cells]
            found.append(
                {
                    "box": (min(xs) - x0, min(ys), max(xs) - x0, max(ys)),
                    "pixels": len(cells) * step * step,
                }
            )
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("shot")
    parser.add_argument("--without", required=True)
    parser.add_argument("--log", required=True)
    parser.add_argument("--least", type=int, default=150)
    parser.add_argument("--tolerance", type=float, default=0.08)
    parser.add_argument("--threshold", type=int, default=45)
    parser.add_argument("--step", type=int, default=4)
    args = parser.parse_args()

    logged = parse(last_log_line(args.log))
    with_models, without = read_png(args.shot), read_png(args.without)
    width, height, differs = marked(with_models, without, args.threshold)
    eye_width = width // 2
    failed = False
    for side, (x0, name) in enumerate(((0, "left eye"), (eye_width, "right eye"))):
        eye = logged["left_eye" if side == 0 else "right_eye"]
        found = blobs(differs, x0, x0 + eye_width, height, args.step, args.least)
        print(f"{name}: {len(found)} blobs of at least {args.least} pixels")
        centers = {}
        for hand in ("left", "right"):
            grip = logged[f"{hand}_grip"]
            if grip is None:
                print(f"  {hand} grip: not tracked in the log")
                continue
            if eye is None:
                print(f"  {hand} grip: the eye is not located")
                failed = True
                continue
            at = project(eye, grip[0:3], eye_width, height)
            if at is None:
                print(f"  {hand} grip: behind the eye")
                failed = True
                continue
            if not (0 <= at[0] < eye_width and 0 <= at[1] < height):
                # A grip beside the field of view puts its model outside this eye's picture
                print(f"  {hand} grip projects to ({at[0]:.0f}, {at[1]:.0f}): outside the eye's picture, not checked")
                continue
            best = None
            for blob in found:
                left, top, right, bottom = blob["box"]
                dx = max(left - at[0], 0, at[0] - right)
                dy = max(top - at[1], 0, at[1] - bottom)
                distance = math.hypot(dx, dy)
                if best is None or distance < best[0]:
                    best = (distance, blob)
            ok = best is not None and best[0] <= args.tolerance * eye_width
            failed |= not ok
            if best is not None:
                centers[hand] = (best[1]["box"][0] + best[1]["box"][2]) / 2
            print(
                f"  {hand} grip projects to ({at[0]:.0f}, {at[1]:.0f}): "
                + (
                    f"blob {best[1]['box']} of {best[1]['pixels']} pixels, {best[0]:.0f} pixels away"
                    if best
                    else "no blob"
                )
                + (" ok" if ok else " FAILED")
            )
        if len(centers) == 2 and not centers["left"] < centers["right"]:
            print("  the left controller's blob does not lie left of the right's FAILED")
            failed = True
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

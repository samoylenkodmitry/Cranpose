"""Reads geometry and color off one captured control state.

    python3 apps/liquid-reference/pixel_probe.py ATTACHMENTS STATE row Y X0 X1
    python3 apps/liquid-reference/pixel_probe.py ATTACHMENTS STATE col X Y0 Y1
    python3 apps/liquid-reference/pixel_probe.py ATTACHMENTS STATE box X0 X1 Y0 Y1
    python3 apps/liquid-reference/pixel_probe.py ATTACHMENTS STATE fill X0 X1 Y0 Y1 BACKDROP_ATTACHMENTS BACKDROP_STATE

ATTACHMENTS is an exported xcresult attachment folder and STATE a capture
name such as control-stepper-dark-solid-0. Coordinates are in points of the
3x capture.

row and col print each pixel's point coordinate and RGB value along a line.
box prints the bounds and the ink-weighted centroid of the pixels in a
region that differ from the region's top-left pixel, to place a glyph or a
stroke. fill fits out = backdrop * (1 - alpha) + color * alpha per channel
over a region where the control lays a flat fill on the checkerboard;
BACKDROP_STATE is a capture with the same checkerboard and nothing in that
region.
"""

import sys
from pathlib import Path

import numpy as np
from PIL import Image

from control_crops import SCALE, THRESHOLD, states


def load(folder, state):
    path = states(Path(folder), [state]).get(state)
    if path is None:
        raise SystemExit(f"no capture named {state} in {folder}")
    return np.asarray(Image.open(path).convert("RGB")).astype(float)


def pixels(values):
    return [round(float(value) * SCALE) for value in values]


def line(image, axis, fixed, start, end):
    for px in range(start, end):
        rgb = image[fixed, px] if axis == "row" else image[px, fixed]
        print(f"{px / SCALE:8.2f} {tuple(int(channel) for channel in rgb)}")


def box(image, x0, x1, y0, y1):
    region = image[y0:y1, x0:x1]
    weight = np.abs(region - region[0, 0]).sum(axis=2)
    rows, columns = np.nonzero(weight > THRESHOLD)
    if len(rows) == 0:
        print("nothing differs from the region's corner")
        return
    ys, xs = np.mgrid[y0:y1, x0:x1]
    center_x = ((weight * xs).sum() / weight.sum() + 0.5) / SCALE
    center_y = ((weight * ys).sum() / weight.sum() + 0.5) / SCALE
    print(
        f"x {(x0 + columns.min()) / SCALE:.2f}-{(x0 + columns.max() + 1) / SCALE:.2f} "
        f"y {(y0 + rows.min()) / SCALE:.2f}-{(y0 + rows.max() + 1) / SCALE:.2f} "
        f"centroid ({center_x:.2f}, {center_y:.2f})"
    )


def fill(image, backdrop, x0, x1, y0, y1):
    out = image[y0:y1, x0:x1].reshape(-1, 3)
    under = backdrop[y0:y1, x0:x1].reshape(-1, 3)
    for channel, name in enumerate("rgb"):
        design = np.stack([under[:, channel], np.ones(len(under))], axis=1)
        (keep, add), residual, *_ = np.linalg.lstsq(design, out[:, channel], rcond=None)
        alpha = 1 - keep
        rms = np.sqrt(residual[0] / len(under)) if len(residual) else 0.0
        color = add / alpha if alpha > 1e-3 else float("nan")
        print(f"{name}: alpha {alpha:.3f} color {color:.1f} rms {rms:.2f}")


def main():
    arguments = sys.argv[1:]
    shapes = {"row": 6, "col": 6, "box": 7, "fill": 9}
    if len(arguments) < 3 or shapes.get(arguments[2]) != len(arguments):
        print(__doc__, file=sys.stderr)
        return 2
    folder, state, mode = arguments[:3]
    image = load(folder, state)
    if mode in ("row", "col"):
        line(image, mode, *pixels(arguments[3:6]))
    elif mode == "box":
        box(image, *pixels(arguments[3:7]))
    else:
        fill(image, load(arguments[7], arguments[8]), *pixels(arguments[3:7]))
    return 0


if __name__ == "__main__":
    sys.exit(main())

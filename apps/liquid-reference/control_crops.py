"""Crops each captured control state to the pixels that differ from its
backdrop, prints the box in points, and writes a contact sheet.

    python3 apps/liquid-reference/control_crops.py ATTACHMENTS SHEET.png [PREFIX...]

ATTACHMENTS is an exported xcresult attachment folder (its manifest.json
names the states). A PREFIX keeps only the states whose name starts with it.
The captures are 3x; the band searched is the middle fifth of the screen,
where the reference screen centers its control.
"""

import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

SCALE = 3
THRESHOLD = 12


def states(folder, prefixes):
    manifest = json.loads((folder / "manifest.json").read_text())
    found = {}
    for test in manifest:
        for attachment in test["attachments"]:
            name = attachment["suggestedHumanReadableName"].split("_0_")[0]
            if not name.startswith("control-") or not attachment["exportedFileName"].endswith(".png"):
                continue
            if prefixes and not any(name.startswith(prefix) for prefix in prefixes):
                continue
            found[name] = folder / attachment["exportedFileName"]
    return dict(sorted(found.items()))


def crop(path):
    image = Image.open(path).convert("RGB")
    pixels = np.asarray(image).astype(int)
    top, bottom = int(image.height * 0.4), int(image.height * 0.6)
    band = pixels[top:bottom]
    # The backdrop at the band's left edge: a solid fill, or the checkerboard
    # whose cells the comparison then shows as differing.
    backdrop = band[:, :4].reshape(-1, 3)
    differs = np.abs(band[:, :, None, :] - backdrop[None, None, ::97, :]).sum(axis=3).min(axis=2) > THRESHOLD
    rows, columns = np.nonzero(differs)
    if len(columns) == 0:
        return image.crop((0, top, image.width, bottom)), None
    box = (columns.min(), top + rows.min(), columns.max() + 1, top + rows.max() + 1)
    return image.crop(box), box


def main():
    folder, sheet = Path(sys.argv[1]), Path(sys.argv[2])
    tiles = []
    for name, path in states(folder, sys.argv[3:]).items():
        image, box = crop(path)
        if box is None:
            print(f"{name}: nothing differs from the backdrop")
        else:
            x0, y0, x1, y1 = (value / SCALE for value in box)
            print(f"{name}: x {x0:.2f}-{x1:.2f} y {y0:.2f}-{y1:.2f} ({x1 - x0:.2f} x {y1 - y0:.2f} pt)")
        tiles.append((name, image))
    width = max(image.width for _, image in tiles)
    out = Image.new("RGB", (width, sum(image.height + 24 for _, image in tiles)), (255, 0, 255))
    draw = ImageDraw.Draw(out)
    y = 0
    for name, image in tiles:
        draw.text((4, y + 4), name, fill=(0, 0, 0))
        out.paste(image, (0, y + 24))
        y += image.height + 24
    out.save(sheet)


if __name__ == "__main__":
    main()

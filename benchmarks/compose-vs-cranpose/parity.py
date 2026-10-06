#!/usr/bin/env python3
"""Checks that two benchmark apps, Cranpose and Compose by default, draw the same
picture.

Each app is launched with a scenario and `--ei freeze K`, which stops it on its
K-th frame. Both screens are captured on that frame and compared below the
status bar, as the eye compares them:
- both captures are softened, as the eye does not resolve antialiasing, and
  cut into tiles;
- each band of tiles of one is found in the other up to `--drift` pixels
  higher or lower: the frameworks put text lines on different pixel grids
  (Flutter rounds each line to whole logical pixels, Compose rounds it up to
  whole device pixels), so a scrolled list drifts down the screen without
  looking any different;
- each tile is then matched at every offset up to `--shift` pixels around its
  band's and its neighbours', for the rounding within the band. Where the
  neighbouring bands drifted differently, as where still content meets a
  scrolled list, each quarter of the tile may also take their offsets, to
  within 3 pixels;
- a band that matches nowhere is left out when the nearest band above or
  below that does match puts it past the other capture's edge or behind its
  still content, such as above the top of a list: the other app does not
  show it;
- a tile whose best match still differs by more than `--tile-delta` on average
  is changed: a missing or extra element, another line break, another color.

Each capture's tiles are found in the other, so an element either one lacks
is changed. A pair passes when at most `--max-changed` percent of either
capture's tiles changed. The captures and a side-by-side image of both with
both maps of changed tiles are written next to `parity.json`.
"""

import argparse
import json
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageFilter, ImageStat

from measure import APPS, Device, app_log, device_lock, launch


def capture(device, app, scenario, extras, destination, timeout_s):
    launch(device, app, scenario, extras)
    deadline = time.monotonic() + timeout_s
    while 'PERF frozen' not in app_log(device, app):
        if time.monotonic() > deadline:
            raise RuntimeError(f'{app} did not reach its freeze frame in {timeout_s} s')
        time.sleep(0.5)
    # One more vsync for the frozen frame to reach the screen.
    time.sleep(0.5)
    path = destination / f'{app}.png'
    with open(path, 'wb') as handle:
        subprocess.run(['adb', '-s', device.serial, 'exec-out', 'screencap', '-p'],
                       stdout=handle, check=True, timeout=30)
    device.shell('am', 'force-stop', APPS[app]['package'])
    return Image.open(path).convert('RGB')


COARSE = 4
# Strips a tile is matched in where its band meets another offset, and how
# far each may move from that offset.
STRIPS = 4
STRIP_SLACK = 3


def band_drift(band, other, x, y, drift):
    """Where, within `drift` pixels up or down, `other` best shows the band
    whose undrifted place in it is (`x`, `y`), to the nearest `COARSE` pixels:
    the offset and the mean channel difference there, at a quarter of the
    resolution."""
    band = band.reduce(COARSE)
    left, top = x // COARSE, y // COARSE
    reach = drift // COARSE
    scores = []
    for step in range(-reach, reach + 1):
        moved = other.crop((left, top + step, left + band.width, top + step + band.height))
        mean = sum(ImageStat.Stat(ImageChops.difference(band, moved)).mean) / 3
        scores.append((mean, abs(step), step))
    mean, _, step = min(scores)
    return step * COARSE, mean


def tile_residuals(band, other, x, y, columns, strips):
    """The mean channel difference of each tile, or each of its `strips`,
    between the band and `other` at (`x`, `y`)."""
    moved = other.crop((x, y, x + band.width, y + band.height))
    return ImageChops.difference(band, moved).convert('L').resize((columns, strips), Image.Resampling.BOX)


def compare(first, second, top, shift, tile, tile_delta, drift):
    """Returns the mean tile residual, the percent of compared tiles that
    changed, the two crops and the map of changed tiles: white changed, blue
    beyond the other capture's edge."""
    box = (0, top, min(first.width, second.width), min(first.height, second.height))
    first, second = (image.crop(box) for image in (first, second))
    # The eye does not resolve antialiasing: soften both before comparing.
    soft = [image.filter(ImageFilter.GaussianBlur(2.0)) for image in (first, second)]
    reference = soft[0].crop((shift, shift, first.width - shift, first.height - shift))
    columns, rows = reference.width // tile, reference.height // tile
    # The other capture with room above and below, for bands that drifted
    # past its edges.
    other = Image.new('RGB', (second.width, second.height + 2 * drift), (128, 128, 128))
    other.paste(soft[1], (0, drift))
    coarse = other.reduce(COARSE)
    bands = [reference.crop((0, row * tile, columns * tile, (row + 1) * tile)) for row in range(rows)]
    places = [(shift, shift + drift + row * tile) for row in range(rows)]
    found = [band_drift(band, coarse, x, y, drift) if drift else (0, 0.0)
             for band, (x, y) in zip(bands, places)]
    # Still content: the bands from the top down that match where they are,
    # such as a toolbar and a panel above a list.
    still = []
    for (_, y), (offset, mean) in zip(places, found):
        if abs(offset) > shift or mean > tile_delta:
            break
        still.append(y)

    def hidden(row):
        """Whether the other capture cannot show this band where the nearest
        matched band above or below puts it: past its edge, or, moved there,
        where it shows still content instead."""
        y = places[row][1]
        for step in (-1, 1):
            near = row + step
            while 0 <= near < rows and found[near][1] > tile_delta:
                near += step
            if not 0 <= near < rows:
                continue
            target = y + found[near][0]
            if target < drift or target + tile > drift + second.height:
                return True
            if abs(target - y) > shift and any(abs(place - target) < tile for place in still):
                return True
        return False

    best = Image.new('L', (columns, rows))
    beyond = set()
    for row, (band, (x, y)) in enumerate(zip(bands, places)):
        offset, mean = found[row]
        if mean > tile_delta and hidden(row):
            beyond.add(row)
            continue
        # Each tile is matched whole around its band's offset and its
        # neighbours'. Where they drifted differently, as where still content
        # meets a scrolled list, each quarter of a tile may also take any of
        # those offsets, to within a few pixels, so it cannot slip past an
        # element.
        candidates = {found[near][0] for near in (row - 1, row, row + 1) if 0 <= near < rows}
        band_best = None
        for candidate in candidates:
            for dy in range(candidate - shift, candidate + shift + 1):
                for dx in range(-shift, shift + 1):
                    residual = tile_residuals(band, other, x + dx, y + dy, columns, 1)
                    band_best = residual if band_best is None else ImageChops.darker(band_best, residual)
        if len(candidates) > 1:
            quarters = None
            for candidate in candidates:
                for dy in range(candidate - STRIP_SLACK, candidate + STRIP_SLACK + 1):
                    for dx in range(-shift, shift + 1):
                        residual = tile_residuals(band, other, x + dx, y + dy, columns, STRIPS)
                        quarters = residual if quarters is None else ImageChops.darker(quarters, residual)
            band_best = ImageChops.darker(band_best, quarters.resize((columns, 1), Image.Resampling.BOX))
        best.paste(band_best, (0, row))
    tiles = [best.getpixel((column, row)) for row in range(rows) if row not in beyond
             for column in range(columns)]
    changed = sum(1 for value in tiles if value > tile_delta)
    heat = best.point(lambda value: 255 if value > tile_delta else value * 8).convert('RGB')
    for row in beyond:
        heat.paste((40, 60, 160), (0, row, columns, row + 1))
    heat = heat.resize(first.size, Image.Resampling.NEAREST)
    return sum(tiles) / len(tiles), 100.0 * changed / len(tiles), first, second, heat


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--a', default='cranpose', choices=sorted(APPS))
    parser.add_argument('--b', default='compose', choices=sorted(APPS))
    parser.add_argument('--scenario', default='gauntlet')
    parser.add_argument('--extra', default='--ei tier 5', help='more `am start` extras')
    parser.add_argument('--freeze', type=int, default=120)
    parser.add_argument('--status-bar', type=int, default=110,
                        help='pixels at the top to leave out: the clock and icons differ')
    parser.add_argument('--shift', type=int, default=8,
                        help='largest offset in pixels a tile is matched at, around its band')
    parser.add_argument('--drift', type=int, default=160,
                        help='largest vertical offset in pixels a band of tiles is found at')
    parser.add_argument('--tile', type=int, default=48, help='tile side in pixels')
    parser.add_argument('--tile-delta', type=float, default=12.0,
                        help='mean channel difference above which a matched tile counts as changed')
    parser.add_argument('--max-changed', type=float, default=2.0)
    parser.add_argument('--timeout', type=float, default=90.0)
    args = parser.parse_args()
    with device_lock(args.serial):
        compare_pictures(args)


def compare_pair(first, second, *options):
    """`compare` each way: each capture's tiles are found in the other, so an
    element either one lacks is changed in the other's tiles."""
    return compare(first, second, *options), compare(second, first, *options)


def compare_pictures(args):
    """Captures and compares both apps: the caller holds the device lock."""
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    extras = [*args.extra.split(), '--ei', 'freeze', str(args.freeze)]
    first_capture = capture(device, args.a, args.scenario, extras, args.output, args.timeout)
    second_capture = capture(device, args.b, args.scenario, extras, args.output, args.timeout)
    (mean_a, changed_a, first, second, heat_a), (mean_b, changed_b, _, _, heat_b) = compare_pair(
        first_capture, second_capture, args.status_bar, args.shift, args.tile, args.tile_delta, args.drift)
    side = Image.new('RGB', (first.width * 4, first.height))
    for index, picture in enumerate((first, second, heat_a, heat_b)):
        side.paste(picture.convert('RGB'), (first.width * index, 0))
    side.save(args.output / 'side-by-side.png')
    changed = max(changed_a, changed_b)
    result = {'a': args.a, 'b': args.b, 'scenario': args.scenario, 'extras': ' '.join(extras),
              'mean_residual': round(max(mean_a, mean_b), 3),
              'changed_pct_of_a': round(changed_a, 3), 'changed_pct_of_b': round(changed_b, 3),
              'changed_pct': round(changed, 3), 'passed': changed <= args.max_changed}
    (args.output / 'parity.json').write_text(json.dumps(result, indent=1))
    print(json.dumps(result))
    raise SystemExit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()

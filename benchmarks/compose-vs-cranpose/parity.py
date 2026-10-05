#!/usr/bin/env python3
"""Checks that the Cranpose and Compose apps draw the same picture.

Each app is launched with a scenario and `--ei freeze K`, which stops it on its
K-th frame. Both screens are captured on that frame and compared below the
status bar, as the eye compares them:
- both captures are softened, as the eye does not resolve antialiasing, and
  cut into tiles;
- each tile of one is matched against the other at every offset up to
  `--shift` pixels, since the frameworks round layout to pixels differently and
  a list scrolled far drifts by a few pixels without looking any different;
- a tile whose best match still differs by more than `--tile-delta` on average
  is changed: a missing or extra element, another line break, another color.

A pair passes when at most `--max-changed` percent of the tiles changed. The
captures, a side-by-side image and a map of the changed tiles are written next
to `parity.json`.
"""

import argparse
import json
import subprocess
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageFilter

from measure import APPS, Device, device_lock, launch


def capture(device, app, scenario, extras, destination, timeout_s):
    launch(device, app, scenario, extras)
    deadline = time.monotonic() + timeout_s
    while 'PERF frozen' not in device.adb('logcat', '-d', '-s', 'PerfCompare:I'):
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


def compare(first, second, top, shift, tile, tile_delta):
    box = (0, top, min(first.width, second.width), min(first.height, second.height))
    first, second = (image.crop(box) for image in (first, second))
    # The eye does not resolve antialiasing: soften both before comparing.
    soft = [image.filter(ImageFilter.GaussianBlur(2.0)) for image in (first, second)]
    inner = (shift, shift, first.width - shift, first.height - shift)
    reference = soft[0].crop(inner)
    grid = (reference.width // tile, reference.height // tile)
    best = None
    for dy in range(-shift, shift + 1):
        for dx in range(-shift, shift + 1):
            moved = soft[1].crop((inner[0] + dx, inner[1] + dy, inner[2] + dx, inner[3] + dy))
            # Per tile, the mean channel difference at this offset.
            residual = ImageChops.difference(reference, moved).convert('L').resize(
                grid, Image.Resampling.BOX)
            best = residual if best is None else ImageChops.darker(best, residual)
    tiles = list(best.get_flattened_data())
    changed = sum(1 for value in tiles if value > tile_delta)
    heat = best.point(lambda value: 255 if value > tile_delta else value * 8).resize(
        first.size, Image.Resampling.NEAREST)
    return sum(tiles) / len(tiles), 100.0 * changed / len(tiles), first, second, heat


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--scenario', default='gauntlet')
    parser.add_argument('--extra', default='--ei tier 5', help='more `am start` extras')
    parser.add_argument('--freeze', type=int, default=240)
    parser.add_argument('--status-bar', type=int, default=110,
                        help='pixels at the top to leave out: the clock and icons differ')
    parser.add_argument('--shift', type=int, default=8,
                        help='largest offset in pixels a tile is matched at')
    parser.add_argument('--tile', type=int, default=48, help='tile side in pixels')
    parser.add_argument('--tile-delta', type=float, default=12.0,
                        help='mean channel difference above which a matched tile counts as changed')
    parser.add_argument('--max-changed', type=float, default=2.0)
    parser.add_argument('--timeout', type=float, default=90.0)
    args = parser.parse_args()
    with device_lock(args.serial):
        compare_pictures(args)


def compare_pictures(args):
    """Captures and compares both apps: the caller holds the device lock."""
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    extras = [*args.extra.split(), '--ei', 'freeze', str(args.freeze)]
    cranpose = capture(device, 'cranpose', args.scenario, extras, args.output, args.timeout)
    compose = capture(device, 'compose', args.scenario, extras, args.output, args.timeout)
    mean, changed, first, second, heat = compare(cranpose, compose, args.status_bar, args.shift,
                                                 args.tile, args.tile_delta)
    side = Image.new('RGB', (first.width * 3, first.height))
    side.paste(first, (0, 0))
    side.paste(second, (first.width, 0))
    side.paste(heat.convert('RGB'), (first.width * 2, 0))
    side.save(args.output / 'side-by-side.png')
    result = {'scenario': args.scenario, 'extras': ' '.join(extras), 'mean_residual': round(mean, 3),
              'changed_pct': round(changed, 3), 'passed': changed <= args.max_changed}
    (args.output / 'parity.json').write_text(json.dumps(result, indent=1))
    print(json.dumps(result))
    raise SystemExit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Reads the blur and the tint of a scroll edge effect, row by row.

Usage: scroll_edge_profile.py PNG [--scale 3] [--rows 0:180]

The `scroll-edge` reference case scrolls four columns under the edge, 100
points wide on whole points: black, stripes that alternate black and white
every 3 points (period 6), stripes that alternate every 12 (period 24), and
white to the right edge. Over one row the effect draws

    t * tint + (1 - t) * blur(content, sigma)

so black reads t * tint and white t * tint + (1 - t) * 255, which give t and
the tint. The stripes keep their fundamental at
(1 - t) * exp(-2 pi^2 sigma^2 / P^2) of its unblurred amplitude for period P;
a Gaussian blur gives both periods one sigma. Each line prints a point row:
the black and white levels, the stripes' means and fundamentals, t, the
tint, and the sigma of each period in points where its stripes still show.

The columns are read over x ranges that miss the status bar's text and the
fixture's bar labels. Pass `--from-bottom` to count rows up from the bottom
edge.
"""
import argparse
import math

from PIL import Image

PERIODS = (6.0, 24.0)
# Unblurred fundamental of a 0/255 square wave.
FULL = 4.0 / math.pi * 127.5


def fundamental(values, period):
    real = imag = 0.0
    mean = sum(values) / len(values)
    for x, value in enumerate(values):
        angle = 2.0 * math.pi * x / period
        real += (value - mean) * math.cos(angle)
        imag += (value - mean) * math.sin(angle)
    return mean, 2.0 * math.hypot(real, imag) / len(values)


def sigma(amplitude, kept, period):
    ratio = amplitude / (FULL * kept) if kept > 0.02 else 0.0
    if ratio <= 0.01:
        return float("nan")
    if ratio >= 1.0:
        return 0.0
    return period * math.sqrt(math.log(1.0 / ratio) / (2.0 * math.pi**2))


def level(image, x, y):
    return sum(image.getpixel((x + dx, y)) for dx in range(-3, 4)) / 7.0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("png")
    parser.add_argument("--scale", type=int, default=3)
    parser.add_argument("--rows", default="0:180")
    parser.add_argument("--from-bottom", action="store_true")
    args = parser.parse_args()
    image = Image.open(args.png).convert("L")
    scale = args.scale
    first, last = (int(part) for part in args.rows.split(":"))
    quarter = 100
    # 12 periods of the narrow stripes and 2 of the wide ones, in pixels.
    ranges = (((quarter + 12) * scale, (quarter + 84) * scale), ((2 * quarter + 26) * scale, (2 * quarter + 74) * scale))
    print("pt  black white  mean6  amp6  mean24 amp24  t     tint  sigma6 sigma24")
    for row in range(first, last):
        y = row * scale + scale // 2
        if args.from_bottom:
            y = image.height - 1 - y
        if not 0 <= y < image.height:
            break
        reads = [
            fundamental([image.getpixel((x, y)) for x in range(start, end)], period * scale)
            for (start, end), period in zip(ranges, PERIODS)
        ]
        black = level(image, 20 * scale, y)
        white = level(image, image.width - 1 - 16 * scale, y)
        kept = (white - black) / 255.0
        t = 1.0 - kept
        tint = black / t if t > 0.05 else float("nan")
        sigmas = [sigma(amplitude, kept, period) for (_, amplitude), period in zip(reads, PERIODS)]
        print(
            f"{row:3d} {black:6.1f} {white:5.1f} {reads[0][0]:6.1f} {reads[0][1]:5.1f} {reads[1][0]:6.1f}"
            f" {reads[1][1]:5.1f}  {t:4.2f} {tint:6.1f}  {sigmas[0]:5.2f}  {sigmas[1]:5.2f}"
        )


if __name__ == "__main__":
    main()

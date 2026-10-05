import random
import sys
from pathlib import Path
import unittest

from PIL import Image, ImageDraw

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
from parity import compare  # noqa: E402

WIDTH, HEIGHT, HEADER = 540, 1000, 120
SCROLL = 300
# The percent of changed tiles `parity.py` passes by default.
GATE = 2.0


def scene(row_height, blocks=lambda row, block: True, color=lambda rgb: rgb):
    """A still header above a list scrolled `SCROLL` pixels: row after row of
    white cards, each holding blocks placed by the row's own seed. A list of
    shorter rows shows the same cards drifting up the screen."""
    rows = (SCROLL + HEIGHT) // row_height + 2
    content = Image.new('RGB', (WIDTH, rows * row_height), (238, 240, 245))
    draw = ImageDraw.Draw(content)
    for row in range(rows):
        top = row * row_height
        draw.rounded_rectangle((8, top + 4, WIDTH - 8, top + 88), 10, fill=(255, 255, 255))
        rng = random.Random(row)
        for block in range(5):
            left, width = rng.randrange(16, WIDTH - 140), rng.randrange(40, 120)
            down = 10 + 15 * block
            rgb = (rng.randrange(256), rng.randrange(256), rng.randrange(256))
            if blocks(row, block):
                draw.rectangle((left, top + down, left + width, top + down + 10), fill=color(rgb))
    picture = Image.new('RGB', (WIDTH, HEIGHT), (30, 42, 74))
    header = ImageDraw.Draw(picture)
    for tile in range(8):
        header.rectangle((12 + 64 * tile, 70, 60 + 64 * tile, 100), fill=(226, 232, 240))
    picture.paste(content.crop((0, SCROLL, WIDTH, SCROLL + HEIGHT - HEADER)), (0, HEADER))
    return picture


def changed(first, second, drift=160):
    return compare(first, second, top=0, shift=8, tile=48, tile_delta=12.0, drift=drift)[1]


class ParityCompareTest(unittest.TestCase):
    def test_a_list_drifting_down_the_screen_looks_the_same(self):
        self.assertGreater(changed(scene(100), scene(97), drift=0), GATE)
        self.assertLessEqual(changed(scene(100), scene(97)), GATE)

    def test_rows_scrolled_past_the_other_screen_are_left_out(self):
        self.assertLessEqual(changed(scene(97), scene(100)), GATE)

    def test_a_missing_element_is_changed(self):
        missing = scene(97, blocks=lambda row, block: (row, block) != (6, 2))
        self.assertGreater(changed(scene(100), missing), changed(scene(100), scene(97)))

    def test_other_colors_are_changed(self):
        swapped = scene(97, color=lambda rgb: (rgb[2], rgb[1], rgb[0]))
        self.assertGreater(changed(scene(100), swapped), GATE)


if __name__ == '__main__':
    unittest.main()

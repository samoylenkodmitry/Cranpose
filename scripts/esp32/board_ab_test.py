#!/usr/bin/env python3
"""Checks board_ab.py's decisions: the verdict of a revision against the
base and the null leg, what makes a run incomplete, and that two placements
linked to one image stop a run."""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import board_ab  # noqa: E402


def runs(*placements):
    """Results of one leg: a frame time by screen for each placement."""
    return [{screen: (time, 60) for screen, time in placement.items()} for placement in placements]


class Verdicts(unittest.TestCase):
    def table(self, base, null, revision, paired=True):
        results = {
            "base": runs(*({"demo": time} for time in base)),
            "null": runs(*({"demo": time} for time in null)),
            "rev": runs(*({"demo": time} for time in revision)),
        }
        return board_ab.verdicts(results, ["base", "null", "rev"], "null", paired)

    def test_a_revision_above_the_base_in_every_placement_past_the_tolerance_loses(self):
        text = self.table([1.00, 1.02, 1.01, 1.00], [1.01, 1.02, 1.00, 1.00], [1.05, 1.06, 1.05, 1.04])
        self.assertIn("tolerance 0.010 ms", text)
        self.assertIn("rev loses", text)

    def test_the_null_leg_sets_the_tolerance_a_revision_must_pass(self):
        text = self.table([1.00, 1.00, 1.00, 1.00], [1.06, 1.00, 0.98, 1.00], [1.04, 1.05, 1.03, 1.04])
        self.assertIn("tolerance 0.060 ms", text)
        self.assertIn("rev holds", text)

    def test_a_revision_under_the_base_in_every_placement_wins(self):
        text = self.table([1.00, 1.00, 1.00, 1.00], [1.00, 1.01, 1.00, 1.00], [0.95, 0.96, 0.94, 0.95])
        self.assertIn("rev wins", text)

    def test_mixed_placements_with_a_mean_past_the_tolerance_stay_open(self):
        text = self.table([1.00, 1.00, 1.00, 1.00], [1.00, 1.00, 1.00, 1.00], [1.10, 0.99, 1.08, 1.07])
        self.assertIn("rev open", text)

    def test_unpaired_sides_lose_only_when_every_reading_lies_above_the_base(self):
        base, null = [1.00, 1.03, 1.01, 1.02], [1.00, 1.03, 1.01, 1.02]
        self.assertIn("rev loses", self.table(base, null, [1.04, 1.05, 1.06, 1.04], paired=False))
        self.assertIn("rev holds", self.table(base, null, [1.02, 1.05, 1.06, 1.04], paired=False))


class Incomplete(unittest.TestCase):
    def test_a_whole_run_lacks_nothing(self):
        results = {"base": runs({"demo": 1.0, "drawn": 2.0}), "rev": runs({"demo": 1.0, "drawn": 2.0})}
        self.assertEqual(board_ab.incomplete(results, ["base", "rev"], 2, 1), [])

    def test_a_missing_screen_a_missing_count_and_a_missing_placement_are_named(self):
        results = {
            "base": runs({"demo": 1.0, "drawn": 2.0}, {"demo": 1.0, "drawn": 2.0}),
            "rev": [{"demo": (1.0, 60)}],
        }
        results["base"][1]["drawn"] = (2.0, None)
        lacks = board_ab.incomplete(results, ["base", "rev"], 2, 2)
        self.assertIn("rev: 1 of 2 placements", lacks)
        self.assertIn("rev: placement 1 lacks drawn", lacks)
        self.assertIn("base: placement 2 lacks the instructions of drawn", lacks)

    def test_fewer_screens_than_the_board_shows_is_named(self):
        results = {"base": runs({"demo": 1.0}), "rev": runs({"demo": 1.0})}
        self.assertEqual(board_ab.incomplete(results, ["base", "rev"], 4, 1), ["1 screens reported, 4 expected"])


class Images(unittest.TestCase):
    def test_two_placements_that_are_one_image_stop_the_run_and_are_dropped(self):
        with tempfile.TemporaryDirectory() as folder:
            elves = [pathlib.Path(folder) / f"p{seed}.elf" for seed in range(3)]
            for elf, content in zip(elves, [b"first", b"second", b"second"]):
                elf.write_bytes(content)
            with self.assertRaises(SystemExit):
                board_ab.distinct_images("0123456789abcdef", elves)
            self.assertEqual([elf.exists() for elf in elves], [True, False, False])

    def test_distinct_images_pass(self):
        with tempfile.TemporaryDirectory() as folder:
            elves = [pathlib.Path(folder) / f"p{seed}.elf" for seed in range(2)]
            for elf, content in zip(elves, [b"first", b"second"]):
                elf.write_bytes(content)
            board_ab.distinct_images("0123456789abcdef", elves)
            self.assertTrue(all(elf.exists() for elf in elves))


if __name__ == "__main__":
    unittest.main()

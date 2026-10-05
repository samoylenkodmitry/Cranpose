import importlib.util
import sys
from pathlib import Path
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('ab', BENCHMARK / 'ab.py')
ab = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ab)

FPS = ab.DECIDING['fps']


def decide(a, b, last=False):
    return ab.verdict(a, b, *FPS, last)


class AbDecisionTest(unittest.TestCase):
    def test_one_leg_a_side_decides_nothing(self):
        self.assertIsNone(decide([30.0], [60.0]))

    def test_close_tight_builds_are_the_same_after_one_round(self):
        self.assertEqual(decide([60.0, 59.9], [60.1, 60.0]), 'same')

    def test_a_large_separated_gap_settles_after_one_round(self):
        self.assertEqual(decide([26.3, 26.2], [52.6, 53.0]), 'better')
        self.assertEqual(decide([52.0, 52.5], [46.0, 46.4]), 'worse')

    def test_a_small_separated_gap_needs_more_legs(self):
        # The A/A run that a 2% rule judged different: same build, two packages.
        release, main = [51.1, 50.7, 52.1], [53.2, 52.8, 52.8]
        self.assertIsNone(decide(release[:2], main[:2]))
        self.assertIsNone(decide(release, main))

    def test_overlapping_noisy_builds_close_as_the_same_at_the_last_pair(self):
        a, b = [52.6, 53.0, 52.4, 52.6], [52.8, 51.5, 52.1, 52.2]
        self.assertEqual(decide(a, b, last=True), 'same')

    def test_lower_is_better_for_cpu_per_frame(self):
        rule = ab.DECIDING['cpu_ms_per_frame']
        self.assertEqual(ab.verdict([65.4, 66.6], [32.2, 32.2], *rule, False), 'better')


if __name__ == '__main__':
    unittest.main()

import importlib.util
import sys
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('desktop', BENCHMARK / 'desktop.py')
desktop = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(desktop)

WEBCONTENT = '/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/' \
             'com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent'


class DesktopAccountingTest(unittest.TestCase):
    def test_ps_times_read_with_days_hours_and_fractions(self):
        self.assertEqual(desktop.seconds('1-02:03:04'), 93_784.0)
        self.assertEqual(desktop.seconds('12:03:04'), 43_384.0)
        self.assertEqual(desktop.seconds('03:04.50'), 184.5)

    def test_an_app_owns_its_children_and_the_webkit_services_started_for_it(self):
        # pid: (parent, cpu seconds, seconds since start, command)
        table = {
            100: (1, 4.0, 10.0, '/apps/perf-compare-tauri'),
            101: (100, 1.0, 9.0, '/apps/helper'),
            200: (1, 30.0, 8.0, WEBCONTENT),
            300: (1, 50.0, 3600.0, WEBCONTENT),
            400: (1, 2.0, 5.0, '/usr/bin/other'),
        }
        self.assertEqual(desktop.tree(100, table, 11.0), {100, 101, 200})


class DesktopRoundsTest(unittest.TestCase):
    """`run` on legs a stand-in for `measure` returns, as the run would read
    `FrameCount`'s rates."""

    def rounds(self, rates):
        measured = []

        def measure(name, args, work, page, stage, label, earlier):
            measured.append(label)
            return {'fps': rates[len(measured) - 1], 'frames': 90, 'window_s': 3.0, 'interval_p50_ms': 16.7,
                    'interval_p99_ms': 20.0, 'cpu_cores': 1.0, 'server_cores': 0.2, 'other_cores': 0.5,
                    'cpu_ms_per_frame': 40.0}

        args = SimpleNamespace(parity=False, output=Path('results'), rounds=2, max_others=1.5, main=None,
                               release=None, tier=16, warmup=1.0, window=3.0, max_window=8.0, min_frames=40)
        with patch.object(desktop, 'measure', measure), \
                patch.object(desktop.subprocess, 'run', return_value=SimpleNamespace(stdout='Apple M3 Pro')):
            report = desktop.run(args, ['fyne'], 'page', Path('stage'))
        return measured, report['scenarios'][0]

    def test_a_leg_far_off_the_earlier_ones_is_measured_once_more_and_the_median_decides(self):
        measured, scenario = self.rounds([24.3, 60.1, 24.6])
        self.assertEqual(measured, ['fyne-1-1', 'fyne-2-1', 'fyne-2-again-1'])
        self.assertEqual([leg['fps'] for leg in scenario['legs']], [24.3, 60.1, 24.6])
        self.assertEqual(scenario['summary']['fyne']['fps'], 24.6)

    def test_legs_that_agree_are_measured_once_each(self):
        measured, scenario = self.rounds([24.3, 25.9])
        self.assertEqual(measured, ['fyne-1-1', 'fyne-2-1'])
        self.assertEqual(len(scenario['legs']), 2)


if __name__ == '__main__':
    unittest.main()

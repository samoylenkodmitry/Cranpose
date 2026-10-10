import importlib.util
import sys
from pathlib import Path
from types import SimpleNamespace
import tempfile
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


class DesktopClocksTest(unittest.TestCase):
    def test_a_line_macmon_left_unfinished_when_stopped_is_not_a_sample(self):
        clocks = desktop.Clocks.__new__(desktop.Clocks)
        output = ('{"pcpu_freq_mhz": 4000, "gpu_freq_mhz": 500}\n'
                  '{"pcpu_freq_mhz": 4100, "gpu_freq_mhz": 700}\n'
                  '{"pcpu_freq_mhz": 41')
        clocks.process = SimpleNamespace(terminate=lambda: None, communicate=lambda timeout: (output, None))
        self.assertEqual(clocks.stop(), {'cpu_mhz': 4050, 'gpu_mhz': 600})


class DesktopRoundsTest(unittest.TestCase):
    """`run` on legs a stand-in for `measure` returns, as the run would read
    `FrameCount`'s rates, with the apps' state cleared by a stand-in that
    notes each clearing."""

    def rounds(self, rates, rounds=1, browser=None, app='fyne'):
        """A rate is a leg, or a (rate, cores other processes spent) pair; a
        `RuntimeError` is an attempt that failed. Each round measures the
        app's first start, then its second."""
        measured = []
        self.cleared = []

        def measure(name, args, work, page, stage, label, earlier, profile):
            measured.append(label)
            rate = rates[len(measured) - 1]
            if isinstance(rate, Exception):
                raise rate
            rate, others = rate if isinstance(rate, tuple) else (rate, 0.5)
            return {'fps': rate, 'frames': 90, 'run_s': 3.0, 'first_frame_s': 0.4,
                    'frame_ms': [400.0, 416.7, 433.3], 'fps_by_second': [rate / 2, rate, rate],
                    'interval_p50_ms': 16.7,
                    'interval_p99_ms': 20.0, 'cpu_cores': 1.0, 'server_cores': 0.2, 'other_cores': others,
                    'cpu_ms_per_frame': 40.0}

        def clear(profile):
            self.cleared.append((len(measured), profile.name))

        args = SimpleNamespace(parity=False, output=Path('results'), rounds=rounds, max_others=1.5, main=None,
                               release=None, tier=16, run=3.0, browser=browser)
        with patch.object(desktop, 'measure', measure), patch.object(desktop, 'clear_app_state', clear), \
                patch.object(desktop.subprocess, 'run', return_value=SimpleNamespace(stdout='Apple M3 Pro')):
            report = desktop.run(args, [app], 'page', Path('stage'))
        return measured, report['scenarios'][0] if browser is None else report

    def test_each_round_clears_the_apps_state_before_the_first_start_and_keeps_it_for_the_second(self):
        measured, scenario = self.rounds([24.3, 30.0, 24.6, 30.5], rounds=2)
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-second-1', 'fyne-2-first-1', 'fyne-2-second-1'])
        self.assertEqual(self.cleared, [(0, 'chrome-profile-fyne-1'), (2, 'chrome-profile-fyne-2')],
                         "each round's first start begins on cleared state and a profile of its own")
        self.assertEqual([leg['start'] for leg in scenario['legs']], ['first', 'second'] * 2)
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.45)
        self.assertEqual(scenario['starts']['second']['summary']['fyne']['fps'], 30.25)

    def test_a_leg_far_off_the_earlier_ones_of_its_start_is_measured_once_more_and_the_median_decides(self):
        measured, scenario = self.rounds([24.3, 30.0, 60.1, 24.6, 30.5], rounds=2)
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-second-1', 'fyne-2-first-1',
                                    'fyne-2-first-again-1', 'fyne-2-second-1'])
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.6)
        self.assertEqual(self.cleared, [(0, 'chrome-profile-fyne-1'), (2, 'chrome-profile-fyne-2'),
                                        (3, 'chrome-profile-fyne-2')],
                         'a first start measured again begins on cleared state again')

    def test_a_failed_attempt_is_measured_again(self):
        measured, scenario = self.rounds([RuntimeError('FrameCount: process 1 shows no window'), 24.3, 30.0])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-second-1'])
        self.assertEqual([leg['fps'] for leg in scenario['legs']], [24.3, 30.0])

    def test_an_app_no_first_start_measured_leaves_its_legs_out_and_the_run_goes_on(self):
        failure = RuntimeError('fyne.log: no "PERF first_frame" within 60 s')
        measured, scenario = self.rounds([failure, failure, failure])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-first-3'],
                         'no second start without a first one')
        self.assertEqual(scenario['legs'], [])
        self.assertEqual(scenario['starts']['first']['summary']['fyne'], {}, 'no values: the dashboard shows a dash')
        self.assertEqual([(failure['subject'], failure['start']) for failure in scenario['failures']],
                         [('fyne', 'first')] * 3,
                         'each failed attempt is kept, so the dashboard can say why fyne has no frames')

    def test_a_browser_run_is_a_run_of_its_own_kind_on_a_device_named_for_the_browser(self):
        with patch.object(desktop.versions, 'chrome_version', return_value='154.0.8037.98'):
            _, report = self.rounds([24.3, 30.0], browser=Path('dist'), app='web')
        self.assertEqual(report['kind'], 'browser')
        self.assertEqual(report['device']['ro.product.model'], 'Apple M3 Pro · Chrome 154')
        self.assertEqual(report['scenarios'][0]['extras'], 'tier 16, 1280 x 820 browser window')
        self.assertEqual(report['subjects'][0]['label'], 'Web Chrome 154.0.8037.98')
        self.assertEqual(report['protocol']['starts'], ['first', 'second'])

    def test_a_disturbed_leg_stays_in_the_run_marked_and_out_of_the_medians(self):
        measured, scenario = self.rounds([(10.0, 3.0), 24.3, 30.0])
        self.assertEqual(measured, ['fyne-1-first-1', 'fyne-1-first-2', 'fyne-1-second-1'])
        self.assertEqual([(leg['fps'], leg['disturbed']) for leg in scenario['legs']],
                         [(10.0, True), (24.3, False), (30.0, False)])
        self.assertEqual(scenario['starts']['first']['summary']['fyne']['fps'], 24.3)

    def test_every_frame_time_from_the_launch_is_kept_in_the_run(self):
        _, scenario = self.rounds([24.0, 26.0])
        self.assertEqual([leg['frame_ms'] for leg in scenario['legs']], [[400.0, 416.7, 433.3]] * 2)


class AppStateTest(unittest.TestCase):
    def test_clearing_removes_the_apps_caches_and_the_profile_and_nothing_else(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for path in ['cache/com.apple.metal/16777235_419/functions.data',
                         'cache/dev.perfcompare.swiftui/com.apple.metal/x', 'cache/dev.other/kept',
                         'support/perf-compare/pipeline-cache.bin', 'support/Code/kept',
                         'profile/Default/GPUCache/data_0']:
                (root / path).parent.mkdir(parents=True, exist_ok=True)
                (root / path).write_text('x')
            state = [(root / 'cache', 'com.apple.metal'), (root / 'cache', 'dev.perfcompare.*'),
                     (root / 'support', 'perf-compare*')]
            with patch.object(desktop, 'APP_STATE', state):
                desktop.clear_app_state(root / 'profile')
            left = sorted(str(path.relative_to(root)) for path in root.rglob('*') if path.is_file())
            self.assertEqual(left, ['cache/dev.other/kept', 'support/Code/kept'])


if __name__ == '__main__':
    unittest.main()

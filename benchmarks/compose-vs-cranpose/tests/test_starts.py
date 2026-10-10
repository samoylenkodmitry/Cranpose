"""Each round measures an app's first start after its data is cleared and its
second start, and keeps the two apart."""
import importlib.util
import sys
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('ab', BENCHMARK / 'ab.py')
ab = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ab)
# The `measure` module ab.py runs with.
measure = sys.modules['measure']

WELCOME = ('<node text="Stay signed out" resource-id="com.android.chrome:id/signin_fre_dismiss_button" '
           'class="android.widget.Button" clickable="true" bounds="[72,1847][1008,1991]" />')
TAB = '<node text="about:blank" resource-id="com.android.chrome:id/url_bar" bounds="[240,101][636,269]" />'


class Phone(measure.Device):
    """A device whose screens are `screens`, in turn, and which notes every
    command it is given."""

    def __init__(self, screens):
        super().__init__('serial')
        self.screens = list(screens)
        self.commands = []

    def shell(self, *args, timeout=120, check=True):
        self.commands.append(args)
        if args[:2] == ('pm', 'clear'):
            return 'Success'
        if args[:1] == ('cat',):
            return self.screens.pop(0) if len(self.screens) > 1 else self.screens[0]
        return ''


class ChromeResetTest(unittest.TestCase):
    def test_chrome_is_cleared_its_first_run_passed_and_stopped_before_a_first_visit(self):
        phone = Phone([WELCOME, TAB])
        with patch.object(measure.time, 'sleep'):
            measure.reset_chrome(phone)
        self.assertIn(('pm', 'clear', measure.CHROME), phone.commands)
        self.assertIn(('input', 'tap', '540', '1919'), phone.commands, '"Stay signed out"')
        self.assertEqual(phone.commands[-1], ('am', 'force-stop', measure.CHROME))

    def test_a_first_run_that_never_reaches_a_tab_names_the_views_it_showed(self):
        phone = Phone([WELCOME])
        with patch.object(measure, 'WELCOME_TIMEOUT_S', 0.0), patch.object(measure.time, 'sleep'):
            with self.assertRaisesRegex(RuntimeError, 'signin_fre_dismiss_button'):
                measure.reset_chrome(phone)


def leg(fps):
    """What `measure_run` reports of a leg, at `fps`."""
    return {'fps': fps, 'frames': int(fps * 10), 'run_s': 10.0, 'first_frame_s': 0.4, 'frame_ms': [400.0],
            'fps_by_second': [fps] * 10, 'first_poll_full': False, 'janky_pct': 0.0, 'interval_p99_ms': 20.0,
            'cpu_ms_per_frame': 30.0, 'desired_to_present_p50_ms': 26.0, 'memory': {'total_pss_kb': 102400},
            'throttled': False, 'temperature_before': {}, 'temperature_after': {}}


class StartsApartTest(unittest.TestCase):
    def test_each_start_is_judged_on_its_own_legs_after_the_data_is_cleared(self):
        # The release and main start alike a second time; main's first start is faster.
        rates = {('cranpose-release', 'first'): 30.0, ('cranpose', 'first'): 36.0,
                 ('cranpose-release', 'second'): 50.0, ('cranpose', 'second'): 50.0}
        events = []
        # Each subject's legs alternate first and second starts.
        measured = (start for _ in range(8) for start in ab.STARTS)

        def reset(target, device):
            events.append(('reset', target.name))

        def measure_run(device, app, scenario, args, destination):
            start = next(measured)
            events.append((start, app))
            return leg(rates[(app, start)])

        args = SimpleNamespace(max_pairs=4, extra='', output=Path('results'))
        with patch.object(ab.AppTarget, 'reset', reset), patch.object(ab, 'measure_run', measure_run):
            scenario = ab.compare_scenario(None, 'gauntlet', ['cranpose-release', 'cranpose'], args)
        self.assertEqual(events[:6], [('reset', 'cranpose-release'), ('first', 'cranpose-release'),
                                      ('second', 'cranpose-release'), ('reset', 'cranpose'),
                                      ('first', 'cranpose'), ('second', 'cranpose')])
        self.assertEqual(len(scenario['legs']), 8, 'both starts settle after the second pair')
        self.assertEqual(scenario['starts']['first']['verdicts']['fps'], 'better')
        self.assertEqual(scenario['starts']['second']['verdicts']['fps'], 'same')
        self.assertEqual(scenario['starts']['first']['summary']['cranpose']['fps'], 36.0)
        self.assertEqual(scenario['starts']['second']['summary']['cranpose']['fps'], 50.0)


if __name__ == '__main__':
    unittest.main()

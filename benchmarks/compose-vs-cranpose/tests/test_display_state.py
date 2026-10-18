import importlib.util
import json
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('desktop', BENCHMARK / 'desktop.py')
desktop = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(desktop)


def display(**changes):
    """macm3's HISENSE television as `FrameCount --display` reported it on
    2026-10-18, with `changes`."""
    return {'id': 3, 'main': True, 'active': True, 'asleep': False, 'builtin': False, 'mirrors': False,
            'name': 'HISENSE', 'points': [3200, 1800], 'pixels': [6400, 3600], 'refresh_hz': 60.0,
            'scale': 2.0, **changes}


def state(shown=None, **changes):
    return {'main': 3, 'displays': [shown or display()], 'on_console': True,
            'link': {'hz': 59.6, 'nominal_hz': 60.0, 'ticks': 119, 'max_gap_ms': 18.7}, **changes}


class DisplayProblemsTest(unittest.TestCase):
    def test_the_display_the_gauntlet_was_calibrated_on_has_no_problem(self):
        self.assertEqual(desktop.display_problems(state()), [])

    def test_a_display_the_window_server_has_put_to_sleep_is_refused(self):
        problems = desktop.display_problems(state(display(active=False, asleep=True)))
        self.assertEqual(problems, ['the main display 3 is asleep'])

    def test_a_display_link_that_ticks_below_its_mode_is_refused(self):
        slow = state(link={'hz': 31.0, 'nominal_hz': 60.0, 'ticks': 62, 'max_gap_ms': 40.0})
        self.assertEqual(desktop.display_problems(slow),
                         ['its display link ticked 31.0 times a second, not the 60 its mode asks for'])

    def test_a_display_link_that_stopped_in_the_middle_is_refused(self):
        stalled = state(link={'hz': 59.0, 'nominal_hz': 60.0, 'ticks': 118, 'max_gap_ms': 900.0})
        self.assertEqual(desktop.display_problems(stalled), ['its display link stopped for 900 ms'])

    def test_a_mode_of_another_rate_or_scale_is_refused(self):
        problems = desktop.display_problems(state(display(refresh_hz=120.0, scale=1.0)))
        self.assertEqual(problems, ['the main display runs at 120.0 Hz, not 60',
                                    'the main display draws windows at 1.0x, not 2x'])

    def test_a_locked_screen_or_a_session_away_from_the_console_is_refused(self):
        problems = desktop.display_problems(state(on_console=False, screen_locked=True))
        self.assertEqual(problems, ['the login session is not on the console', 'the screen is locked'])

    def test_no_main_display_is_refused(self):
        self.assertEqual(desktop.display_problems({'displays': []}), ['the window server has no main display'])

    def observed(self, shown, ambient):
        with patch.object(desktop, 'display_state', return_value=shown), \
                patch.object(desktop, 'ambient_fps', return_value=ambient):
            return desktop.observe_display(Path("stage"), "start")

    def test_a_display_that_is_as_calibrated_and_quiet_leaves_nothing_to_refuse(self):
        observed, problems = self.observed(state(), 0.0)
        self.assertEqual(problems, [])
        self.assertEqual(observed['ambient_fps'], 0.0, 'the run records how quiet it was')

    def test_the_quiet_of_macm3s_television_is_not_refused(self):
        # 2026-10-18 on macm3: a window that never changes was composited 2.3
        # times a second with nothing else moving on the television.
        _, problems = self.observed(state(), 2.3)
        self.assertEqual(problems, [])

    def test_something_moving_on_the_display_is_refused_with_its_rate(self):
        # 2026-10-18 14:35 on macm3: every framework's window was composited
        # 60 times a second, as a Cranpose window animating beside it does.
        _, problems = self.observed(state(), 59.8)
        self.assertEqual(len(problems), 1)
        self.assertIn('something moves on the display', problems[0])
        self.assertIn('59.8 times a second', problems[0])

    def test_every_reason_is_said_when_the_run_refuses(self):
        _, problems = self.observed(state(display(asleep=True)), 30.0)
        said = desktop.refusal(problems)
        self.assertIn('the main display 3 is asleep', said)
        self.assertIn('something moves on the display', said)

    def test_a_display_link_that_never_ticked_is_a_problem(self):
        stopped = {'displays': [display()], 'link': {'hz': 0.0, 'nominal_hz': 60.0, 'max_gap_ms': 0.0}}
        self.assertEqual(desktop.display_problems(stopped),
                         ['its display link ticked 0.0 times a second, not the 60 its mode asks for'])


class DisplayLegTest(unittest.TestCase):
    def test_a_leg_on_a_display_that_stopped_ticking_is_marked_and_measured_again(self):
        def leg(problems):
            return {'fps': 20.0, 'cpu_ms_per_frame': 70.0, 'other_cores': 0.5, 'display_problems': problems,
                    'interval_p99_ms': 80.0, 'first_frame_s': 1.0}

        stopped = ['its display link stopped for 900 ms']
        legs = iter([leg(stopped), leg([])])
        measured = []

        def measure(name, args, work, page, stage, label, earlier, profile, program=None):
            measured.append(label)
            return next(legs)

        args = SimpleNamespace(browser=None, max_others=1.5, output=Path('results'), tier=16, run=10.0)
        with patch.object(desktop, 'measure', measure), patch.object(desktop, 'clear_app_state', lambda _: None), \
                patch.object(desktop, 'fresh_install', lambda name, stage, label: 'installed'):
            found = desktop.undisturbed_leg('flutter', args, 'page', Path('stage'), 'flutter-1-first', [], [],
                                            Path('profile'), 'first', 0.5, [])
        self.assertEqual(measured, ['flutter-1-first-1', 'flutter-1-first-2'])
        self.assertEqual([(leg['disturbed'], leg['display_problems']) for leg in found],
                         [(True, stopped), (False, [])])


class DisplayRunTest(unittest.TestCase):
    """`main` on a run a stand-in for `run` returns, with the display the
    stand-in for `observe_display` reports at its start and at its end."""

    def main(self, observations):
        ran = []
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder) / 'out'
            arguments = ['desktop.py', '--output', str(output), '--apps', 'fyne', '--tier', '16']
            report = {'kind': 'frameworks', 'scenarios': [{'starts': {}}]}
            with patch.object(sys, 'argv', arguments), patch.object(desktop, 'current_framecount'), \
                    patch.object(desktop, 'serve_page', return_value='page'), \
                    patch.object(desktop, 'run', lambda *args: ran.append(args) or report), \
                    patch.object(desktop, 'observe_display', side_effect=observations):
                try:
                    desktop.main()
                    exit_message = None
                except SystemExit as stopped:
                    exit_message = str(stopped)
            written = json.loads((output / 'report.json').read_text()) if (output / 'report.json').exists() else None
        return ran, written, exit_message

    def test_a_run_keeps_the_display_it_began_and_ended_on(self):
        began, ended = (state(), []), (state(), [])
        began[0]['ambient_fps'], ended[0]['ambient_fps'] = 1.6, 2.3
        ran, written, exit_message = self.main([began, ended])
        self.assertEqual(len(ran), 1)
        self.assertIsNone(exit_message)
        self.assertEqual((written['display']['start']['ambient_fps'], written['display']['end']['ambient_fps']),
                         (1.6, 2.3))

    def test_a_display_that_something_moves_on_is_refused_before_the_first_leg(self):
        ran, written, exit_message = self.main([(state(), ['something moves on the display: 60 times a second'])])
        self.assertEqual(ran, [], 'nothing is measured')
        self.assertIsNone(written)
        self.assertIn('something moves on the display', exit_message)

    def test_a_display_that_something_moved_on_by_the_end_is_kept_in_a_report_that_is_not_published(self):
        ran, written, exit_message = self.main([(state(), []), (state(), ['something moves on the display'])])
        self.assertEqual(len(ran), 1)
        self.assertIsNotNone(written, 'the report is kept to look at')
        self.assertIn('by the end of the run', exit_message, 'the failing exit keeps the nightly from publishing it')


if __name__ == '__main__':
    unittest.main()

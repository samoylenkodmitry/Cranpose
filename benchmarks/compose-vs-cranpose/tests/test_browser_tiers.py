"""The phone's browser run measures every page at each of `--tiers`, one
scenario a tier: the first tier keeps the scenario's name, so its series goes
on, and each heavier tier names a scenario of its own."""
import contextlib
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from test_starts import Phone, leg

BENCHMARK = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('frameworks', BENCHMARK / 'frameworks.py')
frameworks = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(frameworks)


class BrowserPhone(Phone):
    """A phone whose adb commands all succeed with no output."""

    def adb(self, *args, timeout=120, check=True):
        self.commands.append(args)
        return ''


class BrowserTiersTest(unittest.TestCase):
    def test_each_tier_is_a_scenario_measured_at_that_tier(self):
        measured = []

        def measure_run(device, target, scenario, args, destination):
            measured.append((scenario, target.name, target.tier))
            return leg(50.0)

        with tempfile.TemporaryDirectory() as folder:
            for app in ('cranpose', 'egui'):
                page = Path(folder) / app
                page.mkdir()
                (page / 'index.html').write_bytes(app.encode())
            output = Path(folder) / 'out'
            arguments = ['frameworks.py', '--serial', 'serial', '--output', str(output),
                         '--browser', folder, '--apps', 'cranpose,egui']
            with patch.object(sys, 'argv', arguments), \
                    patch.object(frameworks, 'Device', lambda serial: BrowserPhone([''])), \
                    patch.object(frameworks, 'BrowserServer', lambda root: SimpleNamespace(port=1)), \
                    patch.object(frameworks, 'device_lock', lambda serial: contextlib.nullcontext()), \
                    patch.object(frameworks.PageTarget, 'reset', lambda target, device: None), \
                    patch.object(frameworks, 'measure_run', measure_run):
                frameworks.main()
            run = json.loads((output / 'frameworks.json').read_text())
        self.assertEqual([(scenario['scenario'], scenario['extras']) for scenario in run['scenarios']],
                         [('gauntlet', 'tier 8, in Chrome'), ('gauntlet-tier-10', 'tier 10, in Chrome')])
        self.assertEqual(sorted(set(measured)),
                         [('gauntlet', 'cranpose', 8), ('gauntlet', 'egui', 8),
                          ('gauntlet-tier-10', 'cranpose', 10), ('gauntlet-tier-10', 'egui', 10)])
        for scenario in run['scenarios']:
            self.assertEqual(scenario['app_bytes'], {'cranpose': 8, 'egui': 4})
            self.assertEqual(len(scenario['legs']), 4)
            self.assertEqual(scenario['starts']['first']['summary']['egui']['fps'], 50.0)

    def test_a_hidden_page_disturbs_its_leg_and_a_stall_on_a_shown_page_is_measured(self):
        shown = {'hidden': False, 'visibility': [[0.9, 'visible']], 'gpu_events': [], 'errors': [],
                 'error_count': 0, 'last_beat_s': 9.9, 'frame_requests': 580, 'last_frame_request_s': 9.9}
        hidden = {**shown, 'hidden': True, 'visibility': [[0.9, 'visible'], [3.1, 'hidden']]}
        stalled = {**shown, 'frame_requests': 130, 'last_frame_request_s': 3.2}
        # Two rounds of a first and a second start: the first round's second
        # start is hidden, the second round's stops drawing on a shown page.
        measured = iter([(50.0, shown), (10.0, hidden), (50.0, shown), (8.0, stalled)])

        def measure_run(device, target, scenario, args, destination):
            fps, page = next(measured)
            return {**leg(fps), 'page': page}

        with tempfile.TemporaryDirectory() as folder:
            page = Path(folder) / 'cranpose'
            page.mkdir()
            (page / 'index.html').write_bytes(b'cranpose')
            output = Path(folder) / 'out'
            arguments = ['frameworks.py', '--serial', 'serial', '--output', str(output), '--browser', folder,
                         '--apps', 'cranpose', '--tiers', '10', '--rounds', '2']
            with patch.object(sys, 'argv', arguments), \
                    patch.object(frameworks, 'Device', lambda serial: BrowserPhone([''])), \
                    patch.object(frameworks, 'BrowserServer', lambda root: SimpleNamespace(port=1)), \
                    patch.object(frameworks, 'device_lock', lambda serial: contextlib.nullcontext()), \
                    patch.object(frameworks.PageTarget, 'reset', lambda target, device: None), \
                    patch.object(frameworks, 'measure_run', measure_run):
                frameworks.main()
            run = json.loads((output / 'frameworks.json').read_text())
        [scenario] = run['scenarios']
        self.assertEqual([(leg['start'], leg['fps'], leg['disturbed']) for leg in scenario['legs']],
                         [('first', 50.0, False), ('second', 10.0, True), ('first', 50.0, False),
                          ('second', 8.0, False)])
        self.assertEqual(scenario['legs'][3]['page']['last_frame_request_s'], 3.2)
        self.assertEqual(scenario['starts']['second']['summary']['cranpose']['fps'], 8.0,
                         'the hidden leg stays out of the median; the stall on a shown page is in it')


if __name__ == '__main__':
    unittest.main()

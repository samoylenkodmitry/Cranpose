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
            self.assertEqual(len(scenario['legs']), 4)
            self.assertEqual(scenario['starts']['first']['summary']['egui']['fps'], 50.0)


if __name__ == '__main__':
    unittest.main()

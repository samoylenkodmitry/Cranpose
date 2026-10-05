import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location('publish', Path(__file__).resolve().parent / 'publish.py')
publish = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publish)


def run():
    return {
        'kind': 'nightly',
        'started_at': '2026-10-05T01:30:00+00:00',
        'device': {'ro.product.model': 'EVR-AL00'},
        'main': 'abc123def456',
        'release': 'v0.9.7',
        'subjects': [{'name': 'cranpose-release', 'label': 'v0.9.7', 'package': 'x'},
                     {'name': 'cranpose', 'label': 'abc123def', 'package': 'y'}],
        'duration_s': 420,
        'scenarios': [{
            'scenario': 'gauntlet',
            'legs': [{}, {}, {}, {}],
            'summary': {'cranpose-release': {'fps': 50.0, 'cpu_ms_per_frame': 33.0, 'gpu_mhz': 139},
                        'cranpose': {'fps': 52.8, 'cpu_ms_per_frame': 32.0, 'gpu_mhz': 139}},
            'verdicts': {'fps': 'better', 'cpu_ms_per_frame': 'same', 'desired_to_present_p50_ms': 'same'},
        }],
        'confirmed_regressions': {},
    }


class IndexEntryTest(unittest.TestCase):
    def test_an_entry_carries_each_scenarios_medians_and_verdicts_without_its_legs(self):
        entry = publish.index_entry(run(), 'runs/x.json')
        gauntlet = entry['scenarios']['gauntlet']
        self.assertEqual(gauntlet['legs'], 4)
        self.assertEqual(gauntlet['summary']['cranpose']['fps'], 52.8)
        self.assertNotIn('gpu_mhz', gauntlet['summary']['cranpose'], 'only the charted metrics')
        self.assertEqual(gauntlet['verdicts']['fps'], 'better')
        self.assertEqual(entry['device'], 'EVR-AL00')
        self.assertEqual([subject['label'] for subject in entry['subjects']], ['v0.9.7', 'abc123def'])


if __name__ == '__main__':
    unittest.main()

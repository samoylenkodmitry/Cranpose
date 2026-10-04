import importlib.util
from pathlib import Path
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('measure', BENCHMARK / 'measure.py')
measure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(measure)


class ClockSamplesTest(unittest.TestCase):
    def test_unreadable_caps_keep_the_clocks_and_never_read_as_throttling(self):
        lines = [
            'F 139000000 830000000 1805000 1920000 1460000 720000000 - - -',
            'F 277000000 830000000 1805000 1920000 2600000 720000000 - - -',
        ]
        summary = measure.clock_summary(measure.clock_samples(lines))
        self.assertEqual(summary['gpu_mhz'], 208.0)
        self.assertEqual(summary['cpu_big_mhz'], 2030.0)
        self.assertEqual(summary['cap_pct'], {'gpu': 100.0})

    def test_a_lowered_cap_reads_as_throttling(self):
        lines = ['F 139000000 830000000 1805000 1920000 1460000 360000000 1805000 1920000 2600000']
        summary = measure.clock_summary(measure.clock_samples(lines))
        self.assertEqual(summary['cap_pct']['gpu'], 50.0)
        self.assertEqual(summary['cap_pct']['cpu_big'], 100.0)

    def test_another_socs_short_line_reports_no_clocks(self):
        summary = measure.clock_summary(measure.clock_samples(['F 139000000 830000000']))
        self.assertEqual(summary, {'cap_pct': {}})


if __name__ == '__main__':
    unittest.main()

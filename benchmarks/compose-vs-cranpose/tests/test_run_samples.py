import importlib.util
from pathlib import Path
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('measure', BENCHMARK / 'measure.py')
measure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(measure)


def stat(pid, ticks):
    """A /proc/PID/stat line whose user and system ticks add up to `ticks`."""
    return f'PS {pid} (app name) S 1 1 0 0 -1 0 0 0 0 0 {ticks} 0 0 0 20 0 1 0'


KIRIN = 'F 600000000 830000000 1805000 1920000 2600000 720000000 - - -'


class RunSamplesTest(unittest.TestCase):
    def test_each_interval_reads_the_cores_cpu_per_frame_memory_and_clocks_since_the_one_before(self):
        lines = [
            'PU 100.5', stat(10, 40), 'PM 10 50000 25600 900', KIRIN,
            'PU 101.0', stat(10, 90), 'PM 10 50000 51200 900', KIRIN,
        ]
        frames = [100.2, 100.4, 100.7, 100.8, 100.9, 101.0]
        samples = measure.run_samples(lines, 100.0, 100, frames)
        self.assertEqual(samples['t_ms'], [500, 1000])
        # 40 ticks of 100 a second in half a second: 0.8 cores, 200 ms over 2 frames.
        self.assertEqual(samples['cores'], [0.8, 1.0])
        self.assertEqual(samples['cpu_ms_per_frame'], [200.0, 125.0])
        self.assertEqual(samples['ram_mb'], [100.0, 200.0])
        self.assertEqual(samples['cpu_mhz'], [2600, 2600])
        self.assertEqual(samples['gpu_mhz'], [600, 600])

    def test_a_helper_process_that_ended_reads_as_no_cpu_rather_than_less_than_none(self):
        lines = [
            'PU 100.5', stat(10, 40), stat(11, 60), 'PM 10 1 10 1', KIRIN,
            # The helper ended: its ticks leave the sum.
            'PU 101.0', stat(10, 45), 'PM 10 1 10 1', KIRIN,
        ]
        samples = measure.run_samples(lines, 100.0, 100, [100.6])
        self.assertEqual(samples['cores'], [2.0, 0.0])

    def test_an_interval_without_frames_has_no_cpu_per_frame(self):
        lines = ['PU 100.5', stat(10, 10), 'PM 10 1 10 1', KIRIN]
        samples = measure.run_samples(lines, 100.0, 100, [])
        self.assertEqual(samples['cpu_ms_per_frame'], [None])

    def test_another_socs_gpu_clock_comes_from_the_mali_node(self):
        lines = ['PU 100.5', stat(10, 10), 'PM 10 1 10 1', 'F 1 2', 'G 848000']
        samples = measure.run_samples(lines, 100.0, 100, [])
        self.assertEqual(samples['gpu_mhz'], [848])
        self.assertEqual(samples['cpu_mhz'], [None])


if __name__ == '__main__':
    unittest.main()

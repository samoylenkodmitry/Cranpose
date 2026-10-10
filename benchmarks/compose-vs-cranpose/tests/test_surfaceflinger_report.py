import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


BENCHMARK = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('measure', BENCHMARK / 'measure.py')
measure = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(measure)


class SurfaceFlingerReportTest(unittest.TestCase):
    def test_invalid_origins_do_not_contaminate_delays_or_drop_presented_frames(self):
        raw = '''LAT_BEGIN 1.2
16666667
1000000000 1020000000 1010000000
0 1040000000 1030000000
9223372036854775807 1060000000 1050000000
1060000000 1080000000 0
1080000000 1100000000 9223372036854775807
1105000000 1120000000 1110000000
0 0 0
1110000000 9223372036854775807 1120000000
LAT_END
'''
        frames, _, period, _ = measure.presents(raw)
        report = measure.frame_stats(frames, 1.0, 2.0, period)
        self.assertEqual(report['frames'], 6)
        self.assertEqual(report['fps'], 6)
        self.assertEqual(report['interval_p50_ms'], 20)
        self.assertEqual(report['desired_to_present_p50_ms'], 20)
        self.assertEqual(report['desired_to_ready_p50_ms'], 10)

    def test_a_run_counts_from_its_launch_with_the_seconds_before_the_first_frame(self):
        # Launched at 10 s; the first frame comes at 12.5 s, then one a
        # second until 13.5 s and none for the last second.
        presents = [12_500_000_000, 12_900_000_000, 13_100_000_000, 13_500_000_000]
        frames = {time: (0, 0) for time in presents}
        report = measure.frame_stats(frames, 10.0, 15.0, 1000.0 / 60.0)
        self.assertEqual(report['fps_by_second'], [0, 0, 2, 2, 0])
        self.assertEqual(report['frame_ms'], [2500.0, 2900.0, 3100.0, 3500.0], 'every frame, from the launch')
        self.assertAlmostEqual(report['first_frame_s'], 2.5)
        self.assertEqual(report['fps'], 4 / 5.0, 'frames over the whole run, its start included')
        self.assertEqual(report['stalled_seconds'], 1, 'only seconds after the first frame are stalls')

    def test_a_first_poll_with_a_full_history_is_flagged(self):
        full = 'LAT_BEGIN 1.0\n16666667\n' + ''.join(
            f'{i} {1_000_000_000 + i * 16_000_000} {i}\n' for i in range(1, 128)) + 'LAT_END\n'
        short = 'LAT_BEGIN 1.0\n16666667\n1 1000000000 1\n0 0 0\nLAT_END\n'
        self.assertTrue(measure.first_poll_full(full))
        self.assertFalse(measure.first_poll_full(short))

    def test_missing_delay_samples_are_reported_as_unavailable(self):
        raw = '''LAT_BEGIN 1.2
16666667
0 1020000000 1010000000
9223372036854775807 1040000000 1030000000
LAT_END
'''
        frames, _, period, _ = measure.presents(raw)
        metrics = measure.frame_stats(frames, 1.0, 2.0, period)
        self.assertEqual(metrics['frames'], 2)
        self.assertIsNone(metrics['desired_to_present_p50_ms'])
        self.assertIsNone(metrics['desired_to_ready_p50_ms'])

    def test_missing_ready_times_preserve_valid_presentation_delays(self):
        raw = '''LAT_BEGIN 1.2
16666667
1000000000 1020000000 0
1020000000 1040000000 9223372036854775807
LAT_END
'''
        frames, _, period, _ = measure.presents(raw)
        report = measure.frame_stats(frames, 1.0, 2.0, period)
        self.assertEqual(report['desired_to_present_p50_ms'], 20)
        self.assertIsNone(report['desired_to_ready_p50_ms'])

    def test_summary_explains_timestamp_origins_and_handles_missing_delays(self):
        runs = [dict(scenario='workspace', app=app,
                     desired_to_present_p50_ms=20 if app == 'cranpose' else None,
                     desired_to_ready_p50_ms=None, memory={}, top_threads=[])
                for app in ('cranpose', 'compose')]
        report = dict(runs=runs, cranpose_apk_bytes=1, compose_apk_bytes=1, failures=[])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'report.json'
            path.write_text(json.dumps(report))
            result = subprocess.run([sys.executable, str(BENCHMARK / 'summarize.py'), str(path)],
                                    capture_output=True, text=True, check=True)
        self.assertIn('| Desired timestamp → present p50 (ms) | 20.0 | n/a |', result.stdout)
        self.assertIn('| Desired timestamp → ready p50 (ms) | n/a | n/a |', result.stdout)
        self.assertIn('producer-selected', result.stdout)
        self.assertNotIn('GPU done', result.stdout)


if __name__ == '__main__':
    unittest.main()

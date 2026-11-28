"""`linux.py` counts MangoHud's presents from the launch beside the app's own
updates, marks a leg whose presents outnumber its updates, and marks every
leg of a display whose GPU scans nothing out."""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('linux', BENCHMARK / 'linux.py')
linux = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(linux)

# A MangoHud 0.8.4 log as its logger writes one: the system line, the column
# names, then a row a frame, `elapsed` in nanoseconds since its log started.
LOG = """os,cpu,gpu,ram,kernel,driver,cpuscheduler
Arch Linux,12th Gen Intel Core i5-12400,Intel(R) UHD Graphics 730 (ADL-S GT1),15.4,7.3.5,Mesa 26.3.1,
fps,frametime,cpu_load,cpu_power,gpu_load,cpu_temp,gpu_temp,gpu_core_clock,gpu_mem_clock,gpu_vram_used,gpu_power,ram_used,swap_used,process_rss,cpu_mhz,elapsed
60,16.6,10,0,20,40,0,1450,0,0,0,4.1,0,0.2,4400,250000000
60,16.7,10,0,20,40,0,1450,0,0,0,4.1,0,0.2,4400,266700000
60,16.6,10,0,20,40,0,1450,0,0,0,4.1,0,0.2,4400,283300000
"""


class MangoHudLogTest(unittest.TestCase):
    def test_rows_count_from_the_moment_mangohud_created_its_log_with_the_first_row(self):
        system, rows = linux.read_log(LOG)
        self.assertEqual(system['gpu'], 'Intel(R) UHD Graphics 730 (ADL-S GT1)')
        # The file appeared 10.25 s on the harness's clock, as the first row,
        # 0.25 s into MangoHud's log, was written.
        times = linux.present_times(rows, created=10.25)
        self.assertEqual([round(time, 4) for time in times], [10.25, 10.2667, 10.2833])

    def test_a_log_without_rows_has_no_presents(self):
        self.assertEqual(linux.read_log(''), ({}, []))
        self.assertEqual(linux.present_times([], created=1.0), [])


class PresentsAgainstUpdatesTest(unittest.TestCase):
    def test_presents_that_each_held_a_new_frame_are_not_marked(self):
        launched, run_s = 100.0, 3.0
        updates = [(launched + 0.5 + index / 60, index + 1) for index in range(150)]
        presents = [stamp + 0.001 for stamp, _ in updates[3:]]
        counted = linux.present_counts(presents, launched, run_s, updates)
        self.assertEqual(counted['presents'], 147)
        self.assertEqual(counted['updates_in_presents_span'], 147)
        self.assertEqual(counted['updates_before_mangohud'], 3)
        self.assertFalse(counted['presents_exceed_updates'])
        self.assertEqual(counted['mangohud_fps'], 49.0)
        self.assertEqual(counted['mangohud_from_s'], 0.551)

    def test_a_leg_that_presents_each_frame_twice_is_marked(self):
        launched, run_s = 0.0, 2.0
        updates = [(index / 30, index + 1) for index in range(60)]
        presents = sorted([stamp for stamp, _ in updates] + [stamp + 1 / 60 for stamp, _ in updates])
        counted = linux.present_counts(presents, launched, run_s, updates)
        self.assertTrue(counted['presents_exceed_updates'])
        self.assertEqual(linux.update_counts(updates, launched, run_s)['fps'], 30.0)
        self.assertEqual(counted['mangohud_fps'], 60.0)

    def test_a_leg_that_moves_on_to_frames_it_never_presents_is_marked(self):
        updates = [(index / 60, index + 1) for index in range(120)]
        presents = [stamp + 0.001 for stamp, _ in updates[::2]]
        counted = linux.present_counts(presents, 0.0, 2.0, updates)
        self.assertFalse(counted['presents_exceed_updates'])
        self.assertTrue(counted['updates_exceed_presents'])

    def test_two_frames_the_edges_split_differently_are_no_excess(self):
        self.assertFalse(linux.exceeds(102, 100))
        self.assertTrue(linux.exceeds(104, 100))

    def test_an_app_that_skips_frame_numbers_says_how_many(self):
        updates = [(0.1, 1), (0.2, 2), (0.3, 5)]
        self.assertEqual(linux.update_counts(updates, 0.0, 1.0)['updates_skipped'], 2)


class ScanoutTest(unittest.TestCase):
    def connectors(self, states):
        folder = Path(tempfile.mkdtemp())
        for name, (status, enabled) in states.items():
            (folder / name).mkdir()
            (folder / name / 'status').write_text(status + '\n')
            (folder / name / 'enabled').write_text(enabled + '\n')
        return linux.outputs(folder)

    def test_a_gpu_with_every_output_disconnected_scans_nothing_out(self):
        found = self.connectors({'card0-HDMI-A-1': ('disconnected', 'disabled'),
                                 'card0-DP-1': ('disconnected', 'disabled')})
        self.assertFalse(linux.scans_out(found))

    def test_a_connected_and_enabled_output_scans_out(self):
        found = self.connectors({'card0-HDMI-A-1': ('connected', 'enabled'),
                                 'card0-DP-1': ('disconnected', 'disabled')})
        self.assertTrue(linux.scans_out(found))

    def test_a_connected_output_the_x_server_left_off_scans_nothing_out(self):
        found = self.connectors({'card0-HDMI-A-1': ('connected', 'disabled')})
        self.assertFalse(linux.scans_out(found))


class SummaryTest(unittest.TestCase):
    def test_a_start_counts_its_legs_whose_presents_outnumbered_their_updates(self):
        legs = [{'subject': 'egui', 'start': 'first', 'disturbed': False, 'fps': 20.0, 'mangohud_fps': 40.0,
                 'presents_exceed_updates': True},
                {'subject': 'egui', 'start': 'first', 'disturbed': True, 'fps': 5.0, 'mangohud_fps': 5.0,
                 'presents_exceed_updates': False},
                {'subject': 'cranpose', 'start': 'first', 'disturbed': False, 'fps': 50.0, 'mangohud_fps': 50.0,
                 'presents_exceed_updates': False}]
        summary = linux.summarize(legs, ['egui', 'cranpose'], 'first')
        self.assertEqual(summary['egui'], {'fps': 20.0, 'mangohud_fps': 40.0, 'presents_exceed_updates_legs': 1,
                                           'updates_exceed_presents_legs': 0})
        self.assertEqual(summary['cranpose']['presents_exceed_updates_legs'], 0)


if __name__ == '__main__':
    unittest.main()

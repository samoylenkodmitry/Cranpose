#!/usr/bin/env python3
"""Checks what the nightly and the board summaries read from a board's log:
the frames a panel can show of each second, and the statistics that
Cranpose's frame loop writes as short lines."""

import pathlib
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1] / "benchmarks/esp32"))
import nightly  # noqa: E402
import summary  # noqa: E402

BOOT = """I (5064) cranpose: BENCH frame_period_us 16000
I (5074) cranpose: BENCH first_ms 286
I (5094) cranpose: BENCH second 0 frames 45
I (5114) cranpose: BENCH second 1 frames 63
I (5124) cranpose: BENCH second 2 frames 62
I (5144) cranpose: BENCH second 3 frames 62
I (5164) cranpose: BENCH second 4 frames 62
"""


class Boot(unittest.TestCase):
    def leg(self, text):
        with tempfile.TemporaryDirectory() as folder:
            log = pathlib.Path(folder) / "boot.log"
            log.write_text(text)
            return nightly.read_leg(log, "lvgl", "first", 59.9)

    def test_frames_past_the_panels_scans_count_as_drawn_and_not_as_shown(self):
        leg = self.leg(BOOT)
        self.assertEqual(leg["fps_by_second"], [45, 63, 62, 62, 62])
        self.assertAlmostEqual(leg["fps"], 58.8)
        # The first second shows at most the scans after its first frame.
        self.assertAlmostEqual(leg["shown_fps"], (0.714 * 59.9 + 4 * 59.9) / 5, places=3)
        self.assertEqual(leg["frame_period_ms"], 16.0)

    def test_a_log_without_a_frame_period_keeps_its_frames(self):
        leg = self.leg(BOOT.split("\n", 1)[1])
        self.assertIsNone(leg["frame_period_ms"])
        self.assertAlmostEqual(leg["fps"], 58.8)


STATS = """I (2100) cranpose: 300 frames in 5.0 s: update 0.46 ms
I (2100) cranpose: draw and send 0.39 ms
I (2100) cranpose: slowest 1.2 ms, sent 12.5 KB a frame
I (2100) cranpose: heap 167 KB free, 160 KB in one block
"""

OLD_STATS = ("I (2100) cranpose_platform_esp_idf: 300 frames in 5.0 s: update 0.46 ms, draw and send "
             "0.39 ms, slowest 1.2 ms, sent 12.5 KB a frame; heap 167 KB free\n")


class Statistics(unittest.TestCase):
    def test_a_window_written_as_short_lines_reads_as_one_written_whole(self):
        for text in (STATS, OLD_STATS):
            frames, _, _ = summary.windows(text.splitlines())
            self.assertEqual(len(frames), 1, text)
            end, seconds, rate, busy, sent = frames[0]
            self.assertEqual((end, seconds, rate, sent), (2.1, 5.0, 60.0, 12.5), text)
            self.assertAlmostEqual(busy, 0.85, msg=text)


if __name__ == "__main__":
    unittest.main()

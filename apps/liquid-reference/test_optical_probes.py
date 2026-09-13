import copy
import importlib.util
import math
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock


_spec = importlib.util.spec_from_file_location("optical_analysis", Path(__file__).with_name("analyze-optical-probes.py"))
_analysis = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_analysis)
_capture_spec = importlib.util.spec_from_file_location("optical_capture", Path(__file__).with_name("capture-optical-probes.py"))
_capture = importlib.util.module_from_spec(_capture_spec)
_capture_spec.loader.exec_module(_capture)


class OpticalProbeTests(unittest.TestCase):
    def test_quadrature_preserves_a_single_source_across_frequencies(self):
        for period in [8, 32, 128, 1024]:
            for source in [0.125, 340.125, 799.125]:
                values = [117 + 23 * math.cos(2 * math.pi * source / period + phase * math.pi / 2) for phase in range(4)]
                position, amplitude = _analysis._quadrature(values, period, source + 0.5)
                self.assertAlmostEqual(position, source)
                self.assertAlmostEqual(amplitude, 23)

    def test_mixed_sources_do_not_masquerade_as_one_ray(self):
        positions = []
        for period in [32, 128]:
            values = [117 + sum(weight * math.cos(2 * math.pi * source / period + phase * math.pi / 2)
                                for source, weight in [(800, 20), (822, 13)]) for phase in range(4)]
            position, _ = _analysis._quadrature(values, period, 800)
            positions.append(position)
        self.assertGreater(abs(positions[0] - positions[1]), 8)

    def test_capture_validation_accepts_complete_stable_quadrature(self):
        _analysis._validate_frames(self._frames())

    def test_capture_validation_rejects_missing_and_duplicate_frames(self):
        frames = self._frames()
        for invalid in [frames[:-1], frames[:3] + [frames[0]]]:
            with self.assertRaises(ValueError):
                _analysis._validate_frames(invalid)

    def test_capture_validation_rejects_wrong_phase_viewport_and_capture_time(self):
        for mutate in [lambda f: f[0]["pattern"].update(phase=1),
                       lambda f: f[0].update(viewport=[400, 874]),
                       lambda f: f[0].update(captureWallMillis=[500, 700]),
                       lambda f: f[0].update(captureWallMillis=[1700, 3100])]:
            frames = copy.deepcopy(self._frames())
            mutate(frames)
            with self.assertRaises(ValueError):
                _analysis._validate_frames(frames)

    def test_channel_groups_are_validated_independently(self):
        frames = []
        for channel in ["red", "green", "blue"]:
            for frame in self._frames():
                frame["index"] = len(frames)
                frame["count"] = 12
                frame["pattern"]["channel"] = channel
                frames.append(frame)
        _analysis._validate_frames(frames)
        frames[4]["pattern"]["channel"] = "red"
        with self.assertRaises(ValueError):
            _analysis._validate_frames(frames)
        frames[4]["pattern"]["channel"] = "invalid"
        with self.assertRaises(ValueError):
            _analysis._validate_frames(frames)

    def test_geometry_checks_glass_outside_the_tab_bar_and_ignores_status_text(self):
        frames = [{"layers": [
            {"path": "UITabBar/0", "kind": "CALayer", "rect": [21, 791, 360, 62]},
            {"path": "Window/pane", "kind": "CASDFElementLayer", "rect": [21, 791, 360, 62]},
            {"path": "Window/label", "kind": "CGDrawingLayer", "rect": [0, 0, 100 + phase, 20]},
        ]} for phase in range(4)]
        geometry, variation = _analysis._geometry_variation(frames)
        self.assertEqual(variation, 0)
        self.assertIn("Window/pane", geometry)
        self.assertNotIn("Window/label", geometry)
        frames[-1]["layers"][1]["rect"][0] += 1
        with self.assertRaisesRegex(ValueError, "native geometry moved"):
            _analysis._geometry_variation(frames)
        with self.assertRaisesRegex(ValueError, "missing native glass geometry"):
            _analysis._geometry_variation([{"layers": []}])

    def test_capture_count_accepts_a_subset_and_default_complete_capture(self):
        for expected, total in [(1, 1), (None, 4)]:
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                source = root / "Documents" / "optical-probe.json"
                source.parent.mkdir()
                source.write_text("{}")
                frames = [dict(frame, changedWallMillis=100000, run=str(index))
                          for index, frame in enumerate(self._frames())]
                values = [json.dumps(frame) for frame in frames[:total] for _ in range(2)]
                with mock.patch.object(_capture.subprocess, "check_output", return_value=str(root)), \
                     mock.patch.object(_capture.subprocess, "run") as screenshot, \
                     mock.patch.object(_capture.time, "time", side_effect=[100] + [100.8] * (3 * total)), \
                     mock.patch.object(_capture.time, "monotonic", return_value=0), \
                     mock.patch.object(Path, "read_text", side_effect=values), \
                     mock.patch("builtins.print"):
                    _capture._capture("device", root / "output", 1, expected)
                self.assertEqual(screenshot.call_count, total)
                self.assertEqual(len(list((root / "output").glob("*.json"))), total)

    def test_capture_count_rejects_nonpositive_values_before_creating_output(self):
        for count in [0, -1]:
            with mock.patch.object(Path, "mkdir") as mkdir:
                with self.assertRaisesRegex(ValueError, "positive"):
                    _capture._capture("device", Path("unused-output"), 1, count)
                mkdir.assert_not_called()

    @staticmethod
    def _frames():
        return [{"index": phase, "count": 4, "viewport": [402, 874], "changedWallMillis": 1000,
                 "captureWallMillis": [1700, 1800], "duration": 2,
                 "pattern": {"kind": "wave", "axis": "x", "period": 32, "phase": phase}}
                for phase in range(4)]

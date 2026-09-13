import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("motion", Path(__file__).with_name("analyze-motion.py"))
motion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(motion)


class NativeMotionTests(unittest.TestCase):
    def frame(self, raised, timestamp):
        bar = {"path": "UITabBar/0", "kind": "_UIMultiLayer" if raised else "CALayer", "bounds": [0, 0, 398, 62], "rect": [21, 873, 398, 62]}
        lens = {"path": "UITabBar/0/0/1" if raised else "UITabBar/0/1", "bounds": [0, 0, 104.5, 54],
                "rect": [25, 877, 104.5, 54], "scale": [1, 1]}
        layers = [bar, lens]
        if not raised:
            layers.append(dict(lens, path="UITabBar/0/0/1", rect=[120.166667, 877, 104.5, 54]))
        return {"timestamp": timestamp, "layers": layers}

    def test_settling_follows_selection_after_native_wrapper_disappears(self):
        frames = [self.frame(True, 4.66), self.frame(False, 4.67), self.frame(False, 5.7)]
        rows = motion._geometry(frames, [{"timestamp": 0}])
        self.assertEqual(len(rows), len(frames))
        self.assertEqual([row[1] for row in rows], [-142.75] * 3)
        self.assertEqual([row[0] for row in rows], [4.66, 4.67, 5.7])

    def test_rigid_surface_motion_does_not_become_internal_lens_motion(self):
        before = self.frame(False, 4.66)
        after = self.frame(False, 4.67)
        for layer in after["layers"]:
            layer["rect"][0] -= 21
        rows = motion._geometry([before, after], [{"timestamp": 0}])
        self.assertEqual(rows[0][1:], rows[1][1:])

    def test_absent_lens_is_explicit_and_invalid_geometry_is_rejected(self):
        self.assertIsNone(motion._native_lens({"layers": []}))
        self.assertIsNone(motion._native_lens({"layers": [{"path": "UITabBar/0", "kind": "CAPortalLayer", "bounds": [0, 0, 0, 0]}]}))
        frame = self.frame(True, 0.1)
        frame["layers"][0]["kind"] = "UnknownLayer"
        with self.assertRaises(ValueError):
            motion._native_lens(frame)
        frame = self.frame(True, 0.1)
        frame["layers"][1]["bounds"][2] = 398
        with self.assertRaises(ValueError):
            motion._native_lens(frame)
        frame = self.frame(True, 0.1)
        frame["layers"][1]["rect"][0] = float("nan")
        with self.assertRaises(ValueError):
            motion._geometry([frame], [{"timestamp": 0}])

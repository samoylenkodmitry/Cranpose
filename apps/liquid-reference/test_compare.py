import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("compare", Path(__file__).with_name("compare.py"))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)


class ComparisonTests(unittest.TestCase):
    @staticmethod
    def timeline(times):
        return {"input_clock": "application-delivery", "gestures": [{
            "planned_events": [{"offset": 0, "coordinate.x": 0},
                               {"offset": 0.8, "coordinate.x": 0},
                               {"offset": 1.8, "coordinate.x": 250},
                               {"offset": 3, "coordinate.x": 250}],
            "frames": [{"gesture_seconds": time, "phase": "rest", "file": f"{i}.png",
                        "source_frame_index": i} for i, time in enumerate(times)]}]}

    def test_content_fixture_identity_is_retained_and_must_match_each_route(self):
        native, cranpose = self.timeline([0, 1]), self.timeline([0, 1])
        environment = {"contentCases": [{"titles": ["A", "Library", "Favorites", "Profile"]}]}
        native["environment"] = cranpose["environment"] = environment
        self.assertEqual(compare._pairs([native, cranpose])[0]["contentLabel"], "A / Library / Favorites / Profile")
        environment["contentCases"] = []
        with self.assertRaisesRegex(ValueError, "content fixture"):
            compare._pairs([native, cranpose])

    def test_duplicate_presentation_timestamps_keep_each_source_frame(self):
        pairs = compare._pairs([self.timeline([0, 0.01, 0.01, 0.02]), self.timeline([0, 0.015, 0.02])])
        self.assertEqual({p["nativeFrame"] for p in pairs}, {0, 1, 2, 3})
        self.assertEqual({p["cranposeFrame"] for p in pairs}, {0, 1, 2})
        self.assertEqual([p["time"] for p in pairs], sorted(p["time"] for p in pairs))
    def test_contact_boundaries_keep_unshared_first_and_last_frames(self):
        pairs = compare._pairs([self.timeline([-0.03, 0.01, 3.01, 3.8]),
                                self.timeline([-0.02, 0.02, 3.02, 3.81])])
        self.assertEqual({p["nativeFrame"] for p in pairs}, set(range(4)))
        self.assertEqual({p["cranposeFrame"] for p in pairs}, set(range(4)))
        self.assertEqual(pairs[0]["time"], -0.03)
        self.assertEqual(pairs[-1]["time"], 3.81)
        self.assertTrue(pairs[0]["nativeAvailable"])
        self.assertFalse(pairs[0]["cranposeAvailable"])
        self.assertFalse(pairs[-1]["nativeAvailable"])
        self.assertTrue(pairs[-1]["cranposeAvailable"])

    def test_reversal_label_retains_all_three_legs(self):
        events = [{"coordinate.x": x, "offset": t} for t, x in
                  [(0, 300), (.8, 300), (1.1, 0), (2.3, 300), (2.6, 0), (3.8, 0)]]
        self.assertEqual(compare._gesture_label(events), "← → ← · 1000/250/1000 pt/s")
        self.assertEqual(compare._gesture_label(self.timeline([0])["gestures"][0]["planned_events"]),
                         "→ 250 pt/s")
        for invalid in [events[:2], [events[0], dict(events[2], offset=0)]]:
            with self.assertRaises(ValueError):
                compare._gesture_label(invalid)

    def test_rejects_unpaired_gestures_instead_of_dropping_them(self):
        with self.assertRaises(ValueError):
            compare._pairs([{"gestures": [{}, {}]}, {"gestures": [{}]}])

    def test_rejects_different_touch_clock_origins(self):
        with self.assertRaisesRegex(ValueError, "delivery time"):
            compare._pairs([{"input_clock": clock, "gestures": [{}]}
                            for clock in ["application-delivery", "platform-event"]])

    def test_rejects_different_planned_trajectories(self):
        native, cranpose = self.timeline([0, 1]), self.timeline([0, 1])
        cranpose["gestures"][0]["planned_events"][2]["coordinate.x"] = 200
        with self.assertRaisesRegex(ValueError, "same planned touch path"):
            compare._pairs([native, cranpose])

    def test_rejects_different_capture_environments(self):
        native, cranpose = self.timeline([0, 1]), self.timeline([0, 1])
        native["environment"] = {"osVersion": "27.0", "glassSettingPercent": 25}
        for environment in [None, {"osVersion": "26.5", "glassSettingPercent": 25},
                            {"osVersion": "27.0", "glassSettingPercent": 50}]:
            cranpose["environment"] = environment
            with self.assertRaisesRegex(ValueError, "same device, OS and settings"):
                compare._pairs([native, cranpose])
        cranpose["environment"] = dict(native["environment"])
        self.assertEqual(len(compare._pairs([native, cranpose])), 4)



if __name__ == "__main__":
    unittest.main()

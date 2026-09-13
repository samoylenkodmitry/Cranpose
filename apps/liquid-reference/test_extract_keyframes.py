import importlib.util
import plistlib
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("extract_keyframes", Path(__file__).with_name("extract-keyframes.py"))
extract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(extract)


class KeyframeTests(unittest.TestCase):
    def test_both_platforms_use_delivery_time_without_losing_native_event_time(self):
        native = [{"timestamp": 10, "wallMillis": 1000, "receivedWallMillis": 1017}]
        delivered = extract._delivery_samples(native)
        self.assertEqual(delivered[0]["wallMillis"], 1017)
        self.assertEqual(native[0]["wallMillis"], 1000)
        self.assertEqual(delivered[0]["timestamp"], 10)
        cranpose = [{"wallMillis": 1017, "platformMillis": None}]
        self.assertEqual(extract._delivery_samples(cranpose), cranpose)
        for incomplete in [[{"timestamp": 10, "wallMillis": 1000}], native + cranpose]:
            with self.assertRaises(ValueError):
                extract._delivery_samples(incomplete)

    def test_clock_alignment_preserves_input_time_when_animation_starts_late(self):
        epoch = 1789213370
        times = [1 + i * 0.02 for i in range(20)]
        codes = [round((epoch + time) * 1000) & 0xFFFFFF for time in times]
        result = extract._clock_alignment(codes, times, epoch)
        self.assertAlmostEqual(result["epoch_at_movie_zero"], epoch)
        self.assertEqual(result["clock_samples"], 20)
        with self.assertRaises(ValueError):
            extract._clock_alignment([None] * 20, times, epoch)
        with self.assertRaises(ValueError):
            extract._clock_alignment(codes, [t + i * 0.1 for i, t in enumerate(times)], epoch)

    def test_clock_marker_rejects_non_marker_and_ambiguous_pixels(self):
        def pixels(code):
            image = bytearray(128 * 8)
            for bit in range(32): image[4 * 128 + bit * 4 + 2] = 255 if code >> (31 - bit) & 1 else 0
            return image
        self.assertEqual(extract._decode_clock(pixels(0xB4123456)), 0x123456)
        self.assertIsNone(extract._decode_clock(pixels(0xA4123456)))
        ambiguous = pixels(0xB4123456)
        ambiguous[4 * 128 + 10] = 128
        self.assertIsNone(extract._decode_clock(ambiguous))

    def test_observed_input_phase_boundaries(self):
        samples = [{"wallMillis": 1000 + t * 1000, "phase": phase, "x": x, "y": 820}
                   for t, phase, x in [(0, "Touch down", 70), (0.8, "Sliding", 71),
                                       (1.8, "Sliding", 330), (3, "Touch up", 330)]]
        events = extract._input_path(samples)
        self.assertEqual([e["offset"] for e in events], [0, 0.8, 1.8, 3])
        self.assertEqual([extract._phase(t, events) for t in [-0.1, 0, 0.4, 0.9, 1.85, 2.5, 3.05, 3.8]],
                         ["rest", "touch-down", "pressed", "sliding", "stopped", "idle-held", "touch-up", "settling"])
        for broken in [samples[:2], [dict(samples[0], wallMillis=float("nan"))] + samples[1:],
                       samples[:2] + [dict(samples[2], wallMillis=1100)] + samples[3:]]:
            with self.assertRaises(ValueError): extract._input_path(broken)

    def test_reversal_keeps_one_contact_and_every_direction_change(self):
        samples = [{"wallMillis": 1000 + t * 1000, "phase": phase, "x": x, "y": 820}
                   for t, phase, x in [(0, "Touch down", 330), (0.8, "Sliding", 329),
                                       (1.0, "Sliding", 70), (1.2, "Sliding", 330),
                                       (1.4, "Sliding", 70), (2.6, "Touch up", 70)]]
        events = extract._input_path(samples)
        self.assertEqual([e["coordinate.x"] for e in events[1:-1]], [329, 70, 330, 70])
        self.assertEqual([e["eventType"] for e in events], [1, 2, 2, 2, 2, 3])
        self.assertEqual([extract._phase(t, events) for t in [0, 0.4, 0.9, 1.1, 1.3, 1.45, 2.0, 2.6, 3.1]],
                         ["touch-down", "pressed", "sliding", "sliding", "sliding", "stopped", "idle-held", "touch-up", "settling"])
        decoded = self._decode(event_types=(1, 2, 2, 2, 2, 3), offsets=(0, 0.8, 1.0, 1.2, 1.4, 2.6))
        self.assertEqual(len(decoded), 6)

    def test_trajectory_validation_rejects_a_missing_return_leg(self):
        planned = [{"coordinate.x": x, "coordinate.y": 820} for x in [330, 330, 70, 330, 70, 70]]
        observed = [{"coordinate.x": x, "coordinate.y": 820} for x in [330, 250, 70, 200, 330, 250, 70, 70]]
        extract._validate_trajectory(planned, observed)
        self.assertEqual(extract._directions(observed), [-1, 1, -1])
        for broken in [observed[:3] + observed[-1:], observed[:-1] + [dict(observed[-1], **{"coordinate.x": 74})]]:
            with self.assertRaises(ValueError):
                extract._validate_trajectory(planned, broken)

    def test_uses_presentation_timestamps_when_decoder_falls_back_to_decode_clock(self):
        frames = [{"pts_time": "3.675", "best_effort_timestamp_time": "3.675"},
                  {"pts_time": "3.675", "best_effort_timestamp_time": "2.102"},
                  {"pts_time": "3.678", "best_effort_timestamp_time": "2.105"}]
        self.assertEqual(extract._source_times(frames), [3.675, 3.675, 3.678])
        for invalid in [[], [{"pts_time": "nan"}]]:
            with self.assertRaises(ValueError): extract._source_times(invalid)

    def test_decoder_order_does_not_rewrite_presentation_times(self):
        frames = [{"pts_time": str(t)} for t in [1.768333, 1.765, 1.771667, 1.765]]
        times = extract._source_times(frames)
        self.assertEqual(times, [1.768333, 1.765, 1.771667, 1.765])
        self.assertEqual(extract._presentation_order(times), [(1, 1.765), (3, 1.765), (0, 1.768333), (2, 1.771667)])

    def test_idle_uses_previous_recorded_frame(self):
        times = [0.0, 0.1, 0.7, 1.3]
        self.assertEqual(extract._frame_index(times, 0.5, 1.8), 1)
        self.assertEqual(extract._frame_index(times, 0.7, 1.8), 2)
        self.assertEqual(extract._frame_index(times, 0.0, 1.8), 0)
        self.assertEqual(extract._frame_index(times, 1.8, 1.8), 3)

    def test_frames_outside_recording_are_rejected(self):
        for times, time, duration in [([], 0, 1), ([0], -0.1, 1), ([0], 1.1, 1), ([0.2], 0.1, 1)]:
            with self.subTest(time=time), self.assertRaises(ValueError):
                extract._frame_index(times, time, duration)

    def _decode(self, event_types=(1, 2, 2, 3), offsets=(0, 0.8, 1.8, 3.0), fingers=1):
        events = [{"eventType": kind, "offset": offset, "coordinate.x": 50.0, "coordinate.y": 820.0}
                  for kind, offset in zip(event_types, offsets)]
        paths = [{"pointerEvents": {"NS.objects": events}}] * fingers
        archive = {"$objects": [{"eventPaths": {"NS.objects": paths}}], "$top": {"root": plistlib.UID(0)}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "event"
            path.write_bytes(plistlib.dumps(archive, fmt=plistlib.FMT_BINARY))
            return extract._decode_event(path)

    def test_decodes_down_slide_idle_up(self):
        events = self._decode()
        self.assertEqual([item["offset"] for item in events], [0, 0.8, 1.8, 3.0])
        self.assertEqual(events[-1]["eventType"], 3)

    def test_rejects_other_gestures_and_invalid_times(self):
        for arguments in [{"event_types": (1, 3)}, {"fingers": 2}, {"fingers": 0},
                          {"offsets": (0, 1, 0.8, 3)}, {"offsets": (0, 0.8, 1.8, float("inf"))}]:
            with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                self._decode(**arguments)


if __name__ == "__main__":
    unittest.main()

import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("pixel_audit", Path(__file__).with_name("pixel-audit.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class PixelAuditTests(unittest.TestCase):
    def test_saved_difference_retains_each_channel_and_uncovered_steps(self):
        import json
        import tempfile
        from PIL import Image
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "comparison"
            (source / "frames").mkdir(parents=True)
            for name, pixels in [("native", [10, 20, 30, 250, 100, 120, 140, 255]),
                                 ("cranpose", [12, 17, 31, 248, 100, 121, 140, 255])]:
                Image.frombytes("RGBA", (2, 1), bytes(pixels)).save(source / "frames" / (name + ".png"))
            pair = {"id": "0-0", "route": 0, "native": "native.png", "cranpose": "cranpose.png",
                    "nativeFrame": 0, "cranposeFrame": 0, "nativeTime": 0, "cranposeTime": 0,
                    "nativeAvailable": True, "cranposeAvailable": True}
            (source / "frames/keyframes.json").write_text(json.dumps([pair, dict(pair, id="0-1", nativeAvailable=False)]))
            output = root / "audit"
            self.assertFalse(audit._audit(source, output, {}))
            report = json.loads((output / "audit.json").read_text())
            self.assertEqual(report["paired_steps"], 2)
            self.assertFalse(report["rows"][1]["covered"])
            self.assertEqual(report["rows"][0]["regions"]["whole_frame"]["unequal_pixels"], 2)
            with Image.open(output / "0-0.png") as difference:
                self.assertEqual(difference.tobytes(), bytes([2, 3, 1, 255, 0, 1, 0, 255]))

    def test_repeated_images_do_not_count_as_additional_temporal_samples(self):
        frames = [{"route": 0, "nativeAvailable": True, "cranposeAvailable": True,
                   "nativeFrame": n, "cranposeFrame": c, "nativeTime": nt, "cranposeTime": ct}
                  for n, c, nt, ct in [(1, 1, 0, 0.01), (1, 2, 0, 0.04), (2, 2, 0.033, 0.04)]]
        result = audit._sampling(frames)
        self.assertEqual(result["native"]["unique_frames"], 2)
        self.assertEqual(result["cranpose"]["unique_frames"], 2)
        self.assertAlmostEqual(result["native"]["maximum_source_gap_seconds"], 0.033)
        frames[-1]["nativeAvailable"] = False
        self.assertEqual(audit._sampling(frames)["native"]["unique_frames"], 1)

    def test_accepts_exact_pixels_and_catches_one_channel_in_one_pixel(self):
        image = bytes([120, 130, 140, 255] * 12)
        regions = {"whole": [0, 0, 4, 3], "icon": [2, 1, 1, 1], "pane": [0, 0, 2, 3]}
        self.assertTrue(all(item["exact"] for item in audit._difference(image, image, 4, 3, regions).values()))
        changed = bytearray(image)
        changed[(1 * 4 + 2) * 4 + 1] += 1
        result = audit._difference(image, changed, 4, 3, regions)
        self.assertEqual(result["whole"]["unequal_pixels"], 1)
        self.assertEqual(result["icon"]["maximum_channel_error"], 1)
        self.assertFalse(result["icon"]["exact"])
        self.assertTrue(result["pane"]["exact"])

    def test_detects_translated_ink_and_refracted_checker_cells(self):
        image = bytearray([180, 190, 200, 255] * 8)
        image[4:8] = bytes([0, 136, 255, 255])
        shifted = bytearray(image)
        shifted[4:8], shifted[8:12] = shifted[8:12], shifted[4:8]
        self.assertEqual(audit._difference(image, shifted, 4, 2, {"icon": [0, 0, 4, 1]})["icon"]["unequal_pixels"], 2)
        shifted[16:20] = bytes([200, 190, 180, 255])
        self.assertEqual(audit._difference(image, shifted, 4, 2, {"pane": [0, 1, 4, 1]})["pane"]["unequal_pixels"], 1)

    def test_histogram_path_matches_direct_rgba_arithmetic_including_alpha(self):
        import random
        randomizer = random.Random(59)
        native = bytes(randomizer.randrange(256) for _ in range(13 * 17 * 4))
        other = bytes(randomizer.randrange(256) for _ in native)
        for x, y, width, height in [(0, 0, 13, 17), (3, 4, 5, 8)]:
            errors = [[abs(native[(row * 13 + column) * 4 + channel] - other[(row * 13 + column) * 4 + channel])
                       for channel in range(4)] for row in range(y, y + height) for column in range(x, x + width)]
            actual = audit._difference(native, other, 13, 17, {"region": [x, y, width, height]})["region"]
            self.assertEqual(actual["unequal_pixels"], sum(any(pixel) for pixel in errors))
            self.assertEqual(actual["maximum_channel_error"], max(map(max, errors)))
            self.assertEqual(actual["mean_absolute_channel_error"], sum(map(sum, errors)) / (width * height * 4))
        self.assertFalse(audit._difference(bytes(4), bytes([0, 0, 0, 1]), 1, 1, {"alpha": [0, 0, 1, 1]})["alpha"]["exact"])

    def test_rejects_missing_pixels_and_invalid_regions(self):
        with self.assertRaises(ValueError):
            audit._difference(bytes(4), bytes(3), 1, 1, {})
        for region in [[-1, 0, 1, 1], [0, 0, 0, 1], [0, 0, 2, 1], [0.5, 0, 1, 1], [False, 0, 1, 1]]:
            with self.assertRaises(ValueError):
                audit._difference(bytes(4), bytes(4), 1, 1, {"invalid": region})

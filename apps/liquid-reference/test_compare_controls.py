import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from compare_controls import compare, read_cases, read_case_sets


class ControlComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def capture(self, directory, case="control-slider-light-0p5", device="simulator"):
        directory.mkdir()
        Image.new("RGBA", (402, 874), "white").save(directory / "capture.png")
        manifest = [{"attachments": [{
            "exportedFileName": "capture.png",
            "suggestedHumanReadableName": case + "_0_UUID.png",
            "deviceId": device,
            "isAssociatedWithFailure": False,
        }]}]
        (directory / "manifest.json").write_text(json.dumps(manifest))
        return directory

    def test_equal_captures_preserve_full_screens_and_report_zero_difference(self):
        a = self.capture(self.root / "native")
        b = self.capture(self.root / "cranpose")
        output = self.root / "comparison"
        rows = compare(a, b, output)
        self.assertEqual(rows[0]["unequal_pixels"], 0)
        self.assertEqual(rows[0]["crop_pixels"], (0, 317, 402, 587))
        self.assertEqual((output / "control-slider-light-0p5-native-full.png").read_bytes(), (a / "capture.png").read_bytes())
        self.assertTrue((output / "index.html").exists())

    def test_one_channel_one_pixel_difference_is_retained(self):
        a = self.capture(self.root / "native")
        b = self.capture(self.root / "cranpose")
        with Image.open(b / "capture.png") as image:
            changed = image.copy()
        changed.putpixel((200, 450), (254, 255, 255, 255))
        changed.save(b / "capture.png")
        row = compare(a, b, self.root / "comparison")[0]
        self.assertEqual(row["unequal_pixels"], 1)
        self.assertEqual(row["worst_channel_difference"], 1)

    def test_mismatched_cases_or_devices_are_rejected(self):
        a = self.capture(self.root / "native")
        different_case = self.capture(self.root / "other-case", case="control-toggle-light-0")
        different_device = self.capture(self.root / "other-device", device="another-simulator")
        for other in [different_case, different_device]:
            with self.assertRaises(ValueError):
                compare(a, other, self.root / "comparison")

    def check_region(self, case, point, region):
        a = self.capture(self.root / "native", case=case)
        b = self.capture(self.root / "cranpose", case=case)
        with Image.open(b / "capture.png") as image:
            changed = image.copy()
        changed.putpixel(point, (254, 255, 255, 255))
        changed.save(b / "capture.png")
        row = compare(a, b, self.root / "comparison")[0]
        self.assertEqual(row["unequal_pixels"], 1)
        self.assertEqual(row["region_points"], region)

    def test_open_menu_comparison_includes_the_top_section(self):
        self.check_region("control-menu-light-grouped", (104, 215), (0, 100, 402, 700))

    def test_navigation_comparison_includes_the_large_title(self):
        self.check_region("control-nav-bar-light-0", (24, 130), (0, 60, 402, 250))

    def test_duplicate_or_failed_captures_are_rejected(self):
        directory = self.capture(self.root / "native")
        manifest_path = directory / "manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest[0]["attachments"].append(manifest[0]["attachments"][0])
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "duplicate"):
            read_cases(directory)
        manifest[0]["attachments"].pop()
        manifest[0]["attachments"][0]["isAssociatedWithFailure"] = True
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "failed"):
            read_cases(directory)

    def test_extra_bundles_preserve_every_case_and_reject_duplicates(self):
        a = self.capture(self.root / "native")
        b = self.capture(self.root / "cranpose")
        extra_a = self.capture(self.root / "extra-native", case="control-menu-light-open")
        extra_b = self.capture(self.root / "extra-cranpose", case="control-menu-light-open")
        rows = compare(a, b, self.root / "comparison", [extra_a], [extra_b])
        self.assertEqual(len(rows), 2)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            read_case_sets([a, a])


if __name__ == "__main__":
    unittest.main()

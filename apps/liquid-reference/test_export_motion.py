import csv
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


def load_script(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


controls = load_script("export-control-motion")
tabs = load_script("export-tab-motion")


class MotionExportTests(unittest.TestCase):
    def write_capture(self, root, frames, component=None):
        touches = [dict(timestamp=10, wallMillis=1000, receivedWallMillis=1020, phase="Touch down", x=72.333),
                   dict(timestamp=10.05, wallMillis=1050, receivedWallMillis=1070, phase="Sliding", x=150),
                   dict(timestamp=10.1, wallMillis=1100, receivedWallMillis=1120, phase="Touch up", x=150)]
        (root / "touches.json").write_text(json.dumps(touches))
        (root / "layers.json").write_text(json.dumps(frames))
        (root / "timeline.json").write_text(json.dumps({
            "environment": {"osVersion": "27.0", "osBuildNumber": "24A434"},
            "viewport": {"component": component},
            "gestures": [{"touch_trace": "touches.json", "layer_trace": "layers.json",
                          "planned_events": [{"offset": 0}, {"offset": 0.6}]}]}))

    def test_tab_export_removes_common_scale_and_uses_delivered_input_clock(self):
        bar = dict(path="UITabBar/0", kind="CALayer", bounds=[0, 0, 360, 62], rect=[42, 0, 720, 124])
        lens = dict(path="UITabBar/0/1", bounds=[0, 0, 95, 54], rect=[50, 8, 190, 108])
        frames = [dict(timestamp=time, layers=[bar, lens]) for time in [10.01, 10.04, 10.09, 10.14]]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_capture(root, frames)
            output = root / "tab.csv"
            tabs.export(root, output)
            rows = list(csv.reader(output.read_text().splitlines()))
            visible = [row for row in rows if row[0] == "frame"]
            self.assertEqual(len(visible), 3)
            self.assertAlmostEqual(float(visible[0][1]), 0.02)
            self.assertAlmostEqual(float(visible[0][2]), 0)
            self.assertEqual([float(value) for value in visible[0][3:5]], [95, 54])
            self.assertEqual([row[-1] for row in visible], ["contact", "drag", "release"])
            times = [float(row[1]) for row in rows[1:]]
            self.assertEqual(times, sorted(times))
            source = json.loads(output.with_suffix(".json").read_text())["sources"][0]
            self.assertEqual(source["frames"], 3)
            self.assertEqual(len(source["layers_sha256"]), 64)

    def test_control_export_keeps_pre_delivery_frame_and_independent_white_fill(self):
        path = "Window/0/0/0/1/0/0/2"
        lens = dict(path=path, rect=[180, 400, 58, 38], bounds=[0, 0, 58, 38], scale=[1.1, 0.9])
        fill = dict(path=path + "/0/1", opacity=0.4)
        frames = [dict(timestamp=time, layers=[lens, fill]) for time in [10.01, 10.24]]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_capture(root, frames, "toggle")
            output = root / "native_contact.csv"
            controls.export([root], output)
            rows = list(csv.DictReader(output.read_text().splitlines()))
            self.assertEqual([row["phase"] for row in rows], ["press", "release"])
            self.assertAlmostEqual(float(rows[0]["time"]), -0.01)
            self.assertAlmostEqual(float(rows[1]["time"]), 0.12)
            self.assertEqual(float(rows[0]["white_opacity"]), 0.4)
            shapes = list(csv.DictReader((root / "native_shape.csv").read_text().splitlines()))
            self.assertEqual(len(shapes), 2)
            self.assertEqual(float(shapes[0]["center_x"]), 209)
            self.assertEqual(float(shapes[0]["scale_x"]), 1.1)

    def test_missing_fill_in_active_native_lens_is_not_treated_as_opaque(self):
        path = "Window/0/0/0/1/0/0/2"
        lens = dict(path=path, rect=[180, 400, 58, 38], bounds=[0, 0, 58, 38], scale=[1, 1])
        shape = dict(path=path + "/0/0", kind="CASDFLayer")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_capture(root, [dict(timestamp=10.04, layers=[lens, shape])], "toggle")
            with self.assertRaisesRegex(ValueError, "missing white fill"):
                controls.export([root], root / "contact.csv")

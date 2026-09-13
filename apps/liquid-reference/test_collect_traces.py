import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("collect_traces", Path(__file__).with_name("collect-traces.py"))
collect = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collect)


class DeviceTraceTests(unittest.TestCase):
    def listing(self, name, **resources):
        return {"result": {"files": [{"name": name, "resources": {
            "isDirectory": False, "isSymbolicLink": False, **resources}}]}}

    def test_selects_only_regular_recorded_trace_files(self):
        for name in ["native-123-touches.json", "cranpose-456-layers.json"]:
            self.assertEqual(collect._device_trace_names(self.listing(name)), [name])
        for listing in [self.listing("unrelated.json"), self.listing("../native-touches.json"),
                        self.listing("native-touches.json", isSymbolicLink=True),
                        self.listing("native-layers.json", isDirectory=True), {"result": {"files": []}}]:
            with self.assertRaises(ValueError):
                collect._device_trace_names(listing)


if __name__ == "__main__":
    unittest.main()

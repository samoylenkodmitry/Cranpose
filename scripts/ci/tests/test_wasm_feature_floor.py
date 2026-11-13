from pathlib import Path
import subprocess
import tempfile
import unittest


CHECK = Path(__file__).resolve().parents[1] / "wasm_feature_floor.sh"
HEADER = bytes.fromhex("0061736d01000000")
# One function of type func() -> ().
PLAIN_FUNCTION = bytes.fromhex("01040160000003020100")
ONE_PAGE_MEMORY = bytes.fromhex("0503010001")
# i32.const 0, i32.const 0, i32.const 0, memory.fill: a bulk memory instruction.
MEMORY_FILL_BODY = bytes.fromhex("0a0d010b00410041004100fc0b000b")
# One function of type func((ref null $0)) -> (): a typed function reference in
# a signature, where WebKit 17 stops ("can't get 1th argument Type").
TYPED_REFERENCE_FUNCTION = bytes.fromhex("010902600000600163000003020101")
EMPTY_BODY = bytes.fromhex("0a040102000b")
# A target_features section that lists +gc, the feature of typed references.
GC_FEATURES_SECTION = b"\x00\x15\x0ftarget_features\x01\x2b\x02gc"


def check(module):
    """Runs the check on `module` bytes, or on a path with no file for None."""
    with tempfile.TemporaryDirectory() as scratch:
        path = Path(scratch) / "module.wasm"
        if module is not None:
            path.write_bytes(module)
        return subprocess.run(
            ["bash", str(CHECK), str(path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            check=False,
        )


class WasmFeatureFloor(unittest.TestCase):
    def test_a_module_with_floor_features_passes(self):
        result = check(HEADER + PLAIN_FUNCTION + ONE_PAGE_MEMORY + MEMORY_FILL_BODY)
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_a_typed_reference_fails_even_when_the_module_lists_gc(self):
        result = check(HEADER + TYPED_REFERENCE_FUNCTION + EMPTY_BODY + GC_FEATURES_SECTION)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn("needs a WebAssembly feature past", result.stdout)

    def test_a_missing_module_is_not_reported_as_a_feature_failure(self):
        result = check(None)
        self.assertEqual(result.returncode, 2, result.stdout)
        self.assertIn("no module at", result.stdout)
        self.assertNotIn("needs a WebAssembly feature past", result.stdout)


if __name__ == "__main__":
    unittest.main()

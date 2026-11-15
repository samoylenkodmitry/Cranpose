"""`versions.py bump` when a framework's latest release does not resolve: its
pin stays where it was, the record says so, and the other pins still move."""

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))
SPEC = importlib.util.spec_from_file_location('versions', BENCHMARK / 'versions.py')
versions = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(versions)


def unresolvable():
    raise subprocess.CalledProcessError(101, ('cargo', 'update'), stderr='error: failed to select a version')


class BumpResolveTest(unittest.TestCase):
    def test_a_release_that_does_not_resolve_keeps_its_pin_and_the_night_goes_on(self):
        with tempfile.TemporaryDirectory() as folder:
            broken, fine, record = Path(folder) / 'broken.toml', Path(folder) / 'fine.toml', Path(folder) / 'bumps.json'
            broken.write_text('broken = "1.0.0"\n')
            fine.write_text('fine = "1.0.0"\n')
            pins = {
                'broken': versions.Pin([str(broken)], r'broken = "([^"]+)"', lambda: '2.0.0', unresolvable),
                'fine': versions.Pin([str(fine)], r'fine = "([^"]+)"', lambda: '2.0.0'),
            }
            argv = ['versions.py', 'bump', '--record', str(record)]
            with patch.object(versions, 'PINS', pins), patch.object(sys, 'argv', argv):
                versions.main()
            moved = json.loads(record.read_text())
            self.assertEqual(broken.read_text(), 'broken = "1.0.0"\n')
            self.assertTrue(moved['broken']['reverted'])
            self.assertEqual(moved['broken']['to'], '2.0.0')
            self.assertIn('2.0.0', fine.read_text())
            self.assertFalse(moved['fine']['reverted'])

    def test_ferroui_moves_as_one_family(self):
        # The crates of FerroUI link one native library, so two versions of
        # them cannot meet in one build.
        with tempfile.TemporaryDirectory() as folder:
            manifest = Path(folder) / 'Cargo.toml'
            manifest.write_text('ferroui-view = { version = "0.4.0", features = ["desktop"] }\n'
                                'ferroui-base = "0.4.0"\n'
                                'ferroui-controls = "0.4.0"\n'
                                'ferroui-themes-simple = "0.4.0"\n')
            ferro = versions.PINS['ferro']
            pin = versions.Pin([str(manifest)], ferro.pattern, ferro.latest)
            self.assertEqual(pin.current(), '0.4.0')
            pin.set('0.5.0')
            self.assertNotIn('0.4.0', manifest.read_text())
            self.assertEqual(manifest.read_text().count('"0.5.0"'), 4)


if __name__ == '__main__':
    unittest.main()

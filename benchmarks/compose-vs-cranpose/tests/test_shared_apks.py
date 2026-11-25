"""Both Android devices measure the same APK files and retain install failures."""

import contextlib
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_browser_tiers import BrowserPhone, frameworks
from test_starts import leg


class BenchmarkDevice(BrowserPhone):
    def __init__(self, serial):
        super().__init__([''])
        self.serial = serial

    def install(self, app, apk):
        if self.serial == 'stick' and app == 'views':
            raise RuntimeError(f'{apk.name}: install failed')


class SharedApksTest(unittest.TestCase):
    def test_both_devices_use_the_same_apks_and_only_launch_installed_builds(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name in ('compose', 'views'):
                (root / f'{name}.apk').write_bytes(name.encode())
            (root / 'versions.json').write_text('{"compose":"test","views":"test"}')
            for serial in ('mate', 'stick'):
                output = root / serial
                argv = ['frameworks.py', '--serial', serial, '--install', folder, '--output', str(output), '--full-run']
                with patch.object(sys, 'argv', argv), patch.object(frameworks, 'Device', BenchmarkDevice), \
                        patch.object(frameworks, 'device_lock', lambda _: contextlib.nullcontext()), \
                        patch.object(frameworks.AppTarget, 'reset', lambda *_: None), \
                        patch.object(frameworks, 'measure_run', return_value=leg(25.0)):
                    frameworks.main()
                report = json.loads((output / 'frameworks.json').read_text())
                self.assertEqual({s['name']: s['apk_sha256'] for s in report['subjects']},
                                 {name: hashlib.sha256(name.encode()).hexdigest() for name in ('compose', 'views')})
                scenario = report['scenarios'][0]
                self.assertEqual(scenario['extras'], '--ei tier 12')
                self.assertEqual(scenario['app_bytes'], {'compose': 7, 'views': 5})
                self.assertEqual({leg['subject'] for leg in scenario['legs']},
                                 {'compose'} if serial == 'stick' else {'compose', 'views'})
                self.assertEqual([(f['subject'], f['start']) for f in scenario['failures']],
                                 [('views', 'first'), ('views', 'second')] if serial == 'stick' else [])


if __name__ == '__main__':
    unittest.main()

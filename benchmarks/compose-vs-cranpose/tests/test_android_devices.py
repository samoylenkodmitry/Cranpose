"""The nightly publishes each device that completes its comparison."""
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


NIGHTLY = Path(__file__).resolve().parents[3] / 'scripts/perf/android_devices.py'
JUST = """#!/usr/bin/env python3
import os
import sys
import time
from pathlib import Path

recipe, path, *args = sys.argv[1:]
path = Path(path)
if recipe == 'perf-publish':
    path.with_suffix('.published').write_bytes(path.read_bytes())
else:
    assert args[0] == str(path.parent / 'apks')
    assert args[1:] == (['mate-serial'] if recipe == 'perf-measure-frameworks' else [])
    (path / 'started').touch()
    other = 'mi-tv' if path.name == 'frameworks' else 'frameworks'
    deadline = time.monotonic() + 3
    while not (path.parent / other / 'started').exists():
        if time.monotonic() > deadline:
            sys.exit('the two independent devices did not start')
        time.sleep(0.01)
    if path.name == os.environ.get('FAILED_DEVICE'):
        sys.exit('installation failed')
    (path / 'frameworks.json').write_text(path.name)
"""


class AndroidDevicesTest(unittest.TestCase):
    def test_parallel_devices_publish_only_completed_runs(self):
        for failed in ('', 'mi-tv'):
            with self.subTest(failed=failed), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                just = root / 'just'
                just.write_text(JUST)
                just.chmod(0o755)
                env = dict(os.environ, PATH=folder + os.pathsep + os.environ['PATH'], FAILED_DEVICE=failed)
                run = subprocess.run([sys.executable, NIGHTLY, '--serial', 'mate-serial', '--output', root],
                                     capture_output=True, text=True, env=env)
                self.assertEqual(run.returncode == 0, not failed, run.stdout + run.stderr)
                published = {p.read_text() for p in root.glob('*/frameworks.published')}
                self.assertEqual(published, {'frameworks', 'mi-tv'} - {failed})
                if failed:
                    self.assertIn('installation failed', run.stdout)


if __name__ == '__main__':
    unittest.main()

import importlib.util
import os
import stat
import subprocess
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location('nightly', Path(__file__).resolve().parent / 'nightly.py')
nightly = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(nightly)

# Fails the first time it runs, as Gradle did when another job's build held
# the cache lock, and records every run.
GRADLEW = """#!/bin/sh
echo "$*" >> "$GRADLE_CALLS"
if [ ! -e "$GRADLE_CALLS.failed" ]; then touch "$GRADLE_CALLS.failed"; exit 1; fi
"""


def tree_with(gradlew, folder):
    tree = Path(folder)
    directory = tree / nightly.ANDROID
    directory.mkdir(parents=True)
    script = directory / 'gradlew'
    script.write_text(gradlew)
    script.chmod(script.stat().st_mode | stat.S_IEXEC)
    return tree


class BuildTest(unittest.TestCase):
    def test_a_build_that_fails_once_is_made_again(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(nightly, 'BUILD_RETRY_DELAY_S', 0):
            calls = Path(folder) / 'calls'
            with patch.dict(os.environ, {'GRADLE_CALLS': str(calls)}):
                apk = nightly.build(tree_with(GRADLEW, Path(folder) / 'tree'), 'release')
            self.assertEqual(len(calls.read_text().splitlines()), 2)
            self.assertEqual(apk.name, 'app-release.apk')

    def test_a_build_that_keeps_failing_stops_the_night_after_its_attempts(self):
        with tempfile.TemporaryDirectory() as folder, patch.object(nightly, 'BUILD_RETRY_DELAY_S', 0):
            calls = Path(folder) / 'calls'
            failing = '#!/bin/sh\necho "$*" >> "$GRADLE_CALLS"\nexit 1\n'
            with patch.dict(os.environ, {'GRADLE_CALLS': str(calls)}):
                with self.assertRaises(subprocess.CalledProcessError):
                    nightly.build(tree_with(failing, Path(folder) / 'tree'))
            self.assertEqual(len(calls.read_text().splitlines()), nightly.BUILD_ATTEMPTS)


if __name__ == '__main__':
    unittest.main()

"""`Device` against a stand-in `adb` whose first install hangs: the install
restarts the adb server, waits for the phone and succeeds."""

import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))

import measure  # noqa: E402

# Records each call; the first install sleeps past the test's timeout.
FAKE_ADB = """#!/bin/sh
echo "$*" >> "$ADB_CALLS"
case "$*" in
  *" install "*)
    if [ ! -e "$ADB_CALLS.hung" ]; then touch "$ADB_CALLS.hung"; sleep 5; fi
    echo Success ;;
esac
"""


class AdbRecoveryTest(unittest.TestCase):
    def test_a_hung_install_restarts_the_server_and_installs(self):
        with tempfile.TemporaryDirectory() as folder:
            tools, calls = Path(folder), Path(folder) / 'calls'
            adb = tools / 'adb'
            adb.write_text(FAKE_ADB)
            adb.chmod(adb.stat().st_mode | stat.S_IEXEC)
            environment = {'PATH': f'{tools}{os.pathsep}{os.environ["PATH"]}', 'ADB_CALLS': str(calls)}
            with patch.dict(os.environ, environment), patch.object(measure, 'INSTALL_TIMEOUT_S', 1):
                measure.Device('SERIAL').install('compose', tools / 'compose.apk')
            lines = calls.read_text().splitlines()
        installs = [index for index, line in enumerate(lines) if ' install ' in f' {line} ']
        self.assertEqual(len(installs), 2, lines)
        between = lines[installs[0] + 1:installs[1]]
        self.assertEqual(between, ['kill-server', 'start-server', '-s SERIAL wait-for-device'], lines)
        self.assertIn('-s SERIAL shell cmd package compile -m speed -f dev.perfcompare.compose', lines)

    def test_an_adb_that_hangs_again_after_the_restart_fails_the_install(self):
        with tempfile.TemporaryDirectory() as folder:
            tools, calls = Path(folder), Path(folder) / 'calls'
            adb = tools / 'adb'
            adb.write_text(FAKE_ADB.replace('if [ ! -e "$ADB_CALLS.hung" ]; then touch "$ADB_CALLS.hung"; sleep 5; fi',
                                            'sleep 5'))
            adb.chmod(adb.stat().st_mode | stat.S_IEXEC)
            environment = {'PATH': f'{tools}{os.pathsep}{os.environ["PATH"]}', 'ADB_CALLS': str(calls)}
            with patch.dict(os.environ, environment), patch.object(measure, 'INSTALL_TIMEOUT_S', 1):
                with self.assertRaisesRegex(RuntimeError, 'hung for 1 s twice'):
                    measure.Device('SERIAL').install('compose', tools / 'compose.apk')


if __name__ == '__main__':
    unittest.main()

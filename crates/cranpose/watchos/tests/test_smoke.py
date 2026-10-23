import importlib.util
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('watchos_smoke', Path(__file__).parents[1] / 'smoke.py')
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)

# What simctl prints on stderr when CoreSimulator goes away under a command.
SERVER_DIED = '''An error was encountered processing the command (domain=NSMachErrorDomain, code=-308):
The operation couldn’t be completed. (Mach error -308 - (ipc/mig) server died)
'''
NOT_BOOTED = '''An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):
Process spawn via launchd failed because device is not booted.
\tBad or unknown session: com.apple.CoreSimulator.SimDevice.3B5A9A2B-918A-4F6A-BCA7-F5EB640C4A5E
'''
# Seen on the showcase's watch job on 2026-10-23: the device shut down while
# the app installed, and it lost its runtime before a spawn.
SHUTTING_DOWN = '''An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):
Unable to lookup in current state: Shutting Down
'''
NO_RUNTIME = '''An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=404):
Invalid device: The specified device is not available for spawning processes.
Underlying error (domain=com.apple.CoreSimulator.SimError, code=401):
\tThe com.apple.CoreSimulator.SimRuntime.watchOS-27-0 simulator runtime is not available.
'''
BAD_BUNDLE = '''An error was encountered processing the command (domain=NSPOSIXErrorDomain, code=22):
Failed to install the requested application: the bundle is not valid
'''

# A stand-in for `xcrun simctl --set <set> <verb> ...`. The plan names, for each
# session (a session starts at its `create`), the verbs that fail and the stderr
# they print. After a failure the session's device is gone: `shutdown` answers
# as simctl does for a device in the Shutdown state.
XCRUN = '''#!{python}
import json
import os
from pathlib import Path
import sys

root = Path(os.environ['FAKE_SIMCTL'])
plan = json.loads((root / 'plan.json').read_text())
assert sys.argv[1:3] == ['simctl', '--set'], sys.argv
scratch, verb, rest = sys.argv[3], sys.argv[4], sys.argv[5:]
log = root / 'calls.jsonl'
with log.open('a') as calls:
    calls.write(json.dumps({{'set': scratch, 'verb': verb}}) + '\\n')
session = sum(json.loads(line)['verb'] == 'create' for line in log.read_text().splitlines()) - 1
behavior = plan[min(session, len(plan) - 1)] if session >= 0 else {{}}
dead = root / f'dead-{{session}}'
if verb == 'shutdown' and dead.exists():
    sys.stderr.write('An error was encountered processing the command (domain=com.apple.CoreSimulator.SimError, code=405):\\n'
                     'Unable to shutdown device in current state: Shutdown\\n')
    sys.exit(149)
if verb in behavior.get('fail', {{}}):
    dead.touch()
    sys.stderr.write(behavior['fail'][verb])
    sys.exit(204)
if verb == 'list':
    print(json.dumps({{'runtimes': [
        {{'isAvailable': True, 'identifier': 'com.apple.CoreSimulator.SimRuntime.watchOS-27-0',
         'version': '27.0', 'name': 'watchOS 27.0'}},
        {{'isAvailable': True, 'identifier': 'com.apple.CoreSimulator.SimRuntime.iOS-27-0',
         'version': '27.0', 'name': 'iOS 27.0'}}]}}))
elif verb == 'create':
    print(f'DEVICE-{{session}}')
elif verb == 'launch':
    print('org.cranpose.test: 4242')
elif verb == 'spawn':
    print('PID\\tStatus\\tLabel')
    print('-\\t0\\tcom.apple.example')
    if not behavior.get('app_exits'):
        print('4242\\t0\\tUIKitApplication:org.cranpose.test')
elif verb == 'io':
    Path(rest[-1]).write_bytes(b'png')
'''


class SmokeTest(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        (self.root / 'bin').mkdir()
        fake = self.root / 'bin/xcrun'
        fake.write_text(XCRUN.format(python=sys.executable))
        fake.chmod(0o755)
        self.bundle = self.root / 'Test.app'
        self.bundle.mkdir()
        (self.bundle / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier': 'org.cranpose.test'}))
        self.evidence = self.root / 'evidence'
        self.sets = self.root / 'sets'
        self.sets.mkdir()
        environment = patch.dict(os.environ, {
            'FAKE_SIMCTL': str(self.root),
            'PATH': f'{self.root / "bin"}{os.pathsep}{os.environ["PATH"]}',
        })
        environment.start()
        self.addCleanup(environment.stop)
        for target in (patch.object(smoke, 'SCRATCH_ROOT', str(self.sets)),
                       patch.object(smoke.time, 'sleep')):
            target.start()
            self.addCleanup(target.stop)

    def run_smoke(self, *sessions):
        (self.root / 'plan.json').write_text(json.dumps(sessions))
        smoke.smoke(self.bundle, self.evidence)

    def calls(self, verb):
        lines = (self.root / 'calls.jsonl').read_text().splitlines()
        return [call['set'] for call in map(json.loads, lines) if call['verb'] == verb]

    def test_a_healthy_session_runs_once_and_keeps_its_evidence(self):
        self.run_smoke({})
        self.assertEqual(len(self.calls('create')), 1)
        self.assertEqual((self.evidence / 'watch.png').read_bytes(), b'png')
        report = (self.evidence / 'launch.txt').read_text()
        self.assertIn('watchOS 27.0', report)
        self.assertNotIn('Repeated', report)
        self.assertEqual(list(self.sets.iterdir()), [])

    def test_coresimulator_dying_at_any_step_repeats_the_session_in_a_new_device_set(self):
        for verb, text in (('bootstatus', SERVER_DIED), ('install', SERVER_DIED),
                           ('launch', SERVER_DIED), ('spawn', NOT_BOOTED),
                           ('install', SHUTTING_DOWN), ('spawn', NO_RUNTIME)):
            with self.subTest(verb=verb, text=text):
                (self.root / 'calls.jsonl').unlink(missing_ok=True)
                for marker in self.root.glob('dead-*'):
                    marker.unlink()
                self.evidence = self.root / f'evidence-{verb}-{len(text)}'
                self.run_smoke({'fail': {verb: text}}, {})
                first, second = self.calls('create')
                self.assertNotEqual(first, second)
                self.assertEqual(self.calls(verb)[-1], second)
                self.assertTrue((self.evidence / 'watch.png').is_file())
                self.assertIn('Repeated', (self.evidence / 'launch.txt').read_text())
                self.assertEqual(list(self.sets.iterdir()), [])

    def test_a_second_death_fails_after_a_single_repeat(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_smoke({'fail': {'install': SERVER_DIED}}, {'fail': {'spawn': NOT_BOOTED}})
        self.assertEqual(len(self.calls('create')), 2)
        self.assertFalse((self.evidence / 'launch.txt').exists())
        self.assertEqual(list(self.sets.iterdir()), [])

    def test_other_simctl_failures_are_not_repeated(self):
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_smoke({'fail': {'install': BAD_BUNDLE}}, {})
        self.assertEqual(len(self.calls('create')), 1)

    def test_an_app_that_exits_is_not_repeated(self):
        with self.assertRaisesRegex(RuntimeError, 'App exited after launch'):
            self.run_smoke({'app_exits': True}, {})
        self.assertEqual(len(self.calls('create')), 1)


if __name__ == '__main__':
    unittest.main()

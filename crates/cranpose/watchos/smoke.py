#!/usr/bin/env python3
"""Launch a watch app in a private simulator, retain evidence, and clean up."""
import argparse
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import time

# CoreSimulator on the build fleet cannot create devices or captures on its
# external build drive. Only the temporary device set uses the internal disk.
SCRATCH_ROOT = '/Users/Shared'

# What simctl prints on stderr when CoreSimulator itself went away: its service
# died under a command, the device shut down under a running session, or the
# device lost its runtime.
CORESIMULATOR_DIED = (
    'Mach error -308',
    'device is not booted',
    'Bad or unknown session',
    'in current state: Shut',
    'not available for spawning processes',
)


def command(*args):
    done = subprocess.run(list(map(str, args)), text=True, capture_output=True, timeout=180)
    sys.stderr.write(done.stderr)
    done.check_returncode()
    return done.stdout.strip()


def session(bundle, evidence, repeated):
    info = plistlib.loads((bundle / 'Info.plist').read_bytes())
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='cranpose-watch-', dir=SCRATCH_ROOT) as scratch:
        def sim(*args):
            return command('xcrun', 'simctl', '--set', scratch, *args)

        runtimes = json.loads(sim('list', 'runtimes', '--json'))['runtimes']
        watches = [r for r in runtimes if r['isAvailable'] and '.watchOS-' in r['identifier']]
        if not watches:
            raise RuntimeError('Install a watchOS simulator runtime with xcodebuild -downloadPlatform watchOS')
        runtime = max(watches, key=lambda r: tuple(map(int, r['version'].split('.'))))
        device = sim('create', 'Cranpose CI',
                     'com.apple.CoreSimulator.SimDeviceType.Apple-Watch-Series-9-45mm', runtime['identifier'])
        try:
            sim('bootstatus', device, '-b')
            sim('install', device, bundle.resolve())
            launched = sim('launch', device, info['CFBundleIdentifier'])
            time.sleep(15)
            processes = sim('spawn', device, 'launchctl', 'list')
            pid = launched.rsplit(':', 1)[-1].strip()
            if not any(line.split()[0] == pid for line in processes.splitlines() if line.split()):
                raise RuntimeError(f'App exited after launch: {launched}')
            capture = Path(scratch) / 'watch.png'
            sim('io', device, 'screenshot', capture)
            shutil.copyfile(capture, evidence / 'watch.png')
            report = f'{runtime["name"]}\n{launched}\nAlive after 15 seconds\n'
            if repeated:
                report += 'Repeated once after CoreSimulator died\n'
            (evidence / 'launch.txt').write_text(report)
            print(f'Simulator smoke passed: {launched}', flush=True)
        finally:
            # Cleanup must not replace the failure that ended the session.
            for verb in ('shutdown', 'delete'):
                subprocess.run(['xcrun', 'simctl', '--set', scratch, verb, device], check=False, timeout=60)


def smoke(bundle, evidence):
    """Run one session. If CoreSimulator died under it, run it once more in a new device set."""
    try:
        session(bundle, evidence, repeated=False)
    except subprocess.CalledProcessError as failure:
        if not any(text in failure.stderr for text in CORESIMULATOR_DIED):
            raise
        print('CoreSimulator died; repeating the session once in a new device set', flush=True)
        session(bundle, evidence, repeated=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    parser.add_argument('evidence', type=Path)
    args = parser.parse_args()
    smoke(args.bundle, args.evidence)

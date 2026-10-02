#!/usr/bin/env python3
"""Launch a watch app in a private simulator, retain evidence, and clean up."""
import argparse
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import time


def command(*args):
    return subprocess.check_output(list(map(str, args)), text=True, timeout=180).strip()


def smoke(bundle, evidence):
    info = plistlib.loads((bundle / 'Info.plist').read_bytes())
    evidence.mkdir(parents=True, exist_ok=True)
    # CoreSimulator on the build fleet cannot create devices or captures on its
    # external build drive. Only this temporary set uses the internal disk.
    with tempfile.TemporaryDirectory(prefix='cranpose-watch-', dir='/Users/Shared') as scratch:
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
            (evidence / 'launch.txt').write_text(f'{runtime["name"]}\n{launched}\nAlive after 15 seconds\n')
            print(f'Simulator smoke passed: {launched}', flush=True)
        finally:
            subprocess.run(['xcrun', 'simctl', '--set', scratch, 'shutdown', device], check=False, timeout=60)
            sim('delete', device)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bundle', type=Path)
    parser.add_argument('evidence', type=Path)
    args = parser.parse_args()
    smoke(args.bundle, args.evidence)

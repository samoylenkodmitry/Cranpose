import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess


def command(*args, **kwargs):
    return subprocess.check_output(list(args), text=True, stderr=subprocess.STDOUT,
                                   timeout=kwargs.pop('timeout', 120), **kwargs)


def boot(device_id, report):
    """Boots the simulator and waits until it is ready. A cold boot on the
    hosted macOS runner took over 120 seconds twice on 2026-10-15, so the
    wait is five minutes, and a boot that hangs past it is shut down and
    started once more."""
    for attempt in (1, 2):
        devices = json.loads(command('xcrun', 'simctl', 'list', 'devices', 'available', '--json'))
        device = next(d for group in devices['devices'].values() for d in group if d['udid'] == device_id)
        if device['state'] != 'Booted':
            command('xcrun', 'simctl', 'boot', device_id)
        try:
            command('xcrun', 'simctl', 'bootstatus', device_id, '-b', timeout=300)
            report['boot_attempts'] = attempt
            return
        except subprocess.TimeoutExpired:
            if attempt == 2:
                raise
            subprocess.run(['xcrun', 'simctl', 'shutdown', device_id], check=False, timeout=120)


def verify_summary(summary):
    if (summary.get('passedTests') != 4 or summary.get('failedTests') != 0
            or summary.get('skippedTests') != 0 or summary.get('expectedFailures') != 0
            or summary.get('totalTestCount') != 4 or summary.get('result') != 'Passed'):
        raise AssertionError(f'Four iOS accessibility and lifecycle tests must execute and pass: {summary}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--device', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    info = plistlib.loads((args.app / 'Info.plist').read_bytes())
    binary = args.app / info['CFBundleExecutable']
    report = {'platform': 'ios-simulator', 'device': args.device, 'status': 'running',
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
    try:
        boot(args.device, report)
        command('xcrun', 'simctl', 'install', args.device, str(args.app.resolve()))
        results = args.output.resolve() / 'results.xcresult'
        with (args.output / 'xcodebuild.log').open('w') as log:
            result = subprocess.run([
                'xcodebuild', '-project', 'apps/liquid-reference/LiquidReference.xcodeproj',
                '-scheme', 'LiquidReference', '-configuration', 'Release',
                '-destination', f'platform=iOS Simulator,id={args.device}',
                '-derivedDataPath', 'target/accessibility-ios-tests',
                '-resultBundlePath', str(results), '-parallel-testing-enabled', 'NO',
                '-only-testing:LiquidReferenceTests/AccessibilityRobotTests', 'test'],
                stdout=log, stderr=subprocess.STDOUT, check=False, timeout=900)
        summary = json.loads(command('xcrun', 'xcresulttool', 'get', 'test-results', 'summary',
                                     '--path', str(results)))
        report['summary'] = summary
        result.check_returncode()
        verify_summary(summary)
        report['status'] = 'passed'
    except BaseException as error:
        report.update(status='failed', error=str(error))
        output = getattr(error, 'output', None)
        if output:
            (args.output / 'command-error.log').write_bytes(output.encode() if isinstance(output, str) else output)
        raise
    finally:
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

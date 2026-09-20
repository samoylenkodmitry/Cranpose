import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess


def command(*args, **kwargs):
    return subprocess.check_output(list(args), text=True, stderr=subprocess.STDOUT,
                                   timeout=kwargs.pop('timeout', 120), **kwargs)


def verify_summary(summary):
    if (summary.get('passedTests') != 3 or summary.get('failedTests') != 0
            or summary.get('skippedTests') != 0 or summary.get('expectedFailures') != 0
            or summary.get('totalTestCount') != 3 or summary.get('result') != 'Passed'):
        raise AssertionError(f'Three iOS accessibility tests must execute and pass: {summary}')


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
        devices = json.loads(command('xcrun', 'simctl', 'list', 'devices', 'available', '--json'))
        device = next(d for group in devices['devices'].values() for d in group if d['udid'] == args.device)
        if device['state'] != 'Booted':
            command('xcrun', 'simctl', 'boot', args.device)
        command('xcrun', 'simctl', 'bootstatus', args.device, '-b')
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

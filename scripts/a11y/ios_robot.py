import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess


# The hosted runner's simulator once left a launch unserviced for 30 seconds while it showed only
# its home screen (nightly 42557380010, 2026-11-09), which fails a test for a reason outside the
# app. A failed run therefore repeats once, and the report keeps the first run's failures.
ATTEMPTS = 2
# The first run builds the app and the tests, which took 14.5 minutes of the former 15-minute limit
# on the hosted runner on 2026-11-09. The second run finds them built.
FIRST_RUN_SECONDS = 1500
RERUN_SECONDS = 600


def command(*args, **kwargs):
    return subprocess.check_output(list(args), text=True, stderr=subprocess.STDOUT,
                                   timeout=kwargs.pop('timeout', 120), **kwargs)


# The hosted runner's simulator is slow at times. A cold boot took over 120 seconds twice on
# 2026-10-15. On 2026-11-13 (nightly 43360157573) a first boot hung past five minutes; booted
# again, the simulator left the app's install unanswered for 120 seconds. Booting and installing
# therefore wait five minutes each, and a hang shuts the simulator down and starts both again.
PREPARE_ATTEMPTS = 3
STEP_SECONDS = 300


def boot(device_id):
    """Boots the simulator, unless it runs already, and waits until it is ready."""
    devices = json.loads(command('xcrun', 'simctl', 'list', 'devices', 'available', '--json'))
    device = next(d for group in devices['devices'].values() for d in group if d['udid'] == device_id)
    if device['state'] != 'Booted':
        command('xcrun', 'simctl', 'boot', device_id)
    command('xcrun', 'simctl', 'bootstatus', device_id, '-b', timeout=STEP_SECONDS)


def prepare(device_id, app, report):
    """Boots the simulator and installs the app on it."""
    for attempt in range(1, PREPARE_ATTEMPTS + 1):
        try:
            boot(device_id)
            command('xcrun', 'simctl', 'install', device_id, str(app), timeout=STEP_SECONDS)
            report['prepare_attempts'] = attempt
            return
        except subprocess.TimeoutExpired:
            if attempt == PREPARE_ATTEMPTS:
                raise
            subprocess.run(['xcrun', 'simctl', 'shutdown', device_id], check=False, timeout=120)


def run_tests(device_id, output, attempt):
    """Runs the XCTest suite once and returns the process result and the test summary."""
    results = output.resolve() / f'results-{attempt}.xcresult'
    with (output / f'xcodebuild-{attempt}.log').open('w') as log:
        result = subprocess.run([
            'xcodebuild', '-project', 'apps/liquid-reference/LiquidReference.xcodeproj',
            '-scheme', 'LiquidReference', '-configuration', 'Release',
            '-destination', f'platform=iOS Simulator,id={device_id}',
            '-derivedDataPath', 'target/accessibility-ios-tests',
            '-resultBundlePath', str(results), '-parallel-testing-enabled', 'NO',
            '-only-testing:LiquidReferenceTests/AccessibilityRobotTests', 'test'],
            stdout=log, stderr=subprocess.STDOUT, check=False,
            timeout=FIRST_RUN_SECONDS if attempt == 1 else RERUN_SECONDS)
    summary = json.loads(command('xcrun', 'xcresulttool', 'get', 'test-results', 'summary',
                                 '--path', str(results)))
    return result, summary


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
        prepare(args.device, args.app.resolve(), report)
        earlier_failures = []
        for attempt in range(1, ATTEMPTS + 1):
            result, summary = run_tests(args.device, args.output, attempt)
            report['summary'] = summary
            try:
                result.check_returncode()
                verify_summary(summary)
                break
            except (subprocess.CalledProcessError, AssertionError):
                if attempt == ATTEMPTS:
                    raise
                earlier_failures.append(summary.get('testFailures', []))
                report['earlier_failures'] = earlier_failures
        report['status'] = 'passed'
    except BaseException as error:
        report.update(status='failed', error=str(error))
        output = getattr(error, 'output', None)
        if output:
            (args.output / 'command-error.log').write_bytes(output.encode() if isinstance(output, str) else output)
        raise
    finally:
        # The app's own log: its pipeline builds and frames it could not draw.
        try:
            (args.output / 'app.log').write_text(command(
                'xcrun', 'simctl', 'spawn', args.device, 'log', 'show', '--last', '1h', '--style', 'compact',
                '--predicate', f'subsystem == "{info["CFBundleIdentifier"]}"'))
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            report['app_log_error'] = str(error)
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

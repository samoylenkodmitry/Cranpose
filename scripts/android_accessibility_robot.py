import argparse
import hashlib
import json
from pathlib import Path
import re

from android_robot_device import locked_device


def completed_tests(output):
    results = re.findall(r'^OK \((\d+) tests?\)$', output, re.M)
    if (len(results) != 1 or int(results[0]) < 1 or 'FAILURES!!!' in output
            or 'INSTRUMENTATION_FAILED' in output
            or re.search(r'^INSTRUMENTATION_STATUS_CODE: -(?:[1-4])\s*$', output, re.M)):
        raise RuntimeError('Android accessibility robot did not pass; inspect instrumentation.log')
    return int(results[0])


def main():
    root = Path(__file__).resolve().parents[1]
    apk_root = root / 'apps/android-demo/android/app/build/outputs/apk'
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--app-apk', type=Path, default=apk_root / 'release/app-release.apk')
    parser.add_argument('--test-apk', type=Path,
                        default=apk_root / 'androidTest/release/app-release-androidTest.apk')
    parser.add_argument('--connected-only', action='store_true')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'serial': args.serial, 'status': 'running', 'apks': {}}
    try:
        with locked_device(args.serial) as device:
            for apk in [args.app_apk, args.test_apk]:
                with apk.open('rb') as source:
                    report['apks'][str(apk)] = hashlib.file_digest(source, 'sha256').hexdigest()
                device.wake()
                device.command('install', '-r', '-t', str(apk), timeout=180)
            device.wake()
            navigation = 'com.compose_rs.demo.CranposeAccessibilityNavigationTest'
            if args.connected_only:
                navigation += '#connectedAccessibilityTracksPageChangesInBothDirections'
            tests = ','.join([navigation,
                              'com.compose_rs.demo.CranposeAccessibilityAuditTest',
                              'com.compose_rs.demo.CranposeAccessibilityParserTest',
                              'com.compose_rs.demo.CranposeAccessibilityInteractionTest'])
            output = device.command(
                'shell', 'am', 'instrument', '-w', '-r', '-e', 'class',
                tests,
                'com.compose_rs.demo.robot.test/androidx.test.runner.AndroidJUnitRunner', timeout=120)
            (args.output / 'instrumentation.log').write_text(output)
            report['tests_passed'] = completed_tests(output)
            expected = 9 if args.connected_only else 10
            if report['tests_passed'] != expected:
                raise RuntimeError(f"Expected {expected} Android tests, got {report['tests_passed']}")
            report['reconnect'] = 'not_requested' if args.connected_only else 'included'
            report['status'] = 'passed'
            print(output)
    except BaseException as error:
        report.update(status='failed', error=str(error))
        output = getattr(error, 'output', None)
        if output:
            (args.output / 'command-error.log').write_bytes(output.encode() if isinstance(output, str) else output)
        raise
    finally:
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

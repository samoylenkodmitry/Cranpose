import argparse
import hashlib
import json
from pathlib import Path
import re

from android_robot_device import locked_device


def completed_tests(output):
    result = re.search(r'^OK \((\d+) tests?\)$', output, re.M)
    if (result is None or int(result[1]) < 1 or 'FAILURES!!!' in output
            or 'INSTRUMENTATION_FAILED' in output):
        raise RuntimeError('Android accessibility robot did not pass; inspect instrumentation.log')
    return int(result[1])


def main():
    root = Path(__file__).resolve().parents[1]
    apk_root = root / 'apps/android-demo/android/app/build/outputs/apk'
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--app-apk', type=Path, default=apk_root / 'release/app-release.apk')
    parser.add_argument('--test-apk', type=Path,
                        default=apk_root / 'androidTest/release/app-release-androidTest.apk')
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
            output = device.command(
                'shell', 'am', 'instrument', '-w', '-r', '-e', 'class',
                'com.compose_rs.demo.CranposeAccessibilityNavigationTest,'
                'com.compose_rs.demo.CranposeAccessibilityParserTest',
                'com.compose_rs.demo.robot.test/androidx.test.runner.AndroidJUnitRunner', timeout=120)
            (args.output / 'instrumentation.log').write_text(output)
            report['tests_passed'] = completed_tests(output)
            report['status'] = 'passed'
            print(output)
    except BaseException as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

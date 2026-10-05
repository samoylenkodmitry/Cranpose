import argparse
import hashlib
import json
from pathlib import Path

from android_accessibility_robot import completed_tests
from android_robot_device import locked_device


def main():
    root = Path(__file__).resolve().parents[1]
    apks = root / 'apps/android-demo/android/app/build/outputs/apk'
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--package', default='com.compose_rs.demo.robot')
    parser.add_argument('--app-apk', type=Path, default=apks / 'release/app-release.apk')
    parser.add_argument('--test-apk', type=Path,
                        default=apks / 'androidTest/release/app-release-androidTest.apk')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'serial': args.serial, 'status': 'running', 'apks': {},
              'scope': 'Physical keyboard correctness; no timing or FPS claim'}
    try:
        with locked_device(args.serial) as device:
            device.wake()
            report['model'] = device.command('shell', 'getprop', 'ro.product.model').strip()
            for name, apk in [('app', args.app_apk), ('test', args.test_apk)]:
                with apk.open('rb') as source:
                    report['apks'][name] = hashlib.file_digest(source, 'sha256').hexdigest()
                device.command('install', '-r', '-t', str(apk), timeout=180)
            device.wake()
            output = device.command(
                'shell', 'am', 'instrument', '-w', '-r', '-e', 'class',
                'com.compose_rs.demo.CranposeInsetsTest',
                args.package + '.test/androidx.test.runner.AndroidJUnitRunner', timeout=120)
            (args.output / 'instrumentation.log').write_text(output)
            report['tests_passed'] = completed_tests(output)
            if report['tests_passed'] != 1:
                raise RuntimeError('Expected one physical keyboard test')
            for name in ['insets-open.png', 'insets-closed.png']:
                device.command('pull', '/sdcard/Android/data/' + args.package + '/files/' + name,
                               str(args.output / name))
            report['status'] = 'passed'
            print(output)
    except BaseException as error:
        report.update(status='failed', error=str(error))
        output = getattr(error, 'output', None)
        if output:
            (args.output / 'command-error.log').write_bytes(
                output.encode() if isinstance(output, str) else output)
        raise
    finally:
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

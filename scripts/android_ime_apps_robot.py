import argparse
import hashlib
import json
from pathlib import Path
import shlex
import uuid

from android_accessibility_robot import completed_tests
from android_robot_device import locked_device


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial', required=True)
    parser.add_argument('--route', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--app-apk', type=Path)
    parser.add_argument('--test-apk', type=Path)
    parser.add_argument('--test-package', default='com.compose_rs.demo.insets.robot')
    args = parser.parse_args()
    route = json.loads(args.route.read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'route': route, 'serial': args.serial, 'status': 'running'}
    (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    run_id = 'ime-apps-' + uuid.uuid4().hex
    try:
        print('Waiting for shared device lock', flush=True)
        with locked_device(args.serial) as device:
            print('Device lock acquired', flush=True)
            device.wake()
            for name, apk in [('app', args.app_apk), ('test', args.test_apk)]:
                if apk:
                    with apk.open('rb') as source:
                        report[name + '_sha256'] = hashlib.file_digest(source, 'sha256').hexdigest()
                    device.command('install', '-r', '-t', str(apk), timeout=180)
            package = route['package']
            report['installed'] = device.command('shell', 'pm', 'path', package).strip()
            if not report['installed'].startswith('package:'):
                raise RuntimeError('Application is not installed: ' + package)
            device.command('shell', 'am', 'force-stop', package)
            print('Starting keyboard test for ' + package, flush=True)
            try:
                output = device.command(
                    'shell', 'am', 'instrument', '-w', '-r', '-e', 'class',
                    'com.compose_rs.demo.CranposeImeAppsTest', '-e', 'run_id', run_id,
                    '-e', 'app_package', package, '-e', 'extras', shlex.quote(json.dumps(route.get('extras', {}))),
                    '-e', 'steps', shlex.quote(json.dumps(route['steps'])),
                    args.test_package + '.test/androidx.test.runner.AndroidJUnitRunner', timeout=180)
                (args.output / 'instrumentation.log').write_text(output)
                print(output)
                report['tests_passed'] = completed_tests(output)
                if report['tests_passed'] != 1:
                    raise RuntimeError('App keyboard test failed; inspect captured geometry and screenshots')
                report['status'] = 'passed'
            finally:
                device.command('pull', '/sdcard/Android/data/' + args.test_package + '/files/' + run_id,
                               str(args.output / 'captures'))
    except BaseException as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


if __name__ == '__main__':
    main()

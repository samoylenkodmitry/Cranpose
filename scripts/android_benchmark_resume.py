import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import time
from types import SimpleNamespace

from android_benchmark_artifacts import source_inventory
from android_benchmark_build import build_settings
from android_benchmark_support import checked_command, digest, interrupted, run_reported


def resume(args, report):
    cache = Path(report['cache'])
    app = cache / 'app'
    environment = dict(os.environ)
    command = report['command']
    settings = report['settings']
    actual_settings = build_settings(SimpleNamespace(package=settings['package'],
                                                     platform=settings['platform']), environment)
    if actual_settings != settings:
        raise ValueError('Resume build settings differ from the original build')
    for name, source in report['sources'].items():
        if source_inventory(cache / name) != source['inventory']:
            raise ValueError('Resume source differs: ' + name)
    for command_name, key in [('rustc', 'toolchain'), ('cargo', 'cargo')]:
        version_args = ['-Vv'] if command_name == 'rustc' else ['-V']
        actual = checked_command([command_name, *version_args], cwd=app, env=environment).decode()
        if actual != report[key]:
            raise ValueError('Resume toolchain differs: ' + key)
    attempt = str(time.time_ns())
    log = args.report.parent / ('resume-' + attempt + '.log')
    report.setdefault('resume_attempts', []).append(log.name)
    checked_command(command, cwd=app, env=environment, timeout=args.timeout, output=log)
    warnings = [line for line in log.read_text().splitlines() if line.startswith('warning:')]
    if warnings:
        raise ValueError('Build emitted warnings: ' + '\n'.join(warnings))
    native = cache / 'native' / report['abi'] / report['native']
    if not native.is_file():
        raise ValueError('Native build output is absent')
    for name, source in report['sources'].items():
        if source_inventory(cache / name) != source['inventory']:
            raise ValueError('Build changed archived source: ' + name)
    destination = args.report.parent / native.name
    shutil.copyfile(native, destination)
    report.update(phase='complete', native=native.name, native_sha256=digest(destination),
                  lock_sha256=digest(app / 'Cargo.lock'))
    report.pop('errors', None)
    report.pop('invalid_reason', None)


def main():
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser()
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=150)
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    if report.get('status') == 'complete':
        raise ValueError('Build already completed')
    if not 0 < args.timeout <= 170:
        raise ValueError('Timeout must be between one and 170 seconds')
    previous = args.report.with_name('build-before-resume-' + str(time.time_ns()) + '.json')
    shutil.copyfile(args.report, previous)
    run_reported(args.report, report, lambda: resume(args, report))


if __name__ == '__main__':
    main()

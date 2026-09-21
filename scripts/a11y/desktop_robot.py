import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from desktop_access import adapter_for_platform


def wait_for(probe, description, timeout=20):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        last = probe()
        if last:
            return last
        time.sleep(0.1)
    raise AssertionError(f'Timed out waiting for {description}; last result: {last!r}')


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def run_checks(adapter, report, allow_linux_disabled_state_bug=False):
    def node(name):
        return wait_for(lambda: next((n for n in adapter.nodes() if n['name'] == name), None), name)

    def passed(name):
        report['passed'].append(name)
        print(f'PASS: {name}', flush=True)

    node('Accessibility robot')
    node('Action count: 0')
    node('Account, Account')
    require(node('Remove')['enabled'], 'the nested control must remain enabled')
    published = json.dumps(adapter.snapshot(), ensure_ascii=False)
    for secret in ['robot-secret-value', 'Decorative secret']:
        require(secret not in published, f'private text published: {secret}')
    passed('merged names preserve repeated words and independent controls; secrets stay hidden')
    for name, expected in [('Increase', 1), ('Remove', 2), ('Decrease', 1)]:
        require(adapter.activate(node(name)), f'{name} has no native activation action')
        node(f'Action count: {expected}')
    passed('native activation updates the application exactly once')
    disabled = node('Disabled action')
    if report.get('platform') == 'linux':
        require(disabled.get('description') == 'Disabled', 'disabled state description missing')
    if disabled['enabled']:
        require(allow_linux_disabled_state_bug and report.get('platform') == 'linux',
                'disabled control reported enabled')
        report.setdefault('known_limitations', []).append(
            'AccessKit AT-SPI reports the disabled button as enabled; '
            'the Disabled description and rejected activation are verified. '
            'https://github.com/AccessKit/accesskit/pull/788')
    require(not adapter.activate(disabled), 'disabled control accepted activation')
    node('Action count: 1')
    passed('disabled controls remain discoverable and reject native activation')
    loading = node('Loading')
    require(loading['role'] == 'progress', f'passive progress role: {loading}')
    require(float(loading['value']) == 40, f'passive progress value: {loading}')
    volume = node('Volume')
    require(volume['role'] == 'slider', f'adjustable role: {volume}')
    require(adapter.set_value(volume, 70), 'native range adjustment failed')
    node('Volume value: 70')
    passed('passive progress and adjustable ranges have distinct native behavior')
    require(adapter.set_value(node('Notes'), 'Robot notes'), 'native text replacement failed')
    node('Edited: Robot notes')
    passed('native editable text reaches application state')
    for opener, closer, background in [
            ('Open preferences', 'Close preferences', 'Increase'),
            ('Open confirmation', 'Close confirmation', 'Close preferences')]:
        require(adapter.activate(node(opener)), f'{opener} has no native activation action')
        node(closer)
        require(not any(n['name'] == background for n in adapter.nodes()),
                f'modal exposes background control {background}')
    for closer, restored in [('Close confirmation', 'Close preferences'),
                             ('Close preferences', 'Increase')]:
        require(adapter.activate(node(closer)), f'{closer} has no native activation action')
        node(restored)
    passed('nested modals isolate the native tree and restore background controls')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--allow-linux-disabled-state-bug', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    report = {'platform': sys.platform, 'status': 'running', 'passed': [],
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
    adapter = None
    process = None
    try:
        with (args.output / 'application.log').open('w') as log:
            process = subprocess.Popen([str(binary), '--test_screen=accessibility_robot'],
                                       stdout=log, stderr=subprocess.STDOUT, env=os.environ.copy())
            adapter = adapter_for_platform(process.pid)
            run_checks(adapter, report, args.allow_linux_disabled_state_bug)
            require(process.poll() is None, 'application exited during the robot test')
            report['status'] = ('passed_with_known_limitations' if report.get('known_limitations')
                                else 'passed')
    except BaseException as error:
        report.update(status='failed', error=str(error))
        raise
    finally:
        if adapter is not None:
            try:
                (args.output / 'native-tree.json').write_text(
                    json.dumps(adapter.snapshot(), indent=2, ensure_ascii=False) + '\n')
            except Exception as error:
                report['capture_error'] = str(error)
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()

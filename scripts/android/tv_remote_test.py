"""Drive the demo's TV home screen with Android remote key events."""

import argparse
import json
import sys
import time
from pathlib import Path
from xml.etree import ElementTree

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from android_robot_device import locked_device


def await_label(device, output, label, *, exact=False):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        device.command('shell', 'uiautomator', 'dump', '/data/local/tmp/cranpose-tv.xml')
        xml = device.command('shell', 'cat', '/data/local/tmp/cranpose-tv.xml')
        (output / 'screen.xml').write_text(xml)
        labels = (node.get(key, '') for node in ElementTree.fromstring(xml).iter('node')
                  for key in ('text', 'content-desc'))
        if any(value == label if exact else label in value for value in labels):
            return
    raise RuntimeError(f'the TV did not show {label!r}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--apk', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    package = 'com.compose_rs.demo.robot'
    with locked_device(args.serial) as device:
        device.wake()
        device.command('install', '-r', str(args.apk))
        device.command('shell', 'am', 'start', '-S', '-W', '-n',
                       f'{package}/dev.cranpose.android.CranposeActivity')
        try:
            await_label(device, args.output, 'Row 1', exact=True)
            for key in ('DOWN', 'DOWN', 'DOWN', 'CENTER'):
                device.command('shell', 'input', 'keyevent', 'KEYCODE_DPAD_' + key)
            await_label(device, args.output, '0:00 of 60:00')
            device.command('shell', 'input', 'keyevent', 'KEYCODE_DPAD_RIGHT')
            await_label(device, args.output, '0:10 of 60:00')
            device.command('shell', 'input', 'keyevent', 'KEYCODE_BACK')
            await_label(device, args.output, 'Row 1', exact=True)
        finally:
            device.command('shell', 'am', 'force-stop', package)
    (args.output / 'report.json').write_text(json.dumps({'device': 'Mi TV Stick', 'status': 'passed',
                                                         'app_bytes': args.apk.stat().st_size}))


if __name__ == '__main__':
    main()

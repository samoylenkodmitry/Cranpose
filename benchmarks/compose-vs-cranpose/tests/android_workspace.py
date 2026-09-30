#!/usr/bin/env python3
"""Check the shared workspace through Android input and saved screen pixels.

Pass the physical rectangle occupied by the 1280 x 820 workspace. For the
Mate 20 X reference it is --viewport 0 101 1080 793. This checks behavior and
captures evidence; it does not measure presentation performance.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / 'scripts'))
from android_benchmark_device import AndroidDevice
from android_robot_device import device_lock

APPS = {
    'cranpose': ('dev.perfcompare.cranpose', 'dev.cranpose.android.CranposeActivity'),
    'compose': ('dev.perfcompare.compose', '.MainActivity'),
}
REGIONS = {
    'body': (0, 40, 1280, 792),
    'quote': (440, 204, 899, 442),
    'price': (447, 250, 620, 286),
    'search': (815, 135, 940, 169),
    'watchlist': (306, 270, 438, 764),
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class WorkspaceCheck:
    def __init__(self, args):
        self.args = args
        self.package, self.activity = APPS[args.app]
        self.device = AndroidDevice(args.serial)
        self.steps = []
        self.number = 0

    def launch(self, mode='quotes', still=True):
        self.device.shell('am', 'force-stop', self.package)
        self.device.shell('am', 'start', '-W', '-n', self.package + '/' + self.activity,
                          '--es', 'scenario', 'workspace', '--es', 'mode', mode,
                          '--ez', 'still', str(still).lower())
        self.device.wait_for_pid(self.package)
        time.sleep(0.5)

    def capture(self, name):
        self.number += 1
        path = self.args.output / f'{self.number:02}-{name}.png'
        self.device.screenshot(path, self.args.viewport)
        with Image.open(path) as raw:
            image = raw.convert('RGB').crop(self.args.viewport).resize((1280, 820))
        normalized = path.with_stem(path.stem + '-1280')
        image.save(normalized)
        self.steps.append({'capture': str(path), 'normalized': str(normalized)})
        return image, normalized

    def text(self, image, region, name):
        path = self.args.output / f'{self.number:02}-{name}-text.png'
        image.crop(REGIONS[region]).save(path)
        rows = json.loads(subprocess.check_output([str(self.args.ocr), str(path)], text=True))
        path.with_suffix('.json').write_text(json.dumps(rows, indent=2) + '\n')
        return ' '.join(row['text'] for row in rows)

    def require_text(self, image, region, expected, name, exact=False):
        text = self.text(image, region, name)
        normalized = re.sub(r'\bO(?= updates\b)', '0', text)
        matches = (expected == normalized.strip() if exact
                   else expected.casefold() in normalized.casefold())
        if not matches:
            raise AssertionError(f'{name}: expected {expected!r}; saw {text!r}')
        self.steps.append({'check': name, 'visible_text': text})

    def tap(self, x, y):
        left, top, right, _ = self.args.viewport
        scale = (right - left) / 1280
        self.device.shell('input', 'tap', round(left + x * scale), round(top + y * scale))
        time.sleep(0.2)

    def key(self, key):
        self.device.shell('input', 'keyevent', key)
        time.sleep(0.2)

    def ime(self, visible):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            state = self.device.shell('dumpsys', 'input_method')
            match = re.search(r'\bmInputShown=(true|false)', state)
            if match and (match[1] == 'true') == visible:
                self.steps.append({'check': 'keyboard visibility', 'visible': visible})
                return
            time.sleep(0.1)
        (self.args.output / 'ime-failure.txt').write_text(state)
        raise AssertionError(f'Expected keyboard visible={visible}')

    def changed(self, first, second, region):
        return ImageChops.difference(first.crop(REGIONS[region]),
                                    second.crop(REGIONS[region])).getbbox() is not None

    def check_tabs(self):
        self.tap(535, 188)
        image, _ = self.capture('profile')
        self.require_text(image, 'quote', 'Profile', 'profile panel')
        self.require_text(image, 'quote', '0 updates', 'frozen subscriber snapshot')
        self.tap(471, 188)
        image, _ = self.capture('quote')
        self.require_text(image, 'quote', 'Summit Logistics', 'quote panel restored')

    def check_search_and_keyboard(self):
        self.tap(875, 152)
        self.ime(True)
        self.device.shell('input', 'text', 'AAPL')
        self.key('KEYCODE_BACK')
        self.ime(False)
        image, _ = self.capture('search')
        self.require_text(image, 'search', 'AAPL', 'typed search text', exact=True)
        self.tap(875, 152)
        self.ime(True)
        self.key('KEYCODE_BACK')
        self.ime(False)

    def check_streaming(self):
        before, _ = self.capture('stream-before')
        self.key('KEYCODE_Q')
        deadline = time.monotonic() + 4
        while time.monotonic() < deadline:
            after, _ = self.capture('stream-running')
            if self.changed(before, after, 'price'):
                break
            time.sleep(0.1)
        else:
            raise AssertionError('Stream shortcut did not change the visible quote')
        self.require_text(after, 'search', 'AAPL', 'preview shortcut preserves search text', exact=True)
        self.key('KEYCODE_Q')
        stopped, _ = self.capture('stream-stopped')
        time.sleep(0.4)
        later, _ = self.capture('stream-still-stopped')
        if self.changed(stopped, later, 'price'):
            raise AssertionError('Quote changed after the stream was stopped')
        self.steps.append({'check': 'stream starts and stops through preview keys'})

    def check_scrolling(self):
        before, _ = self.capture('scroll-before')
        self.tap(764, 20)
        time.sleep(0.2)
        self.tap(500, 20)
        after, _ = self.capture('scroll-stopped')
        if not self.changed(before, after, 'watchlist'):
            raise AssertionError('Watchlist rows did not move after selecting auto-scroll')
        time.sleep(0.3)
        later, _ = self.capture('scroll-remains-stopped')
        if self.changed(after, later, 'watchlist'):
            raise AssertionError('Watchlist rows moved after selecting Off')
        self.steps.append({'check': 'watchlist scroll moves visible rows and stops'})

    def check_hover(self):
        self.launch('hover', False)
        self.tap(1050, 20)
        samples = set()
        for _ in range(6):
            image, _ = self.capture('hover')
            samples.add(tuple(image.getpixel((308, 284 + row * 32)) for row in range(15)))
            time.sleep(0.137)
        if len(samples) < 2:
            raise AssertionError('Synthetic pointer movement did not change visible row backgrounds')
        self.steps.append({'check': 'hover changes visible row backgrounds', 'distinct_samples': len(samples)})

    def run(self):
        self.launch()
        self.ime(False)
        paused, path = self.capture('paused')
        self.require_text(paused, 'body', 'Trading workspace', 'workspace visible')
        self.require_text(paused, 'quote', '264.000', 'deterministic initial quote')
        self.steps.append({'frozen_reference': str(path)})
        self.check_tabs()
        self.check_search_and_keyboard()
        self.check_streaming()
        self.check_scrolling()
        self.check_hover()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--app', choices=APPS, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--ocr', type=Path, required=True)
    parser.add_argument('--viewport', type=int, nargs=4, required=True,
                        metavar=('LEFT', 'TOP', 'RIGHT', 'BOTTOM'))
    args = parser.parse_args()
    left, top, right, bottom = args.viewport
    if min(left, top) < 0 or right <= left or abs((bottom - top) / (right - left) - 820 / 1280) > 0.002:
        parser.error('Viewport must be the physical rectangle of the 1280 x 820 workspace')
    proof = json.loads(args.ocr.with_suffix('.json').read_text())
    if proof != {'source_sha256': digest(ROOT / 'scripts/android/recognize_text.swift'),
                 'executable_sha256': digest(args.ocr)}:
        parser.error('OCR proof does not match the executable and current source')
    args.output.mkdir(parents=True, exist_ok=False)
    check = WorkspaceCheck(args)
    report = {'app': args.app, 'serial': args.serial, 'viewport': args.viewport,
              'ocr': proof, 'steps': check.steps, 'performance_claims': False}
    with device_lock(args.serial):
        report['installed_apk'] = check.device.installed_apk(check.package)
        try:
            check.run()
            report['passed'] = True
        except Exception as error:
            report['passed'] = False
            report['failure'] = str(error)
            raise
        finally:
            (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
            check.device.shell('am', 'force-stop', check.package)


if __name__ == '__main__':
    main()

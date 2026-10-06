#!/usr/bin/env python3
"""Check workspace frame-clock cadence against the physical display refresh.

The public footer reports coroutine frame callbacks, not presentations. Saved
SurfaceFlinger timestamps are separate evidence; a paused scene need not present
at the display rate. Run this with the same release APK before and after a fix.
"""

import argparse
import json
from pathlib import Path
import re
from statistics import median
import subprocess
import sys
import time

from android_workspace import ROOT, WorkspaceCheck, device_lock, digest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from measure import app_layer


def presented_rate(timestamps):
    presents = sorted({int(row.split()[1]) for row in timestamps.splitlines()[1:]
                       if len(row.split()) == 3 and row.split()[1].isdigit()
                       and 0 < int(row.split()[1]) < 2**63 - 1})
    if len(presents) < 2:
        return None
    return {'fps': (len(presents) - 1) * 1e9 / (presents[-1] - presents[0]),
            'first_present_ns': presents[0], 'last_present_ns': presents[-1],
            'present_count': len(presents)}


def sample(check, mode, index):
    capture_started = time.monotonic_ns()
    image, path = check.capture(f'{mode}-{index}')
    capture_finished = time.monotonic_ns()
    sf_started = time.monotonic_ns()
    sf = check.device.shell('dumpsys', 'SurfaceFlinger', '--latency',
                            app_layer(check.device, check.args.app))
    sf_finished = time.monotonic_ns()
    sf_path = path.with_suffix('.surfaceflinger.txt')
    sf_path.write_text(sf)
    footer = path.with_stem(path.stem + '-footer')
    image.crop((0, 792, 1280, 820)).resize((2560, 56)).save(footer)
    rows = json.loads(subprocess.check_output([str(check.args.ocr), str(path)], text=True))
    rows = [row for row in rows if row['bounds'][1] < 28 / 820]
    footer.with_suffix('.json').write_text(json.dumps(rows, indent=2) + '\n')
    text = ' '.join(row['text'] for row in rows)
    match = re.search(r'(\d+(?:\.\d+)?)\s+(?:UI|Ul|U1)\s+updates\s*/\s*s', text)
    if not match:
        raise ValueError('Footer cadence was not readable: ' + text)
    period = int(sf.splitlines()[0])
    if not 1_000_000 <= period <= 100_000_000:
        raise ValueError('Invalid display refresh period: ' + str(period))
    return image, {'ui_updates_per_second': float(match[1]), 'footer': text,
                   'refresh_period_ns': period, 'display_hz': 1e9 / period,
                   'screenshot': str(path), 'surfaceflinger': str(sf_path),
                   'presentation': presented_rate(sf),
                   'capture_started_monotonic_ns': capture_started,
                   'capture_finished_monotonic_ns': capture_finished,
                   'surfaceflinger_started_monotonic_ns': sf_started,
                   'surfaceflinger_finished_monotonic_ns': sf_finished}


def evaluate_mode(entry, tolerance):
    rates = [reading['ui_updates_per_second'] for reading in entry['samples']]
    limits = [reading['display_hz'] * (1 + tolerance) + 1
              for reading in entry['samples']]
    presentations = [reading['presentation']['fps'] for reading in entry['samples']
                     if reading['presentation'] is not None]
    entry['callback_median'] = median(rates)
    entry['presentation_median'] = median(presentations) if presentations else None
    entry['within_display_cadence'] = all(rate <= limit for rate, limit in zip(rates, limits))
    entry['keeps_up_with_presentations'] = entry['mode'] == 'paused' or (
        len(presentations) == len(rates)
        and entry['callback_median'] + 1 >= entry['presentation_median'] * (1 - tolerance))
    entry['passed'] = (entry['progress'] and entry['within_display_cadence']
                       and entry['keeps_up_with_presentations'])


def check_modes(check, report):
    for mode, still in [('paused', True), ('quotes', False), ('scroll', False), ('hover', False)]:
        check.launch('quotes' if mode == 'paused' else mode, still)
        time.sleep(check.args.warmup)
        entry = {'mode': mode, 'samples': []}
        report['modes'].append(entry)
        first = None
        changed = False
        for index in range(check.args.samples):
            image, reading = sample(check, mode, index)
            entry['samples'].append(reading)
            if first is None:
                first = image
            else:
                changed |= check.changed(first, image, 'watchlist' if mode in ('scroll', 'hover') else 'price')
            time.sleep(0.61)
        rates = [reading['ui_updates_per_second'] for reading in entry['samples']]
        entry['progress'] = all(rate > 0 for rate in rates) and (mode == 'paused' or changed)
        evaluate_mode(entry, check.args.tolerance)
        print(json.dumps(entry), flush=True)
        (check.args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--app', choices=['cranpose', 'compose'], default='cranpose')
    parser.add_argument('--apk', type=Path, required=True)
    parser.add_argument('--sha256', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--ocr', type=Path, required=True)
    parser.add_argument('--viewport', type=int, nargs=4, required=True)
    parser.add_argument('--warmup', type=float, default=4)
    parser.add_argument('--samples', type=int, default=3)
    parser.add_argument('--tolerance', type=float, default=0.15)
    args = parser.parse_args()
    if digest(args.apk) != args.sha256:
        parser.error('APK hash mismatch')
    proof = json.loads(args.ocr.with_suffix('.json').read_text())
    if proof != {'source_sha256': digest(ROOT / 'scripts/android/recognize_text.swift'),
                 'executable_sha256': digest(args.ocr)}:
        parser.error('OCR provenance mismatch')
    if args.samples < 3 or args.warmup < 2 or not 0 <= args.tolerance <= 0.2:
        parser.error('Use at least three samples, two seconds warmup and at most 20% tolerance')
    args.output.mkdir(parents=True, exist_ok=False)
    check = WorkspaceCheck(args)
    report = {'apk_sha256': args.sha256, 'serial': args.serial, 'app': args.app,
              'tolerance_fraction': args.tolerance, 'warmup_seconds': args.warmup,
              'modes': [], 'steps': check.steps, 'input_latency_claims': False}
    with device_lock(args.serial):
        try:
            check.device.configure_properties({})
            check.device.run('install', '-r', str(args.apk))
            report['installed_apk'] = check.device.installed_apk(check.package)
            if report['installed_apk']['sha256'] != args.sha256:
                raise ValueError('Installed APK hash mismatch')
            report['before'] = check.device.state()
            check_modes(check, report)
            report['passed'] = all(mode['passed'] for mode in report['modes'])
        except Exception as error:
            report['passed'] = False
            report['harness_failure'] = str(error)
            raise
        finally:
            check.device.shell('am', 'force-stop', check.package)
            check.device.restore_properties()
            report['after'] = check.device.state()
            (args.output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if not report['passed']:
        raise SystemExit('Workspace frame-clock cadence or visible progress failed')


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Measures every framework's app on one scenario, for the dashboard's
framework comparison.

Each app is launched once unmeasured, then measured in rounds, every app once
a round, the order reversed each round so heat and time fall on all alike.
Legs are `ab.py`'s: a warm-up, then a window long enough for `--min-frames`
frames, from 5 to 30 seconds. Writes `frameworks.json` in the dashboard's run
format, kind `frameworks`: device, subjects, every leg and per-app medians.
Usage: frameworks.py --serial SERIAL --output DIR [--apps compose,cranpose,...]
"""

import argparse
import json
import statistics
import time
from datetime import datetime, timezone
from pathlib import Path

from ab import DECIDING, REPORTED, leg_record, metric_values, prime, size_window
from measure import APPS, HEAVY, HERE, REMOTE_WINDOW, Device, device_lock, measure_run

DEFAULT_APPS = 'compose,cranpose,views,flutter,rn,maui,egui,slint,web'


def measure_frameworks(args):
    """Runs the whole comparison: the caller holds the device lock."""
    args.load, args.screenshots = 'heavy', False
    args.started = time.monotonic()
    args.windows = {}
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    apps = args.apps.split(',')
    device.adb('push', str(HERE / 'perf_window.sh'), REMOTE_WINDOW)
    for app in apps:
        prime(device, app, args.scenario, args)
    legs = []
    for round_index in range(args.rounds):
        for app in apps if round_index % 2 == 0 else apps[::-1]:
            run = measure_run(device, app, args.scenario, args, args.output,
                              args.windows.get((app, args.scenario)))
            size_window(args, app, args.scenario, run['fps'])
            legs.append(leg_record(run, app, len(legs)))
            print(f'{app:10} fps {run["fps"]:5.1f} cpu/f {run["cpu_ms_per_frame"]:6.1f} '
                  f'frames {run["frames"]:4d} window {run["window_s"]:4.1f} s', flush=True)
    summary = {
        app: {metric: statistics.median(values)
              for metric in [*DECIDING, *REPORTED]
              if (values := metric_values(legs, app, metric))}
        for app in apps
    }
    return {
        'kind': 'frameworks',
        'started_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'device': {key: device.shell('getprop', key).strip() for key in
                   ['ro.product.model', 'ro.build.version.release', 'ro.hardware']},
        'subjects': [{'name': app, 'package': APPS[app]['package'], 'label': app} for app in apps],
        'protocol': {'warmup_s': args.warmup, 'window_s': args.window, 'min_frames': args.min_frames,
                     'max_window_s': args.max_window, 'rounds': args.rounds},
        'scenarios': [{
            'scenario': args.scenario,
            'extras': (HEAVY.get(args.scenario, '') + ' ' + args.extra).strip(),
            'legs': legs,
            'summary': summary,
            'verdicts': {},
        }],
        'duration_s': round(time.monotonic() - args.started),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apps', default=DEFAULT_APPS)
    parser.add_argument('--scenario', default='gauntlet')
    parser.add_argument('--rounds', type=int, default=2)
    parser.add_argument('--warmup', type=float, default=2.0)
    parser.add_argument('--window', type=float, default=5.0)
    parser.add_argument('--min-frames', type=int, default=40)
    parser.add_argument('--max-window', type=float, default=30.0)
    parser.add_argument('--prime', type=float, default=3.0)
    parser.add_argument('--interval', type=float, default=0.5)
    parser.add_argument('--clock-ticks', type=int, default=100)
    parser.add_argument('--extra', default='', help='more `am start` extras')
    args = parser.parse_args()
    with device_lock(args.serial):
        run = measure_frameworks(args)
    (args.output / 'frameworks.json').write_text(json.dumps(run, indent=1))
    for app, values in run['scenarios'][0]['summary'].items():
        print(f'{app:10} fps {values["fps"]:5.1f} cpu/f {values["cpu_ms_per_frame"]:6.1f}')
    print('DURATION', run['duration_s'], 's')


if __name__ == '__main__':
    main()

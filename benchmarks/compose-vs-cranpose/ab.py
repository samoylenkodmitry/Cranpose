#!/usr/bin/env python3
"""Compares two installed builds scenario by scenario, as briefly as the
evidence allows.

Each scenario runs in pairs of legs, A B then B A, measured from outside the
app as `measure.py` measures (SurfaceFlinger frames, /proc CPU, memory). From
the second pair on, after every pair, each deciding metric is settled one of
two ways:
- same: the medians differ by under half the metric's threshold and neither
  build's own range is as wide as the threshold;
- different: the two builds' ranges do not overlap and their medians differ
  by at least the threshold times three with two legs a side, two with three,
  one with four or more. Separate ranges happen by chance a third of the time
  with two legs a side and a thirty-fifth with four, and launch-to-launch
  noise on the Huawei Mate 20 X is about 3% of a heavy scene's frame rate,
  so only a large difference settles early.

The scenario stops once every deciding metric is settled. After
`--max-pairs` pairs, a metric whose medians differ by under half the
threshold is the same whatever its spread; anything else still open is
inconclusive.
Before measuring, each build is launched once unmeasured so both start from a
written pipeline cache. A leg's window is 5 seconds, or longer for a build too
slow to draw `--min-frames` frames in that, up to `--max-window`: sized from
the unmeasured launch, then from the build's previous leg in the scenario.

Writes `ab.json` in the dashboard's run format: device, subjects, every leg,
per-subject medians and per-metric verdicts.
"""

import argparse
import json
import statistics
import subprocess
import time
from datetime import datetime, timezone
from pathlib import Path

from measure import (APPS, HEAVY, REMOTE_WINDOW, HERE, AppTarget, Device, device_lock, frame_rate,
                     measure_run, memory_mb)

# Metric: (threshold, whether the threshold is relative, whether more is better).
DECIDING = {
    'fps': (0.03, True, True),
    'cpu_ms_per_frame': (0.03, True, False),
    'desired_to_present_p50_ms': (2.0, False, False),
}
REPORTED = ['janky_pct', 'interval_p99_ms', 'cpu_mhz', 'gpu_mhz', 'ram_mb', 'gpu_ram_mb']


def metric_values(legs, subject, metric):
    values = [leg[metric] for leg in legs if leg['subject'] == subject and leg.get(metric) is not None]
    return values


def verdict(a_values, b_values, threshold, relative, higher_is_better, last):
    """'same', 'better' or 'worse' for B against A, or None while open."""
    legs = min(len(a_values), len(b_values))
    if legs < 2:
        return None
    a_median, b_median = statistics.median(a_values), statistics.median(b_values)
    scale = abs(a_median) if relative else 1.0
    if scale == 0.0:
        return None
    effect = (b_median - a_median) / scale
    spread = max(max(a_values) - min(a_values), max(b_values) - min(b_values)) / scale
    separated = max(a_values) < min(b_values) or max(b_values) < min(a_values)
    if abs(effect) < threshold / 2 and (spread < threshold or last):
        return 'same'
    if separated and abs(effect) >= threshold * max(1, 5 - legs):
        return 'better' if (effect > 0) == higher_is_better else 'worse'
    return None


def leg_record(run, subject, order):
    return {
        'subject': subject,
        'order': order,
        'fps': run['fps'],
        'frames': run['frames'],
        'window_s': run['window_s'],
        'janky_pct': run['janky_pct'],
        'interval_p99_ms': run['interval_p99_ms'],
        'cpu_ms_per_frame': run['cpu_ms_per_frame'],
        # The big cores run the UI and render threads.
        'cpu_mhz': run.get('cpu_big_mhz'),
        'gpu_mhz': run.get('gpu_mhz'),
        'desired_to_present_p50_ms': run.get('desired_to_present_p50_ms'),
        **memory_mb(run['memory']),
        'throttled': run['throttled'],
        'temperature_before': run['temperature_before'],
        'temperature_after': run['temperature_after'],
    }


def compare_scenario(device, scenario, subjects, args):
    legs, verdicts = [], {}
    for pair in range(args.max_pairs):
        order = subjects if pair % 2 == 0 else subjects[::-1]
        for subject in order:
            run = measure_run(device, subject, scenario, args, args.output, args.windows.get((subject, scenario)))
            size_window(args, subject, scenario, run['fps'])
            legs.append(leg_record(run, subject, len(legs)))
            print(f'{scenario:10} {subject:16} fps {run["fps"]:5.1f} cpu/f {run["cpu_ms_per_frame"]:5.2f} '
                  f'present {run.get("desired_to_present_p50_ms") or 0:5.1f}', flush=True)
        verdicts = {
            metric: verdict(metric_values(legs, subjects[0], metric),
                            metric_values(legs, subjects[1], metric), *rule,
                            pair == args.max_pairs - 1)
            for metric, rule in DECIDING.items()
        }
        if all(value is not None for value in verdicts.values()):
            break
    summary = {
        subject: {metric: statistics.median(values)
                  for metric in [*DECIDING, *REPORTED]
                  if (values := metric_values(legs, subject, metric))}
        for subject in subjects
    }
    return {
        'scenario': scenario,
        'extras': (HEAVY.get(scenario, '') + ' ' + args.extra).strip(),
        'legs': legs,
        'summary': summary,
        'verdicts': {metric: value or 'inconclusive' for metric, value in verdicts.items()},
    }


def size_window(args, subject, scenario, rate):
    """The window for the build's next leg in the scenario: long enough for
    `--min-frames` frames at `rate`, within `--window` and `--max-window`
    seconds."""
    wanted = args.min_frames / rate if rate > 0 else args.max_window
    args.windows[(subject, scenario)] = min(args.max_window, max(args.window, wanted))


def prime(device, subject, scenario, args):
    """Launches the build once unmeasured, and sizes its first window from the
    frame rate it reached. `subject` is an installed app's name, or the target
    of a page in the browser."""
    target = AppTarget(subject) if isinstance(subject, str) else subject
    target.launch(device, scenario, (HEAVY.get(scenario, '') + ' ' + args.extra).split())
    time.sleep(args.prime)
    size_window(args, target.name, scenario, frame_rate(device, target.layer(device)))
    target.stop(device)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--a', default='cranpose-release', choices=sorted(APPS))
    parser.add_argument('--b', default='cranpose', choices=sorted(APPS))
    parser.add_argument('--label-a', default='', help='version or commit of A, recorded with the run')
    parser.add_argument('--label-b', default='', help='version or commit of B, recorded with the run')
    parser.add_argument('--scenarios', default='gauntlet')
    parser.add_argument('--warmup', type=float, default=2.0)
    parser.add_argument('--window', type=float, default=5.0)
    parser.add_argument('--min-frames', type=int, default=40,
                        help='frames a window holds at least, when the build is slow enough to need longer')
    parser.add_argument('--max-window', type=float, default=30.0)
    parser.add_argument('--prime', type=float, default=3.0,
                        help='seconds of the unmeasured launch each build gets first')
    parser.add_argument('--max-pairs', type=int, default=4)
    parser.add_argument('--interval', type=float, default=0.5)
    parser.add_argument('--clock-ticks', type=int, default=100)
    parser.add_argument('--extra', default='', help='more `am start` extras')
    args = parser.parse_args()
    with device_lock(args.serial):
        compare_builds(args)


def compare_builds(args):
    """Runs the whole comparison: the caller holds the device lock."""
    args.load, args.screenshots = 'heavy', False
    args.windows = {}
    args.started = time.monotonic()
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    subjects = [args.a, args.b]
    device.adb('push', str(HERE / 'perf_window.sh'), REMOTE_WINDOW)
    scenarios = args.scenarios.split(',')
    for subject in subjects:
        prime(device, subject, scenarios[0], args)
    run = {
        'kind': 'ab',
        'started_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'device': {key: device.shell('getprop', key).strip() for key in
                   ['ro.product.model', 'ro.build.version.release', 'ro.hardware']},
        'subjects': [{'name': name, 'package': APPS[name]['package'], 'label': label}
                     for name, label in ((args.a, args.label_a), (args.b, args.label_b))],
        'protocol': {'warmup_s': args.warmup, 'window_s': args.window, 'min_frames': args.min_frames,
                     'max_window_s': args.max_window, 'max_pairs': args.max_pairs,
                     'deciding': {metric: rule[0] for metric, rule in DECIDING.items()}},
        'scenarios': [],
    }
    for scenario in scenarios:
        run['scenarios'].append(compare_scenario(device, scenario, subjects, args))
        (args.output / 'ab.json').write_text(json.dumps(run, indent=1))
    run['duration_s'] = round(time.monotonic() - args.started)
    (args.output / 'ab.json').write_text(json.dumps(run, indent=1))
    for scenario in run['scenarios']:
        print('VERDICT', scenario['scenario'], len(scenario['legs']), 'legs', json.dumps(scenario['verdicts']))
    print('DURATION', run['duration_s'], 's')
    subprocess.run(['adb', '-s', args.serial, 'shell', 'rm', '-f', REMOTE_WINDOW], check=False)


if __name__ == '__main__':
    main()

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
Every leg is one launch measured from its start command for `--run` seconds,
the same span for both builds. Nothing is left unmeasured: a build's first
launch after its install is a leg like the others, and each leg keeps the
frames of every second from its launch.

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

from measure import APPS, HEAVY, REMOTE_WINDOW, HERE, Device, device_lock, measure_run, memory_mb

# Metric: (threshold, whether the threshold is relative, whether more is better).
DECIDING = {
    'fps': (0.03, True, True),
    'cpu_ms_per_frame': (0.03, True, False),
    'desired_to_present_p50_ms': (2.0, False, False),
}
REPORTED = ['first_frame_s', 'janky_pct', 'interval_p99_ms', 'cpu_mhz', 'gpu_mhz', 'ram_mb', 'gpu_ram_mb']


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
        'run_s': run['run_s'],
        'first_frame_s': run['first_frame_s'],
        # When each frame was presented, in milliseconds since the launch.
        'frame_ms': run['frame_ms'],
        'fps_by_second': run['fps_by_second'],
        'first_poll_full': run['first_poll_full'],
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


def summarize(legs, subjects, metrics):
    """Per subject: the median of every metric over its legs."""
    return {
        subject: {metric: statistics.median(values) for metric in metrics
                  if (values := metric_values(legs, subject, metric))}
        for subject in subjects
    }


def compare_scenario(device, scenario, subjects, args):
    legs, verdicts = [], {}
    for pair in range(args.max_pairs):
        order = subjects if pair % 2 == 0 else subjects[::-1]
        for subject in order:
            run = measure_run(device, subject, scenario, args, args.output)
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
    return {
        'scenario': scenario,
        'extras': (HEAVY.get(scenario, '') + ' ' + args.extra).strip(),
        'legs': legs,
        'summary': summarize(legs, subjects, [*DECIDING, *REPORTED]),
        'verdicts': {metric: value or 'inconclusive' for metric, value in verdicts.items()},
    }


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
    parser.add_argument('--run', type=float, default=15.0,
                        help='seconds each leg is measured for, from its start command')
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
    args.started = time.monotonic()
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    subjects = [args.a, args.b]
    device.adb('push', str(HERE / 'perf_window.sh'), REMOTE_WINDOW)
    scenarios = args.scenarios.split(',')
    run = {
        'kind': 'ab',
        'started_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'device': {key: device.shell('getprop', key).strip() for key in
                   ['ro.product.model', 'ro.build.version.release', 'ro.hardware']},
        'subjects': [{'name': name, 'package': APPS[name]['package'], 'label': label}
                     for name, label in ((args.a, args.label_a), (args.b, args.label_b))],
        'protocol': {'run_s': args.run, 'from_launch': True, 'max_pairs': args.max_pairs,
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

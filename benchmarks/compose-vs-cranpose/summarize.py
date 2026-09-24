#!/usr/bin/env python3
"""Turns a `measure.py` report into Markdown tables: median of every run per app."""

import argparse
import json
import statistics
from collections import defaultdict
from pathlib import Path

METRICS = [
    ('fps', 'Presented FPS', '{:.1f}'),
    ('janky_pct', 'Frames > 1.5 vsync (%)', '{:.2f}'),
    ('slowest_second', 'Slowest second (frames)', '{:.0f}'),
    ('stalled_seconds', 'Seconds with no frame', '{:.0f}'),
    ('interval_p99_ms', 'p99 frame interval (ms)', '{:.1f}'),
    ('interval_max_ms', 'Worst frame interval (ms)', '{:.0f}'),
    ('cpu_pct', 'App CPU (% of one core)', '{:.0f}'),
    ('cpu_ms_per_frame', 'App CPU per frame (ms)', '{:.2f}'),
    ('gpu_mhz', 'Mean GPU clock (MHz)', '{:.0f}'),
    ('cpu_big_mhz', 'Mean big-core clock (MHz)', '{:.0f}'),
    ('queue_to_gpu_done_p50_ms', 'Buffer timestamp → GPU done p50 (ms)', '{:.1f}'),
    ('queue_to_present_p50_ms', 'Buffer timestamp → on screen p50 (ms)', '{:.1f}'),
    ('pss_mb', 'Memory PSS (MB)', '{:.0f}'),
    ('big_rise', 'Big-core cluster rise in window (°C)', '{:+.1f}'),
    ('big_end', 'Big-core cluster at window end (°C)', '{:.0f}'),
    ('gpu_rise', 'GPU rise in window (°C)', '{:+.1f}'),
    ('shell_end', 'Phone shell at window end (°C)', '{:.1f}'),
]


def thermal_fields(run):
    """Flattens the per-window thermal summary; runs without one read as NaN."""
    heat = run.get('thermal', {})
    nan = float('nan')

    def reading(name, key):
        return heat[name][key] if name in heat else nan

    run['big_rise'] = reading('cluster1', 'end') - reading('cluster1', 'start')
    run['big_end'] = reading('cluster1', 'end')
    run['gpu_rise'] = reading('gpu', 'end') - reading('gpu', 'start')
    run['shell_end'] = reading('shell_frame', 'end')
    per_second = run.get('fps_by_second') or [nan]
    run['slowest_second'] = min(per_second)
    run.setdefault('stalled_seconds', nan)


def median(values):
    return statistics.median(values) if values else float('nan')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    groups = defaultdict(list)
    for run in report['runs']:
        run['pss_mb'] = run['memory'].get('total_pss_kb', 0) / 1024
        thermal_fields(run)
        groups[(run['scenario'], run['app'])].append(run)
    scenarios = list(dict.fromkeys(run['scenario'] for run in report['runs']))
    lines = []
    for scenario in scenarios:
        cranpose, compose = groups[(scenario, 'cranpose')], groups[(scenario, 'compose')]
        lines += [f'### {scenario} ({len(cranpose)} Cranpose / {len(compose)} Compose runs)', '',
                  '| Metric | Cranpose | Compose |', '| --- | ---: | ---: |']
        for key, label, fmt in METRICS:
            values = [fmt.format(median([run[key] for run in runs])) for runs in (cranpose, compose)]
            lines.append(f'| {label} | {values[0]} | {values[1]} |')
        throttled = [sum(bool(run.get('throttled')) for run in runs) for runs in (cranpose, compose)]
        lines.append(f'| Throttled runs | {throttled[0]} of {len(cranpose)} | {throttled[1]} of {len(compose)} |')
        for app, runs in (('Cranpose', cranpose), ('Compose', compose)):
            threads = defaultdict(list)
            for run in runs:
                for thread in run['top_threads']:
                    threads[thread['name']].append(100 * thread['cpu_s'] / run['window_s'])
            busiest = sorted(threads.items(), key=lambda item: -median(item[1]))[:4]
            lines.append(f'| {app} busiest threads (% core) | ' + ', '.join(
                f'{name} {median(values):.0f}' for name, values in busiest) + ' | |')
        lines.append('')
    startup = report.get('startup', {})
    if any(startup.values()):
        lines += ['### Cold start (median)', '', '| Metric | Cranpose | Compose |', '| --- | ---: | ---: |']
        for key, label in (('TotalTime', '`am start -W` TotalTime (ms)'),
                           ('process_to_first_draw_ms', 'Process start → first content draw (ms)')):
            values = [median([run[key] for run in startup[app] if key in run]) for app in ('cranpose', 'compose')]
            lines.append(f'| {label} | {values[0]:.0f} | {values[1]:.0f} |')
        lines.append('')
    lines.append(f'APK size: Cranpose {report["cranpose_apk_bytes"] / 1e6:.1f} MB, '
                 f'Compose {report["compose_apk_bytes"] / 1e6:.2f} MB.')
    if report['failures']:
        lines.append(f'Failed runs: {len(report["failures"])} (kept in the report).')
    print('\n'.join(lines))


if __name__ == '__main__':
    main()

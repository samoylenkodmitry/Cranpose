#!/usr/bin/env python3
"""Measures the Cranpose and Jetpack Compose benchmark apps on one Android device.

Both apps are driven the same way (an intent extra picks the scenario, the app
animates itself from its frame clock) and measured the same way, from outside
either framework: SurfaceFlinger per-layer present timestamps (`--latency`) for frames,
/proc for process and thread CPU time, devfreq for GPU/DDR/CPU clocks, the
thermal service for temperatures, dumpsys meminfo for memory.

Runs alternate A B A B, then B A B A, with no cooling pauses: heat builds on
both apps, and temperature is recorded as a result in its own right.
"""

import argparse
import json
import re
import statistics
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
APPS = {
    'cranpose': {
        'package': 'dev.perfcompare.cranpose',
        'activity': 'dev.perfcompare.cranpose/dev.cranpose.android.CranposeActivity',
        'apk': HERE / 'cranpose-app/android/app/build/outputs/apk/release/app-release.apk',
    },
    'compose': {
        'package': 'dev.perfcompare.compose',
        'activity': 'dev.perfcompare.compose/.MainActivity',
        'apk': HERE / 'compose-app/app/build/outputs/apk/release/app-release.apk',
    },
}
SCENARIOS = ['feed', 'ticker', 'particles', 'layers', 'grid', 'grid_layer', 'deep', 'deep_layer']
REMOTE_WINDOW = '/data/local/tmp/perf_window.sh'
# Loads that keep Jetpack Compose itself below 60 fps on the Huawei Mate 20 X:
# a test both frameworks pass at 60 fps cannot tell them apart.
HEAVY = {
    'feed': '--ef speed 12000 --ei fcols 4 --ef ui 0.3',
    'ticker': '--ei quotes 72',
    'particles': '--ei particles 20000',
    'layers': '--ei tcols 12 --ei trows 22',
    'grid': '--ei rows 30 --ei cols 12',
    'grid_layer': '--ei rows 30 --ei cols 12',
    'deep': '--ei depth 40 --ei chips 6',
    'deep_layer': '--ei depth 40 --ei chips 6',
}


class Device:
    def __init__(self, serial):
        self.serial = serial

    def adb(self, *args, timeout=120, check=True):
        result = subprocess.run(['adb', '-s', self.serial, *args], capture_output=True,
                                text=True, timeout=timeout)
        if check and result.returncode != 0:
            raise RuntimeError(f'adb {" ".join(args)} failed: {result.stderr.strip()}')
        return result.stdout

    def shell(self, *args, timeout=120, check=True):
        return self.adb('shell', *args, timeout=timeout, check=check)

    def pid(self, package):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            out = self.shell('pidof', package, check=False).strip()
            if out:
                return int(out.split()[0])
            time.sleep(0.1)
        raise RuntimeError(f'{package} did not start')

    def temperatures(self):
        return thermal_reading(self.shell('dumpsys', 'thermalservice'))


# Hardware clock ceilings of the Kirin 980 domains the window samples, in the
# units sysfs reports: GPU in Hz, CPU clusters in kHz.
HARDWARE_MAX = {'gpu': 720_000_000, 'cpu_little': 1_805_000, 'cpu_mid': 1_920_000, 'cpu_big': 2_600_000}


def thermal_reading(text):
    """Live HAL temperatures (°C) and cooling-device levels from `dumpsys thermalservice`.

    The service's "Cached temperatures" section can be minutes old on this
    Huawei, so only the "Current ... from HAL" sections are read.
    """
    current = text.split('Current temperatures from HAL', 1)[-1]
    temperatures, _, cooling = current.partition('Current cooling devices from HAL')
    reading = {name: float(value) for value, name in
               re.findall(r'mValue=([\d.]+), mType=\d+, mName=(\w+)', temperatures)}
    reading.update({'cooling_' + name: int(value) for value, name in
                    re.findall(r'mValue=(\d+), mType=\d+, mName=(\w+)', cooling)})
    return reading


def thermal_series(output):
    return [thermal_reading(block.split('THERMAL_END')[0])
            for block in output.split('THERMAL_BEGIN ')[1:]]


def thermal_summary(series):
    """Start, end and peak of every sensor across the window's samples."""
    summary = {}
    for name in series[0]:
        values = [sample[name] for sample in series if name in sample]
        summary[name] = {'start': values[0], 'end': values[-1], 'max': max(values)}
    return summary


def parse_stat(line):
    # "pid (comm) state ..." -- comm may contain spaces and parentheses.
    head, _, rest = line.rpartition(')')
    fields = rest.split()
    return head.split('(', 1)[1], int(fields[11]) + int(fields[12])


def parse_snap(lines):
    """Reads the STAT/TASK block at the start of `lines`, stopping at its end."""
    process = None
    threads = {}
    for line in lines:
        if not line.startswith(('STAT ', 'TASK ')):
            break
        if line.startswith('STAT '):
            process = parse_stat(line[5:])[1]
        elif line.startswith('TASK ') and len(line) > 5:
            tid = line[5:].split()[0]
            name, ticks = parse_stat(line[5:])
            threads[tid] = (name, ticks)
    return process, threads


PENDING = 9223372036854775807
VSYNC_MS = 1000.0 / 60.0


def app_layer(device, package):
    """The app window's buffer layer: `package/activity#N`, without a handle prefix."""
    names = [line.strip() for line in device.shell('dumpsys', 'SurfaceFlinger', '--list').splitlines()]
    layers = [name for name in names if name.startswith(package + '/') and ' ' not in name]
    if len(layers) != 1:
        raise ValueError(f'expected one buffer layer for {package}: {layers}')
    return layers[0]


def presents(output):
    """Merges every latency poll into unique frames and the clock calibration."""
    frames = {}
    calibration = []
    for block in output.split('LAT_BEGIN ')[1:]:
        header, _, body = block.partition('\n')
        uptime = float(header.split()[0])
        newest = 0
        for line in body.split('LAT_END')[0].splitlines()[1:]:
            fields = line.split()
            if len(fields) != 3:
                continue
            desired, actual, ready = map(int, fields)
            if actual in (0, PENDING):
                continue
            frames[actual] = (desired, ready)
            newest = max(newest, actual)
        if newest:
            calibration.append(uptime - newest / 1e9)
    return frames, calibration


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(fraction * len(ordered)))] if ordered else None


def frame_stats(frames, t0, t1):
    times = sorted(time for time in frames if t0 * 1e9 <= time <= t1 * 1e9)
    if len(times) < 2:
        raise ValueError('the layer presented no frames in the window')
    intervals = [(later - earlier) / 1e6 for earlier, later in zip(times, times[1:])]
    queue_to_present = [(time - frames[time][0]) / 1e6 for time in times]
    queue_to_ready = [(frames[time][1] - frames[time][0]) / 1e6 for time in times]
    per_second = [0] * int(t1 - t0)
    for time in times:
        second = int(time / 1e9 - t0)
        if second < len(per_second):
            per_second[second] += 1
    return {
        # Frames presented in each whole second of the window: a ramp or a
        # stall that the window mean hides shows up here.
        'fps_by_second': per_second,
        # Whole seconds without a single present: a stall, or content that
        # stopped changing, which would make the window mean meaningless.
        'stalled_seconds': sum(count == 0 for count in per_second),
        'frames': len(times),
        'fps': len(times) / (t1 - t0),
        'interval_p50_ms': percentile(intervals, 0.50),
        'interval_p90_ms': percentile(intervals, 0.90),
        'interval_p99_ms': percentile(intervals, 0.99),
        'interval_max_ms': max(intervals),
        # A present more than 1.5 vsyncs after the previous one repeated a frame.
        'janky_pct': 100.0 * sum(interval > 1.5 * VSYNC_MS for interval in intervals) / len(intervals),
        'missed_vsyncs': sum(max(0, round(interval / VSYNC_MS) - 1) for interval in intervals),
        'queue_to_present_p50_ms': percentile(queue_to_present, 0.50),
        'queue_to_gpu_done_p50_ms': percentile(queue_to_ready, 0.50),
    }


def gfxinfo_summary(text):
    keys = ['Total frames rendered', 'Janky frames', '50th percentile', '90th percentile',
            '95th percentile', '99th percentile']
    summary = {}
    for key in keys:
        match = re.search(re.escape(key) + r': ([^\n]+)', text)
        if match:
            summary[key] = match[1].strip()
    return summary


def meminfo(device, package):
    text = device.shell('dumpsys', 'meminfo', package)
    result = {}
    total = re.search(r'TOTAL PSS:\s+(\d+)', text) or re.search(r'\n\s+TOTAL\s+(\d+)', text)
    if total:
        result['total_pss_kb'] = int(total[1])
    for row in ['Native Heap', 'Dalvik Heap', 'Graphics', 'GL mtrack', 'EGL mtrack', 'Code']:
        match = re.search(r'\n\s+' + re.escape(row) + r'\s+(\d+)', text)
        if match:
            result[row] = int(match[1])
    return result


def launch(device, app, scenario, extra=()):
    device.shell('am', 'force-stop', APPS['cranpose']['package'])
    device.shell('am', 'force-stop', APPS['compose']['package'])
    time.sleep(1.0)
    device.shell('input', 'keyevent', 'KEYCODE_WAKEUP')
    device.adb('logcat', '-c')
    out = device.shell('am', 'start', '-W', '-n', APPS[app]['activity'], '--es', 'scenario', scenario,
                       *extra)
    if 'Status: ok' not in out:
        raise RuntimeError('launch failed: ' + out)
    times = {key: int(value) for key, value in re.findall(r'(TotalTime|WaitTime): (\d+)', out)}
    return times, device.pid(APPS[app]['package'])


def measure_run(device, app, scenario, args, destination):
    package = APPS[app]['package']
    run = {'app': app, 'scenario': scenario, 'temperature_before': device.temperatures()}
    extras = (HEAVY.get(scenario, '') if args.load == 'heavy' else '') + ' ' + args.extra
    run['extras'] = extras.strip()
    run['launch'], pid = launch(device, app, scenario, extras.split())
    run['started_s'] = round(time.monotonic() - args.started, 1)
    time.sleep(args.warmup)
    layer = app_layer(device, package)
    samples = int(args.window / args.interval)
    output = device.shell('sh', REMOTE_WINDOW, str(pid), package, layer, str(samples),
                          str(args.interval), '1' if app == 'compose' else '0',
                          timeout=args.window + 60)
    (destination / f'{app}-{scenario}-{int(time.time())}.txt').write_text(output)
    lines = output.splitlines()
    if any(line.strip() == 'STAT' for line in lines):
        raise RuntimeError(f'{package} exited during the window')
    t0 = float(next(line for line in lines if line.startswith('T0 ')).split()[1])
    t1 = float(next(line for line in lines if line.startswith('T1 ')).split()[1])
    t0_index = next(i for i, line in enumerate(lines) if line.startswith('T0 '))
    t1_index = next(i for i, line in enumerate(lines) if line.startswith('T1 '))
    first_proc, first_threads = parse_snap(lines[t0_index + 1:])
    last_proc, last_threads = parse_snap(lines[t1_index + 1:])
    if device.shell('pidof', package, check=False).strip() != str(pid):
        raise RuntimeError(f'{package} restarted or died during the window')
    elapsed = t1 - t0
    frames, calibration = presents(output)
    # /proc/uptime counts suspended time and SurfaceFlinger's monotonic clock
    # does not. A poll's newest present can only precede the poll, so the
    # smallest gap is the clock offset to within about one frame.
    offset = min(calibration)
    stats = frame_stats(frames, t0 - offset, t1 - offset)
    freqs = [list(map(int, line.split()[1:])) for line in lines if line.startswith('F ')]
    ticks_per_s = args.clock_ticks
    cpu_s = (last_proc - first_proc) / ticks_per_s
    threads = []
    for tid, (name, ticks) in last_threads.items():
        before = first_threads.get(tid, (name, 0))[1]
        threads.append({'name': name, 'cpu_s': (ticks - before) / ticks_per_s})
    threads.sort(key=lambda thread: -thread['cpu_s'])
    run.update(
        pid=pid,
        window_s=elapsed,
        layer=layer,
        clock_offset_s=offset,
        clock_offset_spread_s=max(calibration) - offset,
        **stats,
        cpu_pct=100.0 * cpu_s / elapsed,
        cpu_ms_per_frame=1000.0 * cpu_s / stats['frames'],
        top_threads=threads[:6],
        gpu_mhz=statistics.mean(f[0] for f in freqs) / 1e6,
        ddr_mhz=statistics.mean(f[1] for f in freqs) / 1e6,
        cpu_little_mhz=statistics.mean(f[2] for f in freqs) / 1e3,
        cpu_mid_mhz=statistics.mean(f[3] for f in freqs) / 1e3,
        cpu_big_mhz=statistics.mean(f[4] for f in freqs) / 1e3,
        # Lowest cap seen, as a share of the domain's hardware maximum; below
        # 100 means thermal management throttled that domain in the window.
        cap_pct={domain: 100.0 * min(f[5 + index] for f in freqs) / HARDWARE_MAX[domain]
                 for index, domain in enumerate(HARDWARE_MAX)},
        thermal=thermal_summary(thermal_series(output)),
        memory=meminfo(device, package),
    )
    run['throttled'] = (min(run['cap_pct'].values()) < 100.0
                        or any(value['max'] > 0 for name, value in run['thermal'].items()
                               if name.startswith('cooling_')))
    if app == 'compose':
        run['gfxinfo'] = gfxinfo_summary(output.split('GFX_BEGIN', 1)[1].split('GFX_END', 1)[0])
    log = device.adb('logcat', '-d', '-s', 'PerfCompare:I')
    run['app_log'] = [line for line in log.splitlines() if 'PERF' in line]
    if args.screenshots:
        shot = destination / f'{app}-{scenario}.png'
        if not shot.exists():
            with open(shot, 'wb') as handle:
                subprocess.run(['adb', '-s', device.serial, 'exec-out', 'screencap', '-p'],
                               stdout=handle, check=True, timeout=30)
    run['temperature_after'] = device.temperatures()
    device.shell('am', 'force-stop', package)
    return run


def measure_startup(device, app, destination):
    times, _ = launch(device, app, 'feed')
    time.sleep(2.0)
    log = device.adb('logcat', '-d', '-v', 'epoch')
    start = re.search(r'^\s*(\d+\.\d+).*Start proc \d+:' + re.escape(APPS[app]['package']), log, re.M)
    first = re.search(r'^\s*(\d+\.\d+).*PERF first_frame', log, re.M)
    result = dict(times)
    if start and first:
        result['process_to_first_draw_ms'] = round((float(first[1]) - float(start[1])) * 1000)
    device.shell('am', 'force-stop', APPS[app]['package'])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serial', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--reps', type=int, default=2,
                        help='blocks per scenario, alternating ABAB and BABA')
    parser.add_argument('--apps', default='cranpose,compose', help='A,B order of the first block')
    parser.add_argument('--scenarios', default=','.join(SCENARIOS))
    parser.add_argument('--warmup', type=float, default=5.0)
    parser.add_argument('--window', type=float, default=15.0)
    parser.add_argument('--interval', type=float, default=0.5)
    parser.add_argument('--startup-runs', type=int, default=5)
    parser.add_argument('--clock-ticks', type=int, default=100)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--install', action='store_true')
    parser.add_argument('--extra', default='', help='more `am start` extras, e.g. "--ez opaque true"')
    parser.add_argument('--load', choices=['heavy', 'default'], default='heavy',
                        help='heavy: the per-scenario loads in HEAVY; default: the apps\' own sizes')
    args = parser.parse_args()
    args.started = time.monotonic()
    args.output.mkdir(parents=True, exist_ok=True)
    device = Device(args.serial)
    report = {'device': {key: device.shell('getprop', key).strip() for key in
                         ['ro.product.model', 'ro.build.version.release', 'ro.hardware']},
              'runs': [], 'startup': {'cranpose': [], 'compose': []}, 'failures': []}
    for app, spec in APPS.items():
        report[app + '_apk_bytes'] = spec['apk'].stat().st_size
        if args.install:
            device.adb('install', '-r', '-d', str(spec['apk']), timeout=300)
    debuggable = device.shell('dumpsys', 'package', APPS['compose']['package'])
    if 'DEBUGGABLE' in debuggable.split('pkgFlags=')[-1].split('\n')[0]:
        raise RuntimeError('the Compose APK is debuggable; measure a release build')
    report['compose_dexopt'] = device.shell(
        'cmd', 'package', 'compile', '-m', 'speed', '-f', APPS['compose']['package']).strip()
    device.adb('push', str(HERE / 'perf_window.sh'), REMOTE_WINDOW)
    scenarios = args.scenarios.split(',')
    apps = args.apps.split(',')
    for block in range(args.reps):
        for scenario in scenarios:
            # ABAB, then BABA: no cooling pauses, so heat builds up on both apps.
            order = apps if block % 2 == 0 else apps[::-1]
            for app in order * 2:
                try:
                    run = measure_run(device, app, scenario, args, args.output)
                    run['block'] = block
                    report['runs'].append(run)
                    heat = run['thermal']
                    print(f'{scenario:10} {app:8} fps={run["fps"]:5.1f} jank={run["janky_pct"]:4.1f}% '
                          f'stalled={run["stalled_seconds"]}s '
                          f'p99={run["interval_p99_ms"]:.1f}ms '
                          f'cpu={run["cpu_pct"]:5.1f}% gpu={run["gpu_mhz"]:.0f}MHz '
                          f'big={heat["cluster1"]["start"]:.0f}->{heat["cluster1"]["end"]:.0f}C '
                          f'gpuT={heat["gpu"]["start"]:.0f}->{heat["gpu"]["end"]:.0f}C '
                          f'throttled={run["throttled"]}', flush=True)
                except Exception as error:  # retain the failure, keep measuring
                    report['failures'].append({'app': app, 'scenario': scenario, 'error': str(error)})
                    print(f'{scenario:9} {app:8} FAILED {error}', flush=True)
                (args.output / 'report.json').write_text(json.dumps(report, indent=1))
    for index in range(args.startup_runs):
        for app in ['cranpose', 'compose'] if index % 2 == 0 else ['compose', 'cranpose']:
            report['startup'][app].append(measure_startup(device, app, args.output))
    (args.output / 'report.json').write_text(json.dumps(report, indent=1))
    print(json.dumps(report['startup']))


if __name__ == '__main__':
    main()

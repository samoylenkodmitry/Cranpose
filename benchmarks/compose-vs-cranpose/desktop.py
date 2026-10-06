#!/usr/bin/env python3
"""Measures the gauntlet's desktop apps on a Mac, from outside every app.

Each app runs alone in a 1280 x 820 window (`perf-data`'s `DESKTOP_WINDOW`),
with the tier in `PERF_TIER`, the frame to freeze on in `PERF_FREEZE` and the
Roboto files in `PERF_FONTS`; Cranpose takes them as `--tier=N` arguments and
the web page in its address. `FrameCount.app` counts the frames the app's
window presents, the way SurfaceFlinger counts a phone app's, and `ps`
counts the CPU time of the app and every process it started. The apps are
measured in turn, round after round, as `frameworks.py` measures phones, and
the run is a `frameworks` run `scripts/perf/publish.py` publishes. A leg in
which other processes spent more than `--max-others` cores is measured again.

  python3 desktop.py --output results/desktop --tier 12
  python3 desktop.py --output results/desktop-parity --parity --tier 5

`--parity` freezes every app on frame 120, takes a picture of each window
with `FrameCount.app` and compares each with the first app's picture, as
`parity.py` compares phone captures.

Everything an app reads or writes, the fonts and Chrome's profile, sits in a
temporary folder: macOS asks the user before an app it started reads files on
a removable volume, and the checkout may be on one.
"""

import argparse
import http.server
import json
import os
import shutil
import signal
import socketserver
import statistics
import subprocess
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

from PIL import Image

from parity import compare_pair

HERE = Path(__file__).resolve().parent
FONTS = HERE / 'fonts'
FRAMECOUNT = Path.home() / 'Applications/FrameCount.app'
CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# `perf-data`'s `DESKTOP_WINDOW`, in points.
DESKTOP_WINDOW = (1280, 820)
# The window's title bar at the display's 2x scale, which pictures leave out.
TITLE_BAR_PIXELS = 64

# Each app's command: `{tier}`, `{freeze}`, `{page}` and `{profile}` are
# filled in for the run.
APPS = {
    'cranpose': [HERE / 'cranpose-app/target/release/perf-compare',
                 '--scenario=gauntlet', '--tier={tier}', '--freeze={freeze}'],
    'egui': [HERE / 'egui-app/target/release/perf-compare-egui'],
    'slint': [HERE / 'slint-app/target/release/perf-compare-slint'],
    'iced': [HERE / 'iced-app/target/release/perf-compare-iced'],
    'gpui': [HERE / 'gpui-app/target/release/perf-compare-gpui'],
    'avalonia': [HERE / 'avalonia-app/bin/Release/net10.0/osx-arm64/publish/PerfAvalonia'],
    'swiftui': [HERE / 'swiftui-app/build/PerfSwiftUI.app/Contents/MacOS/PerfSwiftUI'],
    'flutter': [HERE / 'flutter-app/build/macos/Build/Products/Release/perf_flutter.app/Contents/MacOS/perf_flutter'],
    'compose': [HERE / 'compose-desktop-app/build/compose/binaries/main/app/PerfCompose.app/Contents/MacOS/PerfCompose'],
    # Chrome, the engine Electron apps ship, in an app window of its own
    # profile. Its title bar is part of the window size it is given.
    'web': [CHROME, '--user-data-dir={profile}', '--no-first-run', '--no-default-browser-check',
            '--enable-logging=stderr', '--window-size=1280,852',
            '--app={page}?tier={tier}&freeze={freeze}'],
}


class Page(http.server.SimpleHTTPRequestHandler):
    """Serves the web page, and the Roboto files under `fonts/` beside it."""

    def translate_path(self, path):
        if path.startswith('/fonts/'):
            return str(FONTS / Path(path).name)
        return str(HERE / 'web-app/www' / path.split('?')[0].lstrip('/'))

    def log_message(self, *_):
        pass


def serve_page():
    """Serves the web page on a free local port for the whole run."""
    server = socketserver.ThreadingTCPServer(('127.0.0.1', 0), Page)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return f'http://127.0.0.1:{server.server_address[1]}/index.html'


def cpu_seconds(root):
    """CPU time of `root` and every process below it, and of all processes."""
    table = subprocess.run(['ps', '-A', '-o', 'pid=,ppid=,time='], capture_output=True, text=True,
                           check=True).stdout
    children, times = {}, {}
    for line in table.splitlines():
        pid, ppid, clock = line.split()
        children.setdefault(int(ppid), []).append(int(pid))
        seconds = 0.0
        for part in clock.split(':'):
            seconds = seconds * 60 + float(part)
        times[int(pid)] = seconds
    total, pending = 0.0, [root]
    while pending:
        pid = pending.pop()
        total += times.get(pid, 0.0)
        pending.extend(children.get(pid, []))
    return total, sum(times.values())


def wait_for(log, text, timeout):
    """Waits until the app's output holds `text`."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if log.exists() and text in log.read_text(errors='replace'):
            return
        time.sleep(0.1)
    raise RuntimeError(f'{log.name}: no "{text}" within {timeout} s')


def framecount(pid, out, *arguments):
    """Runs `FrameCount.app` on the process's window, through `open` so its
    Screen Recording permission applies."""
    subprocess.run(['open', '-W', '-n', str(FRAMECOUNT), '--args', '--pid', str(pid), *arguments],
                   check=True, timeout=120)
    if out.suffix == '.json':
        result = json.loads(out.read_text())
        if 'error' in result:
            raise RuntimeError(f'FrameCount: {result["error"]}')
        return result
    if not out.exists():
        raise RuntimeError(f'FrameCount wrote no {out.name}')
    return out


class App:
    """One app running in its window until `stop`. An app in a bundle starts
    through `open`, as Launch Services starts apps: the JVM, for one, opens
    no window from outside the login session, and an app it starts comes to
    the front."""

    def __init__(self, name, tier, freeze, work, page, stage):
        self.name = name
        self.work = work
        self.log = stage / f'{name}.log'
        profile = stage / 'chrome-profile'
        values = {'tier': tier, 'freeze': freeze, 'page': page, 'profile': profile}
        command = [str(part).format(**values) for part in APPS[name]]
        variables = {'PERF_TIER': str(tier), 'PERF_FREEZE': str(freeze), 'PERF_FONTS': str(stage / 'fonts')}
        executable, arguments = command[0], command[1:]
        self.log.write_text('')
        if '.app/Contents/MacOS/' not in executable:
            with self.log.open('w') as output:
                self.process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                                env=dict(os.environ, **variables), start_new_session=True)
            self.pid = self.process.pid
            return
        bundle = executable.split('.app/Contents/MacOS/')[0] + '.app'
        environment = [part for key, value in variables.items() for part in ('--env', f'{key}={value}')]
        self.process = subprocess.Popen(['open', '-n', '-W', *environment, '--stdout', str(self.log),
                                         '--stderr', str(self.log), bundle, '--args', *arguments])
        # Chrome's helpers share its path's start, and the user's own Chrome
        # its path: this run's Chrome is the oldest process of its profile.
        chrome = executable == CHROME
        pattern = f'--user-data-dir={profile}' if chrome else executable
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            found = subprocess.run(['pgrep', '-o' if chrome else '-n', '-f', '--', pattern], capture_output=True,
                                   text=True).stdout.split()
            if found:
                self.pid = int(found[0])
                return
            time.sleep(0.1)
        raise RuntimeError(f'{name}: no process for {pattern}')

    def stop(self):
        """Ends the app and keeps its output beside the results."""
        self.end()
        shutil.copy(self.log, self.work / self.log.name)

    def end(self):
        if '-W' not in self.process.args:
            target, kill = self.process.pid, os.killpg
        else:
            target, kill = self.pid, os.kill
        for sent in (signal.SIGTERM, signal.SIGKILL):
            try:
                kill(target, sent)
                self.process.wait(timeout=5)
                return
            except ProcessLookupError:
                self.process.wait(timeout=5)
                return
            except subprocess.TimeoutExpired:
                continue


def measure(name, args, work, page, stage):
    """One window of `args.seconds`: frames presented and CPU spent."""
    app = App(name, args.tier, 0, work, page, stage)
    try:
        wait_for(app.log, 'PERF first_frame', args.timeout)
        time.sleep(args.warmup)
        (app_before, all_before), wall_before = cpu_seconds(app.pid), time.monotonic()
        out = stage / f'{name}-frames.json'
        frames = framecount(app.pid, out, '--seconds', str(args.seconds), '--out', str(out))
        (app_after, all_after), wall_after = cpu_seconds(app.pid), time.monotonic()
        shutil.copy(out, work / out.name)
    finally:
        app.stop()
    # FrameCount starts and stops around its window, so CPU counts as a rate
    # over the whole span and divides by the frame rate. What every other
    # process spent in the span, FrameCount's capture included, tells a run
    # that something else disturbed.
    wall = wall_after - wall_before
    cores = (app_after - app_before) / wall
    others = (all_after - all_before) / wall - cores
    fps = frames['fps']
    return {'fps': round(fps, 1), 'frames': frames['frames'],
            'interval_p50_ms': round(frames['interval_p50_ms'], 2),
            'interval_p99_ms': round(frames['interval_p99_ms'], 2),
            'cpu_cores': round(cores, 2), 'other_cores': round(others, 2),
            'cpu_ms_per_frame': round(cores * 1000 / fps, 2) if fps else None}


def picture(name, args, work, page, stage):
    """The window frozen on frame `args.freeze`."""
    app = App(name, args.tier, args.freeze, work, page, stage)
    try:
        wait_for(app.log, 'PERF frozen', args.timeout)
        time.sleep(0.5)
        out = stage / f'{name}.png'
        framecount(app.pid, out, '--screenshot', str(out))
        shutil.copy(out, work / out.name)
        return Image.open(out).convert('RGB')
    finally:
        app.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apps', default=','.join(APPS))
    parser.add_argument('--tier', type=int, default=12)
    parser.add_argument('--rounds', type=int, default=3)
    parser.add_argument('--seconds', type=float, default=5.0)
    parser.add_argument('--warmup', type=float, default=2.0)
    parser.add_argument('--timeout', type=float, default=60.0)
    parser.add_argument('--max-others', type=float, default=1.5,
                        help='cores other processes may spend in a leg before it is measured again')
    parser.add_argument('--parity', action='store_true')
    parser.add_argument('--freeze', type=int, default=120)
    parser.add_argument('--shift', type=int, default=8)
    parser.add_argument('--drift', type=int, default=160)
    parser.add_argument('--tile', type=int, default=48)
    parser.add_argument('--tile-delta', type=float, default=12.0)
    args = parser.parse_args()
    apps = args.apps.split(',')
    args.output.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    page = serve_page()
    with tempfile.TemporaryDirectory(prefix='gauntlet-') as folder:
        stage = Path(folder)
        shutil.copytree(FONTS, stage / 'fonts')
        report = run(args, apps, page, stage)
    report['duration_s'] = round(time.monotonic() - started)
    (args.output / 'report.json').write_text(json.dumps(report, indent=1))
    print(json.dumps(report['scenarios'][0]['summary'] if 'scenarios' in report else report['changed_pct']))


def run(args, apps, page, stage):
    """The pictures and their comparison, or the measured rounds."""
    if args.parity:
        pictures = {name: picture(name, args, args.output, page, stage) for name in apps}
        reference, results = apps[0], {}
        for name in apps[1:]:
            (_, changed_a, first, second, heat_a), (_, changed_b, _, _, heat_b) = compare_pair(
                pictures[name], pictures[reference], TITLE_BAR_PIXELS, args.shift, args.tile,
                args.tile_delta, args.drift)
            side = Image.new('RGB', (first.width * 4, first.height))
            for index, image in enumerate((first, second, heat_a, heat_b)):
                side.paste(image.convert('RGB'), (first.width * index, 0))
            side.save(args.output / f'{name}-side-by-side.png')
            results[name] = round(max(changed_a, changed_b), 3)
            print(f'{name:10} {results[name]:6.2f}% of tiles changed against {reference}', flush=True)
        return {'kind': 'desktop-parity', 'tier': args.tier, 'freeze': args.freeze,
                'reference': reference, 'changed_pct': results}
    started_at = datetime.now(timezone.utc).isoformat(timespec='seconds')
    legs = []
    for round_index in range(args.rounds):
        for name in apps:
            for _ in range(3):
                leg = measure(name, args, args.output, page, stage)
                print(f'round {round_index + 1} {name:10} fps {leg["fps"]:6.1f} '
                      f'cpu/f {leg["cpu_ms_per_frame"]} p99 {leg["interval_p99_ms"]} '
                      f'others {leg["other_cores"]}', flush=True)
                if leg['other_cores'] <= args.max_others:
                    break
            legs.append({'subject': name, 'round': round_index, **leg})
    summary = {name: {key: round(statistics.median(leg[key] or 0 for leg in legs if leg['subject'] == name), 2)
                      for key in ('fps', 'cpu_ms_per_frame', 'cpu_cores', 'other_cores')}
               for name in apps}
    chip = subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], capture_output=True, text=True,
                          check=True).stdout.strip()
    width, height = DESKTOP_WINDOW
    return {
        'kind': 'frameworks',
        'started_at': started_at,
        'device': {'ro.product.model': chip},
        'subjects': [{'name': name, 'label': name} for name in apps],
        'protocol': {'warmup_s': args.warmup, 'window_s': args.seconds, 'rounds': args.rounds,
                     'max_other_cores': args.max_others},
        'scenarios': [{'scenario': 'gauntlet', 'extras': f'tier {args.tier}, {width} x {height} window',
                       'legs': legs, 'summary': summary, 'verdicts': {}}],
    }


if __name__ == '__main__':
    main()

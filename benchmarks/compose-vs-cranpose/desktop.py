#!/usr/bin/env python3
"""Measures the gauntlet's desktop apps on a Mac, from outside every app.

Each app runs alone in a 1280 x 820 window (`perf-data`'s `DESKTOP_WINDOW`),
with the tier in `PERF_TIER`, the frame to freeze on in `PERF_FREEZE` and the
Roboto files in `PERF_FONTS`; Cranpose takes them as `--tier=N` arguments and
the web page in its address. `FrameCount.app` counts the frames the app's
window presents, the way SurfaceFlinger counts a phone app's, `ps` counts
the CPU time of the app and every process it started, `footprint` the memory
they hold and `macmon` the clocks the chip ran at. The apps are measured in
turn, round after round, as `frameworks.py` measures phones, and the run is a
`frameworks` run `scripts/perf/publish.py` publishes. A leg that fails, or in
which other processes spent more than `--max-others` cores, is measured again,
up to three times; an app no attempt measured leaves its leg out. A leg
whose frame rate is more than 1.5 times off the app's earlier legs is measured
once more, with a picture of its window kept beside the results. Each window
is as short as the app's frame rate allows: `--min-frames` frames, within
`--window` and `--max-window` seconds.

  python3 desktop.py --output results/desktop --tier 12
  python3 desktop.py --output results/desktop-parity --parity --tier 5

`--parity` freezes every app on frame 120, takes a picture of each window
with `FrameCount.app` and compares each with Compose's picture, as
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

import versions


HERE = Path(__file__).resolve().parent
FONTS = HERE / 'fonts'
FRAMECOUNT = Path.home() / 'Applications/FrameCount.app'
MACMON = shutil.which('macmon')
CLOCK_INTERVAL_MS = 250
CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# `perf-data`'s `DESKTOP_WINDOW`, in points.
DESKTOP_WINDOW = (1280, 820)
# The window's title bar at the display's 2x scale, which pictures leave out.
TITLE_BAR_PIXELS = 64

# A leg whose frame rate is this many times off the median of the app's
# earlier legs was disturbed or drew something else: the app is measured once
# more, so the median of the three decides, and the window's picture is kept.
DISAGREEMENT = 1.5

# What each app's legs are summarized by.
SUMMARY = ('fps', 'cpu_ms_per_frame', 'cpu_cores', 'server_cores', 'other_cores', 'ram_mb', 'gpu_ram_mb', 'cpu_mhz',
           'gpu_mhz')

# Each app's command: `{tier}`, `{freeze}`, `{page}` and `{profile}` are
# filled in for the run.
APPS = {
    'cranpose': [HERE / 'cranpose-app/target/release/perf-compare',
                 '--scenario=gauntlet', '--tier={tier}', '--freeze={freeze}'],
    # The latest release's, which `build_apps.sh release TAG` links in.
    'cranpose-release': [HERE / 'cranpose-app/target-release/release/perf-compare',
                         '--scenario=gauntlet', '--tier={tier}', '--freeze={freeze}'],
    'egui': [HERE / 'egui-app/target/release/perf-compare-egui'],
    'slint': [HERE / 'slint-app/target/release/perf-compare-slint'],
    'iced': [HERE / 'iced-app/target/release/perf-compare-iced'],
    'gpui': [HERE / 'gpui-app/target/release/perf-compare-gpui'],
    'avalonia': [HERE / 'avalonia-app/bin/Release/net10.0/osx-arm64/publish/PerfAvalonia'],
    'swiftui': [HERE / 'swiftui-app/build/PerfSwiftUI.app/Contents/MacOS/PerfSwiftUI'],
    'appkit': [HERE / 'appkit-app/build/PerfAppKit.app/Contents/MacOS/PerfAppKit'],
    'flutter': [HERE / 'flutter-app/build/macos/Build/Products/Release/perf_flutter.app/Contents/MacOS/perf_flutter'],
    'compose': [HERE / 'compose-desktop-app/build/compose/binaries/main/app/PerfCompose.app/Contents/MacOS/PerfCompose'],
    # Chrome, the engine Electron apps ship, in an app window of its own
    # profile. Its title bar is part of the window size it is given.
    'web': [CHROME, '--user-data-dir={profile}', '--no-first-run', '--no-default-browser-check',
            '--enable-logging=stderr', '--window-size=1280,852',
            '--app={page}?tier={tier}&freeze={freeze}'],
    # The same page in Tauri, on the system's WKWebView.
    'tauri': [HERE / 'tauri-app/target/release/perf-compare-tauri', '--page={page}'],
    'dioxus': [HERE / 'dioxus-app/target/release/perf-compare-dioxus'],
    'freya': [HERE / 'freya-app/target/release/perf-compare-freya'],
    'xilem': [HERE / 'xilem-app/target/release/perf-compare-xilem'],
    'fyne': [HERE / 'fyne-app/build/perf-compare-fyne'],
    'uno': [HERE / 'uno-app/bin/Release/net10.0-desktop/osx-arm64/publish/PerfUno'],
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


# The services a WKWebView runs its page in. launchd starts them, outside the
# app's process tree, for the app that asked.
WEBKIT_SERVICE = 'com.apple.WebKit.'
# The window server composites every window, and renders the layer trees
# SwiftUI, AppKit and WebKit hand it; a Metal app hands it one surface.
WINDOW_SERVER = '/WindowServer'


def seconds(clock):
    """`ps` time, `[[days-]hours:]minutes:seconds`, in seconds."""
    days, _, rest = clock.rpartition('-')
    total = 0.0
    for part in rest.split(':'):
        total = total * 60 + float(part)
    return total + (int(days) * 86_400 if days else 0)


def processes():
    """Every process: its parent, its CPU seconds, how long ago it started and
    its command."""
    table = subprocess.run(['ps', '-A', '-o', 'pid=,ppid=,time=,etime=,comm='], capture_output=True,
                           text=True, check=True).stdout
    found = {}
    for line in table.splitlines():
        pid, ppid, clock, elapsed, command = line.split(None, 4)
        found[int(pid)] = (int(ppid), seconds(clock), seconds(elapsed), command)
    return found


def tree(root, table, age):
    """`root`, every process below it, and the WebKit services started in the
    `age` seconds since the app started, which do a WKWebView's work."""
    children = {}
    for pid, (ppid, *_) in table.items():
        children.setdefault(ppid, []).append(pid)
    services = [pid for pid, (_, _, elapsed, command) in table.items()
                if WEBKIT_SERVICE in command and elapsed <= age]
    found, pending = set(), [root, *services]
    while pending:
        pid = pending.pop()
        if pid not in found:
            found.add(pid)
            pending.extend(children.get(pid, []))
    return found


MB = 1024 * 1024


def memory(pids, stage):
    """What the app's processes hold in memory, as macOS counts a process's
    footprint, and the part of it that is the GPU's: Metal's buffers and
    textures (`IOAccelerator`) and the surfaces the window server composites
    (`IOSurface`)."""
    out = stage / 'footprint.json'
    targets = [part for pid in sorted(pids) for part in ('-p', str(pid))]
    subprocess.run(['footprint', '-f', 'bytes', '-j', str(out), *targets], capture_output=True,
                   check=True, timeout=60)
    report = json.loads(out.read_text())
    graphics = sum(values['dirty'] + values['swapped'] for category, values in report['summary'].items()
                   if category.startswith(('IOAccelerator', 'IOSurface')) or '(graphics)' in category)
    return {'ram_mb': round(report['total footprint'] / MB, 1), 'gpu_ram_mb': round(graphics / MB, 1)}


class Clocks:
    """The performance cores' and the GPU's clocks over a window, as `macmon`
    reads them from the chip's own counters without root. Without `macmon`
    the run has no clocks."""

    def __init__(self):
        self.process = subprocess.Popen(
            [MACMON, 'pipe', '-i', str(CLOCK_INTERVAL_MS)], stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True) if MACMON else None

    def stop(self):
        if not self.process:
            return {}
        self.process.terminate()
        output, _ = self.process.communicate(timeout=10)
        # Stopped, `macmon` may leave its last line unfinished: only the lines
        # it ended are samples.
        samples = [json.loads(line) for line in output.split('\n')[:-1] if line.startswith('{')]
        if not samples:
            return {}
        return {'cpu_mhz': round(statistics.mean(sample['pcpu_freq_mhz'] for sample in samples)),
                'gpu_mhz': round(statistics.mean(sample['gpu_freq_mhz'] for sample in samples))}


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

    def __init__(self, name, tier, freeze, work, page, stage, label=None):
        self.name = name
        self.work = work
        self.log = stage / f'{label or name}.log'
        # A profile for each launch: helpers of an earlier launch's Chrome may
        # outlive it, and must not pass for this one's.
        profile = stage / f'chrome-profile-{label or name}'
        values = {'tier': tier, 'freeze': freeze, 'page': page, 'profile': profile}
        command = [str(part).format(**values) for part in APPS[name]]
        variables = {'PERF_TIER': str(tier), 'PERF_FREEZE': str(freeze), 'PERF_FONTS': str(stage / 'fonts')}
        executable, arguments = command[0], command[1:]
        self.log.write_text('')
        self.started = time.monotonic()
        self.profile = None
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
        # its path: this launch's Chrome is the oldest process of its profile.
        chrome = executable == CHROME
        self.profile = f'--user-data-dir={profile}' if chrome else None
        pattern = self.profile or executable
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
        """Ends the app: its process group, or the app `open` started and,
        for Chrome, every process of its profile."""
        group = '-W' not in self.process.args
        for sent in (signal.SIGTERM, signal.SIGKILL):
            try:
                if group:
                    os.killpg(self.process.pid, sent)
                else:
                    os.kill(self.pid, sent)
            except (ProcessLookupError, PermissionError):
                pass  # Gone already, or no longer ours to signal.
            if self.profile:
                subprocess.run(['pkill', f'-{sent.name.removeprefix("SIG")}', '-f', '--', self.profile],
                               capture_output=True)
            try:
                self.process.wait(timeout=5)
                return
            except subprocess.TimeoutExpired:
                continue
        # `open -W` still waits on an app that did not end: stop waiting.
        self.process.kill()
        self.process.wait(timeout=5)


def disagrees(fps, earlier):
    """Whether `fps` is more than `DISAGREEMENT` times off the median of the
    app's earlier legs' rates."""
    if not earlier:
        return False
    reference = statistics.median(earlier)
    return max(fps, reference) > DISAGREEMENT * min(fps, reference)


def measure(name, args, work, page, stage, label, earlier):
    """One window: frames presented, CPU spent, the clocks the chip ran at and
    the memory the app held at the window's end. The window is long enough for
    `--min-frames` frames at the app's last rate, within `--window` and
    `--max-window` seconds. Its frames, the app's output and, for a rate that
    disagrees with the app's `earlier` legs, the window's picture are kept
    under `label`."""
    window = args.windows.get(name, args.window)
    app = App(name, args.tier, 0, work, page, stage, label)
    try:
        wait_for(app.log, 'PERF first_frame', args.timeout)
        time.sleep(args.warmup)
        clocks = Clocks()
        before, wall_before = processes(), time.monotonic()
        out = stage / f'{label}-frames.json'
        frames = framecount(app.pid, out, '--seconds', f'{window:.1f}', '--out', str(out))
        after, wall_after = processes(), time.monotonic()
        clocked = clocks.stop()
        mine = tree(app.pid, after, wall_after - app.started + 1)
        held = memory(mine, stage)
        shutil.copy(out, work / out.name)
        if disagrees(frames['fps'], earlier):
            shown = stage / f'{label}.png'
            framecount(app.pid, shown, '--screenshot', str(shown))
            shutil.copy(shown, work / shown.name)
    finally:
        app.stop()
    # FrameCount starts and stops around its window, so CPU counts as a rate
    # over the whole span and divides by the frame rate. What every other
    # process spent in the span, FrameCount's capture included, tells a run
    # that something else disturbed; a process that ended within the span
    # counts for neither.
    wall = wall_after - wall_before
    spent = {pid: cpu - before.get(pid, (0, 0.0))[1] for pid, (_, cpu, _, _) in after.items()}
    server = {pid for pid, (_, _, _, command) in after.items() if command.endswith(WINDOW_SERVER)}
    server_cores = sum(spent[pid] for pid in server) / wall
    # The window server's work counts toward the app in front: its layers
    # are what the window server renders.
    cores = sum(value for pid, value in spent.items() if pid in mine) / wall + server_cores
    others = sum(value for pid, value in spent.items() if pid not in mine and pid not in server) / wall
    fps = frames['fps']
    wanted = args.min_frames / fps if fps > 0 else args.max_window
    args.windows[name] = min(args.max_window, max(args.window, wanted))
    return {'fps': round(fps, 1), 'frames': frames['frames'], 'window_s': round(window, 1),
            'interval_p50_ms': round(frames['interval_p50_ms'], 2),
            'interval_p99_ms': round(frames['interval_p99_ms'], 2),
            'cpu_cores': round(cores, 2), 'server_cores': round(server_cores, 2), 'other_cores': round(others, 2),
            'cpu_ms_per_frame': round(cores * 1000 / fps, 2) if fps else None,
            **held, **clocked}


def picture(name, args, work, page, stage):
    """The window frozen on frame `args.freeze`."""
    app = App(name, args.tier, args.freeze, work, page, stage)
    try:
        wait_for(app.log, 'PERF frozen', args.timeout)
        time.sleep(0.5)
        out = stage / f'{name}.png'
        framecount(app.pid, out, '--screenshot', str(out))
        shutil.copy(out, work / out.name)
        from PIL import Image  # pictures alone need Pillow
        return Image.open(out).convert('RGB')
    finally:
        app.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apps', help='the apps to run; by default every app that is built')
    parser.add_argument('--release', help='the release tag `cranpose-release` was built at')
    parser.add_argument('--tier', type=int, default=12)
    parser.add_argument('--rounds', type=int, default=2)
    parser.add_argument('--warmup', type=float, default=1.0)
    parser.add_argument('--window', type=float, default=3.0, help='the shortest window, in seconds')
    parser.add_argument('--max-window', type=float, default=8.0, help='the longest window, in seconds')
    parser.add_argument('--min-frames', type=int, default=40,
                        help='frames a window should hold at the app\'s last rate')
    parser.add_argument('--timeout', type=float, default=60.0)
    parser.add_argument('--main', help='the commit the Cranpose app was built at, recorded with the run')
    parser.add_argument('--max-others', type=float, default=1.5,
                        help='cores other processes may spend in a leg before it is measured again')
    parser.add_argument('--parity', action='store_true')
    parser.add_argument('--freeze', type=int, default=120)
    parser.add_argument('--shift', type=int, default=8)
    parser.add_argument('--drift', type=int, default=160)
    parser.add_argument('--tile', type=int, default=48)
    parser.add_argument('--tile-delta', type=float, default=12.0)
    args = parser.parse_args()
    if args.apps:
        apps = args.apps.split(',')
    else:
        apps = [name for name in APPS if name == 'web' or Path(APPS[name][0]).exists()]
        print('not built:', ', '.join(name for name in APPS if name not in apps) or 'none', flush=True)
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


def undisturbed_leg(name, args, page, stage, label, earlier):
    """A leg, measured again, up to three times, while other processes spend
    more than `--max-others` cores in it or it fails; none when every attempt
    failed, so one app cannot end the run."""
    leg = None
    for attempt in range(3):
        try:
            leg = measure(name, args, args.output, page, stage, f'{label}-{attempt + 1}', earlier)
        except (RuntimeError, subprocess.SubprocessError) as failure:
            print(f'{label:16} failed: {failure}', flush=True)
            continue
        print(f'{label:16} fps {leg["fps"]:6.1f} cpu/f {leg["cpu_ms_per_frame"]} '
              f'p99 {leg["interval_p99_ms"]} others {leg["other_cores"]}', flush=True)
        if leg['other_cores'] <= args.max_others:
            break
    return leg


def run(args, apps, page, stage):
    """The pictures and their comparison, or the measured rounds."""
    if args.parity:
        # Pillow, which comparing pictures needs and measuring does not.
        from PIL import Image
        from parity import compare_pair
        pictures = {name: picture(name, args, args.output, page, stage) for name in apps}
        # Compose's picture is the one the others are held to.
        reference = 'compose' if 'compose' in apps else apps[0]
        results = {}
        for name in (name for name in apps if name != reference):
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
    # The versions `build_apps.sh desktop` built with, the Flutter SDK's
    # among them.
    built_file = HERE / 'desktop-versions.json'
    built = json.loads(built_file.read_text()) if built_file.exists() else {}
    legs = []
    args.windows = {}
    for round_index in range(args.rounds):
        for name in apps:
            earlier = [leg['fps'] for leg in legs if leg['subject'] == name]
            label = f'{name}-{round_index + 1}'
            leg = undisturbed_leg(name, args, page, stage, label, earlier)
            if leg is None:
                continue
            legs.append({'subject': name, 'round': round_index, **leg})
            if disagrees(leg['fps'], earlier):
                print(f'{name}: {leg["fps"]} fps disagrees with its earlier legs; measuring once more', flush=True)
                again = undisturbed_leg(name, args, page, stage, f'{label}-again', earlier)
                if again is not None:
                    legs.append({'subject': name, 'round': round_index, **again})
    summary = {name: {key: round(statistics.median(values), 2)
                      for key in SUMMARY
                      if (values := [leg[key] for leg in legs if leg['subject'] == name and leg.get(key) is not None])}
               for name in apps}
    chip = subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], capture_output=True, text=True,
                          check=True).stdout.strip()
    width, height = DESKTOP_WINDOW
    return {
        'kind': 'frameworks',
        'started_at': started_at,
        'main': args.main,
        'device': {'ro.product.model': chip},
        'subjects': [versions.subject(name, 'desktop', built, args.release) for name in apps],
        'protocol': {'warmup_s': args.warmup, 'window_s': args.window, 'max_window_s': args.max_window,
                     'min_frames': args.min_frames, 'rounds': args.rounds, 'max_other_cores': args.max_others},
        'scenarios': [{'scenario': 'gauntlet', 'extras': f'tier {args.tier}, {width} x {height} window',
                       'legs': legs, 'summary': summary, 'verdicts': {}}],
    }


if __name__ == '__main__':
    main()

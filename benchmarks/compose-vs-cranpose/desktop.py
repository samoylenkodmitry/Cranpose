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
`frameworks` run `scripts/perf/publish.py` publishes. In a round each app
starts twice: first after what the apps keep between launches is removed, as
an install leaves them (`APP_STATE`; in the browser, a new Chrome profile),
then a second time. A leg that fails, or in
which other processes spent more than `--max-others` cores, is measured again,
up to three times; an app no attempt measured leaves its leg out. A leg
whose frame rate is more than 1.5 times off the app's earlier legs is measured
once more, with a picture of its window kept beside the results. Every leg
is one launch measured from its start for `--run` seconds, the same span for
every app, with nothing left out: the launch, the first frame and the first
seconds count as the rest of the run does, and the frames of every second
are kept. A leg whose app presented fewer than `SLOW_FPS` frames a second
over the last `SLOW_WINDOW_S` seconds ends there, marked `stopped`. A
disturbed leg stays in the run, marked, out of the medians.

  python3 desktop.py --output results/desktop --tier 12
  python3 desktop.py --output results/desktop-parity --parity --tier 5
  python3 desktop.py --output results/browser --browser DIST --tier 6

`--browser DIST` runs the apps `build_apps.sh browser DIST` built for the
browser, each in a Chrome window of a profile of its own for the round, at the
browser's own tier.

`--parity` freezes every app on frame 120, takes a picture of each window
with `FrameCount.app` and compares each with Compose's picture, as
`parity.py` compares phone captures.

Everything an app reads or writes, the fonts and Chrome's profile, sits in a
temporary folder: macOS asks the user before an app it started reads files on
a removable volume, and the checkout may be on one.
"""

import argparse
import ctypes
import hashlib
import http.server
import json
import os
import shutil
import signal
import socketserver
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

import versions
from browser import BrowserServer
from timelines import SLOW_FPS, SLOW_WINDOW_S, STARTS, per_second, slow_rule


HERE = Path(__file__).resolve().parent
FONTS = HERE / 'fonts'
FRAMECOUNT = Path.home() / 'Applications/FrameCount.app'
FRAMECOUNT_SOURCE = HERE / 'framecount/FrameCount.swift'
MACMON = shutil.which('macmon')
CLOCK_INTERVAL_MS = 250
CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# `perf-data`'s `DESKTOP_WINDOW`, in points.
DESKTOP_WINDOW = (1280, 820)
# The window's title bar at the display's 2x scale, which pictures leave out.
TITLE_BAR_PIXELS = 64

# What the apps keep between launches, found by listing the files each wrote
# in a launch on macm3: compiled Metal shaders, in the cache unbundled apps
# share and in each bundle's own; the WebKit caches of the web view apps;
# Chrome's compiled Metal shaders (its profile, with its GPU and HTTP caches,
# is the round's own); and the Cranpose app's pipeline cache. Removed before
# every first start.
USER_CACHE = Path(subprocess.run(['getconf', 'DARWIN_USER_CACHE_DIR'], capture_output=True,
                                 text=True).stdout.strip() or tempfile.gettempdir())
LIBRARY = Path.home() / 'Library'
APP_STATE = [
    (USER_CACHE, 'com.apple.metal'),
    (USER_CACHE, 'dev.perfcompare.*'),
    (USER_CACHE, 'com.apple.WebKit.*+perf_compare_*'),
    (USER_CACHE, 'com.google.Chrome*'),
    (LIBRARY / 'Caches', 'perf-compare*'),
    (LIBRARY / 'Application Support', 'perf-compare*'),
    (LIBRARY / 'WebKit', 'perf-compare*'),
]


def clear_app_state(profile):
    """Removes what the apps keep between launches, and Chrome's `profile`,
    so the next launch is an app's first after its install."""
    for root, pattern in APP_STATE:
        for path in root.glob(pattern):
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path, ignore_errors=True)
            else:
                path.unlink(missing_ok=True)
    shutil.rmtree(profile, ignore_errors=True)


# A leg whose frame rate is this many times off the median of the app's
# earlier legs was disturbed or drew something else: the app is measured once
# more, so the median of the three decides, and the window's picture is kept.
DISAGREEMENT = 1.5

# What each app's legs are summarized by.
SUMMARY = ('fps', 'first_frame_s', 'cpu_ms_per_frame', 'cpu_cores', 'server_cores', 'other_cores', 'ram_mb', 'gpu_ram_mb', 'cpu_mhz',
           'gpu_mhz')

# Chrome, the engine Electron apps ship, in an app window of its own profile.
# Its title bar is part of the window size it is given.
CHROME_APP = [CHROME, '--user-data-dir={profile}', '--no-first-run', '--no-default-browser-check',
              '--enable-logging=stderr', '--window-size=1280,852', '--app={page}?tier={tier}&freeze={freeze}']

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
    'web': CHROME_APP,
    # The same page in Tauri, on the system's WKWebView.
    'tauri': [HERE / 'tauri-app/target/release/perf-compare-tauri', '--page={page}'],
    'dioxus': [HERE / 'dioxus-app/target/release/perf-compare-dioxus'],
    'xilem': [HERE / 'xilem-app/target/release/perf-compare-xilem'],
    'fyne': [HERE / 'fyne-app/build/perf-compare-fyne'],
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
        self.started = time.monotonic()
        self.process = subprocess.Popen(
            [MACMON, 'pipe', '-i', str(CLOCK_INTERVAL_MS)], stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True) if MACMON else None

    def stop(self):
        """The mean clocks, and each sample's clocks by the monotonic time it
        ended at: one every `CLOCK_INTERVAL_MS` from the start."""
        if not self.process:
            return {}, []
        self.process.terminate()
        output, _ = self.process.communicate(timeout=10)
        # Stopped, `macmon` may leave its last line unfinished: only the lines
        # it ended are samples.
        samples = [json.loads(line) for line in output.split('\n')[:-1] if line.startswith('{')]
        if not samples:
            return {}, []
        timed = [(self.started + (index + 1) * CLOCK_INTERVAL_MS / 1000, sample['pcpu_freq_mhz'],
                  sample['gpu_freq_mhz']) for index, sample in enumerate(samples)]
        return {'cpu_mhz': round(statistics.mean(sample['pcpu_freq_mhz'] for sample in samples)),
                'gpu_mhz': round(statistics.mean(sample['gpu_freq_mhz'] for sample in samples))}, timed


# macOS only: the harness measures there, and its tests also run on Linux.
LIBPROC = ctypes.CDLL('/usr/lib/libproc.dylib') if sys.platform == 'darwin' else None
# `proc_pid_rusage`'s first flavour: CPU times in Mach ticks and the footprint.
RUSAGE_INFO_V0 = 0


class RusageInfoV0(ctypes.Structure):
    _fields_ = [('uuid', ctypes.c_uint8 * 16), ('user_time', ctypes.c_uint64), ('system_time', ctypes.c_uint64),
                ('pkg_idle_wkups', ctypes.c_uint64), ('interrupt_wkups', ctypes.c_uint64),
                ('pageins', ctypes.c_uint64), ('wired_size', ctypes.c_uint64), ('resident_size', ctypes.c_uint64),
                ('phys_footprint', ctypes.c_uint64), ('proc_start_abstime', ctypes.c_uint64),
                ('proc_exit_abstime', ctypes.c_uint64)]


class MachTimebase(ctypes.Structure):
    _fields_ = [('numerator', ctypes.c_uint32), ('denominator', ctypes.c_uint32)]


TIMEBASE = MachTimebase()
if sys.platform == 'darwin':
    ctypes.CDLL(None).mach_timebase_info(ctypes.byref(TIMEBASE))
# Seconds a Mach tick lasts; nothing reads it off macOS.
TICK_S = TIMEBASE.numerator / max(TIMEBASE.denominator, 1) / 1e9
# How often `Usage` reads the app's processes.
USAGE_INTERVAL_S = 0.5


def child_pids(pid):
    """The processes `pid` started, through libproc: no process is started."""
    buffer = (ctypes.c_int * 1024)()
    count = LIBPROC.proc_listchildpids(pid, buffer, ctypes.sizeof(buffer))
    return list(buffer[:max(count, 0)])


def webkit_services():
    """Every WebKit service process, by its name, through libproc."""
    buffer = (ctypes.c_int * 8192)()
    name = ctypes.create_string_buffer(256)
    found = set()
    for pid in buffer[:max(LIBPROC.proc_listallpids(buffer, ctypes.sizeof(buffer)), 0)]:
        if LIBPROC.proc_name(pid, name, ctypes.sizeof(name)) > 0 and \
                name.value.decode(errors='replace').startswith(WEBKIT_SERVICE):
            found.add(pid)
    return found


def own_usage(pid):
    """The CPU seconds a process of this user spent and the footprint it holds,
    or None for one that ended."""
    info = RusageInfoV0()
    if LIBPROC.proc_pid_rusage(pid, RUSAGE_INFO_V0, ctypes.byref(info)) != 0:
        return None
    return (info.user_time + info.system_time) * TICK_S, info.phys_footprint


class Usage:
    """The app's processes every `USAGE_INTERVAL_S` of its run: the CPU they
    and the window server spent, and the footprint they hold. The app's are
    read through libproc; the window server's, which runs as another user,
    through one `ps` of it. A sample costs about 5 ms of one core on a Mac
    with 2,200 processes, which counts toward the other processes, never the
    app's: `ps -A` there takes 110 ms."""

    def __init__(self, root, server, launched):
        self.root = root
        self.server = server
        # The WebKit services that ran before the launch are not the app's.
        self.earlier_services = webkit_services()
        # At the launch the app's processes had spent nothing.
        self.taken = [(launched, self.server_cpu(), 0)]
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)
        self.thread.start()

    def pids(self):
        found, pending = set(), [self.root, *(webkit_services() - self.earlier_services)]
        while pending:
            pid = pending.pop()
            if pid not in found:
                found.add(pid)
                pending.extend(child_pids(pid))
        return found

    def server_cpu(self):
        if not self.server:
            return 0.0
        spent = subprocess.run(['ps', '-o', 'time=', '-p', str(self.server)], capture_output=True,
                               text=True).stdout.strip()
        return seconds(spent) if spent else 0.0

    def run(self):
        while not self.stopped.wait(USAGE_INTERVAL_S):
            usage = [found for pid in self.pids() if (found := own_usage(pid))]
            self.taken.append((time.monotonic(), sum(cpu for cpu, _ in usage) + self.server_cpu(),
                               sum(footprint for _, footprint in usage)))

    def stop(self):
        self.stopped.set()
        self.thread.join(timeout=5)
        return self.taken


def run_samples(taken, clocks, launched, times):
    """The run at each of `Usage`'s samples since the launch: the cores the app
    and the window server kept busy, their CPU per frame presented in the
    interval, the app's footprint, and the clocks `macmon` read last. The
    first interval starts at the launch, before the app spent anything."""
    samples = {'t_ms': [], 'cores': [], 'cpu_ms_per_frame': [], 'ram_mb': [], 'cpu_mhz': [], 'gpu_mhz': []}
    for (earlier, spent_before, _), (later, spent, footprint) in zip(taken, taken[1:]):
        span = later - earlier
        if span <= 0:
            continue
        # A process that ended takes its CPU with it.
        cpu = max(0.0, spent - spent_before)
        shown = sum(1 for time in times if earlier < time <= later)
        clock = next((sample for sample in reversed(clocks) if sample[0] <= later), None)
        samples['t_ms'].append(round((later - launched) * 1000))
        samples['cores'].append(round(cpu / span, 3))
        samples['cpu_ms_per_frame'].append(round(cpu * 1000 / shown, 2) if shown else None)
        samples['ram_mb'].append(round(footprint / MB, 1))
        samples['cpu_mhz'].append(round(clock[1]) if clock else None)
        samples['gpu_mhz'].append(round(clock[2]) if clock else None)
    return samples


def wait_for(log, text, timeout):
    """Waits until the app's output holds `text`."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if log.exists() and text in log.read_text(errors='replace'):
            return
        time.sleep(0.1)
    raise RuntimeError(f'{log.name}: no "{text}" within {timeout} s')


def current_framecount():
    """Builds `FrameCount.app` again when its source changed since the
    installed build: its signature keeps the Screen Recording grant."""
    built = FRAMECOUNT / 'Contents/Resources/source-sha256'
    source = hashlib.sha256(FRAMECOUNT_SOURCE.read_bytes()).hexdigest()
    if not built.exists() or built.read_text().strip() != source:
        subprocess.run(['sh', str(HERE / 'framecount/build.sh'), str(FRAMECOUNT.parent)], check=True)


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

    def __init__(self, name, args, freeze, work, page, stage, label=None, profile=None):
        tier = args.tier
        self.name = name
        self.work = work
        self.log = stage / f'{label or name}.log'
        profile = profile or stage / f'chrome-profile-{label or name}'
        # In the browser every app is the same Chrome, opened on its own page.
        template, page = (CHROME_APP, f'{page}/{name}/index.html') if args.browser else (APPS[name], page)
        values = {'tier': tier, 'freeze': freeze, 'page': page, 'profile': profile}
        command = [str(part).format(**values) for part in template]
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
        # Every process of an earlier launch on the profile has ended: `end`
        # waits for them.
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
        for Chrome, every process of its profile, which the round's next
        launch must not find."""
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
                break
            except subprocess.TimeoutExpired:
                continue
        else:
            # `open -W` still waits on an app that did not end: stop waiting.
            self.process.kill()
            self.process.wait(timeout=5)
        deadline = time.monotonic() + 10
        while self.profile and time.monotonic() < deadline and subprocess.run(
                ['pgrep', '-f', '--', self.profile], capture_output=True).returncode == 0:
            time.sleep(0.1)


def disagrees(fps, earlier):
    """Whether `fps` is more than `DISAGREEMENT` times off the median of the
    app's earlier legs' rates."""
    if not earlier:
        return False
    reference = statistics.median(earlier)
    return max(fps, reference) > DISAGREEMENT * min(fps, reference)


def measure(name, args, work, page, stage, label, earlier, profile):
    """One launch, measured from its start for `--run` seconds: every frame
    its window presented in each second, the CPU the app and every process it
    started spent, the clocks the chip ran at and the memory the app held at
    the end. Nothing is left out: the launch, the first frame and the first
    seconds count as the rest of the run does. The frames, the app's output
    and, for a rate that disagrees with the app's `earlier` legs, the window's
    picture are kept under `label`. Chrome runs on `profile`."""
    # Every process's CPU before the launch: the app's processes start at
    # zero, so what they hold at the end is the run's.
    before = processes()
    launched = time.monotonic()
    app = App(name, args, 0, work, page, stage, label, profile)
    try:
        clocks = Clocks()
        server_pid = next((pid for pid, (_, _, _, command) in before.items() if command.endswith(WINDOW_SERVER)),
                          None)
        usage = Usage(app.pid, server_pid, launched)
        out = stage / f'{label}-frames.json'
        frames = framecount(app.pid, out, '--until', f'{launched + args.run:.3f}', '--launched', f'{launched:.3f}',
                            '--slow-fps', str(SLOW_FPS), '--slow-window', str(SLOW_WINDOW_S), '--out', str(out))
        after, ended = processes(), time.monotonic()
        taken = usage.stop()
        clocked, clock_samples = clocks.stop()
        # A slow run ends where FrameCount stopped it.
        run_s = frames['stopped'] - launched if 'stopped' in frames else args.run
        if 'PERF first_frame' not in app.log.read_text(errors='replace'):
            raise RuntimeError(f'{name} logged no first frame within its {run_s:.0f} s run')
        mine = tree(app.pid, after, ended - app.started + 1)
        held = memory(mine, stage)
        shutil.copy(out, work / out.name)
        times = [time for time in frames['times'] if launched <= time < launched + run_s]
        if len(times) < 2:
            raise RuntimeError(f'{name} presented fewer than two frames in its run')
        fps = len(times) / run_s
        if disagrees(fps, earlier):
            shown = stage / f'{label}.png'
            framecount(app.pid, shown, '--screenshot', str(shown))
            shutil.copy(shown, work / shown.name)
    finally:
        app.stop()
    # What every other process spent in the run, FrameCount's capture
    # included, tells a run that something else disturbed; a process that
    # ended within the run counts for neither.
    wall = ended - launched
    spent = {pid: cpu - before.get(pid, (0, 0.0))[1] for pid, (_, cpu, _, _) in after.items()}
    server = {pid for pid, (_, _, _, command) in after.items() if command.endswith(WINDOW_SERVER)}
    server_cores = sum(spent[pid] for pid in server) / wall
    # The window server's work counts toward the app in front: its layers
    # are what the window server renders.
    cores = sum(value for pid, value in spent.items() if pid in mine) / wall + server_cores
    others = sum(value for pid, value in spent.items() if pid not in mine and pid not in server) / wall
    intervals = sorted((later - earlier) * 1000 for earlier, later in zip(times, times[1:]))
    return {'fps': round(fps, 1), 'frames': len(times), 'run_s': round(run_s, 3), 'stopped': 'stopped' in frames,
            'first_frame_s': round(times[0] - launched, 3),
            # Where FrameCount's capture began: it waits for the window, so a
            # second before this one saw no window to count.
            'capture_start_s': round(frames['capture_start'] - launched, 3),
            # When each frame was presented, in milliseconds since the launch.
            'frame_ms': [round((time - launched) * 1000, 1) for time in times],
            'fps_by_second': per_second(times, launched, run_s),
            'interval_p50_ms': round(percentile(intervals, 0.50), 2),
            'interval_p99_ms': round(percentile(intervals, 0.99), 2),
            'cpu_cores': round(cores, 2), 'server_cores': round(server_cores, 2), 'other_cores': round(others, 2),
            'cpu_ms_per_frame': round(cores * 1000 / fps, 2),
            # The run at each interval of `Usage`.
            'samples': run_samples(taken, clock_samples, launched, times),
            **held, **clocked}


def percentile(ordered, share):
    return ordered[min(len(ordered) - 1, int(share * len(ordered)))] if ordered else 0.0


def picture(name, args, work, page, stage):
    """The window frozen on frame `args.freeze`."""
    app = App(name, args, args.freeze, work, page, stage)
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
    parser.add_argument('--browser', type=Path, metavar='DIST',
                        help='run the apps `build_apps.sh browser DIST` built, in Chrome')
    parser.add_argument('--release', help='the release tag `cranpose-release` was built at')
    parser.add_argument('--tier', type=int, default=12)
    parser.add_argument('--rounds', type=int, default=1,
                        help="rounds, in each of which every app starts after the apps' state is removed, "
                             'then once more')
    parser.add_argument('--run', type=float, default=10.0,
                        help='seconds each launch is measured for, from its start')
    parser.add_argument('--timeout', type=float, default=60.0)
    parser.add_argument('--main', help='the commit the Cranpose app was built at, recorded with the run')
    parser.add_argument('--max-others', type=float, default=1.5,
                        help='cores other processes may spend in a leg above what they spend while the machine '
                             'idles, before it is measured again')
    parser.add_argument('--parity', action='store_true')
    parser.add_argument('--freeze', type=int, default=120)
    parser.add_argument('--shift', type=int, default=8)
    parser.add_argument('--drift', type=int, default=160)
    parser.add_argument('--tile', type=int, default=48)
    parser.add_argument('--tile-delta', type=float, default=12.0)
    args = parser.parse_args()
    if args.browser:
        built = [name for name in versions.BROWSER_APPS if (args.browser / name / 'index.html').exists()]
        apps = args.apps.split(',') if args.apps else built
        print('not built:', ', '.join(name for name in versions.BROWSER_APPS if name not in built) or 'none',
              flush=True)
    elif args.apps:
        apps = args.apps.split(',')
    else:
        apps = [name for name in APPS if name == 'web' or Path(APPS[name][0]).exists()]
        print('not built:', ', '.join(name for name in APPS if name not in apps) or 'none', flush=True)
    args.output.mkdir(parents=True, exist_ok=True)
    current_framecount()
    started = time.monotonic()
    page = BrowserServer(args.browser).base if args.browser else serve_page()
    with tempfile.TemporaryDirectory(prefix='gauntlet-') as folder:
        stage = Path(folder)
        shutil.copytree(FONTS, stage / 'fonts')
        report = run(args, apps, page, stage)
    report['duration_s'] = round(time.monotonic() - started)
    (args.output / 'report.json').write_text(json.dumps(report, indent=1))
    print(json.dumps(report['scenarios'][0]['starts'] if 'scenarios' in report else report['changed_pct']))


# How long the machine's own background load is read for, before each round.
IDLE_SPAN_S = 5.0


def idle_others(span_s=IDLE_SPAN_S):
    """The cores the machine's processes spend while no app runs, over
    `span_s`: its own background load, which a Mac up for weeks can hold
    above a core (macm3, 2026-10-11: `dasd`, `launchd` and `notifyd`)."""
    before, started = processes(), time.monotonic()
    time.sleep(span_s)
    after, wall = processes(), time.monotonic() - started
    return sum(max(0.0, cpu - before[pid][1]) for pid, (_, cpu, _, _) in after.items() if pid in before) / wall


def undisturbed_leg(name, args, page, stage, label, earlier, failures, profile, start, idle):
    """A leg of `start`, measured again, up to three times, while other
    processes spend more than `--max-others` cores above the machine's `idle`
    load in it or it fails. Every
    attempt at a first start begins after the apps' state and Chrome's
    `profile` are removed; a second start measured again follows the earlier
    ones, its caches as full. Every leg measured is kept, a disturbed one
    marked `disturbed`, and every failed attempt is added to `failures`; none
    when every attempt failed, so one app cannot end the run."""
    legs = []
    for attempt in range(3):
        if start == STARTS[0]:
            clear_app_state(profile)
        try:
            leg = measure(name, args, args.output, page, stage, f'{label}-{attempt + 1}', earlier, profile)
        except (RuntimeError, subprocess.SubprocessError) as failure:
            print(f'{label:16} failed: {failure}', flush=True)
            failures.append({'subject': name, 'start': start, 'label': f'{label}-{attempt + 1}',
                             'error': str(failure)[:300]})
            continue
        leg['idle_other_cores'] = round(idle, 2)
        leg['disturbed'] = leg['other_cores'] > idle + args.max_others
        legs.append(leg)
        print(f'{label:16} fps {leg["fps"]:6.1f} cpu/f {leg["cpu_ms_per_frame"]} first frame '
              f'{leg["first_frame_s"]} s p99 {leg["interval_p99_ms"]} others {leg["other_cores"]}', flush=True)
        if not leg['disturbed']:
            break
    return legs


def summarize(legs, apps, start):
    """Per app, the medians over its legs of `start` no other process
    disturbed; the disturbed ones stay in the run, marked."""
    return {name: {key: round(statistics.median(values), 2)
                   for key in SUMMARY
                   if (values := [leg[key] for leg in legs
                                  if leg['subject'] == name and leg['start'] == start and not leg['disturbed']
                                  and leg.get(key) is not None])}
            for name in apps}


def run(args, apps, page, stage):
    """The pictures and their comparison, or the measured rounds."""
    if args.parity:
        # Pillow, which comparing pictures needs and measuring does not.
        from PIL import Image
        from parity import compare_pair
        pictures = {name: picture(name, args, args.output, page, stage) for name in apps}
        # Compose's picture is the one the others are held to.
        reference = next((name for name in ('compose', 'web') if name in apps), apps[0])
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
    legs, failures = [], []
    for round_index in range(args.rounds):
        idle = idle_others()
        print(f'round {round_index + 1}: the machine idles at {idle:.2f} cores', flush=True)
        for name in apps:
            # Chrome's profile of the round: made by the first start, kept
            # for the second.
            profile = stage / f'chrome-profile-{name}-{round_index + 1}'
            for start in STARTS:
                earlier = [leg['fps'] for leg in legs
                           if leg['subject'] == name and leg['start'] == start and not leg['disturbed']]
                label = f'{name}-{round_index + 1}-{start}'
                measured = undisturbed_leg(name, args, page, stage, label, earlier, failures, profile, start,
                                           idle)
                if not measured:
                    # No second start without a first one.
                    break
                legs += [{'subject': name, 'round': round_index, 'start': start, **leg} for leg in measured]
                kept = [leg for leg in measured if not leg['disturbed']]
                if kept and disagrees(kept[-1]['fps'], earlier):
                    print(f'{name}: {kept[-1]["fps"]} fps disagrees with its earlier legs; measuring once more',
                          flush=True)
                    again = undisturbed_leg(name, args, page, stage, f'{label}-again', earlier, failures,
                                            profile, start, idle)
                    legs += [{'subject': name, 'round': round_index, 'start': start, **leg} for leg in again]
    chip = subprocess.run(['sysctl', '-n', 'machdep.cpu.brand_string'], capture_output=True, text=True,
                          check=True).stdout.strip()
    width, height = DESKTOP_WINDOW
    platform = 'browser' if args.browser else 'desktop'
    if args.browser:
        chip = f'{chip} · Chrome {versions.chrome_version().split(".")[0]}'
    return {
        'kind': platform if args.browser else 'frameworks',
        'started_at': started_at,
        'main': args.main,
        'device': {'ro.product.model': chip},
        'subjects': [versions.subject(name, platform, built, args.release) for name in apps],
        'protocol': {'run_s': args.run, 'starts': list(STARTS), 'rounds': args.rounds,
                     'max_other_cores': args.max_others, **slow_rule()},
        'scenarios': [{'scenario': 'gauntlet',
                       'extras': f'tier {args.tier}, {width} x {height} {"browser " if args.browser else ""}window',
                       'legs': legs, 'failures': failures,
                       'starts': {start: {'summary': summarize(legs, apps, start), 'verdicts': {}}
                                  for start in STARTS}}],
    }


if __name__ == '__main__':
    main()

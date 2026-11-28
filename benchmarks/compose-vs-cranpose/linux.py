#!/usr/bin/env python3
"""Measures the gauntlet's desktop apps on Linux, with MangoHud counting every
present of each app from outside it.

Each app runs alone in its 1280 x 820 window (`perf-data`'s `DESKTOP_WINDOW`)
on the X display in `DISPLAY`, with the screen, tier, fonts and chat pictures
`desktop.py` gives it, and starts through `mangohud APP`, the way MangoHud's
README starts any app: MangoHud's Vulkan layer attaches to a Vulkan app and
its OpenGL hooks to a GL one, and every leg records which one attached
(`mangohud_attach`, from the libraries mapped in the app's processes).
MangoHud's own settings, in `MANGOHUD_CONFIG` and no file: its HUD hidden
(`no_display`), a log row for each frame (`log_interval=0`) in the leg's
folder (`output_folder`), and one present setting for every app, written in
the run's protocol: presents with no wait for the display by default
(`vsync=1`, Vulkan's IMMEDIATE, and `gl_vsync=0`, swap interval 0), each
framework still on its own frame clock, or with `--vsync` at the display's
refresh (`vsync=3`, FIFO, and `gl_vsync=1`).

MangoHud 0.8.4 starts no log by itself while its HUD is hidden: its
`autostart_log` waits for a HUD update that `no_display` skips. The harness
starts the log through MangoHud's control socket (`control=mangohud-%p`,
`:logging=1;`) as soon as the app's process opens it, and MangoHud writes its
rows from its first sampling after that (`mangohud_from_s`, from the launch).
A row's time is its `elapsed` past the moment MangoHud created the log file,
which the harness sees through inotify.

Beside the presents, each app writes a line for each frame it moves on to,
`PERF update K` (`perf_data::updates`, with `PERF_UPDATES` set), which the
harness stamps as it reads it: the content updates. A page (Chrome, Electron,
Tauri) gives its frames to `window.__perfUpdate`, and the script the harness's
server adds to it sends them once a second, each at the page's clock
(`BrowserServer.page_updates`); a leg of a page waits for the report that holds
its last frame. `fps` and
`fps_by_second` count the updates from the launch, warm-up included, as
`desktop.py` counts the pictures that changed; `mangohud_fps` and
`mangohud_fps_by_second` count MangoHud's presents over the same span. Over
the span both cover, a leg whose presents outnumber its updates is marked
`presents_exceed_updates`: it presented pictures that held no new frame; one
whose updates outnumber its presents is marked `updates_exceed_presents`:
it moved on to frames it never showed. A leg whose window the window manager
left at another size than 1280 x 820 is marked `window_mismatch`. A start of
an app whose legs MangoHud logged no present of names what MangoHud did in
its processes (`no_presents`), and an app that did not build stays a subject
of the run with its reason as the failure of each of its starts.

Every launch is measured from its start for `--run` seconds, the same span for
every app, in rounds of two starts as `desktop.py` measures: the first after
what the app keeps between launches is removed (`APP_STATE`, and Mesa's
shader cache and Chrome's profile, which sit in a folder of the app's own),
the second after it. A leg ends early after
`SLOW_SECONDS` seconds in a row under `SLOW_FPS` updates, marked `stopped`.
A leg in which other processes spent more than `--max-others` cores above the
machine's idle load is measured again, up to three times, and marked
`disturbed` when it stays so.

A display whose GPU drives no connected output scans nothing out: every leg
is then marked `no_scanout`, and `scripts/perf/publish.py` publishes no such
run. The X server still takes each present, with no vertical blank to pace
it.

  python3 linux.py --output results/linux --scenario gauntlet:16 --scenario wall:20 --scenario patchwork:17
  python3 linux.py --output results/linux --apps cranpose,gpui --vsync
"""

import argparse
import bisect
import ctypes
import gzip
import json
import os
import re
import select
import shutil
import signal
import socket
import statistics
import struct
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

import desktop
import versions
from timelines import SLOW_FPS, SLOW_SECONDS, STARTS, per_second, slow_rule

HERE = Path(__file__).resolve().parent
CHROME = shutil.which('google-chrome-stable') or shutil.which('google-chrome') or 'google-chrome'

# Each app's Linux build and its command; `{scenario}`, `{tier}`, `{freeze}`,
# `{page}` and `{profile}` are filled in for the run. `build_apps.sh linux`
# builds them.
APPS = {
    'cranpose': [HERE / 'cranpose-app/target/release/perf-compare',
                 '--scenario={scenario}', '--tier={tier}', '--freeze={freeze}'],
    'cranpose-release': [HERE / 'cranpose-app/target-release/release/perf-compare',
                         '--scenario={scenario}', '--tier={tier}', '--freeze={freeze}'],
    **{name: [HERE / f'{name}-app/target/release/perf-compare-{name}']
       for name in ('egui', 'slint', 'iced', 'gpui', 'dioxus', 'xilem', 'rune', 'bui', 'ferro', 'zgui', 'floem',
                    'freya', 'vizia', 'makepad')},
    'tauri': [HERE / 'tauri-app/target/release/perf-compare-tauri', '--page={page}'],
    'avalonia': [HERE / 'avalonia-app/bin/Release/net10.0/linux-x64/publish/PerfAvalonia'],
    'flutter': [HERE / 'flutter-app/build/linux/x64/release/bundle/perf_flutter'],
    'compose': [HERE / 'compose-desktop-app/build/compose/binaries/main/app/PerfCompose/bin/PerfCompose'],
    'electron': [HERE / 'electron-app/build/PerfElectron-linux-x64/PerfElectron', '--user-data-dir={profile}',
                 '--enable-logging=stderr', '--page={page}?tier={tier}&freeze={freeze}&scenario={scenario}'],
    'fyne': [HERE / 'fyne-app/build/perf-compare-fyne'],
    # Without the basic password store Chrome waits for a desktop keyring and
    # never loads the page.
    'web': [CHROME, '--user-data-dir={profile}', '--no-first-run', '--no-default-browser-check',
            '--password-store=basic', '--enable-logging=stderr', '--window-size=1280,820',
            '--app={page}?tier={tier}&freeze={freeze}&scenario={scenario}'],
}
# The apps that show the web page and read it from the harness's server.
PAGE_APPS = {'web', 'tauri', 'electron'}
# Makepad opens its resources by paths relative to where it runs:
# `build_apps.sh linux` puts them beside its program.
WORKING_DIRECTORY = {'makepad': HERE / 'makepad-app/target/release'}
# Why an app that `build_apps.sh linux` did not build is missing, one
# `NAME: REASON` a line.
UNBUILT = HERE / 'linux-unbuilt.txt'
BUILT_VERSIONS = HERE / 'linux-versions.json'

# The present setting every app gets, by MangoHud's options.
PRESENT = {
    'uncapped': {'options': ('vsync=1', 'gl_vsync=0'),
                 'label': 'presents with no wait for the display: Vulkan IMMEDIATE, GL swap interval 0 (MangoHud '
                          "vsync=1, gl_vsync=0), each framework on its own frame clock"},
    'vsync': {'options': ('vsync=3', 'gl_vsync=1'),
              'label': "presents at the display's refresh: Vulkan FIFO, GL swap interval 1 (MangoHud vsync=3, "
                       'gl_vsync=1)'},
}

# How often the harness looks for MangoHud's control socket of a new app, and
# how long it looks.
CONTROL_POLL_S = 0.01

# What the apps keep between launches in the user's folders, as `desktop.py`'s
# `APP_STATE` names it on a Mac: the apps' own caches, data and settings.
# Mesa's shader cache and Chrome's profile sit in each app's state folder.
# Removed before every first start; a fresh cache folder of the user's own
# crashes Avalonia 12.2 on Linux, so the folders themselves stay.
XDG_CACHE = Path(os.environ.get('XDG_CACHE_HOME') or Path.home() / '.cache')
XDG_CONFIG = Path(os.environ.get('XDG_CONFIG_HOME') or Path.home() / '.config')
XDG_DATA = Path(os.environ.get('XDG_DATA_HOME') or Path.home() / '.local/share')
APP_STATE = [(folder, pattern) for folder in (XDG_CACHE, XDG_CONFIG, XDG_DATA)
             for pattern in ('perf-compare*', 'dev.perfcompare.*', 'PerfElectron', 'PerfCompose', 'perf_flutter')] + \
    [(XDG_CACHE / 'fyne', 'dev.perfcompare.*'), (XDG_CONFIG / 'fyne', 'dev.perfcompare.*')]


def clear_app_state():
    """Removes what the apps keep between launches in the user's folders."""
    for root, pattern in APP_STATE:
        for path in root.glob(pattern):
            if path.is_dir() and not path.is_symlink():
                shutil.rmtree(path, ignore_errors=True)
            else:
                path.unlink(missing_ok=True)
CONTROL_WAIT_S = 30.0
# A leg's presents outnumber its updates when they exceed them by more than
# the frames the two counts' edges can split differently.
EDGE_FRAMES = 2
EDGE_SHARE = 0.01
TICKS = os.sysconf('SC_CLK_TCK')
MB = 1024 * 1024
IN_CREATE = 0x100
LIBC = ctypes.CDLL(None, use_errno=True)


def outputs(drm=Path('/sys/class/drm')):
    """Every output of every GPU with its state: `connected` and `enabled`
    for one that scans a picture out."""
    found = {}
    for path in sorted(drm.glob('card*-*')):
        if (path / 'status').exists():
            found[path.name] = {'status': (path / 'status').read_text().strip(),
                                'enabled': (path / 'enabled').read_text().strip()}
    return found


def scans_out(found):
    """Whether any of `outputs` scans a picture out."""
    return any(state['status'] == 'connected' and state['enabled'] == 'enabled' for state in found.values())


def mangohud_version():
    result = subprocess.run(['mangohud', '--version'], capture_output=True, text=True)
    return result.stdout.strip() or result.stderr.strip() or 'unknown'


def mangohud_config(folder, present):
    """MangoHud's settings for one leg: no HUD, a row a frame in `folder`, a
    control socket the harness starts the log on, and the run's present
    setting."""
    return ','.join(('no_display', 'control=mangohud-%p', f'output_folder={folder}', 'log_interval=0',
                     *PRESENT[present]['options']))


def read_log(text):
    """MangoHud's log: the system line above the column names, and each row's
    `frametime` in milliseconds and `elapsed` in nanoseconds since MangoHud
    started the log."""
    lines = text.splitlines()
    header = next((at for at, line in enumerate(lines) if line.startswith('fps,frametime')), None)
    if header is None:
        return {}, []
    system = dict(zip(lines[header - 2].split(','), lines[header - 1].split(','))) if header >= 2 else {}
    names = lines[header].split(',')
    frametime, elapsed = names.index('frametime'), names.index('elapsed')
    rows = []
    for line in lines[header + 1:]:
        cells = line.split(',')
        if len(cells) == len(names):
            rows.append((float(cells[frametime]), int(cells[elapsed])))
    return system, rows


def present_times(rows, created):
    """The monotonic time of each row's present: MangoHud created the file as
    it wrote the first row, `elapsed` of that row past its log's start."""
    if not rows:
        return []
    start = created - rows[0][1] / 1e9
    return [start + elapsed / 1e9 for _, elapsed in rows]


def exceeds(presents, updates):
    """Whether `presents` outnumber `updates` past what the edges of one span
    can split differently."""
    return presents > updates + max(EDGE_FRAMES, EDGE_SHARE * updates)


class Created:
    """The monotonic time a file first appears in a folder, through inotify:
    a thread waits on it without polling."""

    def __init__(self, folder):
        self.at = None
        self.name = None
        self.fd = LIBC.inotify_init1(os.O_CLOEXEC)
        if self.fd < 0 or LIBC.inotify_add_watch(self.fd, str(folder).encode(), IN_CREATE) < 0:
            raise OSError(ctypes.get_errno(), 'inotify')
        self.closed = threading.Event()
        self.thread = threading.Thread(target=self.wait, daemon=True)
        self.thread.start()

    def wait(self):
        while not self.closed.is_set():
            ready, _, _ = select.select([self.fd], [], [], 0.5)
            if not ready:
                continue
            stamp = time.monotonic()
            events = os.read(self.fd, 4096)
            offset = 0
            # Each `struct inotify_event`: wd, mask, cookie, len, then the name.
            while offset + 16 <= len(events):
                _, _, _, length = struct.unpack_from('iIII', events, offset)
                name = events[offset + 16:offset + 16 + length].rstrip(b'\0').decode(errors='replace')
                offset += 16 + length
                if name.endswith('.csv') and not name.endswith('_summary.csv'):
                    self.at, self.name = stamp, name
                    return

    def close(self):
        self.closed.set()
        self.thread.join(timeout=2)
        os.close(self.fd)


def proc_stat(pid):
    """A process's CPU ticks and its reaped children's, or None once it
    ended."""
    try:
        text = Path(f'/proc/{pid}/stat').read_text()
    except OSError:
        return None
    fields = text[text.rindex(')') + 2:].split()
    return int(fields[11]) + int(fields[12]), int(fields[13]) + int(fields[14])


def children(pid):
    """The processes every thread of `pid` started, from the kernel's own
    lists: no scan of every process."""
    found = []
    for task in Path(f'/proc/{pid}/task').glob('*/children'):
        try:
            found += [int(child) for child in task.read_text().split()]
        except OSError:
            continue
    return found


def tree(root):
    """`root` and every process below it."""
    found, pending = set(), [root]
    while pending:
        pid = pending.pop()
        if pid not in found:
            found.add(pid)
            pending.extend(children(pid))
    return found


def tree_ticks(pids, root):
    """The CPU ticks `pids` spent, with the children `root` reaped."""
    total = 0
    for pid in pids:
        if stat := proc_stat(pid):
            total += stat[0] + (stat[1] if pid == root else 0)
    return total


def machine_ticks():
    """Every core's busy and total ticks since boot."""
    cells = [int(cell) for cell in Path('/proc/stat').read_text().split('\n', 1)[0].split()[1:]]
    idle = cells[3] + cells[4]
    return sum(cells) - idle, sum(cells)


def idle_cores(span_s=desktop.IDLE_SPAN_S):
    """The cores the machine keeps busy while no app runs, over `span_s`: its
    own background load. The kernel's tick counts, where `ps`'s whole seconds
    read a light load as none."""
    before = machine_ticks()[0]
    time.sleep(span_s)
    return (machine_ticks()[0] - before) / TICKS / span_s


def quiet_idle(max_idle, max_wait_s):
    """The idle load, read again while it stays above `max_idle`, for up to
    `max_wait_s`."""
    idle, started = idle_cores(), time.monotonic()
    while idle > max_idle and time.monotonic() - started < max_wait_s:
        idle = idle_cores()
    return idle


def display_server(display):
    """The X server of `display`, whose work on each present counts toward the
    app, as the window server's does on a Mac."""
    number = display.split(':')[-1].split('.')[0]
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            command = (entry / 'cmdline').read_bytes().split(b'\0')
        except OSError:
            continue
        name = Path(command[0].decode(errors='replace')).name
        if name in ('Xorg', 'X', 'Xwayland', 'Xvfb') and f':{number}'.encode() in command:
            return int(entry.name)
    return None


def resident_mb(pids):
    total = 0
    for pid in pids:
        try:
            status = Path(f'/proc/{pid}/status').read_text()
        except OSError:
            continue
        if found := re.search(r'^VmRSS:\s+(\d+) kB', status, re.M):
            total += int(found.group(1)) * 1024
    return round(total / MB, 1)


def attachments(pids):
    """How MangoHud attached in the app's processes, and the graphics APIs its
    Mesa drivers serve: Vulkan where its layer or a Vulkan driver is mapped,
    GL where its OpenGL hooks or a GL driver are."""
    hooks, apis = set(), set()
    for pid in pids:
        try:
            maps = Path(f'/proc/{pid}/maps').read_text()
        except OSError:
            continue
        names = {Path(line.split()[-1]).name for line in maps.splitlines() if '/' in line}
        if 'libMangoHud.so' in names:
            hooks.add('vulkan')
        if 'libMangoHud_opengl.so' in names:
            hooks.add('gl')
        if any(name.startswith('libvulkan_') for name in names):
            apis.add('vulkan')
        if any(name.startswith(('libgallium', 'iris_dri', 'libGLX_mesa', 'libEGL_mesa')) for name in names):
            apis.add('gl')
    return sorted(hooks), sorted(apis)


def window_size(pid):
    """The size of the window of `pid`, by its `_NET_WM_PID`, where `xdotool`
    and `xwininfo` are installed; None elsewhere."""
    if not shutil.which('xdotool') or not shutil.which('xwininfo'):
        return None
    windows = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], capture_output=True,
                             text=True).stdout.split()
    for window in windows:
        info = subprocess.run(['xwininfo', '-id', window], capture_output=True, text=True).stdout
        width, height = re.search(r'Width: (\d+)', info), re.search(r'Height: (\d+)', info)
        if width and height and int(width.group(1)) > 100:
            return f'{width.group(1)} x {height.group(1)}'
    return None


class App:
    """One app started through `mangohud`, its output read by a thread that
    stamps each `PERF update` line as it arrives."""

    def __init__(self, name, args, server, stage, label, home, folder):
        self.name = name
        self.log = stage / f'{label}.log'
        profile = home / 'profile'
        page = f'{server.base}/index.html' if server else ''
        values = {'scenario': args.scenario, 'tier': args.tier, 'freeze': 0, 'page': page, 'profile': profile}
        command = ['mangohud', *(str(part).format(**values) for part in APPS[name])]
        environment = {**os.environ, 'MESA_SHADER_CACHE_DIR': str(home / 'mesa-shader-cache'),
                       'XAUTHORITY': args.xauthority, 'DISPLAY': args.display,
                       'PERF_SCENARIO': args.scenario, 'PERF_TIER': str(args.tier), 'PERF_FREEZE': '0',
                       'PERF_FONTS': str(stage / 'fonts'), 'PERF_CHAT': str(stage / 'chat-assets'),
                       'PERF_UPDATES': '1', 'MANGOHUD_CONFIG': mangohud_config(folder, args.present)}
        self.updates = []
        self.first_frame = None
        self.lines = []
        self.process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=environment,
                                        cwd=WORKING_DIRECTORY.get(name, home), start_new_session=True)
        self.pid = self.process.pid
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        with self.log.open('w') as log:
            for raw in self.process.stdout:
                stamp = time.monotonic()
                line = raw.decode(errors='replace').rstrip('\n')
                found = line.find('PERF update ')
                if found >= 0:
                    self.updates.append((stamp, int(line[found + 12:].split()[0])))
                    continue
                if self.first_frame is None and 'PERF first_frame' in line:
                    self.first_frame = stamp
                log.write(line + '\n')
                self.lines.append(line)

    def state(self):
        running = 'the app was still running' if self.process.poll() is None else 'the app had ended'
        drew = 'drew a first frame' if self.first_frame or self.updates else 'drew no frame yet'
        return f'{running}, {drew}{desktop.last_words([line.strip() for line in self.lines if line.strip()])}'

    def stop(self, work):
        for sent in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.killpg(self.pid, sent)
            except ProcessLookupError:
                break
            try:
                self.process.wait(timeout=5)
                break
            except subprocess.TimeoutExpired:
                continue
        self.reader.join(timeout=5)
        shutil.copy(self.log, work / self.log.name)


class Control:
    """MangoHud's control socket in the app's processes: a thread finds it
    and starts the log through it at once."""

    def __init__(self, app):
        self.app = app
        self.socket = None
        self.started = None
        self.stopped = threading.Event()
        self.thread = threading.Thread(target=self.start, daemon=True)
        self.thread.start()

    def start(self):
        deadline = time.monotonic() + CONTROL_WAIT_S
        while not self.stopped.is_set() and time.monotonic() < deadline:
            # `control=mangohud-%p` names the socket after the process MangoHud
            # runs in: the app's, or a browser's GPU process below it.
            for pid in sorted(tree(self.app.pid)):
                connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                try:
                    connection.connect(f'\0mangohud-{pid}')
                    connection.sendall(b':logging=1;')
                except OSError:
                    connection.close()
                    continue
                self.socket, self.started = connection, time.monotonic()
                return
            time.sleep(CONTROL_POLL_S)

    def finish(self):
        """Ends the log, which writes MangoHud's summary beside it."""
        self.stopped.set()
        self.thread.join(timeout=2)
        if self.socket:
            try:
                self.socket.sendall(b':logging=0;')
            except OSError:
                pass  # The app ended.
            time.sleep(0.2)
            self.socket.close()


def slow(updates, first, now):
    """Whether the last `SLOW_SECONDS` whole seconds since the first update each
    held fewer than `SLOW_FPS` updates."""
    whole = int(now - first)
    if whole < SLOW_SECONDS:
        return False
    counts = per_second([stamp for stamp, _ in updates], first, whole)
    return all(count < SLOW_FPS for count in counts[-SLOW_SECONDS:])


# How long past its run a page's report may come: a page reports once a
# second, each report holding the frames before it.
PAGE_REPORT_WAIT_S = 2.0


def page_updates_until(server, since, deadline):
    """A page's updates once its reports hold every frame before `deadline`,
    or those they hold `PAGE_REPORT_WAIT_S` after it."""
    while True:
        updates, covered = server.page_updates(since)
        if (covered is not None and covered >= deadline) or time.monotonic() >= deadline + PAGE_REPORT_WAIT_S:
            return updates
        time.sleep(0.05)


def measure(name, args, server, stage, label, home):
    """One launch measured from its start for `--run` seconds: its updates,
    MangoHud's presents, the CPU the app, its children and the X server spent,
    and the memory it held at the end."""
    work = args.output
    folder = stage / f'{label}-mangohud'
    folder.mkdir(parents=True)
    created = Created(folder)
    xserver = display_server(args.display)
    server_before = tree_ticks([xserver], None) if xserver else 0
    machine_before = machine_ticks()
    # A page says its first frame to the server, not on its program's output.
    since = server.mark() if server else 0
    launched = time.monotonic()
    app = App(name, args, server, stage, label, home, folder)
    control = Control(app)
    # A page tells its frames to the server, a program on its output.
    paged = server is not None and name in PAGE_APPS
    try:
        deadline, stopped, judged = launched + args.run, False, launched
        while (now := time.monotonic()) < deadline:
            if app.process.poll() is not None:
                raise RuntimeError(f'{name} ended {now - launched:.1f} s after its launch; {app.state()}')
            # The slow rule speaks of whole seconds: it is read once a second.
            if now - judged >= 1.0:
                judged = now
                counted = server.page_updates(since)[0] if paged else app.updates
                if counted and slow(counted, counted[0][0], now):
                    deadline, stopped = now, True
                    break
            time.sleep(0.05)
        ended = time.monotonic()
        run_s = deadline - launched
        first = app.first_frame or (app.updates[0][0] if app.updates else None) or \
            (server.heard_at('PERF first_frame', since) if paged else None)
        # The app's state is read before the harness stops it.
        if first is None:
            raise RuntimeError(f'{name} logged no first frame within its {run_s:.0f} s run; {app.state()}')
        if first >= launched + run_s:
            raise RuntimeError(f'{name} logged its first frame {first - launched:.1f} s after its launch, after its '
                               f'{run_s:.0f} s run; {app.state()}')
        mine = tree(app.pid)
        cpu = tree_ticks(mine, app.pid) / TICKS
        server_cpu = (tree_ticks([xserver], None) - server_before) / TICKS if xserver else 0.0
        machine_after = machine_ticks()
        ram = resident_mb(mine)
        hooks, apis = attachments(mine)
        window = window_size(app.pid)
        control.finish()
        counted = page_updates_until(server, since, deadline) if paged else app.updates
    finally:
        control.stopped.set()
        app.stop(work)
        created.close()
    logs = sorted(path for path in folder.glob('*.csv') if not path.name.endswith('_summary.csv'))
    system, rows = read_log(logs[0].read_text()) if logs else ({}, [])
    if logs:
        kept = work / 'mangohud' / f'{label}.csv.gz'
        kept.parent.mkdir(parents=True, exist_ok=True)
        kept.write_bytes(gzip.compress(logs[0].read_bytes(), mtime=0))
    presents = [stamp for stamp in present_times(rows, created.at) if launched <= stamp < launched + run_s] \
        if created.at else []
    updates = [(stamp, frame) for stamp, frame in counted if launched <= stamp < launched + run_s]
    wall = ended - launched
    busy = (machine_after[0] - machine_before[0]) / TICKS / wall
    cores = cpu / wall + server_cpu / wall
    leg = {
        'run_s': round(run_s, 3), 'stopped': stopped, 'first_frame_s': round(first - launched, 3),
        'cpu_cores': round(cores, 2), 'server_cores': round(server_cpu / wall, 2),
        'other_cores': round(max(0.0, busy - cores), 2), 'ram_mb': ram,
        'mangohud_attach': hooks, 'graphics_api': apis, 'window': window,
        # The window manager left the window at another size than every
        # app asks for: a screen smaller than the window clamps it.
        'window_mismatch': window is not None and window != '{} x {}'.format(*desktop.DESKTOP_WINDOW),
        'no_scanout': not scans_out(outputs()),
        'mangohud_log': f'mangohud/{label}.csv.gz' if logs else None,
        'mangohud_rows': len(rows),
        'mangohud_system': {key: system[key] for key in ('cpu', 'gpu', 'kernel', 'driver') if key in system},
    }
    if server and name in PAGE_APPS:
        # What the page said of itself, its frame requests among it.
        leg['page'] = server.page_record(since, launched, ended)
    leg.update(update_counts(updates, launched, run_s))
    leg.update(present_counts(presents, launched, run_s, updates))
    rate = leg['fps'] or leg['mangohud_fps']
    leg['cpu_ms_per_frame'] = round(cores * 1000 / rate, 2) if rate else None
    return leg


def update_counts(updates, launched, run_s):
    """What the app's own updates say of the leg: the harness's frame rate."""
    if not updates:
        return {'fps': None, 'frames': None, 'frame_ms': [], 'fps_by_second': [], 'updates_skipped': None}
    times = [stamp for stamp, _ in updates]
    frames = [frame for _, frame in updates]
    return {
        'fps': round(len(times) / run_s, 1), 'frames': len(times),
        'frame_ms': [round((stamp - launched) * 1000, 1) for stamp in times],
        'fps_by_second': per_second(times, launched, run_s),
        # Frame numbers an update moved past without writing a line.
        'updates_skipped': sum(later - earlier - 1 for earlier, later in zip(frames, frames[1:]) if later > earlier + 1),
    }


def present_counts(presents, launched, run_s, updates):
    """What MangoHud's presents say of the leg over the same span from the
    launch as the updates, and how the two compare over the span both cover:
    from MangoHud's first row to the end."""
    if not presents:
        return {'mangohud_fps': None, 'mangohud_fps_by_second': [], 'mangohud_from_s': None, 'present_ms': [],
                'presents': 0, 'updates_in_presents_span': None, 'updates_before_mangohud': None,
                'presents_exceed_updates': None, 'updates_exceed_presents': None, 'interval_p50_ms': None,
                'interval_p99_ms': None}
    start = presents[0]
    # An app moves on to a frame before it presents it: the last update
    # before MangoHud's first present is the frame that present showed.
    first = max(bisect.bisect_left([stamp for stamp, _ in updates], start) - 1, 0)
    covered = len(updates) - first if updates else None
    intervals = sorted((later - earlier) * 1000 for earlier, later in zip(presents, presents[1:]))
    return {
        'mangohud_fps': round(len(presents) / run_s, 1),
        'mangohud_fps_by_second': per_second(presents, launched, run_s),
        'mangohud_from_s': round(start - launched, 3),
        'present_ms': [round((stamp - launched) * 1000, 1) for stamp in presents],
        'presents': len(presents), 'updates_in_presents_span': covered,
        # The updates before MangoHud's first row, which its log cannot show.
        'updates_before_mangohud': None if covered is None else len(updates) - covered,
        'presents_exceed_updates': None if covered is None else exceeds(len(presents), covered),
        # Frames the app moved on to and never presented: content that should
        # have moved on the screen and did not.
        'updates_exceed_presents': None if covered is None else exceeds(covered, len(presents)),
        'interval_p50_ms': round(desktop.percentile(intervals, 0.50), 2),
        'interval_p99_ms': round(desktop.percentile(intervals, 0.99), 2),
    }


def undisturbed_leg(name, args, server, stage, label, start, home, idle, failures):
    """A leg of `start`, measured again up to three times while it fails or
    other processes disturb it; every leg measured is kept."""
    legs = []
    for attempt in range(3):
        if start == STARTS[0]:
            clear_app_state()
            shutil.rmtree(home, ignore_errors=True)
            home.mkdir(parents=True)
        try:
            leg = measure(name, args, server, stage, f'{label}-{attempt + 1}', home)
        except (RuntimeError, OSError, subprocess.SubprocessError) as failure:
            print(f'{label:20} failed: {failure}', flush=True)
            failures.append({'subject': name, 'start': start, 'label': f'{label}-{attempt + 1}',
                             'error': str(failure)[:600]})
            continue
        leg['idle_other_cores'] = round(idle, 2)
        leg['disturbed'] = leg['other_cores'] > desktop.disturbed_limit(idle, args.max_others, args.max_idle)
        legs.append(leg)
        print(f'{label:20} fps {leg["fps"]} mangohud {leg["mangohud_fps"]} from {leg["mangohud_from_s"]} s, '
              f'presents {leg["presents"]} updates {leg["updates_in_presents_span"]}, attach {leg["mangohud_attach"]}, '
              f'others {leg["other_cores"]}', flush=True)
        if not leg['disturbed']:
            break
    return legs


SUMMARY = ('fps', 'mangohud_fps', 'first_frame_s', 'cpu_ms_per_frame', 'cpu_cores', 'server_cores', 'other_cores',
           'ram_mb')


def summarize(legs, apps, start):
    """Per app, the medians over its undisturbed legs of `start`, and how many
    of them presented more pictures than the frames they moved on to."""
    summary = {}
    for name in apps:
        kept = [leg for leg in legs if leg['subject'] == name and leg['start'] == start and not leg['disturbed']]
        medians = {key: round(statistics.median(values), 2) for key in SUMMARY
                   if (values := [leg[key] for leg in kept if leg.get(key) is not None])}
        if kept:
            for mark in ('presents_exceed_updates', 'updates_exceed_presents'):
                medians[f'{mark}_legs'] = sum(1 for leg in kept if leg.get(mark))
        summary[name] = medians
    return summary


def no_presents(legs, apps, start):
    """Per app whose undisturbed legs of `start` all ran with no present in
    MangoHud's log, what MangoHud did in its processes."""
    reasons = {}
    for name in apps:
        kept = [leg for leg in legs if leg['subject'] == name and leg['start'] == start and not leg['disturbed']]
        if kept and not any(leg['presents'] for leg in kept):
            hooks = sorted({hook for leg in kept for hook in leg['mangohud_attach']})
            reasons[name] = f"MangoHud's {' and '.join(hooks)} hook loaded and logged no present" if hooks else \
                'MangoHud loaded into none of its processes'
    return reasons


def measure_screen(args, apps, server, stage):
    """Every app's rounds on the screen `args` names."""
    legs, failures = [], []
    for round_index in range(args.rounds):
        for name in apps:
            with desktop.host_lock(args.host_lock):
                idle = quiet_idle(args.max_idle, args.max_idle_wait)
                print(f'round {round_index + 1} {name}: the machine idles at {idle:.2f} cores', flush=True)
                home = stage / f'home-{name}-{round_index + 1}'
                for start in STARTS:
                    label = f'{args.scenario}-{name}-{round_index + 1}-{start}'
                    measured = undisturbed_leg(name, args, server, stage, label, start, home, idle, failures)
                    if not measured:
                        break
                    legs += [{'subject': name, 'round': round_index, 'start': start, **leg} for leg in measured]
    width, height = desktop.DESKTOP_WINDOW
    return {'scenario': args.scenario, 'extras': f'tier {args.tier}, {width} x {height} window, '
                                                 f'{PRESENT[args.present]["label"]}',
            'legs': legs, 'failures': failures, 'unimplemented': [],
            'starts': {start: {'summary': summarize(legs, apps, start), 'verdicts': {},
                               'no_presents': no_presents(legs, apps, start)} for start in STARTS}}


def unbuilt():
    """Why each app `build_apps.sh linux` did not build is missing."""
    if not UNBUILT.exists():
        return {}
    return dict(line.split(': ', 1) for line in UNBUILT.read_text().splitlines() if ': ' in line)


def device_name(legs):
    """The machine as MangoHud names its CPU and GPU, or as Linux does."""
    system = next((leg['mangohud_system'] for leg in legs if leg.get('mangohud_system')), {})
    cpu = system.get('cpu') or re.search(r'^model name\s*:\s*(.+)$', Path('/proc/cpuinfo').read_text(), re.M).group(1)
    return ' · '.join(part.strip() for part in (cpu, system.get('gpu', ''), 'Linux') if part.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--apps', help='the apps to run; by default every app that is built')
    parser.add_argument('--release', help='the release tag `cranpose-release` was built at')
    parser.add_argument('--scenario', type=desktop.screen, action='append', metavar='NAME[:TIER]')
    parser.add_argument('--tier', type=int, default=16)
    parser.add_argument('--rounds', type=int, default=1)
    parser.add_argument('--run', type=float, default=10.0, help='seconds each launch is measured for, from its start')
    parser.add_argument('--vsync', dest='present', action='store_const', const='vsync', default='uncapped',
                        help="present at the display's refresh instead of uncapped")
    parser.add_argument('--display', default=os.environ.get('DISPLAY', ':0'))
    parser.add_argument('--xauthority', default=os.environ.get('XAUTHORITY', str(Path.home() / '.Xauthority')))
    parser.add_argument('--main', help='the commit the Cranpose app was built at, recorded with the run')
    parser.add_argument('--max-others', type=float, default=1.5)
    parser.add_argument('--max-idle-others', dest='max_idle', type=float, default=desktop.MAX_IDLE_OTHERS)
    parser.add_argument('--max-idle-wait', dest='max_idle_wait', type=float, default=desktop.MAX_IDLE_WAIT_S)
    parser.add_argument('--host-lock', action=argparse.BooleanOptionalAction, default=True)
    args = parser.parse_args()
    if not shutil.which('mangohud'):
        sys.exit('MangoHud is not installed: the distribution package `mangohud` provides it')
    screens = [(name, tier or args.tier) for name, tier in args.scenario or [('gauntlet', None)]]
    built = [name for name in APPS if Path(APPS[name][0]).exists() or shutil.which(str(APPS[name][0]))]
    apps = args.apps.split(',') if args.apps else built
    missing = {**{name: 'not built' for name in versions.PLATFORM_APPS['linux'] if name not in built}, **unbuilt()}
    print('not built:', ', '.join(missing) or 'none', flush=True)
    args.output.mkdir(parents=True, exist_ok=True)
    started, started_at = time.monotonic(), datetime.now(timezone.utc).isoformat(timespec='seconds')
    found = outputs()
    if not scans_out(found):
        print('no output of the GPU is connected: every leg is marked no_scanout and the run is not published',
              flush=True)
    server = desktop.serve_page() if any(name in PAGE_APPS for name in apps) else None
    with tempfile.TemporaryDirectory(prefix='gauntlet-linux-') as folder:
        stage = Path(folder)
        shutil.copytree(desktop.FONTS, stage / 'fonts')
        shutil.copytree(desktop.CHAT_ASSETS, stage / 'chat-assets')
        scenarios = []
        for name, tier in screens:
            screen_args = argparse.Namespace(**{**vars(args), 'scenario': name, 'tier': tier})
            scenarios.append(measure_screen(screen_args, apps, server, stage))
    # An app that did not build stays a subject of the run, its reason the
    # failure of each of its starts.
    absent = {name: reason for name, reason in missing.items() if name not in apps}
    for scenario in scenarios:
        scenario['failures'] += [{'subject': name, 'start': start, 'error': reason}
                                 for name, reason in absent.items() for start in STARTS]
    built_versions = json.loads(BUILT_VERSIONS.read_text()) if BUILT_VERSIONS.exists() else {}
    legs = [leg for scenario in scenarios for leg in scenario['legs']]
    report = {
        'kind': 'frameworks', 'platform': 'linux', 'started_at': started_at, 'main': args.main,
        'release': args.release,
        'device': {'ro.product.model': device_name(legs)},
        'subjects': [versions.subject(name, 'linux', built_versions, args.release) for name in [*apps, *absent]],
        'protocol': {'run_s': args.run, 'starts': list(STARTS), 'rounds': args.rounds,
                     'max_other_cores': args.max_others, 'max_idle_other_cores': args.max_idle,
                     'present': PRESENT[args.present]['label'], 'mangohud': mangohud_version(),
                     'mangohud_config': mangohud_config('LEG_FOLDER', args.present), **slow_rule()},
        'display': {'name': args.display, 'outputs': found, 'scanout': scans_out(found)},
        'scenarios': scenarios,
        'duration_s': round(time.monotonic() - started),
    }
    (args.output / 'report.json').write_text(json.dumps(report, indent=1))
    print(json.dumps([scenario['starts'] for scenario in scenarios]))


if __name__ == '__main__':
    main()

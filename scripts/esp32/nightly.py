#!/usr/bin/env python3
"""Measure release firmware builds on one physical ESP32 board."""

import argparse
import json
import os
import re
import selectors
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import board_run as board

BENCH = board.ROOT / 'benchmarks/esp32'
sys.path.insert(0, str(board.ROOT / 'scripts'))
from android_robot_device import device_lock

STARTS = ('first', 'second')
SECONDS = 5
PORTS = {'es3c35p': '/dev/serial/by-id/usb-Espressif_USB_JTAG_serial_debug_unit_*-if00',
         'e32r35t': '/dev/serial/by-id/usb-1a86_USB_Serial-if00-port0'}
SOURCES = {'cranpose': 'apps/esp32-demo/src/main.rs', 'lvgl': 'benchmarks/esp32/lvgl-app/main/main.c',
           'slint': 'benchmarks/esp32/slint-app/ui/dashboard.slint'}


def firmware(name, screen, model, log):
    if name == 'cranpose':
        features = [model, 'cranpose-platform-esp-idf/bench-frames']
        if screen == 'dashboard':
            features.append('dashboard')
        return board.build_rust(board.DEMO, 'esp32-demo', model, [], [], features, log)
    if name == 'slint':
        board.unpack_slint_core()
        return board.build_rust(board.SLINT, 'slint-bench', model, [BENCH / 'panel'],
                                [board.SLINT / 'sdkconfig.slint'], ['bench-frames'], log)
    settings = [BENCH / 'sdkconfig.frames']
    if screen == 'dashboard':
        settings.append(BENCH / 'sdkconfig.dashboard')
    return board.build_lvgl(model, None, settings, log)


def monitor(command, log):
    # The first command flashes and monitors the same boot. The next resets
    # the installed app. Both clocks start on the chip, before its UI starts.
    end = f'BENCH second {SECONDS - 1} frames '.encode()
    deadline = time.monotonic() + 120
    with log.open('wb') as out, subprocess.Popen(command, stdout=subprocess.PIPE,
                                                 stderr=subprocess.STDOUT) as process:
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(process.stdout, selectors.EVENT_READ)
                pending = b''
                while remaining := max(0, deadline - time.monotonic()):
                    if not ready.select(remaining):
                        break
                    chunk = os.read(process.stdout.fileno(), 8192)
                    if not chunk:
                        break
                    out.write(chunk)
                    lines = (pending + chunk).split(b'\n')
                    pending = lines.pop()
                    if any(end in line for line in lines):
                        return
        finally:
            process.kill()
    raise RuntimeError(f'no complete firmware trace in {log}')


def read_leg(path, name, start, panel_hz):
    """A boot's leg. `fps` counts the frames the app drew; `shown_fps`
    counts each second's frames up to what the panel scans in that second
    from the first frame on, `panel_hz` pictures a whole second, as no more
    of them reach the glass whole."""
    text = path.read_text(errors='replace')
    first = re.findall(r'BENCH first_ms (\d+)', text)
    bins = re.findall(r'BENCH second (\d+) frames (\d+)', text)
    period = re.findall(r'BENCH frame_period_us (\d+)', text)
    if len(first) != 1 or len(period) > 1 or [int(index) for index, _ in bins] != list(range(SECONDS)):
        raise ValueError(f'incomplete or repeated boot in {path}')
    counts = [int(frames) for _, frames in bins]
    first_ms = int(first[0])
    if not sum(counts) or first_ms // 1000 != next(i for i, count in enumerate(counts) if count):
        raise ValueError(f'first frame disagrees with frame counts in {path}')
    first_s = first_ms / 1000
    shown = [min(count, panel_hz * min(1.0, max(0.0, second + 1 - first_s)))
             for second, count in enumerate(counts)]
    return {'subject': name, 'start': start, 'run_s': SECONDS, 'frames': sum(counts),
            'fps': sum(counts) / SECONDS, 'shown_fps': round(sum(shown) / SECONDS, 3),
            'frame_period_ms': int(period[0]) / 1000 if period else None, 'first_frame_s': first_ms / 1000,
            'fps_by_second': counts,
            'samples': {'t_ms': list(range(1000, 1000 * SECONDS + 1, 1000)), 'fps': counts}}


def build_image(model, name, screen, stem):
    config = board.BOARDS[model]
    elf, bootloader, partitions = firmware(name, screen, model, stem)
    # A later build for another screen replaces the executable in place.
    shutil.copyfile(elf, stem.with_suffix('.elf'))
    shutil.copyfile(bootloader, stem.with_suffix('.bootloader'))
    image = stem.with_suffix('.bin')
    command = ['espflash', 'save-image', '--chip', config['mcu'], '--flash-mode', 'dio', '--flash-freq', '80mhz',
               '--flash-size', config['flash_size'], '--partition-table', str(partitions), str(elf), str(image)]
    board.run(command, board.ROOT, None, stem.with_suffix('.size'))


def measure(model, device, name, stem):
    partitions = board.DEMO / board.BOARDS[model]['partitions']
    elf = stem.with_suffix('.elf')
    flash = board.flash_command(model, device, stem.with_suffix('.bootloader'), partitions)
    commands = ([*flash, '--monitor', '--non-interactive', str(elf)],
                ['espflash', 'monitor', '--port', device, '--non-interactive'])
    legs = []
    with device_lock(str(Path(device).resolve())):
        for start, command in zip(STARTS, commands):
            log = stem.with_suffix(f'.{start}.log')
            monitor(command, log)
            legs.append(read_leg(log, name, start, board.BOARDS[model]['panel_hz']))
    return legs, stem.with_suffix('.bin').stat().st_size


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('phase', choices=('build', 'measure'))
    parser.add_argument('--board', choices=board.BOARDS, required=True)
    parser.add_argument('--port')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    port = args.port
    if args.phase == 'measure' and not port:
        pattern = PORTS[args.board] if sys.platform.startswith('linux') else board.BOARDS[args.board]['port']
        port = board.one(pattern, 'board port')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=board.ROOT, text=True).strip()
    lvgl = (board.LVGL / 'main/idf_component.yml').read_text()
    version = re.search(r'lvgl/lvgl:\s*"([^"]+)"', lvgl)[1]
    labels = {'cranpose': 'Cranpose ' + revision[:9], 'lvgl': f'LVGL {version}',
              'slint': 'Slint ' + board.SLINT_CORE.removeprefix('i-slint-core-')}
    report = {'kind': 'esp32', 'started_at': datetime.now(timezone.utc).isoformat(timespec='seconds'),
              'device': {'ro.product.model': args.board.upper()}, 'main': revision,
              'subjects': [{'name': key, 'label': label, 'source': SOURCES[key]} for key, label in labels.items()],
              'protocol': {'run_s': SECONDS, 'starts': STARTS, 'panel_hz': board.BOARDS[args.board]['panel_hz']},
              'scenarios': []}
    for screen in ('demo', 'dashboard'):
        names = ['cranpose', 'lvgl'] if screen == 'demo' else ['lvgl', 'slint']
        # The classic board has no PSRAM for the Cranpose dashboard.
        if screen == 'dashboard' and args.board == 'es3c35p':
            names.insert(0, 'cranpose')
        scenario = {'scenario': screen, 'app_bytes': {}, 'legs': [],
                    'starts': {start: {'summary': {}, 'verdicts': {}} for start in STARTS}}
        for name in names:
            print(f'{args.board} {screen} {name}', flush=True)
            stem = args.output / f'{screen}-{name}'
            if args.phase == 'build':
                build_image(args.board, name, screen, stem)
                continue
            legs, size = measure(args.board, port, name, stem)
            scenario['app_bytes'][name] = size
            scenario['legs'].extend(legs)
            for leg in legs:
                scenario['starts'][leg['start']]['summary'][name] = {
                    metric: leg[metric] for metric in ('fps', 'shown_fps', 'frame_period_ms', 'first_frame_s')
                    if leg[metric] is not None}
        report['scenarios'].append(scenario)
    if args.phase == 'measure':
        (args.output / 'report.json').write_text(json.dumps(report, indent=1) + '\n')


if __name__ == '__main__':
    main()

import argparse
import fcntl
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
import time

from PIL import Image

from android_surface_frames import inspect_video


def run_device(args):
    output = args.output
    output.mkdir(parents=True, exist_ok=False)
    adb = ['adb', '-s', args.serial]

    def command(*parts):
        return subprocess.check_output(adb + list(parts), text=True, stderr=subprocess.STDOUT, timeout=60)

    def wake():
        command('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        if 'mWakefulness=Awake' not in command('shell', 'dumpsys', 'power'):
            raise RuntimeError('Device screen is not awake')

    density = command('shell', 'wm', 'density')
    scale = int(re.findall(r'density: (\d+)', density)[-1]) / 160
    installed = command('shell', 'pm', 'path', args.package).strip().removeprefix('package:')
    proof = {'package': args.package, 'serial': args.serial, 'scale': scale,
             'installed_apk': command('shell', 'sha256sum', installed).strip()}
    (output / 'device.json').write_text(json.dumps(proof, indent=2) + '\n')
    command('shell', 'am', 'force-stop', args.package)
    wake()
    command('shell', 'am', 'start', '-W', '-n', args.package + '/dev.cranpose.android.CranposeActivity',
            '--es', 'test_screen', 'surface_prefix')
    time.sleep(2)
    initial = subprocess.check_output(adb + ['exec-out', 'screencap', '-p'], timeout=30)
    (output / 'start.png').write_bytes(initial)
    with Image.open(output / 'start.png') as image:
        width, height = image.size
    video = output / 'scroll.mp4'
    with (output / 'recorder.log').open('w') as log:
        recorder = subprocess.Popen([
            'scrcpy', '--serial=' + args.serial, '--no-playback', '--no-audio', '--no-control',
            '--video-bit-rate=4M', '--time-limit=60', '--record=' + str(video),
        ], stdout=log, stderr=subprocess.STDOUT)
        try:
            time.sleep(2)
            x, top, bottom = round(width * 0.14), round(height * 0.2), round(height * 0.82)
            for reverse in (False, True):
                for _ in range(4):
                    wake()
                    start, end = (top, bottom) if reverse else (bottom, top)
                    command('shell', 'input', 'swipe', str(x), str(start), str(x), str(end), '2100')
                wake()
                name = 'return.png' if reverse else 'turn.png'
                (output / name).write_bytes(subprocess.check_output(adb + ['exec-out', 'screencap', '-p'], timeout=30))
            if recorder.poll() is not None:
                raise RuntimeError('Recording ended before the return capture')
            if recorder.wait(timeout=60) != 0:
                raise RuntimeError('Recording failed')
        finally:
            if recorder.poll() is None:
                recorder.terminate()
                recorder.wait(timeout=40)
    inspect_video(video, output / 'frames', scale)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--serial')
    parser.add_argument('--package', default='com.compose_rs.demo')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--video', type=Path)
    parser.add_argument('--scale', type=float)
    args = parser.parse_args()
    if args.video:
        if not args.scale:
            parser.error('--video requires --scale')
        inspect_video(args.video, args.output, args.scale)
    else:
        if not args.serial:
            parser.error('device robot requires --serial')
        device_key = hashlib.sha256(args.serial.encode()).hexdigest()[:16]
        lock_path = Path(tempfile.gettempdir()) / ('cranpose-device-' + device_key + '.lock')
        with lock_path.open('a+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            run_device(args)


if __name__ == '__main__':
    main()

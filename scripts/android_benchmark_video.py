import json
import os
import select
import shlex
import signal
import subprocess
import time
import uuid

from android_benchmark_support import checked_command, digest
from android_visual_contract import inspect_visual_frames


def inspect_recording(video, size, report, visual_regions=None):
    probe = json.loads(checked_command([
        'ffprobe', '-v', 'error', '-count_frames', '-select_streams', 'v:0',
        '-show_entries', 'stream=width,height,nb_read_frames', '-of', 'json', str(video)]))
    streams = probe.get('streams', [])
    if (len(streams) != 1 or int(streams[0].get('nb_read_frames', '0')) < 2
            or [streams[0]['width'], streams[0]['height']] != size):
        raise ValueError('Recording did not contain full-size route frames')
    report.setdefault('video', {}).update(path=str(video), sha256=digest(video), probe=probe)
    if visual_regions is not None:
        result = inspect_visual_frames(video, size, visual_regions, video.parent / 'visual-frames')
        report['video']['visual_contract'] = result
        if result['failing_frames']:
            raise ValueError(f"Rendering continuity failed in {result['failing_frames']} recorded frames")


class AndroidRecording:
    def __init__(self, device, directory, size, report, bit_rate, visual_regions=None):
        self.device = device
        self.directory = directory
        self.size = size
        self.report = report
        self.bit_rate = bit_rate
        self.visual_regions = visual_regions
        self.remote = '/data/local/tmp/cranpose-recording-' + uuid.uuid4().hex + '.mp4'
        self.pid_file = self.remote + '.pid'
        self.process = None
        self.pid = None

    def start(self):
        self.device.shell('screenrecord', '--help')
        command = 'echo $$ > ' + shlex.quote(self.pid_file) + ' && echo $$ && exec ' + shlex.join([
            'screenrecord', '--bit-rate', str(self.bit_rate), '--time-limit', '180', self.remote])
        self.process = subprocess.Popen(
            self.device.command + ['shell', shlex.join(['sh', '-c', command])],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        ready, _, _ = select.select([self.process.stdout], [], [], 10)
        if not ready:
            raise RuntimeError('Device recorder did not report its process ID')
        line = self.process.stdout.readline().decode().strip()
        if not line.isdigit() or int(line) <= 1:
            raise ValueError('Invalid device recorder response: ' + line)
        self.pid = line
        self.report['video'] = {'remote': self.remote, 'pid': self.pid, 'bit_rate': self.bit_rate,
                                'purpose': 'Recording adds load; FPS is ineligible'}

    def finish(self):
        if self.process is None:
            return
        errors = []
        try:
            if self.pid is None:
                saved = self.device.shell('cat', self.pid_file).strip()
                if not saved.isdigit() or int(saved) <= 1:
                    raise ValueError('Recorder ownership file has no valid PID')
                self.pid = saved
            if self.process.poll() is not None:
                errors.append(RuntimeError('Device recording ended before route completion'))
            if self.pid:
                self.device.stop_owned_process(self.pid, self.remote, first_signal='INT')
            else:
                self.process.terminate()
            output, _ = self.process.communicate(timeout=15)
            (self.directory / 'recorder.log').write_bytes(output)
            if self.process.returncode not in {0, 130}:
                errors.append(RuntimeError('Device recorder failed: ' + output.decode(errors='replace')))
            video = self.directory / 'scroll.mp4'
            self.device.run('pull', self.remote, str(video), timeout=120)
            inspect_recording(video, self.size, self.report, self.visual_regions)
        except BaseException as error:
            errors.append(error)
        finally:
            try:
                if self.process.poll() is None:
                    self.process.terminate()
                    self.process.communicate(timeout=10)
            except BaseException as error:
                errors.append(error)
            for path in [self.remote, self.pid_file]:
                try:
                    self.device.shell('rm', '-f', path)
                except BaseException as error:
                    errors.append(error)
        if errors:
            raise BaseExceptionGroup('Recording failed; partial evidence retained', errors)


class ScrcpyRecording:
    def __init__(self, device, directory, size, report, bit_rate, duration_seconds=60,
                 visual_regions=None):
        self.device = device
        self.directory = directory
        self.size = size
        self.report = report
        self.bit_rate = bit_rate
        self.visual_regions = visual_regions
        self.duration_seconds = duration_seconds
        self.process = None
        self.log = None
        self.started_at = None

    def start(self):
        self.log = (self.directory / 'recorder.log').open('wb')
        command = ['scrcpy', '--serial=' + self.device.serial, '--no-playback', '--no-audio',
                   '--no-control', '--video-bit-rate=' + str(self.bit_rate),
                   '--time-limit=' + str(self.duration_seconds),
                   '--record=' + str(self.directory / 'scroll.mp4')]
        self.process = subprocess.Popen(command, stdout=self.log, stderr=subprocess.STDOUT,
                                        start_new_session=True)
        self.started_at = time.monotonic()
        self.report['video'] = {'command': command, 'pid': self.process.pid,
                                'purpose': 'Recording adds load; FPS is ineligible'}
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError('scrcpy exited before recording became ready')
            video = self.directory / 'scroll.mp4'
            if video.is_file():
                with video.open('rb') as file:
                    header = file.read(64)
                if header[4:8] == b'ftyp' and b'mdat' in header:
                    return
            time.sleep(0.1)
        raise TimeoutError('scrcpy did not initialize the recording container')

    def finish(self):
        errors = []
        try:
            if self.process is not None:
                if self.process.poll() is not None:
                    errors.append(RuntimeError('scrcpy ended before route completion'))
                try:
                    remaining = max(0, self.started_at + self.duration_seconds - time.monotonic())
                    self.process.wait(timeout=remaining + 15)
                except subprocess.TimeoutExpired as error:
                    errors.append(error)
                    os.killpg(self.process.pid, signal.SIGKILL)
                    self.process.wait(timeout=5)
                if self.process.returncode != 0:
                    errors.append(RuntimeError('scrcpy recording failed; see recorder.log'))
                inspect_recording(self.directory / 'scroll.mp4', self.size, self.report, self.visual_regions)
        except BaseException as error:
            errors.append(error)
        finally:
            if self.log is not None:
                self.log.close()
        if errors:
            raise BaseExceptionGroup('Recording failed; partial evidence retained', errors)

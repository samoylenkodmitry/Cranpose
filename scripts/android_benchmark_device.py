import hashlib
import io
import json
import re
import shlex
import subprocess
import time
import xml.etree.ElementTree as ET
import uuid

from PIL import Image

from android_benchmark_support import checked_command


class AndroidDevice:
    def __init__(self, serial, adb='adb', ocr=None, evidence=None):
        self.command = [adb, '-s', serial]
        self.serial = serial
        self.saved_properties = {}
        self.desired_properties = {}
        self.ocr = ocr
        self.evidence = evidence

    def run(self, *parts, **options):
        return checked_command(self.command + list(parts), **options)

    def shell(self, *parts, **options):
        return self.run('shell', shlex.join([str(part) for part in parts]), **options).decode()

    def wake(self):
        self.shell('input', 'keyevent', 'KEYCODE_WAKEUP')
        if 'mWakefulness=Awake' not in self.shell('dumpsys', 'power'):
            raise ValueError('Device is not awake')

    def properties(self):
        return dict(re.findall(r'^\[(debug\.cranpose\.[^\]]+)\]: \[([^\]]*)\]$',
                               self.shell('getprop'), re.M))

    def configure_properties(self, overrides):
        if any(not name.startswith('debug.cranpose.') for name in overrides):
            raise ValueError('Only Cranpose diagnostic properties can be changed')
        current = self.properties()
        self.desired_properties = dict.fromkeys(current, '') | overrides
        self.saved_properties = {name: current.get(name, '') for name in self.desired_properties}
        for name, value in self.desired_properties.items():
            if current.get(name, '') != value:
                self.shell('setprop', name, value)
        effective = self.properties()
        if any(effective.get(name, '') != value for name, value in self.desired_properties.items()):
            raise ValueError('Diagnostic property setup did not take effect')

    def restore_properties(self):
        if not self.saved_properties:
            return
        current = self.properties()
        errors = []
        for name, original in self.saved_properties.items():
            try:
                if current.get(name, '') not in {original, self.desired_properties[name]}:
                    raise ValueError('Diagnostic property changed outside the sequence: ' + name)
                if current.get(name, '') != original:
                    self.shell('setprop', name, original)
            except Exception as error:
                errors.append(error)
        restored = self.properties()
        if any(restored.get(name, '') != value for name, value in self.saved_properties.items()):
            errors.append(ValueError('Diagnostic property restoration did not match saved values'))
        if errors:
            raise ExceptionGroup('Diagnostic property restoration failed', errors)

    def screenshot(self, path, region):
        self.wake()
        data = self.run('exec-out', 'screencap', '-p')
        path.write_bytes(data)
        return screenshot_pixels(data, region)

    def endpoint(self, package, expected, size, region=None, text_source='accessibility', image_path=None):
        if not expected.strip():
            raise ValueError('Endpoint checks require nonempty text')
        if text_source == 'image':
            return self.visual_endpoint(expected, size, region, image_path)
        if text_source != 'accessibility':
            raise ValueError('Unsupported endpoint text source')
        self.wake()
        path = '/data/local/tmp/cranpose-endpoint-' + uuid.uuid4().hex + '.xml'
        try:
            self.shell('uiautomator', 'dump', '--compressed', path)
            text = self.shell('cat', path)
        finally:
            self.shell('rm', '-f', path)
        start = text.find('<?xml')
        end = text.rfind('</hierarchy>')
        if start < 0 or end < 0:
            raise ValueError('Accessibility hierarchy was not returned')
        hierarchy = ET.fromstring(text[start:end + len('</hierarchy>')])
        matches = []
        for node in hierarchy.iter('node'):
            label = node.get('text', '') + ' ' + node.get('content-desc', '')
            bounds = [int(value) for value in re.findall(r'-?\d+', node.get('bounds', ''))]
            if expected.casefold() not in label.casefold() or node.get('package') != package or len(bounds) != 4:
                continue
            left, top, right, bottom = bounds
            if region is not None and not (region[0] <= (left + right) / 2 <= region[2]
                                          and region[1] <= (top + bottom) / 2 <= region[3]):
                continue
            if min(right, size[0]) > max(left, 0) and min(bottom, size[1]) > max(top, 0):
                matches.append({'label': label, 'bounds': bounds})
        if not matches:
            raise ValueError('Visible route endpoint was not reached: ' + expected)
        return matches

    def visual_endpoint(self, expected, size, region, image_path):
        if self.ocr is None or self.evidence is None or region is None:
            raise ValueError('Image endpoints require a verified OCR helper, evidence directory and region')
        self.evidence.mkdir(parents=True, exist_ok=True)
        if image_path is None:
            self.wake()
        path = image_path or self.evidence / (uuid.uuid4().hex + '.png')
        pixels = screenshot_pixels(path.read_bytes(), region) if image_path else self.screenshot(path, region)
        if pixels['size'] != size:
            raise ValueError('Image endpoint has an unexpected display size')
        cropped = path.with_suffix('.crop.png')
        with Image.open(path) as image:
            image.crop(region).save(cropped)
        rows = json.loads(checked_command([str(self.ocr), str(cropped)]))
        path.with_suffix('.json').write_text(json.dumps(rows, indent=2) + '\n')
        text = ' '.join(row['text'] for row in rows)
        if expected.casefold() not in text.casefold():
            raise ValueError('Visible image endpoint was not reached: ' + expected)
        return {'image': str(path), 'pixels': pixels, 'text': text, 'rows': rows}

    def state(self):
        battery = self.shell('dumpsys', 'battery')
        temperature = re.search(r'temperature:\s*(-?\d+)', battery)
        if not temperature:
            raise ValueError('Device temperature was not reported')
        return {'battery': battery, 'temperature_c': int(temperature[1]) / 10,
                'thermal': self.shell('dumpsys', 'thermalservice'),
                'background_cpu': self.shell('dumpsys', 'cpuinfo')}

    def installed_apk(self, package):
        paths = self.shell('pm', 'path', package).splitlines()
        if not paths:
            return None
        if len(paths) != 1 or not paths[0].startswith('package:'):
            raise ValueError('Benchmark requires a single installed APK')
        path = paths[0].removeprefix('package:')
        return {'path': path, 'sha256': self.shell('sha256sum', path).split()[0]}

    def wait_for_pid(self, package, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                pid = self.shell('pidof', package).strip()
                if pid.isdigit() and int(pid) > 1:
                    return pid
            except subprocess.CalledProcessError:
                pass
            time.sleep(0.2)
        raise ValueError('Application did not report a PID before the launch deadline')

    def require_foreground(self, package, pid):
        current = self.shell('pidof', package).strip()
        activities = self.shell('dumpsys', 'activity', 'activities')
        resumed = [line for line in activities.splitlines() if 'ResumedActivity' in line or 'mResumed' in line]
        if current != pid or not any(package + '/' in line for line in resumed):
            raise ValueError('Measured application restarted or left the foreground')
        return resumed

    def stop_owned_process(self, pid, marker, first_signal='TERM'):
        if not str(pid).isdigit() or int(pid) <= 1 or not marker:
            raise ValueError('Process cleanup requires a positive PID and unique ownership marker')
        def owned():
            command = self.shell('sh', '-c', f'if test -d /proc/{pid}; then cat /proc/{pid}/cmdline; fi')
            return marker in command
        for signal_name, timeout in [(first_signal, 5), ('KILL', 2)]:
            if not owned():
                return
            self.shell('kill', '-' + signal_name, str(pid))
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if not owned():
                    return
                time.sleep(0.1)
        raise RuntimeError('Owned device process did not terminate: ' + str(pid))


def screenshot_pixels(data, region):
    with Image.open(io.BytesIO(data)) as image:
        image.load()
        colors = image.convert('RGB')
        if max(max(channel) for channel in colors.getextrema()) < 16:
            raise ValueError('Device screenshot is black')
        left, top, right, bottom = region
        if not 0 <= left < right <= image.width or not 0 <= top < bottom <= image.height:
            raise ValueError('Motion region falls outside the screenshot')
        return {'sha256': hashlib.sha256(data).hexdigest(), 'size': list(image.size),
                'motion_pixels_sha256': hashlib.sha256(colors.crop(region).tobytes()).hexdigest()}


def surface_frames(text, package):
    layers = []
    for block in re.split(r'(?=layerName = )', text):
        name = re.search(r'layerName = (.*)', block)
        frames = re.search(r'totalFrames = (\d+)', block)
        if name and frames and package + '/' in name[1]:
            layers.append({'name': name[1], 'frames': int(frames[1])})
    if len(layers) != 1 or layers[0]['frames'] == 0:
        raise ValueError('Expected one active presentation surface: ' + str(layers))
    return layers[0]


def verify_route_window(output, count, duration_ms, period_ms, window_ms, tolerance_ms):
    gestures = [tuple(map(int, match)) for match in re.findall(r'gesture=(\d+) start=(\d+) end=(\d+)', output)]
    elapsed = re.search(r'elapsed_ms=(\d+)', output)
    if not elapsed or len(gestures) != count:
        raise ValueError('Device input sequence was incomplete')
    if any(index != expected or abs(start - expected * period_ms) > tolerance_ms
           or end - start < duration_ms
           for expected, (index, start, end) in enumerate(gestures)):
        raise ValueError('Device input sequence missed its timing contract')
    elapsed_ms = int(elapsed[1])
    if not window_ms <= elapsed_ms <= window_ms + tolerance_ms:
        raise ValueError('Device measurement window overran its timing contract')
    stats = re.search(r'SF_BEGIN\n(.*?)SF_END', output, re.S)
    if not stats:
        raise ValueError('Device did not return presentation statistics')
    return elapsed_ms / 1000, stats[1]

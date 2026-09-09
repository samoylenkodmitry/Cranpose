from contextlib import contextmanager
import fcntl
import hashlib
from pathlib import Path
import subprocess
import tempfile


class AndroidRobotDevice:
    def __init__(self, serial):
        self.adb = ['adb', '-s', serial]

    def command(self, *parts, timeout=60):
        return subprocess.check_output(self.adb + list(parts), text=True,
                                       stderr=subprocess.STDOUT, timeout=timeout)

    def wake(self):
        self.command('shell', 'input', 'keyevent', 'KEYCODE_WAKEUP')
        if 'mWakefulness=Awake' not in self.command('shell', 'dumpsys', 'power'):
            raise RuntimeError('Device screen is not awake')


@contextmanager
def device_lock(serial):
    key = hashlib.sha256(serial.encode()).hexdigest()[:16]
    path = Path(tempfile.gettempdir()) / ('cranpose-device-' + key + '.lock')
    with path.open('a+') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


@contextmanager
def locked_device(serial):
    with device_lock(serial):
        yield AndroidRobotDevice(serial)

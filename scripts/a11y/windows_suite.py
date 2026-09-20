import os
from pathlib import Path
import subprocess
import sys


def main():
    if sys.platform != 'win32':
        raise RuntimeError('This suite requires a native Windows desktop session')
    output = Path('target/windows-accessibility')
    output.mkdir(parents=True, exist_ok=False)
    subprocess.run([
        sys.executable, 'scripts/a11y/desktop_robot.py',
        '--binary', 'target/ci/desktop-app.exe', '--output', str(output / 'native'),
    ], check=True, timeout=180)
    environment = dict(os.environ, CRANPOSE_INSPECTOR_ARTIFACTS=str(output / 'inspector'))
    with (output / 'inspector.log').open('w') as log:
        subprocess.run(['target/ci/examples/robot_developer_inspector.exe'],
                       env=environment, stdout=log, stderr=subprocess.STDOUT,
                       check=True, timeout=120)
    if 'PASS: floating inspector' not in (output / 'inspector.log').read_text():
        raise AssertionError('The inspector robot did not reach its final assertions')


if __name__ == '__main__':
    main()

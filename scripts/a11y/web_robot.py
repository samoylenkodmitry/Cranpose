import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
from threading import Thread


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--site', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--chrome', default=os.environ.get('CHROME'))
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    if not (args.site / 'index.html').is_file():
        raise RuntimeError('Pass the packaged release website directory')
    chrome = args.chrome or (shutil.which('chromium') or shutil.which('google-chrome'))
    if chrome is None and sys.platform == 'darwin':
        chrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    if not chrome:
        raise RuntimeError('Set CHROME to a Chrome or Chromium executable')
    handler = partial(SimpleHTTPRequestHandler, directory=str(args.site.resolve()))
    with ThreadingHTTPServer(('127.0.0.1', 0), handler) as server:
        serving = Thread(target=server.serve_forever)
        serving.start()
        process = None
        try:
            url = f'http://127.0.0.1:{server.server_port}/'
            environment = dict(os.environ, CHROME=chrome, A11Y_DEBUG_PORT='0')
            with (args.output / 'browser.log').open('w') as log:
                process = subprocess.Popen(
                    ['node', 'scripts/a11y/web-page-check.mjs', url, str(args.output.resolve())],
                    env=environment, stdout=log, stderr=subprocess.STDOUT,
                    start_new_session=os.name == 'posix')
                status = process.wait(timeout=180)
            if status:
                raise RuntimeError(f'Browser robot failed ({status}); see {args.output}/browser.log')
        finally:
            if process is not None and process.poll() is None:
                if os.name == 'posix':
                    os.killpg(process.pid, signal.SIGTERM)
                else:
                    subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], check=True)
                process.wait(timeout=10)
            server.shutdown()
            serving.join(timeout=10)


if __name__ == '__main__':
    main()

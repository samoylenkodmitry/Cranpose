"""`BrowserServer` as a page meets it: it serves a built app with the console
script first in its head, hears the lines the script posts back, and tells a
run waiting on a page what that page said when it failed or stayed silent."""

import sys
import tempfile
import threading
import time
import unittest
import urllib.request
from pathlib import Path

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))

from browser import BrowserServer  # noqa: E402

PAGE = '<!doctype html>\n<html><head><meta charset="utf-8"><title>app</title></head><body>app</body></html>'


def fetch(server, path):
    with urllib.request.urlopen(f'{server.base}{path}') as response:
        return response.read().decode(), response.headers


def post(server, text):
    request = urllib.request.Request(f'{server.base}/__perf', data=text.encode(), method='POST')
    urllib.request.urlopen(request).close()


class BrowserServerTest(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        (Path(folder.name) / 'app').mkdir()
        (Path(folder.name) / 'app' / 'index.html').write_text(PAGE)
        self.server = BrowserServer(Path(folder.name))
        self.addCleanup(self.server.server.server_close)
        self.addCleanup(self.server.server.shutdown)

    def test_a_page_gets_the_script_after_head_opens_and_before_its_own_markup(self):
        body, headers = fetch(self.server, '/app/index.html?tier=8&freeze=0')
        self.assertIn('<head><script src="/__perf.js"></script><meta charset', body)
        self.assertTrue(body.startswith('<!doctype html>'))
        self.assertEqual(headers['Cross-Origin-Opener-Policy'], 'same-origin')
        self.assertEqual(headers['Cache-Control'], 'max-age=3600',
                         "a page's second start takes its files from the browser's cache")
        script, _ = fetch(self.server, '/__perf.js')
        self.assertIn('sendBeacon', script)

    def test_a_line_is_heard_with_the_time_it_arrived(self):
        since = self.server.mark()
        self.assertIsNone(self.server.heard_at('PERF first_frame', since))
        before = time.monotonic()
        post(self.server, 'PERF first_frame')
        post(self.server, 'PERF first_frame again')
        arrived = self.server.heard_at('PERF first_frame', since)
        self.assertLessEqual(before, arrived)
        self.assertLessEqual(arrived, time.monotonic())
        self.assertEqual(arrived, self.server.heard[since][0])

    def test_a_posted_line_ends_the_wait_that_names_it(self):
        since = self.server.mark()
        threading.Timer(0.1, post, (self.server, 'PERF ready tier=8')).start()
        self.assertEqual(self.server.wait_for('PERF ready', 5, since), 'PERF ready tier=8')

    def test_a_line_from_before_the_mark_is_not_heard_again(self):
        post(self.server, 'PERF ready old')
        since = self.server.mark()
        with self.assertRaisesRegex(RuntimeError, 'no "PERF ready" within 0.3 s'):
            self.server.wait_for('PERF ready', 0.3, since)

    def test_a_page_error_ends_the_wait_with_what_the_page_said(self):
        since = self.server.mark()
        post(self.server, 'PERF console.error: shader failed to compile')
        post(self.server, 'PERF error Uncaught RuntimeError: unreachable')
        with self.assertRaisesRegex(RuntimeError, 'the page failed: PERF error Uncaught RuntimeError.*shader failed'):
            self.server.wait_for('PERF ready', 5, since)

    def test_a_gpu_device_error_since_the_mark_fails_the_run(self):
        post(self.server, 'PERF console.error: [gpu-device] the WebGL context was lost')
        since = self.server.mark()
        self.server.check_device(since)
        post(self.server, 'PERF ready tier=8')
        self.server.check_device(since)
        # wasm-logger's styled arguments, as the console script joins them.
        post(self.server, 'PERF console.error: %cERROR%c render.rs:1068%c [gpu-device] device lost (Unknown): gone')
        with self.assertRaisesRegex(RuntimeError, r'the page lost its GPU device: .*device lost \(Unknown\)'):
            self.server.check_device(since)

    def test_a_silent_page_times_out_with_its_last_lines(self):
        since = self.server.mark()
        post(self.server, 'PERF loading')
        with self.assertRaisesRegex(RuntimeError, 'the page said: PERF loading'):
            self.server.wait_for('PERF ready', 0.3, since)


if __name__ == '__main__':
    unittest.main()


class NativePageServerTest(unittest.TestCase):
    """The page a native web view loads: no isolation headers, and a path
    prefix served from another folder."""

    def test_a_native_page_gets_the_script_its_aliases_and_no_isolation(self):
        page, fonts = tempfile.TemporaryDirectory(), tempfile.TemporaryDirectory()
        self.addCleanup(page.cleanup)
        self.addCleanup(fonts.cleanup)
        (Path(page.name) / 'index.html').write_text(PAGE)
        (Path(fonts.name) / 'Roboto.ttf').write_text('font')
        server = BrowserServer(Path(page.name), isolate=False, aliases={'/fonts/': fonts.name})
        self.addCleanup(server.server.server_close)
        self.addCleanup(server.server.shutdown)
        body, headers = fetch(server, '/index.html')
        self.assertIn('/__perf.js', body)
        self.assertIsNone(headers['Cross-Origin-Opener-Policy'])
        self.assertEqual(fetch(server, '/fonts/Roboto.ttf')[0], 'font')

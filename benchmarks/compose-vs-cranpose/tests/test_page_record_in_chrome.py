"""The console script in a real Chrome: a page served by `BrowserServer` that
asks for frames for a while, then stops, loses its WebGL context and logs an
error, leaves all of it in its leg's record. Needs Chrome or Chromium (`CHROME`,
or one on the path); skipped without one."""

import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

BENCHMARK = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(BENCHMARK))

from browser import BrowserServer  # noqa: E402

MAC_CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# Frames for half a second, then none; the context is lost after a second.
PAGE = """<!doctype html>
<html><head><meta charset="utf-8"></head><body><canvas id="c" width="8" height="8"></canvas><script>
var gl = document.getElementById('c').getContext('webgl');
var until = performance.now() + 500;
var frame = function () { if (performance.now() < until) requestAnimationFrame(frame); };
requestAnimationFrame(frame);
setTimeout(function () {
  console.error('the page failed on purpose');
  if (gl) gl.getExtension('WEBGL_lose_context').loseContext();
  else console.log('PERF test no webgl');
}, 1000);
console.log('PERF first_frame');
</script></body></html>"""


def chrome():
    found = os.environ.get('CHROME') or shutil.which('chromium') or shutil.which('google-chrome')
    if not found and Path(MAC_CHROME).exists():
        found = MAC_CHROME
    return found


def tied_chrome(arguments):
    """Headless Chrome that quits once its DevTools pipe on fds 3 and 4
    closes, as `launchChrome` in scripts/a11y/chrome.mjs starts it: this
    process holds the pipe's other ends, so Chrome ends with it even after a
    SIGKILL, and leaves no Chrome to move a later measure on the host."""
    commands, to_chrome = os.pipe()
    from_chrome, replies = os.pipe()

    def wire():
        os.dup2(commands, 3)
        os.dup2(replies, 4)

    browser = subprocess.Popen(
        # Without the store and keychain switches Chrome on a Linux runner
        # waits for a desktop keyring and never loads a page.
        [chrome(), '--headless=new', '--remote-debugging-pipe', '--no-first-run', '--no-default-browser-check',
         '--password-store=basic', '--use-mock-keychain', '--enable-unsafe-swiftshader', *arguments],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, preexec_fn=wire, pass_fds=(3, 4))
    os.close(commands)
    os.close(replies)
    return browser, (to_chrome, from_chrome)


@unittest.skipUnless(chrome(), 'no Chrome or Chromium')
class PageRecordInChromeTest(unittest.TestCase):
    def test_a_page_that_stops_asking_for_frames_and_loses_its_context_says_so(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        (Path(folder.name) / 'app').mkdir()
        (Path(folder.name) / 'app' / 'index.html').write_text(PAGE)
        server = BrowserServer(Path(folder.name))
        self.addCleanup(server.server.server_close)
        self.addCleanup(server.server.shutdown)
        since, started = server.mark(), time.monotonic()
        browser, pipe = tied_chrome([f'--user-data-dir={folder.name}/profile', f'{server.base}/app/index.html'])
        for end in pipe:
            self.addCleanup(os.close, end)
        self.addCleanup(browser.wait, 10)
        self.addCleanup(browser.kill)
        server.wait_for('PERF first_frame', 60, since)
        # Two beats after the context's loss.
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            record = server.page_record(since, started, time.monotonic())
            if record and record['gpu_events'] and record['last_beat_s'] > record['gpu_events'][0][0] + 1.5:
                break
            time.sleep(0.2)
        self.assertIsNotNone(record, 'the page posted no line of its own')
        self.assertEqual(record['visibility'][0][1], 'visible')
        self.assertFalse(record['hidden'])
        self.assertEqual([event for _, event, _ in record['gpu_events']][:1], ['context_lost'])
        self.assertIn('console.error: the page failed on purpose', [text for _, text in record['errors']])
        # The page asked for about 30 frames in its half second, then no more:
        # its last request came well before its last beat.
        self.assertGreater(record['frame_requests'], 5)
        self.assertGreater(record['last_beat_s'] - record['last_frame_request_s'], 1.0)


if __name__ == '__main__':
    unittest.main()

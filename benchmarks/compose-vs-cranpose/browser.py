"""The gauntlet's apps built for a browser, and the server they run from.

`build_apps.sh browser DIR` makes `DIR/APP/index.html` for each app that
targets the browser; `desktop.py --browser DIR` and `frameworks_browser.py`
open each in Chrome with `?tier=N&freeze=K`, as the other apps get theirs on a
command line or in intent extras.

The server serves the folder and adds one script to every page that comes from
it. The script forwards the app's `PERF` console lines to the server, which a
phone's Chrome needs: it logs a page's console nowhere a harness can read.
"""

import http.server
import re
import socketserver
import threading
import time

# Every app reports through the console, whatever its framework calls it:
# `log::info!`, `print`, `println`. The wrapped methods still print there.
PERF_SCRIPT = """\
(function () {
  var report = function (text) {
    try { navigator.sendBeacon('/__perf', text); } catch (error) { }
  };
  ['log', 'info', 'debug', 'warn'].forEach(function (method) {
    var original = console[method].bind(console);
    console[method] = function () {
      try {
        var text = Array.prototype.map.call(arguments, String).join(' ');
        if (text.indexOf('PERF ') >= 0) report(text);
      } catch (error) { }
      original.apply(null, arguments);
    };
  });
  // winit on the web leaves its event loop's run by throwing: not a failure.
  var failure = function (text) {
    if (String(text).indexOf('Using exceptions for control flow') < 0) report('PERF error ' + text);
  };
  window.addEventListener('error', function (event) { failure(event.message); });
  window.addEventListener('unhandledrejection', function (event) { failure(event.reason); });
})();
"""
SCRIPT_TAG = b'<script src="/__perf.js"></script>'


def with_script(page):
    """The page with the script first in its head: before the doctype it
    would put the page in quirks mode."""
    for opening in (rb'(<head[^>]*>)', rb'(<html[^>]*>)', rb'(<!doctype[^>]*>)'):
        page, found = re.subn(opening, lambda match: match.group(1) + SCRIPT_TAG, page, count=1,
                              flags=re.IGNORECASE)
        if found:
            return page
    return SCRIPT_TAG + page


class BrowserServer:
    """Serves `root` on a free local port and hears the pages' `PERF` lines."""

    def __init__(self, root, inject=True):
        self.root = root
        self.heard = []
        self.condition = threading.Condition()
        owner = self

        class Handler(http.server.SimpleHTTPRequestHandler):
            extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                              '.wasm': 'application/wasm', '.mjs': 'text/javascript'}

            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(root), **kwargs)

            def end_headers(self):
                # Cross-origin isolation, which Flutter's and Kotlin's
                # multi-threaded WebAssembly builds ask for, and no caching:
                # a build is not the one the last run served.
                self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
                self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
                self.send_header('Cache-Control', 'no-store')
                super().end_headers()

            def do_GET(self):
                path = self.path.split('?')[0]
                if path == '/__perf.js':
                    self.reply(PERF_SCRIPT.encode(), 'text/javascript')
                elif inject and (path.endswith(('.html', '/'))):
                    page = self.translate_path(path)
                    if path.endswith('/'):
                        page += '/index.html'
                    try:
                        body = open(page, 'rb').read()
                    except OSError:
                        self.send_error(404)
                        return
                    self.reply(with_script(body), 'text/html; charset=utf-8')
                else:
                    super().do_GET()

            def do_POST(self):
                length = int(self.headers.get('Content-Length', 0))
                owner.hear(self.rfile.read(length).decode(errors='replace'))
                self.send_response(204)
                self.end_headers()

            def reply(self, body, content_type):
                self.send_response(200)
                self.send_header('Content-Type', content_type)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        self.server = socketserver.ThreadingTCPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    @property
    def base(self):
        return f'http://127.0.0.1:{self.port}'

    def url(self, app, tier, freeze=0):
        return f'{self.base}/{app}/index.html?tier={tier}&freeze={freeze}'

    def hear(self, text):
        with self.condition:
            self.heard.append(text)
            self.condition.notify_all()

    def mark(self):
        """A place in what was heard, for `wait_for` to count from."""
        return len(self.heard)

    def wait_for(self, text, timeout, since=0):
        """Waits until a page has reported a line holding `text`; the page's
        own error, if it reports one first, ends the wait."""
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                for line in self.heard[since:]:
                    if text in line:
                        return line
                    if line.startswith('PERF error'):
                        raise RuntimeError(f'the page failed: {line}')
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError(f'no "{text}" within {timeout} s')
                self.condition.wait(remaining)

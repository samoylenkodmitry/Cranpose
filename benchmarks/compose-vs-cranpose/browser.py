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
from pathlib import Path

# Every app reports through the console, whatever its framework calls it:
# `log::info!`, `print`, `println`. The wrapped methods still print there.
PERF_SCRIPT = """\
(function () {
  var report = function (text) {
    try { navigator.sendBeacon('/__perf', text); } catch (error) { }
  };
  ['log', 'info', 'debug', 'warn', 'error'].forEach(function (method) {
    var original = console[method].bind(console);
    console[method] = function () {
      try {
        var text = Array.prototype.map.call(arguments, String).join(' ');
        if (text.indexOf('PERF ') >= 0) report(text);
        // What a page says when it fails is what a run's log needs, though
        // an error here does not end the run.
        else if (method === 'error') report('PERF console.error: ' + text.slice(0, 600));
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
  // What a leg's record needs of the page (`BrowserServer.page_record`): when
  // it was hidden, when it lost its WebGL context or its WebGPU device, and
  // until when it asked for frames. A beat each second carries the page's
  // clock and its last frame request: a beat that stops says the page's
  // thread stopped running.
  var now = function () { return Math.round(performance.now()); };
  var visibility = function () { report('PERF page visibility ' + document.visibilityState + ' ' + now()); };
  visibility();
  document.addEventListener('visibilitychange', visibility);
  var lastRequest = -1, requests = 0;
  var requestFrame = window.requestAnimationFrame;
  window.requestAnimationFrame = function (callback) {
    lastRequest = performance.now();
    requests++;
    return requestFrame.call(window, callback);
  };
  setInterval(function () {
    report('PERF page beat ' + now() + ' ' + Math.round(lastRequest) + ' ' + requests);
  }, 1000);
  var watched = new WeakSet();
  [window.HTMLCanvasElement, window.OffscreenCanvas].forEach(function (type) {
    if (!type) return;
    var getContext = type.prototype.getContext;
    type.prototype.getContext = function () {
      if (!watched.has(this)) {
        watched.add(this);
        this.addEventListener('webglcontextlost', function (event) {
          report('PERF page context_lost ' + now() + ' ' + (event.statusMessage || ''));
        });
        this.addEventListener('webglcontextrestored', function () { report('PERF page context_restored ' + now()); });
      }
      return getContext.apply(this, arguments);
    };
  });
  if (window.GPUAdapter) {
    var requestDevice = GPUAdapter.prototype.requestDevice;
    GPUAdapter.prototype.requestDevice = function () {
      return requestDevice.apply(this, arguments).then(function (device) {
        device.lost.then(function (info) {
          report('PERF page device_lost ' + now() + ' ' + info.reason + ' ' + String(info.message).slice(0, 300));
        });
        return device;
      });
    };
  }
})();
"""
SCRIPT_TAG = b'<script src="/__perf.js"></script>'
# The lines the script posts of the page itself, times on the page's clock in
# milliseconds; a beat's request is -1 before the page's first.
PAGE_VISIBILITY = re.compile(r'PERF page visibility (?P<state>\w+) \d+$')
PAGE_BEAT = re.compile(r'PERF page beat (?P<clock>\d+) (?P<request>-?\d+) (?P<requests>\d+)$')
PAGE_GPU_EVENT = re.compile(r'PERF page (?P<event>context_lost|context_restored|device_lost) \d+ ?(?P<detail>.*)$',
                            re.DOTALL)
# A page that logs an error each frame would fill its leg's record.
PAGE_ERRORS_KEPT = 20


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
    """Serves `root` on a free local port and hears the pages' `PERF` lines,
    each with the `time.monotonic()` it arrived at. `isolate` sends the
    headers a browser app needs; `aliases` serves a path prefix from another
    folder."""

    def __init__(self, root, inject=True, isolate=True, aliases=None):
        self.root = root
        # (time.monotonic() on arrival, line)
        self.heard = []
        self.condition = threading.Condition()
        owner = self

        class Handler(http.server.SimpleHTTPRequestHandler):
            extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map,
                              '.wasm': 'application/wasm', '.mjs': 'text/javascript'}

            def __init__(self, *args, **kwargs):
                super().__init__(*args, directory=str(root), **kwargs)

            def translate_path(self, path):
                for prefix, folder in (aliases or {}).items():
                    if path.startswith(prefix):
                        return str(Path(folder) / Path(path.split('?')[0]).name)
                return super().translate_path(path)

            def end_headers(self):
                # Cross-origin isolation, which Flutter's and Kotlin's
                # multi-threaded WebAssembly builds ask for. A page's second
                # start takes its files, and the browser the code it compiled
                # from them, from the browser's cache, as a site's second
                # visit does: each first start begins in a browser whose data
                # was cleared, so no earlier run's build is in that cache.
                if isolate:
                    self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
                    self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
                    self.send_header('Cache-Control', 'max-age=3600')
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
                        body = Path(page).read_bytes()
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
            self.heard.append((time.monotonic(), text))
            self.condition.notify_all()

    def heard_at(self, text, since=0):
        """When the first line holding `text` since `since` arrived, on
        `time.monotonic()`; None while none has."""
        with self.condition:
            return next((at for at, line in self.heard[since:] if text in line), None)

    def mark(self):
        """A place in what was heard, for `wait_for` to count from."""
        return len(self.heard)

    def page_record(self, since, started, ended):
        """What the page of a leg said of itself in the lines heard since
        `since` that arrived between `started` and `ended` (both on
        `time.monotonic()`), each at the seconds after `started` it arrived:
        its visibility at its start and at each change, its WebGL context
        losses and restores and its WebGPU device losses, its console errors
        and uncaught errors (a Rust panic logs one; Cranpose logs a
        `[gpu-device]` one when it loses its device), its last beat and its
        last frame request. `hidden` says the page was hidden after it was
        shown, or never shown: a page that opens hidden while its tab comes
        up is not. None when no page spoke."""
        with self.condition:
            lines = [(round(at - started, 2), line) for at, line in self.heard[since:] if started <= at <= ended]
        if not any(line.startswith('PERF page ') for _, line in lines):
            return None
        record = {'hidden': False, 'visibility': [], 'gpu_events': [], 'errors': [], 'error_count': 0,
                  'last_beat_s': None, 'frame_requests': 0, 'last_frame_request_s': None}
        for at, line in lines:
            if found := PAGE_VISIBILITY.match(line):
                record['visibility'].append([at, found['state']])
            elif found := PAGE_BEAT.match(line):
                clock, request = int(found['clock']), int(found['request'])
                record['last_beat_s'] = at
                record['frame_requests'] = int(found['requests'])
                # The beat's own clock dates its last request on the host's.
                record['last_frame_request_s'] = round(at - (clock - request) / 1000, 2) if request >= 0 else None
            elif found := PAGE_GPU_EVENT.match(line):
                record['gpu_events'].append([at, found['event'], found['detail'][:300]])
            elif line.startswith(('PERF console.error', 'PERF error')):
                record['error_count'] += 1
                if len(record['errors']) < PAGE_ERRORS_KEPT:
                    record['errors'].append([at, line[len('PERF '):][:300]])
        states = [state for _, state in record['visibility']]
        shown = next((index for index, state in enumerate(states) if state == 'visible'), None)
        record['hidden'] = bool(states) and (shown is None or 'hidden' in states[shown:])
        return record

    def wait_for(self, text, timeout, since=0):
        """Waits until a page has reported a line holding `text`; the page's
        own error, if it reports one first, ends the wait."""
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                for _, line in self.heard[since:]:
                    if text in line:
                        return line
                    if line.startswith('PERF error'):
                        said = ' | '.join(heard[:300] for _, heard in self.heard[since:] if 'console.error' in heard)
                        raise RuntimeError(f'the page failed: {line}' + (f' ({said})' if said else ''))
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    said = ' | '.join(line[:200] for _, line in self.heard[max(since, len(self.heard) - 3):])
                    raise RuntimeError(f'no "{text}" within {timeout} s' + (f'; the page said: {said}' if said else ''))
                self.condition.wait(remaining)

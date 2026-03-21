#!/usr/bin/env python3

from __future__ import annotations

import argparse
import contextlib
import http.server
import json
import shutil
import socket
import socketserver
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT_DIR = Path(__file__).resolve().parents[1]
DEMO_DIR = ROOT_DIR / "apps" / "desktop-demo"
DEFAULT_ARTIFACT_DIR = Path("/tmp") / "cranpose_web_gl_diag"
DEFAULT_FAIL_PATTERNS = [
    "CONTEXT_LOST_WEBGL",
    "Graphics context lost",
    "Failed to initialize app:",
]
DEFAULT_RELEVANT_PATTERNS = [
    "Graphics startup preference:",
    "Web backend preference=",
    "Applying startup tab override:",
    "CONTEXT_LOST_WEBGL",
    "Graphics context lost",
    "RuntimeShader",
    "Failed to initialize app:",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Build the desktop web demo, open it in Chrome on the requested backend, "
            "capture browser console output, and shut everything down."
        )
    )
    parser.add_argument("--backend", default="gl", help="Graphics backend query param.")
    parser.add_argument("--tab", default="shaders", help="Initial demo tab query param.")
    parser.add_argument(
        "--build-mode",
        choices=("fast", "release"),
        default="fast",
        help="Build mode passed to apps/desktop-demo/build-web.sh.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        help="Maximum seconds to wait for browser diagnostics.",
    )
    parser.add_argument(
        "--post-match-wait",
        type=float,
        default=2.0,
        help="Extra seconds to keep collecting logs after the first fail pattern appears.",
    )
    parser.add_argument(
        "--window-size",
        default="1920,1200",
        help="Chrome window size, WIDTH,HEIGHT.",
    )
    parser.add_argument(
        "--artifact-dir",
        default=str(DEFAULT_ARTIFACT_DIR),
        help="Directory where log artifacts are written.",
    )
    parser.add_argument(
        "--chrome-binary",
        default="google-chrome-stable",
        help="Chrome binary to use.",
    )
    parser.add_argument(
        "--chromedriver-binary",
        default="chromedriver",
        help="Chromedriver binary to use.",
    )
    parser.add_argument(
        "--chrome-arg",
        action="append",
        default=[],
        help="Additional raw Chrome argument. Repeatable.",
    )
    parser.add_argument(
        "--headful",
        action="store_true",
        help="Run Chrome with a visible window instead of headless mode.",
    )
    return parser.parse_args()


def choose_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args: Any, directory: str, **kwargs: Any) -> None:
        super().__init__(*args, directory=directory, **kwargs)

    def log_message(self, format: str, *args: Any) -> None:
        return


class ThreadingTCPServer(socketserver.ThreadingMixIn, socketserver.TCPServer):
    allow_reuse_address = True
    daemon_threads = True


@contextlib.contextmanager
def serve_directory(directory: Path, port: int) -> Any:
    server = ThreadingTCPServer(
        ("127.0.0.1", port),
        lambda *args, **kwargs: QuietHandler(*args, directory=str(directory), **kwargs),
    )
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5.0)


class OutputCollector:
    def __init__(self, process: subprocess.Popen[str]) -> None:
        self._lines: list[str] = []
        self._thread = threading.Thread(target=self._consume, args=(process,), daemon=True)
        self._thread.start()

    def _consume(self, process: subprocess.Popen[str]) -> None:
        assert process.stdout is not None
        for line in process.stdout:
            self._lines.append(line.rstrip("\n"))

    def lines(self) -> list[str]:
        return list(self._lines)


def run_build(build_mode: str) -> None:
    build_arg = "--release" if build_mode == "release" else "--fast"
    command = ["./build-web.sh", build_arg]
    print(f"Building web demo with {' '.join(command)}", flush=True)
    subprocess.run(command, cwd=DEMO_DIR, check=True)


def resolve_executable(name_or_path: str) -> str:
    candidate = Path(name_or_path)
    if candidate.is_absolute():
        return str(candidate)
    resolved = shutil.which(name_or_path)
    if resolved is None:
        raise RuntimeError(f"Executable not found: {name_or_path}")
    return resolved


@contextlib.contextmanager
def start_chromedriver(binary: str, port: int) -> Any:
    process = subprocess.Popen(
        [binary, f"--port={port}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        cwd=ROOT_DIR,
    )
    collector = OutputCollector(process)
    try:
        wait_for_http_ready(f"http://127.0.0.1:{port}/status", timeout=10.0)
        yield process, collector
    finally:
        terminate_process(process)


def terminate_process(process: subprocess.Popen[Any]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5.0)


def wait_for_http_ready(url: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=2.0) as response:
                if response.status < 500:
                    return
        except urllib.error.URLError:
            time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {url}")


class WebDriverClient:
    def __init__(self, endpoint: str) -> None:
        self.endpoint = endpoint.rstrip("/")
        self.session_id: str | None = None

    def _request(self, method: str, path: str, payload: dict[str, Any] | None = None) -> dict[str, Any]:
        body = None
        headers = {"Content-Type": "application/json; charset=utf-8"}
        if payload is not None:
            body = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            f"{self.endpoint}{path}",
            data=body,
            headers=headers,
            method=method,
        )
        try:
            with urllib.request.urlopen(request, timeout=15.0) as response:
                data = response.read()
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"WebDriver {method} {path} failed: {exc.code} {detail}") from exc
        return json.loads(data.decode("utf-8"))

    def create_session(
        self,
        chrome_binary: str,
        headless: bool,
        window_size: str,
        extra_args: list[str],
    ) -> None:
        chrome_args = [
            "--no-first-run",
            "--no-default-browser-check",
            "--enable-gpu",
            "--enable-webgl",
            "--ignore-gpu-blocklist",
            f"--window-size={window_size}",
        ]
        if headless:
            chrome_args.append("--headless=new")
        chrome_args.extend(extra_args)
        payload = {
            "capabilities": {
                "alwaysMatch": {
                    "browserName": "chrome",
                    "goog:chromeOptions": {
                        "binary": chrome_binary,
                        "args": chrome_args,
                    },
                    "acceptInsecureCerts": True,
                }
            }
        }
        response = self._request("POST", "/session", payload)
        self.session_id = response.get("sessionId") or response.get("value", {}).get("sessionId")
        if not self.session_id:
            raise RuntimeError(f"Chromedriver did not return a session id: {response}")

    def close(self) -> None:
        if not self.session_id:
            return
        try:
            self._request("DELETE", f"/session/{self.session_id}")
        finally:
            self.session_id = None

    def navigate(self, url: str) -> None:
        self._request("POST", f"/session/{self.session_id}/url", {"url": url})

    def execute(self, script: str, args: list[Any] | None = None) -> Any:
        response = self._request(
            "POST",
            f"/session/{self.session_id}/execute/sync",
            {"script": script, "args": args or []},
        )
        return response.get("value")


@dataclass
class ConsoleEntry:
    level: str
    text: str
    timestamp_ms: int
    args: list[str]


def normalize_console_entries(raw_entries: list[dict[str, Any]]) -> list[ConsoleEntry]:
    entries: list[ConsoleEntry] = []
    for item in raw_entries:
        entries.append(
            ConsoleEntry(
                level=str(item.get("level", "log")),
                text=str(item.get("text", "")),
                timestamp_ms=int(item.get("timestampMs", 0)),
                args=[str(arg) for arg in item.get("args", [])],
            )
        )
    return entries


def fetch_page_state(client: WebDriverClient) -> dict[str, Any]:
    return client.execute(
        """
        return {
            href: window.location.href,
            appState: window.__cranposeAppState || null,
            consoleMessages: Array.isArray(window.__cranposeConsoleMessages)
                ? window.__cranposeConsoleMessages.slice()
                : [],
            title: document.title,
        };
        """
    )


def matches_any(text: str, patterns: list[str]) -> bool:
    return any(pattern in text for pattern in patterns)


def write_artifacts(
    artifact_dir: Path,
    metadata: dict[str, Any],
    console_entries: list[ConsoleEntry],
) -> tuple[Path, Path]:
    artifact_dir.mkdir(parents=True, exist_ok=True)
    timestamp = time.strftime("%Y%m%d-%H%M%S")
    base_name = f"web-gl-diag-{timestamp}"
    text_path = artifact_dir / f"{base_name}.log"
    json_path = artifact_dir / f"{base_name}.json"

    with text_path.open("w", encoding="utf-8") as handle:
        handle.write("# Cranpose web GL diagnostic console capture\n")
        handle.write(json.dumps(metadata, indent=2))
        handle.write("\n\n")
        for entry in console_entries:
            handle.write(f"[{entry.level}] {entry.text}\n")

    with json_path.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "metadata": metadata,
                "console_messages": [
                    {
                        "level": entry.level,
                        "text": entry.text,
                        "timestamp_ms": entry.timestamp_ms,
                        "args": entry.args,
                    }
                    for entry in console_entries
                ],
            },
            handle,
            indent=2,
        )

    return text_path, json_path


def print_relevant_logs(console_entries: list[ConsoleEntry], patterns: list[str]) -> None:
    relevant = [entry for entry in console_entries if matches_any(entry.text, patterns)]
    if not relevant:
        tail = console_entries[-20:]
        print("No matching diagnostic lines were captured; showing the last console lines instead:")
        for entry in tail:
            print(f"[{entry.level}] {entry.text}")
        return
    print("Relevant console lines:")
    for entry in relevant:
        print(f"[{entry.level}] {entry.text}")


def main() -> int:
    args = parse_args()
    artifact_dir = Path(args.artifact_dir)
    fail_patterns = DEFAULT_FAIL_PATTERNS
    relevant_patterns = DEFAULT_RELEVANT_PATTERNS
    headless = not args.headful
    chrome_binary = resolve_executable(args.chrome_binary)
    chromedriver_binary = resolve_executable(args.chromedriver_binary)

    run_build(args.build_mode)

    server_port = choose_free_port()
    chromedriver_port = choose_free_port()
    url = f"http://127.0.0.1:{server_port}/?backend={args.backend}&tab={args.tab}"

    page_state: dict[str, Any] = {}
    console_entries: list[ConsoleEntry] = []
    matched_failures: list[str] = []
    chromedriver_lines: list[str] = []
    timed_out = False

    with serve_directory(DEMO_DIR, server_port):
        with start_chromedriver(chromedriver_binary, chromedriver_port) as (
            chromedriver_process,
            chromedriver_output,
        ):
            client = WebDriverClient(f"http://127.0.0.1:{chromedriver_port}")
            try:
                client.create_session(
                    chrome_binary=chrome_binary,
                    headless=headless,
                    window_size=args.window_size,
                    extra_args=args.chrome_arg,
                )
                print(f"Opening {url}")
                client.navigate(url)

                deadline = time.monotonic() + args.timeout
                first_match_time: float | None = None
                while time.monotonic() < deadline:
                    try:
                        page_state = fetch_page_state(client)
                    except RuntimeError:
                        time.sleep(0.25)
                        continue

                    console_entries = normalize_console_entries(page_state.get("consoleMessages", []))
                    if not matched_failures:
                        matched_failures = [
                            pattern
                            for pattern in fail_patterns
                            if any(pattern in entry.text for entry in console_entries)
                        ]
                        if matched_failures:
                            first_match_time = time.monotonic()

                    app_state = page_state.get("appState") or {}
                    if app_state.get("phase") == "error" and first_match_time is None:
                        first_match_time = time.monotonic()

                    if first_match_time is not None and (
                        time.monotonic() - first_match_time
                    ) >= args.post_match_wait:
                        break
                    time.sleep(0.25)
                else:
                    timed_out = True
            finally:
                chromedriver_lines = chromedriver_output.lines()
                client.close()
                terminate_process(chromedriver_process)

    metadata = {
        "url": url,
        "backend": args.backend,
        "tab": args.tab,
        "build_mode": args.build_mode,
        "timeout_seconds": args.timeout,
        "post_match_wait_seconds": args.post_match_wait,
        "headless": headless,
        "chrome_binary": chrome_binary,
        "chromedriver_binary": chromedriver_binary,
        "timed_out": timed_out,
        "app_state": page_state.get("appState"),
        "page_title": page_state.get("title"),
        "matched_fail_patterns": matched_failures,
        "chromedriver_tail": chromedriver_lines[-20:],
    }
    text_path, json_path = write_artifacts(artifact_dir, metadata, console_entries)
    print_relevant_logs(console_entries, relevant_patterns)
    print(f"Saved console log to {text_path}")
    print(f"Saved structured capture to {json_path}")

    if matched_failures:
        print(f"Detected fail patterns: {', '.join(matched_failures)}")
        return 1
    if (page_state.get("appState") or {}).get("phase") == "error":
        print("The page entered the error phase.")
        return 1
    if not page_state:
        print("The harness could not read any page state from Chrome.")
        return 1
    if timed_out and (page_state.get("appState") or {}).get("phase") != "running":
        print("The page did not reach the running phase before the timeout.")
        return 1
    print("No fail patterns detected.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

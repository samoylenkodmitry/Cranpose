"""Exercise the real desktop demo over MCP, including file saves and clean exit."""

import argparse
import json
import pathlib
import queue
import subprocess
import tempfile
import threading
import time


def run(binary, source, log, hold_seconds, screenshot):
    with tempfile.TemporaryDirectory(prefix="cranpose-mcp-") as directory, log.open("w") as errors:
        document = pathlib.Path(directory) / "screen.rs"
        initial = source.read_text()
        document.write_text(initial)
        process = subprocess.Popen([str(binary), "--mcp", str(document)], stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE, stderr=errors, text=True, bufsize=1)
        replies = queue.Queue()

        def read():
            for line in process.stdout:
                replies.put(line)
            replies.put(None)

        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        request_id = 0

        def send(value):
            process.stdin.write(json.dumps(value) + "\n")
            process.stdin.flush()

        def rpc(method, params):
            nonlocal request_id
            request_id += 1
            send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
            line = replies.get(timeout=30)
            assert line is not None, f"app disconnected; see {log}"
            reply = json.loads(line)
            assert reply.get("id") == request_id and "error" not in reply, reply
            return reply["result"]

        def call(request, error=False):
            reply = rpc("tools/call", {"name": "cranpose_ui", "arguments": {"request": request}})
            assert bool(reply.get("isError")) == error, reply
            return reply.get("structuredContent")

        def state():
            return call({"method": "state", "api": "counter", "member": "count"})["value"]

        try:
            rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                               "clientInfo": {"name": "cranpose-desktop-smoke", "version": "1"}})
            send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            tools = rpc("tools/list", {})["tools"]
            assert tools[0]["name"] == "cranpose_ui", tools
            catalogue = call({"method": "catalogue"})["catalogue"]
            assert "counter" in catalogue["apis"], catalogue
            assert call({"method": "snapshot"})["revision"] == 0
            call({"method": "dispatch", "node": "increment", "event": "on_click"})
            assert state() == 1
            edited = initial.replace("Column", "Row").replace("counter.add(1)", "counter.add(5)")
            call({"method": "source", "base_revision": 0, "source": edited, "function": "Screen"})
            assert state() == 1
            call({"method": "dispatch", "node": "increment", "event": "on_click"})
            assert state() == 6
            patch = {"method": "patch", "patch": {"base_revision": 1, "edits": [
                {"kind": "set_argument", "target": "increment", "name": "label",
                 "value": {"kind": "literal", "value": "Agent +5"}}]}}
            assert call(patch)["revision"] == 2
            call(patch, error=True)
            assert call({"method": "snapshot"})["revision"] == 2
            print(json.dumps({"stage": "agent-edited", "pid": process.pid, "count": state(), "revision": 2}), flush=True)
            if screenshot:
                time.sleep(0.2)
                helper = pathlib.Path(__file__).parents[3] / "scripts/dev/drag_window.sh"
                windows = subprocess.check_output(["bash", str(helper), "screenwindows", binary.name], text=True)
                window = next(line.split()[0][3:] for line in windows.splitlines()
                              if "onscreen=true" in line and "name=Cranpose · live runtime" in line)
                subprocess.run(["bash", str(helper), "shotwindow", window, str(screenshot)], check=True)
            if hold_seconds:
                time.sleep(hold_seconds)
            saved = edited.replace("Add one", "Saved from editor")
            replacement = document.with_suffix(".new")
            replacement.write_text(saved)
            replacement.replace(document)
            deadline = time.monotonic() + 5
            while True:
                snapshot = call({"method": "snapshot"})
                if snapshot["revision"] == 3:
                    break
                assert time.monotonic() < deadline, "file watcher did not commit the save"
                time.sleep(0.05)
            assert snapshot["program"]["root"]["children"][2]["arguments"]["label"]["value"] == "Saved from editor"
            assert state() == 6
            process.stdin.close()
            assert process.wait(timeout=10) == 0
            reader.join(timeout=2)
            assert replies.get(timeout=2) is None, "unexpected non-MCP stdout"
            print(json.dumps({"result": "passed", "pid": process.pid, "count": 6,
                              "revision": 3, "client_disconnect_exited": True}), flush=True)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
            process.stdout.close()
            if not process.stdin.closed:
                process.stdin.close()
            reader.join(timeout=2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--source", default=pathlib.Path(__file__).parents[1] / "screen.rs", type=pathlib.Path)
    parser.add_argument("--log", required=True, type=pathlib.Path)
    parser.add_argument("--hold-seconds", type=float, default=0)
    parser.add_argument("--screenshot", type=pathlib.Path)
    options = parser.parse_args()
    run(options.binary.resolve(), options.source.resolve(), options.log.resolve(), options.hold_seconds, options.screenshot)

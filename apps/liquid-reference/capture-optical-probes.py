import argparse
import json
import subprocess
import time
from pathlib import Path


def _capture(device, output, timeout, expected_count=None):
    if expected_count is not None and expected_count <= 0:
        raise ValueError("expected count must be positive")
    output.mkdir(parents=True, exist_ok=False)
    source = None
    checked_container = 0
    started = time.time()
    deadline = time.monotonic() + timeout
    captured = set()
    while time.monotonic() < deadline:
        if source is None or time.monotonic() - checked_container > 2:
            container = Path(subprocess.check_output(
                ["xcrun", "simctl", "get_app_container", device, "io.cranpose.liquid-reference", "data"], text=True).strip())
            source = container / "Documents" / "optical-probe.json"
            checked_container = time.monotonic()
        if not source.exists():
            time.sleep(0.05)
            continue
        frame = json.loads(source.read_text())
        if frame["changedWallMillis"] < started * 1000:
            time.sleep(0.05)
            continue
        index = frame["index"]
        age = time.time() - frame["changedWallMillis"] / 1000
        if index in captured or age < 0.6 or age > frame["duration"] - 0.5:
            time.sleep(0.05)
            continue
        name = f"probe-{index:02d}"
        before = time.time()
        subprocess.run(["xcrun", "simctl", "io", device, "screenshot", str(output / (name + ".png"))], check=True)
        after = time.time()
        current = json.loads(source.read_text())
        if (current["run"], current["index"]) != (frame["run"], index) or after - frame["changedWallMillis"] / 1000 > frame["duration"]:
            raise ValueError(f"probe changed during screenshot {index}")
        frame["captureWallMillis"] = [before * 1000, after * 1000]
        (output / (name + ".json")).write_text(json.dumps(frame, indent=2) + "\n")
        captured.add(index)
        print(f"Captured {len(captured)}/{frame['count']}: {frame['pattern']}", flush=True)
        if len(captured) == (expected_count or frame["count"]):
            return
    raise TimeoutError(f"captured {len(captured)} probes before deadline")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("device")
    parser.add_argument("output", type=Path)
    parser.add_argument("--timeout", type=float, default=420)
    parser.add_argument("--count", type=int)
    args = parser.parse_args()
    if args.count is not None and args.count <= 0:
        parser.error("--count must be positive")
    _capture(args.device, args.output, args.timeout, args.count)

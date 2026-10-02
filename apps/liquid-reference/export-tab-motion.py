import argparse
import csv
import hashlib
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location("motion", Path(__file__).with_name("analyze-motion.py"))
motion = importlib.util.module_from_spec(spec)
spec.loader.exec_module(motion)


def export(capture, output):
    timeline = json.loads((capture / "timeline.json").read_text())
    sources = []
    rows = []
    for route, gesture in enumerate(timeline["gestures"]):
        touch_path = capture / gesture["touch_trace"]
        layer_path = capture / gesture["layer_trace"]
        touches = json.loads(touch_path.read_text())
        frames = json.loads(layer_path.read_text())
        origin = touches[0]["timestamp"] + (touches[0]["receivedWallMillis"] - touches[0]["wallMillis"]) / 1000
        release = (touches[-1]["receivedWallMillis"] - touches[0]["receivedWallMillis"]) / 1000
        move = next((sample["receivedWallMillis"] - touches[0]["receivedWallMillis"]) / 1000
                    for sample in touches if abs(sample["x"] - touches[0]["x"]) > 1)
        events = []
        for sample in touches[1:]:
            time = (sample["receivedWallMillis"] - touches[0]["receivedWallMillis"]) / 1000
            events.append(["up" if sample["phase"] == "Touch up" else "input", time, sample["x"] - 72.333, 0, 0, "input"])
        initial = None
        count = 0
        for frame in frames:
            geometry = motion._native_lens(frame)
            if geometry is None:
                continue
            bar, lens = geometry
            x, _, width, height = lens["rect"]
            bx, _, bw, bh = bar["rect"]
            time = frame["timestamp"] - origin
            if time < 0:
                continue
            position = (x + width * 0.5 - bx - bw * 0.5) / (bw / 360) + 128.5
            if initial is None:
                initial = round(position / (257 / 3)) * (257 / 3)
            events.append(["frame", time, position, width / (bw / 360), height / (bh / 62),
                           "contact" if time < move else "drag" if time < release else "release"])
            count += 1
        rows.append(["start", route, initial, 0, 0, "start"])
        rows.extend(sorted(events, key=lambda event: (event[1], event[0] == "frame")))
        sources.append({"touches": touch_path.name, "layers": layer_path.name, "frames": count,
                        "touches_sha256": hashlib.sha256(touch_path.read_bytes()).hexdigest(),
                        "layers_sha256": hashlib.sha256(layer_path.read_bytes()).hexdigest()})
    with output.open("w") as stream:
        csv.writer(stream, lineterminator="\n").writerows(rows)
    output.with_suffix(".json").write_text(json.dumps({"environment": timeline["environment"], "sources": sources,
        "clock": "CADisplayLink timestamp and input delivery relative to the first delivered touch. No trajectory registration.",
        "normalization": "Lens position and dimensions relative to enclosing 360 x 62 point bar. Recorded global input x is offset by the replay fixture's 72.333 point origin."}, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    export(args.capture, args.output)

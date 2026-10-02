import argparse
import csv
import hashlib
import json
from pathlib import Path


def export(captures, output):
    rows = []
    shapes = []
    sources = []
    for capture in captures:
        timeline = json.loads((capture / "timeline.json").read_text())
        component = timeline["viewport"]["component"]
        path = {"slider": "Window/0/0/0/1/0/0/2", "toggle": "Window/0/0/0/1/0/0/2",
                "segmented": "Window/0/0/0/1/0/4/0"}[component]
        for gesture in timeline["gestures"]:
            touches_path = capture / gesture["touch_trace"]
            layers_path = capture / gesture["layer_trace"]
            touches = json.loads(touches_path.read_text())
            origin = touches[0]["timestamp"] + (touches[0]["receivedWallMillis"] - touches[0]["wallMillis"]) / 1000
            release = (touches[-1]["receivedWallMillis"] - touches[0]["receivedWallMillis"]) / 1000
            for frame in json.loads(layers_path.read_text()):
                time = frame["timestamp"] - origin
                layers = {layer["path"]: layer for layer in frame["layers"]}
                lens = layers[path]
                rect = lens["rect"]
                shapes.append([component, time, rect[0] + rect[2] * 0.5, *lens["scale"]])
                phase = "release" if release <= time <= release + 0.7 else "press" if time < min(0.5, release, gesture["planned_events"][1]["offset"]) else None
                if phase is None:
                    continue
                width, height = lens["bounds"][2:]
                white_layer = layers.get(path + "/0/1")
                if white_layer is None:
                    if layers[path + "/0/0"]["kind"] != "CALayer" or any(
                            layer["kind"].startswith("CASDF") for key, layer in layers.items() if key.startswith(path + "/")):
                        raise ValueError("missing white fill in an active native lens")
                    white = 1.0
                else:
                    white = white_layer["opacity"]
                rows.append([component, phase, time if phase == "press" else time - release, width, height, white])
            sources.append({"component": component, "environment": timeline["environment"],
                            "touches": touches_path.name, "layers": layers_path.name,
                            "touches_sha256": hashlib.sha256(touches_path.read_bytes()).hexdigest(),
                            "layers_sha256": hashlib.sha256(layers_path.read_bytes()).hexdigest()})
    with output.open("w") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(["component", "phase", "time", "width", "height", "white_opacity"])
        writer.writerows(rows)
    with output.with_name(output.stem.removesuffix("_contact") + "_shape" + output.suffix).open("w") as stream:
        writer = csv.writer(stream, lineterminator="\n")
        writer.writerow(["component", "time", "center_x", "scale_x", "scale_y"])
        writer.writerows(shapes)
    output.with_suffix(".json").write_text(json.dumps({"sources": sources, "clock": "CADisplayLink timestamp relative to application input delivery; first callback can precede delivery. No sample registration or timing adjustment."}, indent=2) + "\n")
    print(f"Saved {len(rows)} native contact frames")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("captures", type=Path, nargs="+")
    args = parser.parse_args()
    export(args.captures, args.output)

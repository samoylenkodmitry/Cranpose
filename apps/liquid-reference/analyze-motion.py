import argparse
import csv
import json
import math
import statistics
from pathlib import Path


def _native_lens(frame):
    layers = {layer["path"]: layer for layer in frame["layers"]}
    bar = layers.get("UITabBar/0")
    if bar is None:
        return None
    if bar["kind"] == "CAPortalLayer" and bar["bounds"][2:] == [0, 0]:
        return None
    if bar["kind"] not in ("CALayer", "_UIMultiLayer"):
        raise ValueError("unrecognized native bar layer")
    path = "UITabBar/0/0/1" if bar["kind"] == "_UIMultiLayer" else "UITabBar/0/1"
    lens = layers.get(path)
    if lens is None:
        return None
    if not (0 < lens["bounds"][2] < bar["bounds"][2] and 0 < lens["bounds"][3] < bar["bounds"][3] * 1.5):
        raise ValueError("native selection must be smaller than its enclosing bar")
    return bar, lens


def _geometry(frames, touches):
    origin = touches[0]["timestamp"]
    rows = []
    for frame in frames:
        geometry = _native_lens(frame)
        if geometry is None:
            continue
        bar, lens = geometry
        x, _, width, _ = lens["rect"]
        bar_x, _, bar_width, _ = bar["rect"]
        values = [frame["timestamp"] - origin, x + width * 0.5 - bar_x - bar_width * 0.5,
                  lens["scale"][0] - 1, lens["scale"][1] - 1]
        if not all(math.isfinite(value) for value in values):
            raise ValueError("motion geometry must be finite")
        rows.append(values)
    return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("layers", type=Path)
    parser.add_argument("touches", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    frames = json.loads(args.layers.read_text())
    touches = json.loads(args.touches.read_text())
    rows = _geometry(frames, touches)
    with args.output.open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(["time", "surface_center", "width_strain", "height_strain"])
        writer.writerows(rows)
    print(json.dumps({"recorded_frames": len(frames), "geometry_frames": len(rows),
                      "median_interval_ms": statistics.median(b["timestamp"] - a["timestamp"] for a, b in zip(frames, frames[1:])) * 1000}))

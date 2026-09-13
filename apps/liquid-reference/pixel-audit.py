import argparse
import json
import os
from pathlib import Path


def _difference(native, cranpose, width, height, regions):
    if width <= 0 or height <= 0 or len(native) != width * height * 4 or len(cranpose) != len(native):
        raise ValueError("both images must have identical, nonempty RGBA dimensions")
    result = {}
    for name, (x, y, w, h) in regions.items():
        if not all(type(value) is int for value in (x, y, w, h)) or min(x, y) < 0 or min(w, h) <= 0 or x + w > width or y + h > height:
            raise ValueError(f"region {name} is outside the original pixels")
        from PIL import Image, ImageChops
        box = (x, y, x + w, y + h)
        a = Image.frombytes("RGBA", (width, height), bytes(native)).crop(box)
        b = Image.frombytes("RGBA", (width, height), bytes(cranpose)).crop(box)
        difference = ImageChops.difference(a, b)
        histogram = difference.histogram()
        total = sum((index % 256) * count for index, count in enumerate(histogram))
        maximum = max(index % 256 for index, count in enumerate(histogram) if count)
        channels = difference.split()
        any_channel = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]),
                                        ImageChops.lighter(channels[2], channels[3]))
        unequal = w * h - any_channel.histogram()[0]
        result[name] = {"pixels": w * h, "unequal_pixels": unequal, "maximum_channel_error": maximum,
                        "mean_absolute_channel_error": total / (w * h * 4), "exact": unequal == 0}
    return result


def _sampling(frames):
    result = {}
    for source in ["native", "cranpose"]:
        routes = {}
        for frame in frames:
            if frame[source + "Available"]:
                routes.setdefault(frame["route"], {})[frame[source + "Frame"]] = frame[source + "Time"]
        gaps = []
        for samples in routes.values():
            times = sorted(samples.values())
            gaps.extend(b - a for a, b in zip(times, times[1:]))
        result[source] = {"unique_frames": sum(len(samples) for samples in routes.values()),
                          "maximum_source_gap_seconds": max(gaps, default=0)}
    return result


def _audit(comparison, output, regions):
    from PIL import Image, ImageChops
    if "whole_frame" in regions:
        raise ValueError("the whole_frame region cannot be replaced")
    frames = json.loads((comparison / "frames/keyframes.json").read_text())
    if not frames:
        raise ValueError("comparison contains no source frames")
    if len({frame["id"] for frame in frames}) != len(frames):
        raise ValueError("paired frame identifiers must be unique")
    output.mkdir(parents=True, exist_ok=False)
    results = []
    for pair in frames:
        record = {key: pair[key] for key in ("id", "route", "native", "cranpose", "nativeFrame", "cranposeFrame", "nativeTime", "cranposeTime")}
        record["contentLabel"] = pair.get("contentLabel", "Original content")
        record["covered"] = pair["nativeAvailable"] and pair["cranposeAvailable"]
        if record["covered"]:
            with Image.open(comparison / "frames" / pair["native"]) as a, Image.open(comparison / "frames" / pair["cranpose"]) as b:
                if a.size != b.size:
                    raise ValueError("source pixel dimensions differ; resizing is forbidden")
                a, b = a.convert("RGBA"), b.convert("RGBA")
                width, height = a.size
                record["regions"] = _difference(a.tobytes(), b.tobytes(), width, height,
                                                 {"whole_frame": [0, 0, width, height], **regions})
                difference = ImageChops.difference(a, b)
                difference.putalpha(255)
                name = f"{pair['id']}.png"
                difference.save(output / name, compress_level=1)
                record["difference"] = name
        results.append(record)
    exact = all(row["covered"] and row["regions"]["whole_frame"]["exact"] for row in results)
    report = {"exact": exact, "paired_steps": len(results), "rows": results, "sampling": _sampling(frames),
              "scope": "Original decoded pixels, no registration, scaling, smoothing, tolerance or omitted frames. Unequal timestamps remain explicit; compressed recordings cannot establish lossless pixel equality."}
    report["source"] = os.path.relpath(comparison / "frames", output)
    (output / "audit.json").write_text(json.dumps(report, indent=2) + "\n")
    template = Path(__file__).with_name("pixel-audit.html").read_text()
    (output / "index.html").write_text(template.replace("__AUDIT__", json.dumps(report).replace("<", "\\u003c")))
    return exact


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("comparison", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--regions", type=Path)
    args = parser.parse_args()
    regions = json.loads(args.regions.read_text()) if args.regions else {}
    raise SystemExit(0 if _audit(args.comparison, args.output, regions) else 1)

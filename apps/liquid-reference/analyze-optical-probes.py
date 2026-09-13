import argparse
import hashlib
import json
import math
from pathlib import Path


def _quadrature(values, period, anchor):
    cosine = values[0] - values[2]
    sine = values[3] - values[1]
    coordinate = math.atan2(sine, cosine) * period / (2 * math.pi)
    coordinate += round((anchor - coordinate) / period) * period
    return coordinate, math.hypot(cosine, sine) / 2


def _validate_frames(frames):
    if not frames or len(frames) != frames[0]["count"]:
        raise ValueError("incomplete optical capture")
    if sorted(frame["index"] for frame in frames) != list(range(len(frames))):
        raise ValueError("duplicate or missing probe index")
    for frame in frames:
        if frame["viewport"] != frames[0]["viewport"] or frame["count"] != len(frames):
            raise ValueError("inconsistent probe viewport or count")
        start, end = frame["captureWallMillis"]
        ready = frame["changedWallMillis"]
        if not ready + 600 <= start <= end <= ready + frame["duration"] * 1000:
            raise ValueError("screenshot outside stable hold interval")
    groups = {}
    for frame in frames:
        pattern = frame["pattern"]
        if pattern["kind"] == "wave":
            channel = pattern.get("channel", "all")
            if channel not in ["all", "red", "green", "blue"]:
                raise ValueError("unknown optical input channel")
            groups.setdefault((channel, pattern["axis"], pattern["period"]), []).append(pattern["phase"])
    if not groups or any(sorted(phases) != [0, 1, 2, 3] for phases in groups.values()):
        raise ValueError("incomplete quadrature phase group")


def _geometry_variation(frames):
    rectangles = {}
    for frame in frames:
        for layer in frame["layers"]:
            glass = layer["path"].startswith("UITabBar") or layer["kind"] in ["CASDFLayer", "CASDFElementLayer", "CABackdropLayer"]
            if glass and layer["rect"] is not None:
                rectangles.setdefault(layer["path"], []).append(layer["rect"])
    geometry = {path: [max(axis) - min(axis) for axis in zip(*rects)]
                for path, rects in rectangles.items() if len(rects) == len(frames)}
    if not geometry:
        raise ValueError("missing native glass geometry")
    variation = max(max(value) for value in geometry.values())
    if variation > 0.1:
        raise ValueError(f"native geometry moved {variation} pt across probes")
    return geometry, variation


def _analyze(root, output, minimum_amplitude):
    import numpy as np
    from PIL import Image, ImageDraw

    paths = sorted(root.glob("probe-*.json"))
    frames = [json.loads(path.read_text()) for path in paths]
    _validate_frames(frames)
    images = [np.asarray(Image.open(path.with_suffix(".png")).convert("RGB"), dtype=np.float32) for path in paths]
    if any(image.shape != images[0].shape for image in images):
        raise ValueError("inconsistent screenshot dimensions")
    output.mkdir(parents=True, exist_ok=False)
    scale = images[0].shape[1] / frames[0]["viewport"][0]
    viewport_height = frames[0]["viewport"][1]
    geometry, variation = _geometry_variation(frames)
    bar = next(layer["rect"] for layer in frames[0]["layers"] if layer["path"] == "UITabBar/0")
    top = max(0, int((bar[1] - bar[3] / 2) * scale))
    bottom = min(images[0].shape[0], int((bar[1] + bar[3] * 1.5) * scale))
    yy, xx = np.mgrid[top:bottom, :images[0].shape[1]]
    actual = {"x": (xx + 0.5) / scale, "y": (yy + 0.5) / scale}
    report = {"scale": scale, "roi_pixels": [0, top, images[0].shape[1], bottom - top],
              "count": len(frames), "max_rect_variation_pt": variation, "geometry_variation_pt": geometry,
              "interpretation": "Phase-equivalent coordinates, not a proven single ray field. Low-amplitude pixels are excluded.",
              "source_sha256": {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                                for json_path in paths for path in [json_path, json_path.with_suffix(".png")]},
              "fields": {}, "unobstructed_source_errors": {}}
    rows = []
    checker = next((frame["index"] for frame in frames if frame["pattern"]["kind"] == "checkerboard"), None)
    if checker is not None:
        rows.append(("Native held-out checkerboard", Image.fromarray(images[checker][top:bottom].astype(np.uint8))))
    channels = sorted({frame["pattern"].get("channel", "all") for frame in frames})
    for channel in channels:
        component = {"all": 1, "red": 0, "green": 1, "blue": 2}[channel]
        for axis in ["x", "y"]:
            previous = actual[axis][..., None]
            periods = sorted({frame["pattern"]["period"] for frame in frames if frame["pattern"]["axis"] == axis and frame["pattern"].get("channel", "all") == channel}, reverse=True)
            for period in periods:
                ids = [frame["index"] for frame in sorted(frames, key=lambda frame: frame["pattern"]["phase"])
                       if frame["pattern"]["axis"] == axis and frame["pattern"]["period"] == period
                       and frame["pattern"].get("channel", "all") == channel]
                name = f"{axis}-{period:g}" if channel == "all" else f"{channel}-{axis}-{period:g}"
                quartet = [images[index][top:bottom] for index in ids]
                cosine, sine = quartet[0] - quartet[2], quartet[3] - quartet[1]
                coordinate = np.arctan2(sine, cosine) * period / (2 * np.pi)
                coordinate += np.rint((previous - coordinate) / period) * period
                amplitude = np.hypot(cosine, sine) / 2
                closure = (quartet[0] + quartet[2] - quartet[1] - quartet[3]) / 2
                valid = amplitude > minimum_amplitude
                np.savez_compressed(output / f"{name}.npz", coordinate=coordinate, amplitude=amplitude, valid=valid, closure=closure)
                difference = coordinate - actual[axis][..., None]
                report["fields"][name] = {
                    "valid_channel_samples": int(valid.sum()),
                    "displacement_percentiles_pt": np.percentile(difference[valid], [0, 5, 50, 95, 100]).tolist(),
                    "closure_rms_rgb255": float(np.sqrt(np.mean(closure ** 2))),
                    "center_pixel_coordinate_rgb": [_quadrature([float(value[coordinate.shape[0] // 2, coordinate.shape[1] // 2, channel]) for value in quartet], period,
                                                                float(actual[axis][coordinate.shape[0] // 2, coordinate.shape[1] // 2]))[0] for channel in range(3)],
                }
                previous = coordinate
                normalized = np.clip(difference[:, :, component] / 25, -1, 1)
                colors = np.stack((255 * np.maximum(normalized, 0), 60 * (1 - np.abs(normalized)), 255 * np.maximum(-normalized, 0)), axis=-1).astype(np.uint8)
                colors[~valid[:, :, component]] = 32
                heatmap = Image.fromarray(colors)
                heatmap.save(output / f"{name}-displacement.png")
                if period in [32, periods[0]]:
                    rows.append((f"{channel} input, {axis.upper()} phase-equivalent displacement, {period:g} pt waves; red +25 pt, blue -25 pt", heatmap))
    source_top, source_bottom = int(viewport_height * 0.72 * scale), int(viewport_height * 0.78 * scale)
    source_left, source_right = int(images[0].shape[1] * 0.05), int(images[0].shape[1] * 0.95)
    y, x = np.mgrid[source_top:source_bottom, source_left:source_right]
    for frame, image in zip(frames, images):
        pattern = frame["pattern"]
        if pattern["kind"] != "wave":
            continue
        position = (x + 0.5) / scale if pattern["axis"] == "x" else (y + 0.5) / scale
        expected = 255 * (0.5 + 0.15 * np.cos(2 * np.pi * position / pattern["period"] + pattern["phase"] * np.pi / 2))
        channel = pattern.get("channel", "all")
        expected_rgb = np.stack([expected if channel in ["all", name] else np.full_like(expected, 127.5)
                                 for name in ["red", "green", "blue"]], axis=-1)
        observed = image[source_top:source_bottom, source_left:source_right]
        report["unobstructed_source_errors"][str(frame["index"])] = float(np.sqrt(np.mean((observed - expected_rgb) ** 2)))
    sheet = Image.new("RGB", (images[0].shape[1], (bottom - top + 36) * len(rows)), "#121212")
    draw = ImageDraw.Draw(sheet)
    for row, (label, image) in enumerate(rows):
        draw.text((12, row * (bottom - top + 36) + 8), label, fill="white")
        sheet.paste(image, (0, row * (bottom - top + 36) + 36))
    sheet.save(output / "measured-fields.png")
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"captures": len(frames), "geometry_variation_pt": variation,
                      "maximum_source_rms_rgb255": max(report["unobstructed_source_errors"].values())}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--minimum-amplitude", type=float, default=3)
    args = parser.parse_args()
    _analyze(args.capture, args.output, args.minimum_amplitude)

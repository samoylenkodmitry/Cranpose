import argparse
import bisect
import json
import math
import plistlib
import subprocess
import statistics
import shutil
from pathlib import Path


def _run(*arguments):
    return subprocess.run(arguments, check=True, stdout=subprocess.PIPE, text=True).stdout


def _walk(activities):
    for activity in activities:
        yield activity
        yield from _walk(activity.get("childActivities", []))


def _decode_event(path):
    archive = plistlib.loads(path.read_bytes())

    def resolve(value):
        if isinstance(value, plistlib.UID):
            return resolve(archive["$objects"][value.data])
        if isinstance(value, dict):
            return {key: resolve(item) for key, item in value.items() if key != "$class"}
        if isinstance(value, list):
            return [resolve(item) for item in value]
        return value

    event = resolve(archive["$top"]["root"])
    paths = event["eventPaths"]["NS.objects"]
    if len(paths) != 1:
        raise ValueError("expected one finger")
    events = paths[0]["pointerEvents"]["NS.objects"]
    kinds = [item["eventType"] for item in events]
    if len(kinds) < 4 or kinds[0] != 1 or kinds[-1] != 3 or any(kind != 2 for kind in kinds[1:-1]):
        raise ValueError("expected one continuous down, moves, up path")
    offsets = [item["offset"] for item in events]
    if not all(math.isfinite(item[key]) for item in events
               for key in ("offset", "coordinate.x", "coordinate.y")):
        raise ValueError("gesture coordinates and offsets must be finite")
    if not all(left < right for left, right in zip(offsets, offsets[1:])):
        raise ValueError("gesture offsets must increase")
    return [{key: item[key] for key in ("eventType", "offset", "coordinate.x", "coordinate.y")}
            for item in events]


def _decode_clock(pixels):
    values = [pixels[4 * 128 + bit * 4 + 2] for bit in range(32)]
    if any(60 < value < 195 for value in values):
        return None
    code = 0
    for value in values:
        code = code << 1 | int(value >= 195)
    return code & 0xFFFFFF if code >> 24 == 0xB4 else None


def _clock_alignment(codes, times, recording_epoch):
    offsets = []
    previous = None
    period = 0x1000000
    for code, time in zip(codes, times):
        if code is None or code == previous:
            continue
        previous = code
        epoch_millis = code + round((recording_epoch * 1000 - code) / period) * period
        offsets.append(epoch_millis / 1000 - time)
    if len(offsets) < 8:
        raise ValueError("at least eight changing clock markers are required")
    origin = statistics.median(offsets)
    deviations = sorted(abs(value - origin) for value in offsets)
    p95 = deviations[min(len(deviations) - 1, int(len(deviations) * 0.95))]
    if p95 > 0.080:
        raise ValueError(f"recorded clock is inconsistent: 95% residual {p95:.3f}s")
    return {"epoch_at_movie_zero": origin, "clock_samples": len(offsets),
            "clock_residual_p95_seconds": p95}


def _recorded_clock(movie, viewport, frame_times, epoch):
    probe = json.loads(_run("ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                           "stream=width:stream=height", "-of", "json", str(movie)))["streams"][0]
    scale = probe["width"] / viewport["width"]
    geometry = [round(value * scale) for value in (128, 8, 16, 100)]
    crop = "crop=" + ":".join(map(str, geometry)) + ",scale=128:8:flags=neighbor,setpts=N/(60*TB)"
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", str(movie), "-vf", crop,
                          "-fps_mode", "passthrough", "-pix_fmt", "gray", "-f", "rawvideo", "-"],
                         check=True, stdout=subprocess.PIPE).stdout
    stride = 128 * 8
    if len(raw) != stride * len(frame_times):
        raise ValueError("clock frames do not match source presentation timestamps")
    codes = [_decode_clock(raw[index:index + stride]) for index in range(0, len(raw), stride)]
    return _clock_alignment(codes, frame_times, epoch)


def _delivery_samples(samples):
    if any("receivedWallMillis" in sample for sample in samples):
        if not all("receivedWallMillis" in sample for sample in samples):
            raise ValueError("every touch needs its application delivery timestamp")
        return [dict(sample, wallMillis=sample["receivedWallMillis"]) for sample in samples]
    if any("timestamp" in sample for sample in samples):
        raise ValueError("native recording must include application delivery timestamps")
    return samples


def _input_path(samples):
    if len(samples) < 4 or samples[0]["phase"] != "Touch down" or samples[-1]["phase"] != "Touch up":
        raise ValueError("expected a complete recorded scrub")
    origin = samples[0]["wallMillis"]
    if not all(math.isfinite(item[key]) for item in samples for key in ("wallMillis", "x", "y")):
        raise ValueError("touch samples must be finite")
    if not all(a["wallMillis"] < b["wallMillis"] for a, b in zip(samples, samples[1:])):
        raise ValueError("touch timestamps must increase")
    moving = [i for i in range(1, len(samples) - 1)
              if math.hypot(samples[i]["x"] - samples[i-1]["x"],
                            samples[i]["y"] - samples[i-1]["y"]) > 0.1]
    if len(moving) < 2:
        raise ValueError("expected observed movement samples")
    return [{"eventType": 1 if index == 0 else 3 if index == len(samples) - 1 else 2,
             "offset": (item["wallMillis"] - origin) / 1000,
             "coordinate.x": item["x"], "coordinate.y": item["y"]}
            for index, item in enumerate(samples)]


def _directions(events):
    result = []
    for left, right in zip(events, events[1:]):
        delta = right["coordinate.x"] - left["coordinate.x"]
        if abs(delta) > 0.1:
            direction = 1 if delta > 0 else -1
            if not result or direction != result[-1]:
                result.append(direction)
    return result


def _validate_trajectory(planned, observed):
    if _directions(planned) != _directions(observed):
        raise ValueError("recorded touch path did not execute every planned direction change")
    for index in (0, -1):
        if math.hypot(planned[index]["coordinate.x"] - observed[index]["coordinate.x"],
                      planned[index]["coordinate.y"] - observed[index]["coordinate.y"]) > 1:
            raise ValueError("recorded touch endpoint differs from the planned gesture")


def _phase(offset, events):
    down, slide, stop, up = [events[index]["offset"] for index in (0, 1, -2, -1)]
    if offset < down: return "rest"
    if offset < min(down + 0.15, slide): return "touch-down"
    if offset < slide: return "pressed"
    if offset < stop: return "sliding"
    if offset < min(stop + 0.2, up): return "stopped"
    if offset < up: return "idle-held"
    if offset < up + 0.3: return "touch-up"
    return "settling"


def _source_times(frames):
    times = [float(frame["pts_time"]) for frame in frames]
    if not times or not all(math.isfinite(time) for time in times):
        raise ValueError("source presentation times must be present and finite")
    return times


def _presentation_order(times):
    return sorted(enumerate(times), key=lambda item: (item[1], item[0]))


def _frame_index(frame_times, time, duration):
    if not frame_times or not frame_times[0] <= time <= duration:
        raise ValueError("keyframe falls outside the recording")
    return bisect.bisect_right(frame_times, time) - 1


def _extract(bundle, output, suite, traces):
    output.mkdir(parents=True, exist_ok=False)
    _run("xcrun", "xcresulttool", "export", "attachments", "--path", str(bundle),
         "--output-path", str(output / "attachments"))
    activities = json.loads(_run("xcrun", "xcresulttool", "get", "test-results", "activities",
                                "--path", str(bundle), "--test-id", f"{suite}/testInteractionKeyframes()"))
    (output / "activities.json").write_text(json.dumps(activities, indent=2) + "\n")
    runs = activities["testRuns"]
    if len(runs) != 1:
        raise ValueError("compare one run per result bundle")
    summary = json.loads(_run("xcrun", "xcresulttool", "get", "test-results", "summary",
                              "--path", str(bundle), "--format", "json"))
    devices = summary["devicesAndConfigurations"]
    if len(devices) != 1:
        raise ValueError("record on one device and configuration per result bundle")
    environment = devices[0]["device"]
    flat = list(_walk(runs[0]["activities"]))
    attachments = {item["uuid"]: item for activity in flat for item in activity.get("attachments", [])}
    viewports = [item for item in attachments.values() if item["name"].startswith("gesture-viewport_")]
    if len(viewports) != 1:
        raise ValueError("expected one viewport attachment")
    viewport_attachment = viewports[0]
    viewport_path = next((output / "attachments").glob(viewport_attachment["uuid"] + ".*"))
    viewport = json.loads(viewport_path.read_text())
    settling_seconds = viewport.get("settlingSeconds", 0.8)
    if not math.isfinite(settling_seconds) or settling_seconds < 0.8:
        raise ValueError("capture must include at least 0.8 seconds after release")
    profiles = [item for item in attachments.values() if item["name"].startswith("reference-profile_")]
    if len(profiles) > 1:
        raise ValueError("record one material profile per result bundle")
    if profiles:
        profile_path = next((output / "attachments").glob(profiles[0]["uuid"] + ".*"))
        profile = json.loads(profile_path.read_text())
        if not isinstance(profile["glassTintPercent"], int) or not 0 <= profile["glassTintPercent"] <= 100:
            raise ValueError("unexpected calibrated tint level")
        environment.update(profile)
    if not all(math.isfinite(viewport[key]) and viewport[key] > 0 for key in ("width", "height")):
        raise ValueError("viewport dimensions must be positive and finite")
    videos = [item for item in attachments.values() if item["name"].endswith(".mp4")]
    if len(videos) != 1:
        raise ValueError("expected one retained screen recording; use LiquidReference.xctestplan")
    recording = videos[0]
    movie = output / "attachments" / (recording["uuid"] + ".mp4")
    probe = json.loads(_run("ffprobe", "-v", "error", "-select_streams", "v:0", "-show_frames",
                           "-show_entries", "frame=pts_time:format=duration",
                           "-of", "json", str(movie)))
    frame_times = _source_times(probe["frames"])
    duration = float(probe["format"]["duration"])
    event_prefix = "Reference pointer path" if any(item["name"].startswith("Reference pointer path") for item in attachments.values()) else "Synthesized Event"
    events = sorted((item for item in attachments.values() if item["name"].startswith(event_prefix)),
                    key=lambda item: item["timestamp"])
    if not events:
        raise ValueError("expected recorded gestures")
    synthesis_title = "Reference pointer synthesis" if event_prefix == "Reference pointer path" else "Synthesize event"
    synthesis = [activity for activity in flat if activity["title"] == synthesis_title]
    if len(synthesis) != len(events):
        raise ValueError("each gesture must have a synthesis activity")
    paths = [_decode_event(next((output / "attachments").glob(item["uuid"] + "*"))) for item in events]
    alignment = _recorded_clock(movie, viewport, frame_times, recording["timestamp"])
    epoch = alignment["epoch_at_movie_zero"]
    observed = []
    for file in sorted(traces.glob("*-touches.json")):
        samples = json.loads(file.read_text())
        if samples and "wallMillis" in samples[0] and epoch <= samples[0]["wallMillis"] / 1000 <= epoch + duration:
            observed.append((file, _delivery_samples(samples)))
    observed.sort(key=lambda item: item[1][0]["wallMillis"])
    if len(observed) != len(paths):
        raise ValueError("expected one actual touch trace per synthesized gesture")
    directory = output / "frames"
    directory.mkdir(exist_ok=False)
    _run("ffmpeg", "-v", "error", "-i", str(movie), "-vf",
         f"crop=iw:ih*120/{viewport['height']}:0:ih-oh,setpts=N/(60*TB)",
         "-fps_mode", "passthrough", "-enc_time_base", "1/60000", "-start_number", "0", str(directory / "%06d.png"))
    files = sorted(directory.glob("*.png"))
    if len(files) != len(frame_times):
        raise ValueError("dense extraction must retain every source frame")
    timeline = []
    for index, ((file, samples), planned) in enumerate(zip(observed, paths)):
        shutil.copy2(file, output / file.name)
        layers = file.with_name(file.name.replace("-touches.json", "-layers.json"))
        if layers.is_file():
            shutil.copy2(layers, output / layers.name)
        path = _input_path(samples)
        _validate_trajectory(planned, path)
        origin = samples[0]["wallMillis"] / 1000 - epoch
        up = path[-1]["offset"]
        dense = [{"file": str(files[i].relative_to(output)), "movie_seconds": time,
                  "gesture_seconds": time - origin, "source_frame_index": i, "source_pts": time,
                  "phase": _phase(time - origin, path)}
                 for i, time in _presentation_order(frame_times) if origin - 0.25 <= time <= origin + up + settling_seconds]
        if not dense or dense[0]["phase"] != "rest" or dense[-1]["phase"] != "settling":
            raise ValueError("recording must include rest, contact, movement, release and settling")
        timeline.append({"events": path, "planned_events": planned, "input_origin_seconds": origin,
                         "reported_synthesis_seconds": synthesis[index]["startTime"] - recording["timestamp"],
                         "touch_trace": file.name, "layer_trace": layers.name if layers.is_file() else None, "frames": dense})
    (output / "timeline.json").write_text(json.dumps({
        "recording": str(movie.relative_to(output)), "viewport": viewport,
        "environment": environment,
        "alignment": alignment, "gestures": timeline,
        "input_clock": "application-delivery",
        "capture": "Every source frame retained at its original presentation timestamp. Clock markers map recording time to observed input time; capture and rendering latency remain bounded by recording cadence and clock residuals.",
    }, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("bundle", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--suite", default="TabBarTests")
    parser.add_argument("--traces", type=Path, required=True)
    args = parser.parse_args()
    _extract(args.bundle, args.output, args.suite, args.traces)

import argparse
import json
import shutil
from pathlib import Path


def _gesture_label(events):
    legs = [(right["coordinate.x"] - left["coordinate.x"], right["offset"] - left["offset"])
            for left, right in zip(events, events[1:])
            if abs(right["coordinate.x"] - left["coordinate.x"]) > 0.1]
    if not legs or any(duration <= 0 for _, duration in legs):
        raise ValueError("gesture needs motion with increasing timestamps")
    directions = " ".join("→" if distance > 0 else "←" for distance, _ in legs)
    speeds = "/".join(f"{abs(distance) / duration:.0f}" for distance, duration in legs)
    return f"{directions}{' ·' if len(legs) > 1 else ''} {speeds} pt/s"


def _pairs(timelines):
    if len(timelines) != 2 or not timelines[0]["gestures"] or len(timelines[0]["gestures"]) != len(timelines[1]["gestures"]):
        raise ValueError("each native gesture needs a Cranpose recording")
    if any(timeline.get("input_clock") != "application-delivery" for timeline in timelines):
        raise ValueError("both recordings must use application touch delivery time")
    if timelines[0].get("environment") != timelines[1].get("environment"):
        raise ValueError("both recordings must use the same device, OS and settings")
    if timelines[0].get("viewport") != timelines[1].get("viewport"):
        raise ValueError("both recordings must use the same viewport")
    cases = (timelines[0].get("environment") or {}).get("contentCases")
    if cases is not None and len(cases) != len(timelines[0]["gestures"]):
        raise ValueError("every content fixture needs exactly one captured gesture")
    result = []
    for route, gestures in enumerate(zip(*(item["gestures"] for item in timelines))):
        events = gestures[0]["planned_events"]
        if events != gestures[1]["planned_events"]:
            raise ValueError("both recordings must execute the same planned touch path")
        content_label = " / ".join(cases[route]["titles"]) if cases and cases[route] else "Original content"
        label = content_label + " · " + _gesture_label(events)
        sequences = [item["frames"] for item in gestures]
        times = [[frame["gesture_seconds"] for frame in frames] for frames in sequences]
        union = sorted((time, source, index) for source, sequence in enumerate(times)
                       for index, time in enumerate(sequence))
        positions = [0, 0]
        for time, source, index in union:
            positions[source] = index
            pair = [sequence[position] for sequence, position in zip(sequences, positions)]
            result.append({"id": f"{route}-{len(result)}", "route": route, "gestureLabel": label, "contentLabel": content_label, "time": time,
                           "phase": pair[0]["phase"], "cranposePhase": pair[1]["phase"],
                           "native": "native-" + Path(pair[0]["file"]).name,
                           "cranpose": "cranpose-" + Path(pair[1]["file"]).name,
                           "nativeFrame": pair[0]["source_frame_index"],
                           "cranposeFrame": pair[1]["source_frame_index"],
                           "nativeAvailable": times[0][0] <= time <= times[0][-1],
                           "cranposeAvailable": times[1][0] <= time <= times[1][-1],
                           "nativeTime": pair[0]["gesture_seconds"],
                           "cranposeTime": pair[1]["gesture_seconds"]})
    return result


def _report(native, cranpose, output, app_resources=None):
    output.mkdir(parents=True, exist_ok=False)
    assets = output / "frames"
    assets.mkdir()
    timelines = []
    for name, source in [("native", native), ("cranpose", cranpose)]:
        timeline = json.loads((source / "timeline.json").read_text())
        directory = output / name
        directory.mkdir()
        shutil.copy2(source / timeline["recording"], directory / "recording.mp4")
        shutil.copy2(source / "timeline.json", directory / "timeline.json")
        for gesture in timeline["gestures"]:
            shutil.copy2(source / gesture["touch_trace"], directory / gesture["touch_trace"])
            if gesture.get("layer_trace"):
                shutil.copy2(source / gesture["layer_trace"], directory / gesture["layer_trace"])
            for frame in gesture["frames"]:
                shutil.copy2(source / frame["file"], assets / (name + "-" + Path(frame["file"]).name))
        timelines.append(timeline)
    pairs = _pairs(timelines)
    (assets / "keyframes.json").write_text(json.dumps(pairs, separators=(",", ":")) + "\n")
    template = Path(__file__).with_name("comparison.html").read_text()
    data = {"timelines": timelines, "frames": pairs}
    (output / "index.html").write_text(template.replace("__TIMELINES__", json.dumps(data).replace("<", "\\u003c")))
    if app_resources:
        shutil.copytree(assets, app_resources)
    print(f"{len(pairs)} paired steps; every captured motion frame retained")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("native", type=Path)
    parser.add_argument("cranpose", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--app-resources", type=Path)
    args = parser.parse_args()
    _report(args.native, args.cranpose, args.output, args.app_resources)

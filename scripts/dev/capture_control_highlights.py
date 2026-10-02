import argparse
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def capture(device, app, output, probes):
    bundle = "io.cranpose.liquid-reference"

    def sim(*arguments, **kwargs):
        return subprocess.run(
            ["xcrun", "simctl", *arguments],
            check=True,
            capture_output=True,
            text=True,
            **kwargs,
        ).stdout.strip()

    output.mkdir(parents=True, exist_ok=False)
    sim("install", device, str(app))
    binary = app / "LiquidReference"
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    installed = Path(sim("get_app_container", device, bundle, "app")) / binary.name
    assert digest(binary) == digest(installed)
    repository = Path(__file__).resolve().parents[2]
    source = repository / "apps/liquid-reference/LiquidReference/NativeTrace.swift"
    manifest = {
        "binary_sha256": digest(binary),
        "native_trace_sha256": digest(source),
        "device": device,
        "probes": probes,
        "cases": [],
    }
    for component, scheme, probe in itertools.product(
        ["card", "button"], ["light", "dark"], probes
    ):
        subprocess.run(["xcrun", "simctl", "terminate", device, bundle], capture_output=True)
        environment = dict(os.environ)
        for key, value in {
            "REFERENCE_COMPONENT": component,
            "REFERENCE_SCHEME": scheme,
            "REFERENCE_BACKDROP": "solid",
            "REFERENCE_CAPTURE_CONTROL_LAYERS": "1",
            "REFERENCE_CONTROL_HIGHLIGHT": probe,
        }.items():
            environment["SIMCTL_CHILD_" + key] = value
        documents = Path(sim("get_app_container", device, bundle, "data")) / "Documents"
        archive = documents / f"native-control-{component}-{scheme}-solid-layers.json"
        error_path = documents / "control-probe-error.txt"
        previous_error = error_path.stat().st_mtime_ns if error_path.exists() else None
        previous_mtime = archive.stat().st_mtime_ns if archive.exists() else None
        sim("launch", device, bundle, env=environment)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if archive.exists() and archive.stat().st_mtime_ns != previous_mtime:
                break
            time.sleep(0.2)
        else:
            raise RuntimeError(f"Missing fresh layer archive: {archive}")
        time.sleep(1)
        if error_path.exists() and error_path.stat().st_mtime_ns != previous_error:
            raise RuntimeError(error_path.read_text())
        name = f"{component}-{scheme}-{probe}"
        shutil.copy2(archive, output / f"{name}-layers.json")
        image = output / f"{name}.png"
        sim("io", device, "screenshot", str(image))
        manifest["cases"].append({"name": name, "sha256": digest(image)})
        print(name, flush=True)
    (output / "provenance.json").write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("device")
    parser.add_argument("app", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--probes", nargs="+", choices=["raw", "filtered", "disabled"],
        default=["raw", "filtered", "disabled"],
    )
    args = parser.parse_args()
    capture(args.device, args.app, args.output, args.probes)

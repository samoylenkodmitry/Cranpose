"""Capture continuous native/Cranpose control gestures and retain source frames."""

import argparse
import hashlib
import json
import plistlib
import subprocess
import sys
from pathlib import Path


def installed_binary_sha256(device, bundle):
    app = Path(subprocess.check_output(
        ["xcrun", "simctl", "get_app_container", device, bundle, "app"], text=True).strip())
    with (app / "Info.plist").open("rb") as stream:
        executable = app / plistlib.load(stream)["CFBundleExecutable"]
    return hashlib.sha256(executable.read_bytes()).hexdigest()


def capture(device, output, platform, components):
    root = Path(__file__).resolve().parents[2]
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bundle = "io.cranpose.liquid-" + ("reference" if platform == "native" else "cranpose")
    patch = subprocess.check_output(["git", "diff", "HEAD", "--binary"], cwd=root)
    (output / "source.patch").write_bytes(patch)
    provenance = {"device": device, "bundle": bundle,
                  "source_patch_sha256": hashlib.sha256(patch).hexdigest(),
                  "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                  "components": components, "captures": []}
    (output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    for component in components:
        suite = ("Native" if platform == "native" else "Cranpose") + component.title() + "MotionTests"
        prefix = output / component
        result = prefix.with_suffix(".xcresult")
        print(f"Capturing {suite}", flush=True)
        with prefix.with_suffix(".log").open("w") as log:
            subprocess.run(["just", "liquid-reference-test", f"platform=iOS Simulator,id={device}",
                            str(result), suite, "never"], cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True)
        provenance["captures"].append({"component": component, "result": result.name,
                                      "binary_sha256": installed_binary_sha256(device, bundle)})
        (output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        traces = output / (component + "-traces")
        subprocess.run([sys.executable, "apps/liquid-reference/collect-traces.py", str(traces),
                        "--bundle", bundle, "--device", device], cwd=root, check=True)
        with (output / (component + "-extract.log")).open("w") as log:
            subprocess.run([sys.executable, "apps/liquid-reference/extract-keyframes.py", str(result), str(prefix),
                            "--suite", suite, "--traces", str(traces)], cwd=root,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        print(f"Saved {prefix}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device")
    parser.add_argument("output", type=Path)
    parser.add_argument("platform", choices=["native", "cranpose"])
    parser.add_argument("--components", nargs="+", choices=["slider", "toggle", "segmented"],
                        default=["slider", "toggle", "segmented"])
    args = parser.parse_args()
    capture(args.device, args.output, args.platform, args.components)

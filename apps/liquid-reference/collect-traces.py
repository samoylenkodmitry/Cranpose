import argparse
import json
import shutil
import subprocess
from pathlib import Path


def _device_trace_names(listing):
    names = []
    for item in listing["result"]["files"]:
        name = item["name"]
        if not name.endswith(("-touches.json", "-layers.json")):
            continue
        if Path(name).name != name or item["resources"]["isSymbolicLink"] or item["resources"]["isDirectory"]:
            raise ValueError("trace must be a regular file directly in the app's Documents directory")
        names.append(name)
    if not names:
        raise ValueError("no input traces; record the interaction test first")
    return names


def _collect_device(output, bundle, device):
    output.mkdir(parents=True, exist_ok=False)
    listing = output / "device-files.json"
    domain = ["--device", device, "--domain-type", "appDataContainer", "--domain-identifier", bundle]
    subprocess.run(["xcrun", "devicectl", "device", "info", "files", *domain, "--subdirectory", "Documents",
                    "--no-recurse", "--json-output", str(listing), "--quiet"], check=True)
    for name in _device_trace_names(json.loads(listing.read_text())):
        subprocess.run(["xcrun", "devicectl", "device", "copy", "from", *domain,
                        "--source", "Documents/" + name, "--destination", str(output / name), "--quiet"], check=True)


def _collect(output, bundle, device, physical=False):
    if physical:
        _collect_device(output, bundle, device)
        return
    source = Path(subprocess.check_output(["xcrun", "simctl", "get_app_container", device, bundle, "data"], text=True).strip()) / "Documents"
    files = list(source.glob("*-touches.json")) + list(source.glob("*-layers.json"))
    if not files:
        raise ValueError("no input traces; record the interaction test first")
    output.mkdir(parents=True, exist_ok=False)
    for file in files:
        shutil.copy2(file, output / file.name)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--bundle", default="io.cranpose.liquid-reference")
    parser.add_argument("--device", default="booted")
    parser.add_argument("--physical", action="store_true")
    args = parser.parse_args()
    _collect(args.output, args.bundle, args.device, args.physical)

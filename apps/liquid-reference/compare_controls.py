import argparse
import html
import json
import shutil
from pathlib import Path

from PIL import Image, ImageChops


def read_cases(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    cases = {}
    devices = set()
    for test in manifest:
        for attachment in test["attachments"]:
            filename = attachment["exportedFileName"]
            name = attachment["suggestedHumanReadableName"].split("_0_")[0]
            if not name.startswith("control-") or not filename.endswith(".png"):
                continue
            if name in cases:
                raise ValueError(f"duplicate control capture: {name}")
            if attachment.get("isAssociatedWithFailure"):
                raise ValueError(f"failed control capture: {name}")
            cases[name] = directory / filename
            devices.add(attachment["deviceId"])
    if not cases or len(devices) != 1:
        raise ValueError("expected control captures from exactly one device")
    return cases, devices.pop()


def read_case_sets(directories):
    cases = {}
    devices = set()
    for directory in directories:
        batch, device = read_cases(directory)
        if cases.keys() & batch.keys():
            raise ValueError("duplicate control capture across result bundles")
        cases.update(batch)
        devices.add(device)
    if len(devices) != 1:
        raise ValueError("control captures use different devices")
    return cases, devices.pop()


def compare(native, cranpose, output, native_extra=(), cranpose_extra=()):
    left, left_device = read_case_sets([native, *native_extra])
    right, right_device = read_case_sets([cranpose, *cranpose_extra])
    if left.keys() != right.keys():
        raise ValueError(f"control cases differ: {sorted(left.keys() ^ right.keys())}")
    if left_device != right_device:
        raise ValueError("control captures use different devices")
    output.mkdir(parents=True, exist_ok=False)
    rows = []
    panels = []
    for name in sorted(left):
        with Image.open(left[name]) as a, Image.open(right[name]) as b:
            if a.size != b.size or a.width * 874 != a.height * 402:
                raise ValueError(f"{name}: expected matching 402 by 874 point viewports")
            scale = a.width / 402
            if name.startswith("control-menu-"):
                region = (0, 100, 402, 700)
            elif name.startswith("control-nav-bar-"):
                region = (0, 60, 402, 250)
            else:
                region = (0, 317, 402, 587)
            crop = tuple(round(coordinate * scale) for coordinate in region)
            a = a.convert("RGBA").crop(crop)
            b = b.convert("RGBA").crop(crop)
            difference = ImageChops.difference(a, b)
            channels = difference.split()
            maximum = channels[0]
            for channel in channels[1:]:
                maximum = ImageChops.lighter(maximum, channel)
            histogram = maximum.histogram()
            unequal = a.width * a.height - histogram[0]
            worst = max(index for index, count in enumerate(histogram) if count)
            rows.append({"case": name, "region_points": region, "crop_pixels": crop, "unequal_pixels": unequal, "worst_channel_difference": worst})
            for suffix, image in [("native", a), ("cranpose", b), ("difference", difference.convert("RGB"))]:
                image.save(output / f"{name}-{suffix}.png")
            for suffix, source in [("native", left[name]), ("cranpose", right[name])]:
                shutil.copyfile(source, output / f"{name}-{suffix}-full.png")
            title = html.escape(name)
            images = "".join(f'<figure><figcaption>{label}</figcaption><a href="{name}-{suffix}-full.png"><img src="{name}-{suffix}.png"></a></figure>'
                             for suffix, label in [("native", "iOS"), ("cranpose", "Cranpose")])
            panels.append(f'<section><h2>{title}</h2><p>{unequal:,} unequal pixels; largest channel difference {worst}</p><div>{images}<figure><figcaption>Absolute difference</figcaption><img src="{name}-difference.png"></figure></div></section>')
    (output / "measurements.json").write_text(json.dumps({"device": left_device, "cases": rows}, indent=2) + "\n")
    (output / "index.html").write_text('<!doctype html><meta charset="utf-8"><title>Liquid controls: iOS and Cranpose</title><style>body{font:16px system-ui;margin:24px;background:#eee}section{margin:30px 0}section div{display:flex;gap:12px}figure{margin:0;width:33.333%}img{width:100%;image-rendering:auto}figcaption{margin-bottom:8px}</style><h1>Liquid controls: iOS and Cranpose</h1><p>Fixed regions: x 0–402, y 317–587 points for controls; y 100–700 for menus; y 60–250 for navigation headers. Click either capture for its unmodified full screenshot. No alignment, resizing, or tolerance is applied to the comparison.</p>' + "".join(panels))
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("native", type=Path)
    parser.add_argument("cranpose", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--native-extra", type=Path, action="append", default=[])
    parser.add_argument("--cranpose-extra", type=Path, action="append", default=[])
    args = parser.parse_args()
    rows = compare(args.native, args.cranpose, args.output, args.native_extra, args.cranpose_extra)
    print(f"{len(rows)} control states compared; {sum(row['unequal_pixels'] for row in rows):,} unequal pixels retained")


if __name__ == "__main__":
    main()

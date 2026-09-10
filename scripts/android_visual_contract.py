import argparse
import json
from pathlib import Path
import subprocess

from PIL import Image, ImageChops


def region_coverage(image, region):
    crop = image.crop(region['bounds']).convert('RGB')
    mask = Image.new('L', crop.size, 255)
    for channel, low, high in zip(crop.split(), region['rgb_min'], region['rgb_max']):
        mask = ImageChops.multiply(mask, channel.point([
            255 if low <= value <= high else 0 for value in range(256)
        ]))
    return mask.histogram()[255] / (crop.width * crop.height)


def inspect_visual_frames(video, size, regions, output):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    width, height = size
    if not regions or len({region['name'] for region in regions}) != len(regions):
        raise ValueError('Visual regions must have distinct names and must not be empty')
    for region in regions:
        left, top, right, bottom = region['bounds']
        if not (0 <= left < right <= width and 0 <= top < bottom <= height):
            raise ValueError('Visual region is outside the display: ' + region['name'])
        if not 0 < region['minimum_coverage'] <= 1:
            raise ValueError('Visual region requires a positive coverage threshold')
        if (len(region['rgb_min']) != 3 or len(region['rgb_max']) != 3
                or any(not 0 <= low <= high <= 255
                       for low, high in zip(region['rgb_min'], region['rgb_max']))):
            raise ValueError('Visual region requires three valid color channel ranges')
    probe = json.loads(subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'stream=width,height:frame=best_effort_timestamp_time', '-of', 'json', str(video),
    ]))
    streams = probe.get('streams', [])
    if len(streams) != 1 or [streams[0]['width'], streams[0]['height']] != size:
        raise ValueError('Visual recording size differs from the route')
    times = [float(frame['best_effort_timestamp_time']) for frame in probe['frames']]
    if not times:
        raise ValueError('Visual recording has no frames')
    records, failures = [], []
    saved = set()
    previous = None
    with (output / 'decoder.log').open('wb') as log, subprocess.Popen([
        'ffmpeg', '-v', 'error', '-i', str(video), '-f', 'rawvideo', '-pix_fmt', 'rgb24',
        '-fps_mode', 'passthrough', '-enc_time_base', '1:1000000', '-',
    ], stdout=subprocess.PIPE, stderr=log) as decoder:
        try:
            for index, seconds in enumerate(times):
                raw = decoder.stdout.read(width * height * 3)
                if len(raw) != width * height * 3:
                    raise RuntimeError(f'Truncated visual frame {index}')
                image = Image.frombytes('RGB', (width, height), raw)
                coverage = {region['name']: region_coverage(image, region) for region in regions}
                failed = [region['name'] for region in regions
                          if coverage[region['name']] < region['minimum_coverage']]
                record = {'frame': index, 'seconds': seconds, 'coverage': coverage, 'failed': failed}
                records.append(record)
                if failed:
                    failures.append(record)
                if len(saved) < 9 and (failed or (previous and previous[0]['failed'])):
                    for entry, pixels in ([previous] if previous else []) + [(record, image)]:
                        number = entry['frame']
                        if number not in saved and len(saved) < 9:
                            pixels.save(output / f'frame-{number:05}.png')
                            saved.add(number)
                previous = (record, image)
            if decoder.stdout.read(1) or decoder.wait() != 0:
                raise RuntimeError('Visual video decode did not complete exactly')
        finally:
            if decoder.poll() is None:
                decoder.kill()
                decoder.wait()
    summary = {
        'frames_checked': len(records), 'failing_frames': len(failures),
        'first_failure': failures[0] if failures else None,
        'minimum_coverage': {region['name']: min(r['coverage'][region['name']] for r in records)
                             for region in regions},
        'regions': regions,
    }
    (output / 'every-frame.json').write_text(json.dumps(records) + '\n')
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    return summary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--video', type=Path, required=True)
    parser.add_argument('--route', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    route = json.loads(args.route.read_text())
    report = inspect_visual_frames(args.video, route['size'], route['visual_regions'], args.output)
    print(json.dumps(report), flush=True)
    if report['failing_frames']:
        raise SystemExit('Rendering continuity failed; see ' + str(args.output))


if __name__ == '__main__':
    main()

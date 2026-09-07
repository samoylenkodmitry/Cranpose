import csv
import json
from pathlib import Path
import subprocess

import numpy as np
from PIL import Image


class SurfaceContract:
    def __init__(self, width, height, scale):
        self.top = height // 5
        self.bottom = height * 4 // 5
        self.marker_left = round(33.5 * scale)
        self.marker_right = round(66.5 * scale)
        self.card_left = round(80 * scale)
        self.card_right = width - round(32 * scale)
        self.pitch = round(102 * scale)
        rows = np.arange(self.top, self.bottom)
        self.markers = np.stack([
            ((rows - phase) % self.pitch) < round(33 * scale)
            for phase in range(self.pitch)
        ])
        self.cards = np.stack([
            ((rows - phase + round(20 * scale)) % self.pitch) < round(70 * scale)
            for phase in range(self.pitch)
        ])
        self.marker_area = self.markers.sum(axis=1) * (self.marker_right - self.marker_left)
        self.card_area = self.cards.sum(axis=1) * (self.card_right - self.card_left)
        if min(self.marker_area) <= 0 or min(self.card_area) <= 0:
            raise ValueError('Display is too small for the surface fixture')

    def check(self, rgb):
        body = rgb[self.top:self.bottom]
        stripe = body[:, self.marker_left:self.marker_right]
        blue = ((stripe[:, :, 0] < 40) & (stripe[:, :, 1] > 80)
                & (stripe[:, :, 1] < 190) & (stripe[:, :, 2] > 210))
        scores = self.markers @ blue.sum(axis=1)
        coverage = scores / self.marker_area
        phase = int(coverage.argmax())
        card = body[self.cards[phase], self.card_left:self.card_right]
        white = np.all(card >= 252, axis=2)
        return {
            'phase': phase,
            'missing_marker_fraction': float(1 - coverage[phase]),
            'missing_card_fraction': float(1 - white.sum() / self.card_area[phase]),
        }


def inspect_video(video, output, scale):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    probe = json.loads(subprocess.check_output([
        'ffprobe', '-v', 'error', '-select_streams', 'v:0', '-show_entries',
        'stream=width,height:frame=best_effort_timestamp_time', '-of', 'json', str(video),
    ]))
    width, height = [probe['streams'][0][key] for key in ('width', 'height')]
    times = [float(frame['best_effort_timestamp_time']) for frame in probe['frames']]
    if not times:
        raise ValueError('Recording contains no frames')
    contract = SurfaceContract(width, height, scale)
    records = []
    saved = set()
    previous = None
    with subprocess.Popen([
        'ffmpeg', '-v', 'error', '-i', str(video), '-f', 'rawvideo', '-pix_fmt', 'rgb24',
        '-fps_mode', 'passthrough', '-enc_time_base', '1:1000000', '-',
    ], stdout=subprocess.PIPE) as decoder:
        for index, seconds in enumerate(times):
            raw = decoder.stdout.read(width * height * 3)
            if len(raw) != width * height * 3:
                raise RuntimeError(f'Truncated decoded frame {index}')
            rgb = np.frombuffer(raw, dtype=np.uint8).reshape(height, width, 3)
            record = {'frame': index, 'seconds': seconds, **contract.check(rgb)}
            record['failing'] = max(record['missing_marker_fraction'], record['missing_card_fraction']) > 0.03
            records.append(record)
            transition = previous is not None and record['failing'] != previous[0]['failing']
            if len(saved) < 24 and (index == 0 or transition):
                for item, pixels in ([previous] if previous else []) + [(record, rgb)]:
                    number = item['frame']
                    if number not in saved:
                        Image.fromarray(pixels).save(output / f'frame-{number:05}.png')
                        saved.add(number)
            previous = (record, rgb.copy())
        if decoder.stdout.read(1):
            raise RuntimeError('Decoder produced uncounted frames')
        if decoder.wait() != 0:
            raise RuntimeError('Video decoder failed')
    with (output / 'every-frame.csv').open('w') as file:
        writer = csv.DictWriter(file, fieldnames=list(records[0]))
        writer.writeheader()
        writer.writerows(records)
    failures = [record for record in records if record['failing']]
    phases = {record['phase'] for record in records}
    summary = {
        'frames_checked': len(records),
        'failing_frames': len(failures),
        'first_failure': failures[0] if failures else None,
        'distinct_scroll_positions': len(phases),
        'maximum_missing_marker_fraction': max(r['missing_marker_fraction'] for r in records),
        'maximum_missing_card_fraction': max(r['missing_card_fraction'] for r in records),
    }
    (output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary), flush=True)
    if failures:
        raise AssertionError(f'{len(failures)} corrupted frames; see {output}')
    if len(phases) < 20:
        raise AssertionError('Recording did not exercise enough scroll positions')
    return summary

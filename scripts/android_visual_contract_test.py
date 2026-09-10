import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from PIL import Image

from android_visual_contract import inspect_visual_frames, region_coverage


REGION = {'name': 'stable_control', 'bounds': [8, 8, 24, 24],
          'rgb_min': [220, 220, 220], 'rgb_max': [255, 255, 255], 'minimum_coverage': 0.9}


class VisualContractTests(unittest.TestCase):
    def test_color_coverage_ignores_content_outside_the_control(self):
        image = Image.new('RGB', (32, 32), 'red')
        image.paste('white', (8, 8, 24, 24))
        self.assertEqual(region_coverage(image, REGION), 1)
        image.paste('black', (8, 8, 24, 16))
        self.assertEqual(region_coverage(image, REGION), 0.5)

    def test_a_single_missing_control_frame_is_rejected_and_its_neighbors_are_saved(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            second = {'name': 'second_control', 'bounds': [0, 24, 8, 32],
                      'rgb_min': [0, 220, 0], 'rgb_max': [50, 255, 50], 'minimum_coverage': 0.9}
            for missing in [None, REGION['name'], second['name']]:
                frames = []
                for index in range(5):
                    image = Image.new('RGB', (32, 32), (index * 30, 0, 0))
                    if not (missing == REGION['name'] and index == 2):
                        image.paste('white', (8, 8, 24, 24))
                    if not (missing == second['name'] and index == 2):
                        image.paste((0, 255, 0), second['bounds'])
                    frames.append(image.tobytes())
                video = root / f'{missing}.mkv'
                subprocess.run(['ffmpeg', '-v', 'error', '-f', 'rawvideo', '-pix_fmt', 'rgb24',
                                '-s', '32x32', '-r', '30', '-i', '-', '-c:v', 'ffv1', str(video)],
                               input=b''.join(frames), check=True)
                output = root / str(missing)
                result = inspect_visual_frames(video, [32, 32], [REGION, second], output)
                self.assertEqual(result['frames_checked'], 5)
                self.assertEqual(result['failing_frames'], int(missing is not None))
                if missing:
                    self.assertEqual(result['first_failure']['frame'], 2)
                    self.assertEqual(result['first_failure']['failed'], [missing])
                    for index in [1, 2, 3]:
                        self.assertTrue((output / f'frame-{index:05}.png').exists())
                self.assertEqual(json.loads((output / 'summary.json').read_text()), result)


if __name__ == '__main__':
    unittest.main()

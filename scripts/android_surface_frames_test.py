import unittest

import numpy as np

from android_surface_frames import SurfaceContract


def picture(offset):
    rgb = np.full((748, 360, 3), [247, 247, 252], dtype=np.uint8)
    for top in range(-102 + offset, 748, 102):
        rgb[max(0, top):max(0, top + 90), 16:344] = 255
        rgb[max(0, top + 27):max(0, top + 63), 32:68] = [0, 128, 255]
    return rgb


class SurfaceContractTest(unittest.TestCase):
    def setUp(self):
        self.contract = SurfaceContract(360, 748, 1)

    def test_every_fraction_of_a_row_keeps_complete_moving_content(self):
        for offset in range(102):
            with self.subTest(offset=offset):
                result = self.contract.check(picture(offset))
                self.assertLess(result['missing_marker_fraction'], 0.01)
                self.assertLess(result['missing_card_fraction'], 0.01)

    def test_background_cutouts_in_markers_are_rejected(self):
        rgb = picture(0)
        rgb[240:260, 32:68] = [247, 247, 252]
        self.assertGreater(self.contract.check(rgb)['missing_marker_fraction'], 0.03)

    def test_background_cutouts_in_cards_are_rejected(self):
        rgb = picture(0)
        rgb[215:260, 80:300] = [247, 247, 252]
        result = self.contract.check(rgb)
        self.assertLess(result['missing_marker_fraction'], 0.01)
        self.assertGreater(result['missing_card_fraction'], 0.03)

    def test_a_blank_frame_is_rejected(self):
        rgb = np.full((748, 360, 3), [247, 247, 252], dtype=np.uint8)
        self.assertEqual(self.contract.check(rgb)['missing_marker_fraction'], 1)


if __name__ == '__main__':
    unittest.main()

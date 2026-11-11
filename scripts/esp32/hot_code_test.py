#!/usr/bin/env python3
"""Checks hot_code.py --check's reading of list files and its decision on
which patterns an image lost."""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import hot_code  # noqa: E402

LIST = """SECTIONS
{
  .iram0.text :
  {
    *(.literal._RNvM*NtCs*_4demo5frameNt*5Frame4draw* .text._RNvM*NtCs*_4demo5frameNt*5Frame4draw*)
    *(.literal._RNvNtCs*_4demo4text6layout* .text._RNvNtCs*_4demo4text6layout*)
    . = . + 16;
  }
}
"""


class Lists(unittest.TestCase):
    def test_a_list_gives_each_function_pattern_once(self):
        self.assertEqual(
            hot_code.listed_patterns(LIST + LIST),
            ["_RNvM*NtCs*_4demo5frameNt*5Frame4draw*", "_RNvNtCs*_4demo4text6layout*"],
        )


class Lost(unittest.TestCase):
    def setUp(self):
        self.patterns = hot_code.listed_patterns(LIST)
        self.base = ["_RNvMs0_NtCsabc_4demo5frameNtB5_5Frame4draw", "_RNvNtCsabc_4demo4text6layout", "main"]

    def test_an_image_with_the_same_functions_under_another_hash_loses_none(self):
        names = ["_RNvMs0_NtCsxyz_4demo5frameNtB5_5Frame4draw", "_RNvNtCsxyz_4demo4text6layout"]
        self.assertEqual(hot_code.lost_patterns(self.patterns, self.base, names), [])

    def test_a_function_that_went_is_named(self):
        names = ["_RNvNtCsxyz_4demo4text6layout"]
        self.assertEqual(
            hot_code.lost_patterns(self.patterns, self.base, names),
            ["_RNvM*NtCs*_4demo5frameNt*5Frame4draw*"],
        )

    def test_a_pattern_that_never_matched_the_base_is_not_a_loss(self):
        self.assertEqual(hot_code.lost_patterns(["_RNvNtCs*_4demo4none5never*"], self.base, []), [])


if __name__ == "__main__":
    unittest.main()

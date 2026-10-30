# Run with fontTools: python3 scripts/fonts/make_test_wide.py crates/cranpose-text/tests/fonts/CranposeTestWide.ttf
"""Builds Cranpose Test Wide: `a` and `b` 500 units wide and `W` 20,000,
wider than a layout keeps a glyph's advance in eight bytes."""

import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPEM = 1000

names = [".notdef", "space", "a", "b", "W"]
cmap = {0x20: "space", ord("a"): "a", ord("b"): "b", ord("W"): "W"}
advances = {name: 500 for name in names}
advances["W"] = 20000

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics({name: (advances[name], 0) for name in names})
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Wide", "styleName": "Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0, usWinAscent=800,
                 usWinDescent=200, usWeightClass=400, achVendID="CRNP")
builder.setupPost()
builder.save(sys.argv[1])

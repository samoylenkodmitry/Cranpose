# Run with fontTools: python3 scripts/fonts/make_test_mono.py crates/cranpose-ui/assets/CranposeTestMono.ttf
"""Builds Cranpose Test Mono: a font whose every glyph, .notdef included,
advances 600 units of a 1000-unit em, with an 800-unit ascent and a 200-unit
descent, so text measures 0.6 em a character and 1 em a line."""

import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPEM = 1000
ADVANCE = 600
ASCENT = 800
DESCENT = -200


def box(pen, left, bottom, right, top):
    pen.moveTo((left, bottom))
    pen.lineTo((left, top))
    pen.lineTo((right, top))
    pen.lineTo((right, bottom))
    pen.closePath()


def glyph(drawn):
    pen = TTGlyphPen(None)
    if drawn:
        box(pen, 80, 0, 520, 700)
    return pen.glyph()


codepoints = list(range(0x21, 0x7F)) + list(range(0xA1, 0x100)) + [
    0x2010, 0x2011, 0x2013, 0x2014, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2026,
]
spaces = [0x20, 0xA0]

names = [".notdef"] + [f"space{cp:04X}" for cp in spaces] + [f"uni{cp:04X}" for cp in codepoints]
cmap = {cp: f"space{cp:04X}" for cp in spaces}
cmap.update({cp: f"uni{cp:04X}" for cp in codepoints})

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
glyphs = {".notdef": glyph(False)}
glyphs.update({name: glyph(False) for name in names if name.startswith("space")})
glyphs.update({name: glyph(True) for name in names if name.startswith("uni")})
builder.setupGlyf(glyphs)
builder.setupHorizontalMetrics({name: (ADVANCE, 80 if name.startswith("uni") else 0) for name in names})
builder.setupHorizontalHeader(ascent=ASCENT, descent=DESCENT, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Mono", "styleName": "Regular"})
builder.setupOS2(
    sTypoAscender=ASCENT,
    sTypoDescender=DESCENT,
    sTypoLineGap=0,
    usWinAscent=ASCENT,
    usWinDescent=-DESCENT,
    usWeightClass=400,
    yStrikeoutPosition=300,
    yStrikeoutSize=50,
    achVendID="CRNP",
)
builder.setupPost(isFixedPitch=1, underlinePosition=-100, underlineThickness=50)
builder.save(sys.argv[1])

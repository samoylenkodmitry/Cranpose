# Run with fontTools: python3 scripts/fonts/make_test_kerns.py crates/cranpose-text/tests/fonts/CranposeTestKerns.ttf
"""Builds Cranpose Test Kerns: a font whose `kern` moves the advance of each
pair of letters by a value of its own, 2,704 values in all, more than the
plain glyph path names in its pair cells."""

import sys

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPEM = 1000

letters = [chr(code) for code in range(ord("a"), ord("z") + 1)]
letters += [chr(code) for code in range(ord("A"), ord("Z") + 1)]
names = [".notdef", "space"] + letters
cmap = {0x20: "space"}
cmap.update({ord(letter): letter for letter in letters})

pairs = [(first, second) for first in letters for second in letters]
rules = "\n".join(
    f"    pos {first} {second} {index + 1};" for index, (first, second) in enumerate(pairs)
)
features = f"""languagesystem DFLT dflt;
languagesystem latn dflt;
feature kern {{
{rules}
}} kern;
"""

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics({name: (500, 0) for name in names})
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Kerns", "styleName": "Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0, usWinAscent=800,
                 usWinDescent=200, usWeightClass=400, achVendID="CRNP")
builder.setupPost()
addOpenTypeFeaturesFromString(builder.font, features)
builder.save(sys.argv[1])

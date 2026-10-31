# Run with fontTools: python3 scripts/fonts/make_test_space_kerns.py crates/cranpose-text/tests/fonts/CranposeTestSpaceKerns.ttf
"""Builds Cranpose Test Space Kerns: a font whose `kern` moves the space by
class pairs only in Latin, after T, V, W and Y and before A and L, so the
glyphs read with a space come from the classes of a pair table, and before
Te alone in Cyrillic, so the two scripts cut words on opposite sides of the
space. Its `liga` joins f and i, and Cyrillic a and a, which
sends a run that holds them through HarfRust, words apart."""

import sys

from fontTools.feaLib.builder import addOpenTypeFeaturesFromString
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPEM = 1000

letters = [chr(code) for code in range(ord("a"), ord("z") + 1)]
letters += [chr(code) for code in range(ord("A"), ord("Z") + 1)]
names = [".notdef", "space"] + letters + ["f_i", "uni0422", "uni0430", "uni0430_uni0430"]
cmap = {0x20: "space", 0x422: "uni0422", 0x430: "uni0430"}
cmap.update({ord(letter): letter for letter in letters})

features = """languagesystem DFLT dflt;
languagesystem latn dflt;
languagesystem cyrl dflt;
@BEFORE_SPACE = [T V W Y];
@AFTER_SPACE = [A L];
feature kern {
    script latn;
    pos @BEFORE_SPACE space -120;
    pos space @AFTER_SPACE -60;
    script cyrl;
    pos space uni0422 -90;
} kern;
feature liga {
    script latn;
    sub f i by f_i;
    script cyrl;
    sub uni0430 uni0430 by uni0430_uni0430;
} liga;
"""

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics({name: (500, 0) for name in names})
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Space Kerns", "styleName": "Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0, usWinAscent=800,
                 usWinDescent=200, usWeightClass=400, achVendID="CRNP")
builder.setupPost()
addOpenTypeFeaturesFromString(builder.font, features)
builder.save(sys.argv[1])

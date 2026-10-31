# Run with fontTools: python3 scripts/fonts/make_test_substitutions.py crates/cranpose-text/tests/fonts/CranposeTestSubstitutions.ttf
"""Builds Cranpose Test Substitutions: a font whose substitutions of one
glyph each take a case a text shaped without HarfRust must follow:

  ab    by one delta, in every language
  cd    by a list, in every language: `c` takes `d`'s alternate and `d`
        `c`'s
  e     into `f`, which a later lookup substitutes again
  g     twice, by two lookups: the first takes it
  i     in Turkish only
  j     in the default language only
  k     by a lookup that skips base glyphs, so never
  p     into a glyph a positioning moves
  u     in the second of two `liga` features, which HarfBuzz skips
  t     in the feature every language requires
  s     in a feature a variation of the font empties
  n     after `o`, which a rule before it substitutes before `n`
  bl    `b`'s alternate and `l` join
  1 7   `tnum`: `1` takes its tabular figure, which kerns with `2`, and
        `7` takes `1`
  а  in Cyrillic, which has a Bulgarian language and no default

Alternates are wider than the glyphs they replace."""

import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib import builder as otl
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables import otTables
from fontTools.varLib import featureVars

UPEM = 1000

letters = [chr(code) for code in range(ord("a"), ord("z") + 1)]
digits = [f"d{digit}" for digit in range(10)]
names = [".notdef", "space"] + letters + digits + [f"{letter}.alt" for letter in letters]
names += ["d1.tnum", "b_l", "acyr", "acyr.alt"]
glyph_map = {name: index for index, name in enumerate(names)}
cmap = {0x20: "space", 0x430: "acyr"}
cmap.update({ord(letter): letter for letter in letters})
cmap.update({ord(str(digit)): f"d{digit}" for digit in range(10)})
advances = {name: 500 for name in names}
advances.update({name: 800 for name in names if name.endswith(".alt")})
advances.update({"d1.tnum": 600, "b_l": 900})


def single(mapping, flags=0):
    return otl.buildLookup([otl.buildSingleSubstSubtable(mapping)], flags=flags)


def coverage(glyphs):
    return otl.buildCoverage(glyphs, glyph_map)


# `o` before `n` takes its alternate, by the lookup only this rule calls.
rule = otTables.ChainContextSubst()
rule.Format = 3
rule.BacktrackGlyphCount = 0
rule.BacktrackCoverage = []
rule.InputGlyphCount = 1
rule.InputCoverage = [coverage(["o"])]
rule.LookAheadGlyphCount = 1
rule.LookAheadCoverage = [coverage(["n"])]
record = otTables.SubstLookupRecord()
record.SequenceIndex = 0
record.LookupListIndex = 0
rule.SubstCount = 1
rule.SubstLookupRecord = [record]

lookups = [
    single({"o": "o.alt"}),
    otl.buildLookup([rule]),
    single({"a": "a.alt", "b": "b.alt", "n": "n.alt"}),
    single({"c": "d.alt", "d": "c.alt"}),
    single({"e": "f"}),
    single({"f": "f.alt"}),
    single({"g": "h"}),
    single({"g": "g.alt"}),
    single({"i": "i.alt"}),
    single({"j": "j.alt"}),
    single({"k": "k.alt"}, flags=otl.LOOKUP_FLAG_IGNORE_BASE_GLYPHS),
    single({"p": "p.alt"}),
    single({"u": "u.alt"}),
    single({"t": "t.alt"}),
    single({"s": "s.alt"}),
    single({"d1": "d1.tnum", "d7": "d1"}),
    single({"acyr": "acyr.alt"}),
    otl.buildLookup([otl.buildLigatureSubstSubtable({("b.alt", "l"): "b_l"})]),
]

# The features in tag order, each with its lookups.
features = [
    ("calt", [1]),
    ("ccmp", [2, 3, 4, 5, 6, 7, 10, 11]),
    ("ccmp", [16]),
    ("liga", [17]),
    ("liga", [12]),
    ("locl", [8]),
    ("locl", [9]),
    ("rclt", [14]),
    ("ss05", [13]),
    ("tnum", [15]),
]
RCLT = 7
SS05 = 8


def language(indices, required=0xFFFF):
    table = otTables.LangSys()
    table.LookupOrder = None
    table.ReqFeatureIndex = required
    table.FeatureIndex = indices
    table.FeatureCount = len(indices)
    return table


def script_list(scripts):
    scripts_table = otTables.ScriptList()
    scripts_table.ScriptRecord = []
    for tag, default, listed in scripts:
        script = otTables.Script()
        script.DefaultLangSys = default
        script.LangSysRecord = []
        for language_tag, table in listed:
            record = otTables.LangSysRecord()
            record.LangSysTag = language_tag
            record.LangSys = table
            script.LangSysRecord.append(record)
        script.LangSysCount = len(script.LangSysRecord)
        record = otTables.ScriptRecord()
        record.ScriptTag = tag
        record.Script = script
        scripts_table.ScriptRecord.append(record)
    scripts_table.ScriptCount = len(scripts_table.ScriptRecord)
    return scripts_table


def feature_list(records):
    table = otTables.FeatureList()
    table.FeatureRecord = [featureVars.buildFeatureRecord(tag, indices) for tag, indices in records]
    table.FeatureCount = len(table.FeatureRecord)
    return table


def lookup_list(all_lookups):
    table = otTables.LookupList()
    table.Lookup = all_lookups
    table.LookupCount = len(all_lookups)
    return table


everywhere = [0, 1, 3, 4, 7, 9]
gsub = otTables.GSUB()
gsub.Version = 0x00010001
gsub.ScriptList = script_list(
    [
        ("DFLT", language(everywhere + [6], SS05), []),
        ("cyrl", None, [("BGR ", language([2]))]),
        (
            "latn",
            language(everywhere + [6], SS05),
            [("TRK ", language(everywhere + [5], SS05))],
        ),
    ]
)
gsub.FeatureList = feature_list(features)
gsub.LookupList = lookup_list(lookups)
# At the default weight, and at every other, `rclt` holds no lookup.
gsub.FeatureVariations = featureVars.buildFeatureVariations(
    [
        featureVars.buildFeatureVariationRecord(
            [featureVars.buildConditionTable(0, -1.0, 1.0)],
            [featureVars.buildFeatureTableSubstitutionRecord(RCLT, [])],
        )
    ]
)

value = otTables.ValueRecord
kern = value()
kern.XAdvance = -100
wider = value()
wider.XAdvance = 50
positions = [
    otl.buildLookup([otl.buildPairPosGlyphsSubtable({("d1.tnum", "d2"): (kern, None)}, glyph_map)]),
    otl.buildLookup([otl.buildSinglePosSubtable({"p.alt": wider}, glyph_map)]),
]
gpos = otTables.GPOS()
gpos.Version = 0x00010000
gpos.ScriptList = script_list(
    [
        ("DFLT", language([0]), []),
        ("cyrl", None, [("BGR ", language([0]))]),
        ("latn", language([0]), [("TRK ", language([0]))]),
    ]
)
gpos.FeatureList = feature_list([("kern", [0, 1])])
gpos.LookupList = lookup_list(positions)

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics({name: (advances[name], 0) for name in names})
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Substitutions", "styleName": "Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0, usWinAscent=800,
                 usWinDescent=200, usWeightClass=400, achVendID="CRNP")
builder.setupPost()
builder.setupFvar([("wght", 100, 400, 900, "Weight")], [])
builder.font["GSUB"] = newTable("GSUB")
builder.font["GSUB"].table = gsub
builder.font["GPOS"] = newTable("GPOS")
builder.font["GPOS"].table = gpos
gdef = otTables.GDEF()
gdef.Version = 0x00010000
classes = otTables.ClassDef()
classes.classDefs = {name: 2 if name == "b_l" else 1 for name in names[2:]}
gdef.GlyphClassDef = classes
gdef.AttachList = None
gdef.LigCaretList = None
gdef.MarkAttachClassDef = None
builder.font["GDEF"] = newTable("GDEF")
builder.font["GDEF"].table = gdef
builder.save(sys.argv[1])

# Run with fontTools: python3 scripts/fonts/make_test_contexts.py crates/cranpose-text/tests/fonts/CranposeTestContexts.ttf
"""Builds Cranpose Test Contexts: a font whose `calt` holds one contextual
substitution of each kind, each starting at letters no other lookup starts
at, so a test finds whether text that a rule reads takes its alternates:

  ab    chained, by coverage: `a` before `b`
  cd    chained, by coverage: `c` and `d` as its input
  ef    chained, by coverage: `f` after `e`
  g     chained, by coverage: `g` alone
  hi jk chained, by glyph: `h` before `i`, `k` after `j`
  lm pn oq rs  chained, by class: `l` before `m`, `p` before `n`, `q`
        after `o`, `r` before any glyph no class names
  tu    by glyph: `t` before `u`
  vw    by class: `v` before `w`
  xy    by coverage: `x` before `y`
  zA    chained, by coverage, skipping ligatures: `z` before `A`, also
        with the ligature glyph of U+FB01 between them
  B..Z  chained, by coverage: an uppercase letter before any lowercase
        letter or digit, more pairs than a plan keeps

Every letter's alternate is wider than the letter."""

import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.otlLib import builder as otl
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables import otTables

UPEM = 1000

lower = [chr(code) for code in range(ord("a"), ord("z") + 1)]
upper = [chr(code) for code in range(ord("A"), ord("Z") + 1)]
digits = [chr(code) for code in range(ord("0"), ord("9") + 1)]
letters = lower + upper
names = [".notdef", "space"] + letters + [f"d{digit}" for digit in digits] + ["f_i"]
names += [f"{letter}.alt" for letter in letters]
glyph_map = {name: index for index, name in enumerate(names)}
cmap = {0x20: "space", 0xFB01: "f_i"}
cmap.update({ord(letter): letter for letter in letters})
cmap.update({ord(digit): f"d{digit}" for digit in digits})


def coverage(glyphs):
    return otl.buildCoverage(glyphs, glyph_map)


def class_def(classes):
    table = otTables.ClassDef()
    table.classDefs = dict(classes)
    return table


def record():
    lookup = otTables.SubstLookupRecord()
    lookup.SequenceIndex = 0
    lookup.LookupListIndex = 0
    return lookup


def chained_coverage(backtrack, inputs, lookahead):
    subtable = otTables.ChainContextSubst()
    subtable.Format = 3
    subtable.BacktrackGlyphCount = len(backtrack)
    subtable.BacktrackCoverage = [coverage(glyphs) for glyphs in backtrack]
    subtable.InputGlyphCount = len(inputs)
    subtable.InputCoverage = [coverage(glyphs) for glyphs in inputs]
    subtable.LookAheadGlyphCount = len(lookahead)
    subtable.LookAheadCoverage = [coverage(glyphs) for glyphs in lookahead]
    subtable.SubstCount = 1
    subtable.SubstLookupRecord = [record()]
    return subtable


def chained_rule(rule, backtrack, rest, lookahead):
    rule.BacktrackGlyphCount = len(backtrack)
    rule.Backtrack = backtrack
    rule.InputGlyphCount = len(rest) + 1
    rule.Input = rest
    rule.LookAheadGlyphCount = len(lookahead)
    rule.LookAhead = lookahead
    rule.SubstCount = 1
    rule.SubstLookupRecord = [record()]
    return rule


def chained_glyphs(rules):
    subtable = otTables.ChainContextSubst()
    subtable.Format = 1
    firsts = sorted(rules, key=glyph_map.get)
    subtable.Coverage = coverage(firsts)
    subtable.ChainSubRuleSet = []
    for first in firsts:
        rule_set = otTables.ChainSubRuleSet()
        rule_set.ChainSubRule = [chained_rule(otTables.ChainSubRule(), *rule) for rule in rules[first]]
        rule_set.ChainSubRuleCount = len(rule_set.ChainSubRule)
        subtable.ChainSubRuleSet.append(rule_set)
    subtable.ChainSubRuleSetCount = len(firsts)
    return subtable


def chained_classes(classes, rules):
    subtable = otTables.ChainContextSubst()
    subtable.Format = 2
    backtrack, inputs, lookahead = classes
    subtable.Coverage = coverage(sorted(inputs, key=glyph_map.get))
    subtable.BacktrackClassDef = class_def(backtrack)
    subtable.InputClassDef = class_def(inputs)
    subtable.LookAheadClassDef = class_def(lookahead)
    count = max(inputs.values()) + 1
    subtable.ChainSubClassSet = []
    for index in range(count):
        if index not in rules:
            subtable.ChainSubClassSet.append(None)
            continue
        rule_set = otTables.ChainSubClassSet()
        rule_set.ChainSubClassRule = [
            chained_rule(otTables.ChainSubClassRule(), *rule) for rule in rules[index]
        ]
        rule_set.ChainSubClassRuleCount = len(rule_set.ChainSubClassRule)
        subtable.ChainSubClassSet.append(rule_set)
    subtable.ChainSubClassSetCount = count
    return subtable


def context_glyphs(first, rest):
    subtable = otTables.ContextSubst()
    subtable.Format = 1
    subtable.Coverage = coverage([first])
    rule = otTables.SubRule()
    rule.GlyphCount = len(rest) + 1
    rule.Input = rest
    rule.SubstCount = 1
    rule.SubstLookupRecord = [record()]
    rule_set = otTables.SubRuleSet()
    rule_set.SubRule = [rule]
    rule_set.SubRuleCount = 1
    subtable.SubRuleSet = [rule_set]
    subtable.SubRuleSetCount = 1
    return subtable


def context_classes(classes, first_class, rest):
    subtable = otTables.ContextSubst()
    subtable.Format = 2
    subtable.Coverage = coverage([glyph for glyph, of in classes.items() if of == first_class])
    subtable.ClassDef = class_def(classes)
    rule = otTables.SubClassRule()
    rule.GlyphCount = len(rest) + 1
    rule.Class = rest
    rule.SubstCount = 1
    rule.SubstLookupRecord = [record()]
    rule_set = otTables.SubClassSet()
    rule_set.SubClassRule = [rule]
    rule_set.SubClassRuleCount = 1
    subtable.SubClassSet = [None] * first_class + [rule_set]
    subtable.SubClassSetCount = first_class + 1
    return subtable


def context_coverage(inputs):
    subtable = otTables.ContextSubst()
    subtable.Format = 3
    subtable.GlyphCount = len(inputs)
    subtable.Coverage = [coverage(glyphs) for glyphs in inputs]
    subtable.SubstCount = 1
    subtable.SubstLookupRecord = [record()]
    return subtable


def lookup(subtable, flags=0):
    return otl.buildLookup([subtable], flags=flags)


lookups = [
    otl.buildLookup([otl.buildSingleSubstSubtable({letter: f"{letter}.alt" for letter in letters})]),
    lookup(chained_coverage([], [["a"]], [["b"]])),
    lookup(chained_coverage([], [["c"], ["d"]], [])),
    lookup(chained_coverage([["e"]], [["f"]], [])),
    lookup(chained_coverage([], [["g"]], [])),
    lookup(chained_glyphs({"h": [([], [], ["i"])], "k": [(["j"], [], [])]})),
    lookup(
        chained_classes(
            ({"o": 1}, {"l": 1, "m": 2, "p": 3, "q": 4, "r": 5}, {"n": 1}),
            {
                1: [([], [2], [])],
                3: [([], [], [1])],
                4: [([1], [], [])],
                5: [([], [], [0])],
            },
        )
    ),
    lookup(context_glyphs("t", ["u"])),
    lookup(context_classes({"v": 1, "w": 2}, 1, [2])),
    lookup(context_coverage([["x"], ["y"]])),
    lookup(chained_coverage([], [["z"]], [["A"]]), flags=otl.LOOKUP_FLAG_IGNORE_LIGATURES),
    lookup(chained_coverage([], [upper[1:]], [lower + [f"d{digit}" for digit in digits]])),
]

gsub = otTables.GSUB()
gsub.Version = 0x00010000
feature = otTables.Feature()
feature.FeatureParams = None
feature.LookupListIndex = list(range(1, len(lookups)))
feature.LookupCount = len(feature.LookupListIndex)
feature_record = otTables.FeatureRecord()
feature_record.FeatureTag = "calt"
feature_record.Feature = feature
gsub.FeatureList = otTables.FeatureList()
gsub.FeatureList.FeatureRecord = [feature_record]
gsub.FeatureList.FeatureCount = 1
gsub.ScriptList = otTables.ScriptList()
gsub.ScriptList.ScriptRecord = []
for tag in ("DFLT", "latn"):
    language = otTables.DefaultLangSys()
    language.ReqFeatureIndex = 0xFFFF
    language.FeatureIndex = [0]
    language.FeatureCount = 1
    language.LookupOrder = None
    script = otTables.Script()
    script.DefaultLangSys = language
    script.LangSysRecord = []
    script.LangSysCount = 0
    script_record = otTables.ScriptRecord()
    script_record.ScriptTag = tag
    script_record.Script = script
    gsub.ScriptList.ScriptRecord.append(script_record)
gsub.ScriptList.ScriptCount = 2
gsub.LookupList = otTables.LookupList()
gsub.LookupList.Lookup = lookups
gsub.LookupList.LookupCount = len(lookups)

builder = FontBuilder(UPEM, isTTF=True)
builder.setupGlyphOrder(names)
builder.setupCharacterMap(cmap)
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics(
    {name: (800 if name.endswith(".alt") else 500, 0) for name in names}
)
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({"familyName": "Cranpose Test Contexts", "styleName": "Regular"})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0, usWinAscent=800,
                 usWinDescent=200, usWeightClass=400, achVendID="CRNP")
builder.setupPost()
builder.font["GSUB"] = newTable("GSUB")
builder.font["GSUB"].table = gsub
gdef = otTables.GDEF()
gdef.Version = 0x00010000
gdef.GlyphClassDef = class_def({name: 2 if name == "f_i" else 1 for name in names[2:]})
gdef.AttachList = None
gdef.LigCaretList = None
gdef.MarkAttachClassDef = None
builder.font["GDEF"] = newTable("GDEF")
builder.font["GDEF"].table = gdef
builder.save(sys.argv[1])

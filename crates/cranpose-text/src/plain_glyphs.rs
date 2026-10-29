use std::ops::Range;

use harfrust::{Direction, Feature, Script as ShapingScript, ShaperFont, font::Font};
use icu_properties::{CodePointMapData, props::GraphemeClusterBreak};
use read_fonts::{
    FontRef, ReadError, TableProvider,
    collections::IntSet,
    tables::{
        gpos::{
            CursivePos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PairPosFormat1,
            PairPosFormat2, PositionLookup, PositionSubtables, SinglePos,
        },
        gsub::{
            AlternateSubst, LigatureSubst, LigatureSubstFormat1, MultipleSubst,
            ReverseChainSingleSubst, SingleSubst, SubstitutionLookup, SubstitutionSubtables,
        },
        layout::{
            ChainedSequenceContext, ChainedSequenceContextFormat1, ChainedSequenceContextFormat2,
            ChainedSequenceContextFormat3, ClassDef, CoverageTable, FeatureList, LookupFlag,
            ScriptList, SequenceContext, SequenceContextFormat1, SequenceContextFormat2,
            SequenceContextFormat3,
        },
    },
    types::{BigEndian, GlyphId, GlyphId16, Tag},
};

use crate::{
    font_fallback::needs_no_glyph,
    glyph_id,
    plain_chars::{CharGlyph, PairFit, PlainChar, PlainChars},
    plain_kerning::{KernSubtable, PlainKerning},
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
    shaper::font_bytes,
    word_split::{chosen_script, class_index, everything, moves, single},
};

// The glyphs of a font that a shape plan leaves as they are: no lookup of
// the plan's features starts at them and none is a mark. A run of
// characters that map to such glyphs shapes to their nominal glyphs at their
// advances, so it needs no shaping, as DirectWrite's text complexity check
// finds simple text.
pub(crate) struct PlainGlyphs {
    // The glyphs a lookup starts at, and the marks.
    busy: IntSet<GlyphId>,
    // The pairs of glyphs side by side that a ligature or a pair adjustment
    // starting at a plain glyph reads, sorted.
    pairs: Box<[(u32, u32)]>,
    // The pair adjustments that move only a plain glyph's advance.
    kerning: PlainKerning,
    // What the plan found for the characters it met; made at the first.
    chars: Option<PlainChars>,
}

// The features HarfBuzz applies to horizontal left to right text in a
// script without a shaper of its own.
const DEFAULT_FEATURES: [&[u8; 4]; 22] = [
    b"abvm", b"blwm", b"ccmp", b"locl", b"mark", b"mkmk", b"rlig", b"calt", b"clig", b"curs",
    b"dist", b"kern", b"liga", b"rclt", b"rvrn", b"ltra", b"ltrm", b"rand", b"Harf", b"HARF",
    b"Buzz", b"BUZZ",
];

// The glyphs lookups start at, the pairs of glyphs side by side that
// lookups starting at the other glyphs read, and the pair adjustments plain
// glyphs take.
struct Starts<'a> {
    busy: IntSet<GlyphId>,
    pairs: Vec<(u32, u32)>,
    kerning: Vec<KernSubtable>,
    // The space, which starts every word cut before spaces: a font may
    // adjust it with the few glyphs after it it pairs with.
    space: Option<GlyphId>,
    // The font's bytes, where kerning subtables are found again.
    font: &'a [u8],
    // How many more pairs contextual rules may add.
    room: usize,
}

// The most pairs the contextual rules of a plan add; the glyphs a rule that
// would add more starts at are busy.
const MOST_CONTEXT_PAIRS: usize = 512;

// The glyphs a contextual rule reads beside the glyph it starts at: in a
// run of plain glyphs, the glyph next to it on that side.
#[derive(Clone)]
enum Beside<'a> {
    // It reads none: it applies to its first glyph alone.
    Nothing,
    After(Glyphs<'a>),
    Before(Glyphs<'a>),
}

// The glyphs a rule reads at one place.
#[derive(Clone)]
enum Glyphs<'a> {
    One(GlyphId),
    Covered(CoverageTable<'a>),
    // A class of glyphs; class 0 holds every glyph no class names.
    Class(ClassDef<'a>, u32),
}

impl Glyphs<'_> {
    // Runs `each` on the glyphs; `None` when `each` gives `None`, or for
    // class 0, whose glyphs no list holds.
    fn each(&self, mut each: impl FnMut(GlyphId) -> Option<()>) -> Option<()> {
        match self {
            Self::One(glyph) => each(*glyph),
            Self::Covered(coverage) => coverage.iter().try_for_each(each),
            Self::Class(_, 0) => None,
            Self::Class(classes, class) => classes
                .iter()
                .filter(|&(_, of)| of == *class)
                .try_for_each(|(glyph, _)| each(glyph)),
        }
    }
}

// What a rule reads beside its first glyph: after it, else before it.
fn beside<'a>(after: Option<Glyphs<'a>>, before: Option<Glyphs<'a>>) -> Beside<'a> {
    match (after, before) {
        (Some(after), _) => Beside::After(after),
        (None, Some(before)) => Beside::Before(before),
        (None, None) => Beside::Nothing,
    }
}

fn one(glyph: &BigEndian<GlyphId16>) -> Glyphs<'static> {
    Glyphs::One(glyph.get().into())
}

fn class<'a>(classes: &ClassDef<'a>, class: &BigEndian<u16>) -> Glyphs<'a> {
    Glyphs::Class(classes.clone(), u32::from(class.get()))
}

impl PlainGlyphs {
    // The plain glyphs of `font` under a plan for `script` and `direction`
    // with `features` on top of HarfBuzz's own; `None` when the plan may
    // change glyphs this does not follow: a script with a shaper of its own,
    // right to left text, AAT or `kern` tables, or a table it cannot read.
    pub(crate) fn of(
        font: &FontRef<'_>,
        script: Option<ShapingScript>,
        direction: Direction,
        features: &[Feature],
    ) -> Option<Self> {
        let script = script.filter(|&script| shapes_by_default(script))?;
        let unread = [*b"morx", *b"mort", *b"kerx", *b"kern"]
            .iter()
            .any(|tag| font.table_data(Tag::new(tag)).is_some());
        if unread || direction != Direction::LeftToRight {
            return None;
        }
        let mut tags = IntSet::empty();
        tags.extend_unsorted(DEFAULT_FEATURES.iter().map(|tag| Tag::new(tag)));
        // The shaper sets each feature over the whole text: it turns the
        // feature on or off, the later setting winning, as HarfBuzz takes
        // them.
        for feature in features {
            if feature.value == 0 {
                tags.remove(feature.tag);
            } else {
                tags.insert(feature.tag);
            }
        }
        let scripts = script.tags();
        let mut starts = Starts {
            busy: IntSet::empty(),
            pairs: Vec::new(),
            kerning: Vec::new(),
            space: font.cmap().ok().and_then(|cmap| cmap.map_codepoint(' ')),
            font: font.data().as_bytes(),
            room: MOST_CONTEXT_PAIRS,
        };
        // The marks first: a contextual rule needs no pair with a glyph that
        // is busy already.
        mark_glyphs(font, &mut starts.busy)?;
        substitution_starts(font, scripts.as_slice(), &tags, &mut starts)?;
        positioning_starts(font, scripts.as_slice(), &tags, &mut starts)?;
        starts.pairs.sort_unstable();
        starts.pairs.dedup();
        Some(Self {
            busy: starts.busy,
            pairs: starts.pairs.into_boxed_slice(),
            kerning: PlainKerning::new(starts.kerning),
            chars: None,
        })
    }

    // Appends the nominal glyphs of `range` of `text` at `font`'s advances,
    // kerned as the plan kerns them, when every one is plain, and returns
    // whether it did.
    pub(crate) fn append(
        &mut self,
        font: &Font,
        text: &str,
        range: Range<usize>,
        glyphs: &mut ShapedGlyphs,
    ) -> bool {
        let first = glyphs.len();
        let Some(run) = text.get(range.clone()) else {
            return false;
        };
        let Self {
            busy,
            pairs,
            kerning,
            chars,
        } = self;
        let chars = chars.get_or_insert_with(PlainChars::default);
        // The shaping font, made for the first character the plan has not
        // met.
        let mut shaping = None;
        let mut previous: Option<PlainChar> = None;
        for (at, ch) in run.char_indices() {
            let read = || {
                let (glyph, advance) = plain_glyph(
                    shaping.get_or_insert_with(|| ShaperFont::new(font)),
                    busy,
                    ch,
                )?;
                let starts = pairs.partition_point(|&(first, _)| first < glyph);
                Some(CharGlyph {
                    glyph,
                    advance,
                    pairs: pairs.get(starts).is_some_and(|&(first, _)| first == glyph)
                        || kerning.starts(font_bytes(font), glyph.into()),
                })
            };
            let Some(plain) = chars.char(ch, read) else {
                glyphs.truncate(first);
                return false;
            };
            // A glyph that starts no pair kerns with nothing after it.
            let fit = previous
                .filter(|before| before.pairs)
                .map_or(PairFit::Kern(0), |before| {
                    chars.pair(before, plain, || {
                        let pair = (u32::from(before.glyph), u32::from(plain.glyph));
                        if pairs.binary_search(&pair).is_ok() {
                            PairFit::Joins
                        } else {
                            PairFit::Kern(kerning.between(
                                font_bytes(font),
                                pair.0.into(),
                                pair.1.into(),
                            ))
                        }
                    })
                });
            // A pair adjustment moves the glyph before; a line break between
            // the two would undo it.
            let kern = match fit {
                PairFit::Joins => {
                    glyphs.truncate(first);
                    return false;
                }
                PairFit::Kern(kern) => kern,
            };
            if kern != 0
                && let Some(before) = glyphs.glyphs_mut().last_mut()
            {
                before.advance += kern;
            }
            previous = Some(plain);
            glyphs.push(
                ShapedGlyph::new(
                    glyph_id::GlyphId(u32::from(plain.glyph)),
                    u32::try_from(range.start + at).unwrap_or(u32::MAX),
                    i32::from(plain.advance),
                    if kern == 0 {
                        BreakBefore::Safe
                    } else {
                        BreakBefore::Unsafe
                    },
                ),
                GlyphOffset::default(),
            );
        }
        true
    }
}

// The nominal glyph of `ch` in `font` and its advance, when it is plain: it
// stands as itself and maps to a glyph no lookup starts at.
fn plain_glyph(font: &ShaperFont<'_, '_>, busy: &IntSet<GlyphId>, ch: char) -> Option<(u32, i32)> {
    if !stands_as_itself(ch) {
        return None;
    }
    let glyph = font
        .nominal_glyph(u32::from(ch))
        .filter(|&glyph| glyph != GlyphId::NOTDEF && !busy.contains(glyph))?;
    Some((glyph.to_u32(), font.glyph_h_advance(glyph)))
}

// Whether HarfBuzz shapes `script` with its default shaper, whatever the
// font: these scripts have no shaper of their own.
fn shapes_by_default(script: ShapingScript) -> bool {
    [
        ShapingScript::COMMON,
        ShapingScript::LATIN,
        ShapingScript::GREEK,
        ShapingScript::CYRILLIC,
        ShapingScript::ARMENIAN,
        ShapingScript::GEORGIAN,
        ShapingScript::HAN,
        ShapingScript::HIRAGANA,
        ShapingScript::KATAKANA,
        ShapingScript::BOPOMOFO,
    ]
    .contains(&script)
}

// Whether HarfBuzz gives `ch` its nominal glyph in a cluster of its own,
// whatever stands beside it: a character that starts a grapheme cluster
// and ends it, so no mark, joiner, control, prepended or regional indicator
// character, with no canonical decomposition and no glyph hidden.
fn stands_as_itself(ch: char) -> bool {
    if ch.is_ascii() {
        return ch == ' ' || ch.is_ascii_graphic();
    }
    CodePointMapData::<GraphemeClusterBreak>::new().get(ch) == GraphemeClusterBreak::Other
        && !needs_no_glyph(ch)
        && harfrust::unicode::decompose(u32::from(ch)).is_none()
        // The fraction slash turns fraction features on around it.
        && ch != '\u{2044}'
}

// Adds to `tags` the features each language of `script` requires, which
// HarfBuzz applies whatever features are asked.
fn add_required(
    scripts: &ScriptList<'_>,
    features: &FeatureList<'_>,
    script: Tag,
    tags: &mut IntSet<Tag>,
) -> Result<(), ReadError> {
    let Some(index) = scripts.index_for_tag(script) else {
        return Ok(());
    };
    let Some(record) = scripts.script_records().get(usize::from(index)) else {
        return Ok(());
    };
    let script = record.script(scripts.offset_data())?;
    let mut languages = vec![];
    if let Some(language) = script.default_lang_sys() {
        languages.push(language?);
    }
    for record in script.lang_sys_records() {
        languages.push(record.lang_sys(script.offset_data())?);
    }
    for language in languages {
        let required = usize::from(language.required_feature_index());
        if let Some(feature) = features.feature_records().get(required) {
            tags.insert(feature.feature_tag());
        }
    }
    Ok(())
}

fn substitution_starts(
    font: &FontRef<'_>,
    scripts: &[Tag],
    features: &IntSet<Tag>,
    starts: &mut Starts<'_>,
) -> Option<()> {
    let Ok(gsub) = font.gsub() else {
        return Some(());
    };
    let script_list = gsub.script_list().ok()?;
    let Some(script) = chosen_script(&script_list, scripts) else {
        return Some(());
    };
    let mut tags = features.clone();
    add_required(&script_list, &gsub.feature_list().ok()?, script, &mut tags).ok()?;
    let indices = gsub
        .collect_features(&single(script), &everything(), &tags)
        .ok()?;
    let list = gsub.lookup_list().ok()?;
    for index in gsub.collect_lookups(&indices).ok()?.iter() {
        let lookup = list.lookups().get(usize::from(index)).ok()?;
        substitution_lookup_starts(&lookup, starts)?;
    }
    Some(())
}

fn positioning_starts(
    font: &FontRef<'_>,
    scripts: &[Tag],
    features: &IntSet<Tag>,
    starts: &mut Starts<'_>,
) -> Option<()> {
    let Ok(gpos) = font.gpos() else {
        return Some(());
    };
    let script_list = gpos.script_list().ok()?;
    let Some(script) = chosen_script(&script_list, scripts) else {
        return Some(());
    };
    let mut tags = features.clone();
    add_required(&script_list, &gpos.feature_list().ok()?, script, &mut tags).ok()?;
    let indices = gpos
        .collect_features(&single(script), &everything(), &tags)
        .ok()?;
    let list = gpos.lookup_list().ok()?;
    for index in gpos.collect_lookups(&indices).ok()?.iter() {
        let lookup = list.lookups().get(usize::from(index)).ok()?;
        positioning_lookup_starts(&lookup, index, starts)?;
    }
    Some(())
}

// Adds the font's marks to `busy`: HarfBuzz zeroes their advances.
fn mark_glyphs(font: &FontRef<'_>, busy: &mut IntSet<GlyphId>) -> Option<()> {
    const MARK: u32 = 3;
    let Ok(gdef) = font.gdef() else {
        return Some(());
    };
    let Some(classes) = gdef.glyph_class_def() else {
        return Some(());
    };
    busy.extend_unsorted(
        classes
            .ok()?
            .iter()
            .filter(|&(_, class)| class == MARK)
            .map(|(glyph, _)| glyph),
    );
    Some(())
}

// Runs `start` on each of `subtables`; `None` when one does not read.
fn each<T>(
    subtables: impl Iterator<Item = Result<T, ReadError>>,
    mut start: impl FnMut(T) -> Option<()>,
) -> Option<()> {
    for subtable in subtables {
        start(subtable.ok()?)?;
    }
    Some(())
}

impl Starts<'_> {
    fn cover(&mut self, coverage: Result<CoverageTable<'_>, ReadError>) -> Option<()> {
        self.busy.extend_unsorted(coverage.ok()?.iter());
        Some(())
    }

    // The pair adjustments of `pair`, in the lookup at `lookup` with
    // `flag`: kerning when they move only the first glyph's advance and the
    // two glyphs stand side by side, else the glyphs they start at. The
    // space moves only with the glyphs after it it pairs with, which go to
    // the pairs, unless `flag` lets the adjustment skip glyphs between the
    // two.
    fn pair_adjustments(
        &mut self,
        pair: &PairPos<'_>,
        (lookup, flag): (u16, LookupFlag),
    ) -> Option<()> {
        if !skips_glyphs(flag)
            && let Some(kerning) = KernSubtable::of(pair, lookup, self.font)
        {
            self.kerning.push(kerning);
            return Some(());
        }
        let coverage = match pair {
            PairPos::Format1(format) => format.coverage(),
            PairPos::Format2(format) => format.coverage(),
            _ => return None,
        }
        .ok()?;
        let space = self
            .space
            .filter(|&space| !skips_glyphs(flag) && coverage.get(space).is_some());
        self.busy
            .extend_unsorted(coverage.iter().filter(|&glyph| Some(glyph) != space));
        let Some(space) = space else {
            return Some(());
        };
        let mut after = IntSet::empty();
        let read = match pair {
            PairPos::Format1(format) => glyph_pairs_after(format, coverage.get(space)?, &mut after),
            PairPos::Format2(format) => class_pairs_after(format, space, &mut after),
            _ => None,
        };
        match read {
            Some(true) => self
                .pairs
                .extend(after.iter().map(|second| (space.to_u32(), second.to_u32()))),
            _ => {
                self.busy.insert(space);
            }
        }
        Some(())
    }

    // The ligatures of `format`; one that may skip glyphs between its
    // components, as `flag` lets it, makes its first glyph busy.
    fn ligatures(&mut self, format: &LigatureSubstFormat1<'_>, flag: LookupFlag) -> Option<()> {
        if skips_glyphs(flag) {
            return self.cover(format.coverage());
        }
        let sets = format.ligature_sets();
        for (at, first) in format.coverage().ok()?.iter().enumerate() {
            for ligature in sets.get(at).ok()?.ligatures().iter() {
                match ligature.ok()?.component_glyph_ids().first() {
                    Some(next) => self
                        .pairs
                        .push((first.to_u32(), u32::from(next.get().to_u16()))),
                    None => {
                        self.busy.insert(first);
                    }
                }
            }
        }
        Some(())
    }

    // The rules of a contextual subtable, each as the pairs of the glyph it
    // starts at with the glyphs it reads beside it: a plain run holds such a
    // pair nowhere, so the rule cannot match in it. The glyphs a subtable
    // starts at are busy when it may skip glyphs between those it reads, as
    // `flag` lets it, or when `read` gives up: on rules past the room left,
    // or reading class 0.
    fn rules(
        &mut self,
        firsts: Start<'_>,
        flag: LookupFlag,
        read: impl FnOnce(&mut Self) -> Option<()>,
    ) -> Option<()> {
        let firsts = firsts?.ok()?;
        let (added, room) = (self.pairs.len(), self.room);
        if skips_glyphs(flag) || read(self).is_none() {
            self.pairs.truncate(added);
            self.room = room;
            self.busy.extend_unsorted(firsts.iter());
        }
        Some(())
    }

    // A rule starting at `first` reading `beside`: its pairs with the glyphs
    // that are not busy, or `first` busy when it reads nothing beside.
    fn rule(&mut self, first: GlyphId, beside: Beside<'_>) -> Option<()> {
        let (glyphs, after) = match beside {
            Beside::Nothing => {
                self.busy.insert(first);
                return Some(());
            }
            Beside::After(glyphs) => (glyphs, true),
            Beside::Before(glyphs) => (glyphs, false),
        };
        if self.busy.contains(first) {
            return Some(());
        }
        glyphs.each(|other| {
            if self.busy.contains(other) {
                return Some(());
            }
            self.room = self.room.checked_sub(1)?;
            let (one, two) = if after {
                (first, other)
            } else {
                (other, first)
            };
            self.pairs.push((one.to_u32(), two.to_u32()));
            Some(())
        })
    }

    fn context(&mut self, context: &SequenceContext<'_>, flag: LookupFlag) -> Option<()> {
        self.rules(context_start(context), flag, |starts| match context {
            SequenceContext::Format1(format) => starts.glyph_rules(format),
            SequenceContext::Format2(format) => starts.class_rules(format),
            SequenceContext::Format3(format) => starts.coverage_rule(format),
            _ => None,
        })
    }

    fn chained_context(
        &mut self,
        context: &ChainedSequenceContext<'_>,
        flag: LookupFlag,
    ) -> Option<()> {
        self.rules(
            chained_context_start(context),
            flag,
            |starts| match context {
                ChainedSequenceContext::Format1(format) => starts.chained_glyph_rules(format),
                ChainedSequenceContext::Format2(format) => starts.chained_class_rules(format),
                ChainedSequenceContext::Format3(format) => starts.chained_coverage_rule(format),
                _ => None,
            },
        )
    }

    fn glyph_rules(&mut self, format: &SequenceContextFormat1<'_>) -> Option<()> {
        let sets = format.seq_rule_sets();
        for (first, set) in format.coverage().ok()?.iter().zip(sets.iter()) {
            let Some(set) = set else {
                continue;
            };
            for rule in set.ok()?.seq_rules().iter() {
                let after = rule.ok()?.input_sequence().first().map(one);
                self.rule(first, beside(after, None))?;
            }
        }
        Some(())
    }

    fn class_rules(&mut self, format: &SequenceContextFormat2<'_>) -> Option<()> {
        let classes = format.class_def().ok()?;
        let sets = format.class_seq_rule_sets();
        for first in format.coverage().ok()?.iter() {
            let index = class_index(classes.get(first));
            let Some(set) = sets.get(index).filter(|_| index < sets.len()) else {
                continue;
            };
            for rule in set.ok()?.class_seq_rules().iter() {
                let after = rule
                    .ok()?
                    .input_sequence()
                    .first()
                    .map(|next| class(&classes, next));
                self.rule(first, beside(after, None))?;
            }
        }
        Some(())
    }

    fn coverage_rule(&mut self, format: &SequenceContextFormat3<'_>) -> Option<()> {
        let coverages = format.coverages();
        let after = match coverages.len() {
            0 | 1 => None,
            _ => Some(Glyphs::Covered(coverages.get(1).ok()?)),
        };
        let beside = beside(after, None);
        for first in coverages.get(0).ok()?.iter() {
            self.rule(first, beside.clone())?;
        }
        Some(())
    }

    fn chained_glyph_rules(&mut self, format: &ChainedSequenceContextFormat1<'_>) -> Option<()> {
        let sets = format.chained_seq_rule_sets();
        for (first, set) in format.coverage().ok()?.iter().zip(sets.iter()) {
            let Some(set) = set else {
                continue;
            };
            for rule in set.ok()?.chained_seq_rules().iter() {
                let rule = rule.ok()?;
                let next = rule.input_sequence().first();
                let after = next.or_else(|| rule.lookahead_sequence().first()).map(one);
                let before = rule.backtrack_sequence().first().map(one);
                self.rule(first, beside(after, before))?;
            }
        }
        Some(())
    }

    fn chained_class_rules(&mut self, format: &ChainedSequenceContextFormat2<'_>) -> Option<()> {
        let input = format.input_class_def().ok()?;
        let lookahead = format.lookahead_class_def().ok()?;
        let backtrack = format.backtrack_class_def().ok()?;
        let sets = format.chained_class_seq_rule_sets();
        for first in format.coverage().ok()?.iter() {
            let index = class_index(input.get(first));
            let Some(set) = sets.get(index).filter(|_| index < sets.len()) else {
                continue;
            };
            for rule in set.ok()?.chained_class_seq_rules().iter() {
                let rule = rule.ok()?;
                let after = match rule.input_sequence().first() {
                    Some(next) => Some(class(&input, next)),
                    None => rule
                        .lookahead_sequence()
                        .first()
                        .map(|next| class(&lookahead, next)),
                };
                let before = rule
                    .backtrack_sequence()
                    .first()
                    .map(|previous| class(&backtrack, previous));
                self.rule(first, beside(after, before))?;
            }
        }
        Some(())
    }

    fn chained_coverage_rule(&mut self, format: &ChainedSequenceContextFormat3<'_>) -> Option<()> {
        let inputs = format.input_coverages();
        let lookahead = format.lookahead_coverages();
        let backtrack = format.backtrack_coverages();
        let after = if inputs.len() > 1 {
            Some(inputs.get(1).ok()?)
        } else if !lookahead.is_empty() {
            Some(lookahead.get(0).ok()?)
        } else {
            None
        };
        let before = match backtrack.len() {
            0 => None,
            _ => Some(backtrack.get(0).ok()?),
        };
        let beside = beside(after.map(Glyphs::Covered), before.map(Glyphs::Covered));
        for first in inputs.get(0).ok()?.iter() {
            self.rule(first, beside.clone())?;
        }
        Some(())
    }
}

// Adds to `after` the glyphs a pair set of `format` at `index` in its
// coverage moves with its first glyph; `Some(true)` when it read them.
fn glyph_pairs_after(
    format: &PairPosFormat1<'_>,
    index: u32,
    after: &mut IntSet<GlyphId>,
) -> Option<bool> {
    let set = format.pair_sets().get(class_index(index)).ok()?;
    for record in set.pair_value_records().iter() {
        let record = record.ok()?;
        if moves(record.value_record1()) || moves(record.value_record2()) {
            after.insert(GlyphId::from(record.second_glyph()));
        }
    }
    Some(true)
}

// Adds to `after` the glyphs of each class `format` moves with `first`'s
// class; `Some(false)` when it moves every glyph no class names.
fn class_pairs_after(
    format: &PairPosFormat2<'_>,
    first: GlyphId,
    after: &mut IntSet<GlyphId>,
) -> Option<bool> {
    let row = class_index(format.class_def1().ok()?.get(first));
    let row = format.class1_records().get(row).ok()?;
    let seconds = format.class_def2().ok()?;
    for (class, cell) in row.class2_records().iter().enumerate() {
        let cell = cell.ok()?;
        if !moves(cell.value_record1()) && !moves(cell.value_record2()) {
            continue;
        }
        if class == 0 {
            return Some(false);
        }
        let class = u32::try_from(class).ok()?;
        after.extend_unsorted(
            seconds
                .iter()
                .filter(|&(_, of)| of == class)
                .map(|(glyph, _)| glyph),
        );
    }
    Some(true)
}

// Whether a lookup with `flag` may skip base glyphs or ligatures between
// the glyphs it reads.
fn skips_glyphs(flag: LookupFlag) -> bool {
    flag.to_bits()
        & (LookupFlag::IGNORE_BASE_GLYPHS.to_bits() | LookupFlag::IGNORE_LIGATURES.to_bits())
        != 0
}

// Covers the glyphs each of `subtables` starts at, which `start` reads;
// `None` when one does not read.
fn covers<'a, T>(
    subtables: impl Iterator<Item = Result<T, ReadError>>,
    starts: &mut Starts<'_>,
    start: impl Fn(&T) -> Option<Result<CoverageTable<'a>, ReadError>>,
) -> Option<()> {
    each(subtables, |subtable| starts.cover(start(&subtable)?))
}

fn substitution_lookup_starts(
    lookup: &SubstitutionLookup<'_>,
    starts: &mut Starts<'_>,
) -> Option<()> {
    let flag = lookup.lookup_flag();
    match lookup.subtables().ok()? {
        SubstitutionSubtables::Single(subtables) => covers(subtables.iter(), starts, single_start),
        SubstitutionSubtables::Multiple(subtables) => {
            covers(subtables.iter(), starts, multiple_start)
        }
        SubstitutionSubtables::Alternate(subtables) => {
            covers(subtables.iter(), starts, alternate_start)
        }
        SubstitutionSubtables::Ligature(subtables) => each(subtables.iter(), |subtable| {
            let LigatureSubst::Format1(format) = subtable else {
                return None;
            };
            starts.ligatures(&format, flag)
        }),
        SubstitutionSubtables::Contextual(subtables) => {
            each(subtables.iter(), |subtable| starts.context(&subtable, flag))
        }
        SubstitutionSubtables::ChainContextual(subtables) => each(subtables.iter(), |subtable| {
            starts.chained_context(&subtable, flag)
        }),
        SubstitutionSubtables::Reverse(subtables) => {
            covers(subtables.iter(), starts, reverse_start)
        }
        SubstitutionSubtables::EmptyExtension => Some(()),
    }
}

fn positioning_lookup_starts(
    lookup: &PositionLookup<'_>,
    index: u16,
    starts: &mut Starts<'_>,
) -> Option<()> {
    let flag = lookup.lookup_flag();
    match lookup.subtables().ok()? {
        PositionSubtables::Single(subtables) => covers(subtables.iter(), starts, adjust_start),
        PositionSubtables::Pair(subtables) => each(subtables.iter(), |subtable| {
            starts.pair_adjustments(&subtable, (index, flag))
        }),
        PositionSubtables::Cursive(subtables) => covers(subtables.iter(), starts, cursive_start),
        PositionSubtables::MarkToBase(subtables) => {
            covers(subtables.iter(), starts, mark_base_start)
        }
        PositionSubtables::MarkToLig(subtables) => {
            covers(subtables.iter(), starts, mark_ligature_start)
        }
        PositionSubtables::MarkToMark(subtables) => {
            covers(subtables.iter(), starts, mark_mark_start)
        }
        PositionSubtables::Contextual(subtables) => {
            each(subtables.iter(), |subtable| starts.context(&subtable, flag))
        }
        PositionSubtables::ChainContextual(subtables) => each(subtables.iter(), |subtable| {
            starts.chained_context(&subtable, flag)
        }),
        PositionSubtables::EmptyExtension => Some(()),
    }
}

// The coverage of the glyphs each kind of subtable starts at; `None` for a
// format this does not read.
type Start<'a> = Option<Result<CoverageTable<'a>, ReadError>>;

fn single_start<'a>(subtable: &SingleSubst<'a>) -> Start<'a> {
    match subtable {
        SingleSubst::Format1(format) => Some(format.coverage()),
        SingleSubst::Format2(format) => Some(format.coverage()),
        _ => None,
    }
}

fn multiple_start<'a>(subtable: &MultipleSubst<'a>) -> Start<'a> {
    match subtable {
        MultipleSubst::Format1(format) => Some(format.coverage()),
        MultipleSubst::Format2(_) => None,
    }
}

fn alternate_start<'a>(subtable: &AlternateSubst<'a>) -> Start<'a> {
    match subtable {
        AlternateSubst::Format1(format) => Some(format.coverage()),
        AlternateSubst::Format2(_) => None,
    }
}

fn reverse_start<'a>(subtable: &ReverseChainSingleSubst<'a>) -> Start<'a> {
    match subtable {
        ReverseChainSingleSubst::Format1(format) => Some(format.coverage()),
        ReverseChainSingleSubst::Format2(_) => None,
    }
}

fn adjust_start<'a>(subtable: &SinglePos<'a>) -> Start<'a> {
    match subtable {
        SinglePos::Format1(format) => Some(format.coverage()),
        SinglePos::Format2(format) => Some(format.coverage()),
        _ => None,
    }
}

fn cursive_start<'a>(subtable: &CursivePos<'a>) -> Start<'a> {
    match subtable {
        CursivePos::Format1(format) => Some(format.coverage()),
        CursivePos::Format2(_) => None,
    }
}

// A mark attachment starts at the mark.
fn mark_base_start<'a>(subtable: &MarkBasePos<'a>) -> Start<'a> {
    match subtable {
        MarkBasePos::Format1(format) => Some(format.mark_coverage()),
        MarkBasePos::Format2(_) => None,
    }
}

fn mark_ligature_start<'a>(subtable: &MarkLigPos<'a>) -> Start<'a> {
    match subtable {
        MarkLigPos::Format1(format) => Some(format.mark_coverage()),
        MarkLigPos::Format2(_) => None,
    }
}

fn mark_mark_start<'a>(subtable: &MarkMarkPos<'a>) -> Start<'a> {
    match subtable {
        MarkMarkPos::Format1(format) => Some(format.mark1_coverage()),
        MarkMarkPos::Format2(_) => None,
    }
}

fn context_start<'a>(context: &SequenceContext<'a>) -> Start<'a> {
    match context {
        SequenceContext::Format1(format) => Some(format.coverage()),
        SequenceContext::Format2(format) => Some(format.coverage()),
        SequenceContext::Format3(format) => Some(format.coverages().get(0)),
        _ => None,
    }
}

fn chained_context_start<'a>(context: &ChainedSequenceContext<'a>) -> Start<'a> {
    match context {
        ChainedSequenceContext::Format1(format) => Some(format.coverage()),
        ChainedSequenceContext::Format2(format) => Some(format.coverage()),
        ChainedSequenceContext::Format3(format) => Some(format.input_coverages().get(0)),
        _ => None,
    }
}

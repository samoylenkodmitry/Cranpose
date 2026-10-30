use std::ops::Range;

use harfrust::{Direction, Feature, Scale, Script as ShapingScript, font::Font};
use icu_properties::{CodePointMapData, props::GraphemeClusterBreak};
use read_fonts::{
    FontRef, TableProvider,
    collections::IntSet,
    tables::{
        gpos::{PairPos, PairPosFormat1, PairPosFormat2},
        gsub::{LigatureSubst, LigatureSubstFormat1, SingleSubst},
        layout::{
            ChainedSequenceContext, ChainedSequenceContextFormat1, ChainedSequenceContextFormat2,
            ChainedSequenceContextFormat3, ClassDef, CoverageTable, LookupFlag, SequenceContext,
            SequenceContextFormat1, SequenceContextFormat2, SequenceContextFormat3,
        },
    },
    types::{BigEndian, GlyphId, GlyphId16, Tag},
};

use crate::{
    font_fallback::needs_no_glyph,
    glyph_id,
    plain_chars::{CharGlyph, PairFit, PlainChar, PlainChars},
    plain_kerning::{KernSubtable, PlainKerning},
    script_lookups::{
        Lookup, LookupSubtable, ScriptLookups, insert_class, insert_covered,
        substitutions_everywhere,
    },
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
    shaper::font_bytes,
    word_split::{class_index, moves},
};

// The glyphs of a font that a shape plan leaves as they are, or puts one
// glyph in place of: no lookup of the plan's features starts at them but a
// substitution of one glyph that every language applies, and none is a
// mark. A run of characters that map to such glyphs shapes to their nominal
// glyphs, or the glyphs put in their place, at their advances, so it needs
// no shaping, as DirectWrite's text complexity check finds simple text.
pub(crate) struct PlainGlyphs {
    // The glyphs a lookup starts at, and the marks.
    busy: IntSet<GlyphId>,
    // The glyphs put in place of plain glyphs, by the glyph each replaces,
    // sorted; the glyphs put in place are plain.
    substitutes: Box<[(u32, u32)]>,
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
    // The substitutions of one glyph that every language applies, each
    // glyph with the glyph put in its place.
    substitutes: Vec<(u32, u32)>,
    // The substitution lookups every language of the script applies.
    everywhere: IntSet<u16>,
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

fn one(glyph: BigEndian<GlyphId16>) -> Glyphs<'static> {
    Glyphs::One(glyph.get().into())
}

fn class<'a>(classes: &ClassDef<'a>, class: BigEndian<u16>) -> Glyphs<'a> {
    Glyphs::Class(classes.clone(), u32::from(class.get()))
}

impl PlainGlyphs {
    // The plain glyphs of `font` under a plan for `script` and `direction`
    // with `features` on top of HarfBuzz's own; `None` when the plan may
    // change glyphs this does not follow: a script with a shaper of its own,
    // right to left text, AAT or `kern` tables, or a table it cannot read.
    #[inline(never)]
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
        for tag in DEFAULT_FEATURES {
            tags.insert(Tag::new(tag));
        }
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
        let scripts = scripts.as_slice();
        let mut starts = Starts {
            busy: IntSet::empty(),
            pairs: Vec::new(),
            substitutes: Vec::new(),
            everywhere: IntSet::empty(),
            kerning: Vec::new(),
            space: font.cmap().ok().and_then(|cmap| cmap.map_codepoint(' ')),
            font: font.data().as_bytes(),
            room: MOST_CONTEXT_PAIRS,
        };
        // The marks first: a contextual rule needs no pair with a glyph that
        // is busy already.
        mark_glyphs(font, &mut starts.busy)?;
        starts.lookups(font, scripts, &tags)?;
        starts.pairs.sort_unstable();
        starts.pairs.dedup();
        let substitutes = starts.substitutes();
        Some(Self {
            busy: starts.busy,
            substitutes,
            pairs: starts.pairs.into_boxed_slice(),
            kerning: PlainKerning::new(starts.kerning),
            chars: None,
        })
    }

    // Whether `other` leaves the same glyphs plain and kerns them the same,
    // so the characters and pairs it meets read as this one reads them.
    pub(crate) fn reads_as(&self, other: &Self) -> bool {
        self.busy == other.busy
            && self.substitutes == other.substitutes
            && self.pairs == other.pairs
            && self.kerning == other.kerning
    }

    // Appends the plain glyphs of `range` of `text` at `font`'s advances,
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
            substitutes,
            pairs,
            kerning,
            chars,
        } = self;
        let chars = chars.get_or_insert_with(PlainChars::default);
        let mut previous: Option<PlainChar> = None;
        for (at, ch) in run.char_indices() {
            let read = || {
                let (glyph, advance) = plain_glyph(font, (busy, substitutes), ch)?;
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

// The glyph of `ch` in `font` and its advance, when it is plain: it stands
// as itself and maps to a glyph no lookup starts at but one of the
// `substitutes`, which puts its glyph in place. Both are read as HarfRust
// reads them by default, without the layout data a shaping font prepares:
// a character the `cmap` leaves out goes to HarfRust, which also tries the
// private use area of an Arabic symbol font.
#[inline(never)]
fn plain_glyph(
    font: &Font,
    (busy, substitutes): (&IntSet<GlyphId>, &[(u32, u32)]),
    ch: char,
) -> Option<(u32, i32)> {
    if !stands_as_itself(ch) {
        return None;
    }
    let nominal = font
        .charmap()
        .map_unicode(ch)
        .filter(|&glyph| glyph != GlyphId::NOTDEF && !busy.contains(glyph))?
        .to_u32();
    let glyph = substitutes
        .binary_search_by_key(&nominal, |&(from, _)| from)
        .map_or(nominal, |at| substitutes[at].1);
    let advance = font
        .glyph_metrics()
        .scaled(Scale::default())
        .h_advance(GlyphId::new(glyph));
    Some((glyph, advance.max(0)))
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

// Adds the font's marks to `busy`: HarfBuzz zeroes their advances. This
// and the walk of the lookups run once a plan, at the bottom of a layout;
// each stays out of line, so the stack holds one frame of them at a time,
// which a microcontroller's main task has room for.
#[inline(never)]
fn mark_glyphs(font: &FontRef<'_>, busy: &mut IntSet<GlyphId>) -> Option<()> {
    const MARK: u32 = 3;
    let Ok(gdef) = font.gdef() else {
        return Some(());
    };
    let Some(classes) = gdef.glyph_class_def() else {
        return Some(());
    };
    insert_class(busy, &classes.ok()?, MARK);
    Some(())
}

impl Starts<'_> {
    // What the lookups `features` apply to `scripts` in `font`'s GSUB and
    // GPOS start at.
    fn lookups(
        &mut self,
        font: &FontRef<'_>,
        scripts: &[Tag],
        features: &IntSet<Tag>,
    ) -> Option<()> {
        if let Ok(gsub) = font.gsub()
            && let Some(lookups) = ScriptLookups::substitutions(&gsub, scripts, features).ok()?
        {
            self.everywhere = substitutions_everywhere(&gsub, scripts, features).ok()?;
            lookups.each(&mut |at, subtable| self.subtable(at, subtable))?;
        }
        if let Ok(gpos) = font.gpos()
            && let Some(lookups) = ScriptLookups::positionings(&gpos, scripts, features).ok()?
        {
            lookups.each(&mut |at, subtable| self.subtable(at, subtable))?;
        }
        Some(())
    }

    // What `subtable`, of the lookup `at`, starts at: kerning for a pair
    // adjustment that moves only plain advances, the glyphs put in place of
    // others by a substitution of one glyph every language applies, else
    // the glyphs it starts at, or the pairs a rule reads beside them; `None`
    // for a format this does not read.
    #[inline(never)]
    fn subtable(&mut self, at: Lookup, subtable: LookupSubtable<'_>) -> Option<()> {
        let flag = at.flag;
        if let LookupSubtable::Single(single) = &subtable
            && !skips_glyphs(flag)
            && self.everywhere.contains(at.index)
        {
            return self.substitutions(single);
        }
        if let LookupSubtable::Pair(pair) = &subtable
            && !skips_glyphs(flag)
            && let Some(kerning) = KernSubtable::of(pair, at.index, self.font)
        {
            self.kerning.push(kerning);
            return Some(());
        }
        let firsts = subtable.start()?.ok()?;
        match subtable {
            LookupSubtable::Ligature(LigatureSubst::Format1(format)) => {
                self.ligatures(&format, &firsts, flag)
            }
            LookupSubtable::Pair(pair) => self.pair_adjustments(&pair, &firsts, flag),
            LookupSubtable::Context(context) => {
                self.rules(&firsts, flag, |starts| starts.context(&context))
            }
            LookupSubtable::ChainedContext(context) => {
                self.rules(&firsts, flag, |starts| starts.chained_context(&context))
            }
            _ => {
                insert_covered(&mut self.busy, &firsts);
                Some(())
            }
        }
    }

    // Adds each glyph `single` substitutes, with the glyph it puts in its
    // place, to the substitutes; `None` for a format this does not read.
    fn substitutions(&mut self, single: &SingleSubst<'_>) -> Option<()> {
        match single {
            SingleSubst::Format1(format) => {
                let delta = i32::from(format.delta_glyph_id());
                for glyph in format.coverage().ok()?.iter() {
                    let to = glyph.to_u32().wrapping_add_signed(delta) & u32::from(u16::MAX);
                    self.substitutes.push((glyph.to_u32(), to));
                }
            }
            SingleSubst::Format2(format) => {
                let to = format.substitute_glyph_ids();
                for (glyph, to) in format.coverage().ok()?.iter().zip(to) {
                    self.substitutes
                        .push((glyph.to_u32(), u32::from(to.get().to_u16())));
                }
            }
            SingleSubst::Format3(_) | SingleSubst::Format4(_) => return None,
        }
        Some(())
    }

    // The substitutes, sorted, each of a plain glyph to a plain glyph. A
    // glyph substituted twice, into a glyph substituted again or busy, or
    // that a rule reads beside another, goes to the busy glyphs: the shaper
    // takes those in the order of their lookups.
    #[inline(never)]
    fn substitutes(&mut self) -> Box<[(u32, u32)]> {
        let Self {
            busy,
            pairs,
            substitutes,
            ..
        } = self;
        substitutes.sort_unstable();
        let substituted = |glyph: u32| {
            substitutes
                .binary_search_by_key(&glyph, |&(from, _)| from)
                .is_ok()
        };
        for (at, &(from, to)) in substitutes.iter().enumerate() {
            let twice = substitutes
                .get(at + 1)
                .is_some_and(|&(next, _)| next == from);
            if twice || substituted(to) || busy.contains(GlyphId::new(to)) {
                busy.insert(GlyphId::new(from));
            }
        }
        for &(first, second) in pairs.iter() {
            for glyph in [first, second] {
                if substituted(glyph) {
                    busy.insert(GlyphId::new(glyph));
                }
            }
        }
        substitutes.retain(|&(from, _)| !busy.contains(GlyphId::new(from)));
        std::mem::take(substitutes).into_boxed_slice()
    }

    // The pair adjustments of `pair`, which start at `coverage`, in a
    // lookup with `flag` that does not kern plain glyphs: the glyphs they
    // start at. The space moves only with the glyphs after it it pairs
    // with, which go to the pairs, unless `flag` lets the adjustment skip
    // glyphs between the two.
    #[inline(never)]
    fn pair_adjustments(
        &mut self,
        pair: &PairPos<'_>,
        coverage: &CoverageTable<'_>,
        flag: LookupFlag,
    ) -> Option<()> {
        let space = self
            .space
            .filter(|&space| !skips_glyphs(flag) && coverage.get(space).is_some());
        // The space stays as plain as it was.
        let plain_space = space.filter(|&space| !self.busy.contains(space));
        insert_covered(&mut self.busy, coverage);
        if let Some(space) = plain_space {
            self.busy.remove(space);
        }
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

    // The ligatures of `format`, which start at `firsts`; one that may skip
    // glyphs between its components, as `flag` lets it, makes its first
    // glyph busy.
    fn ligatures(
        &mut self,
        format: &LigatureSubstFormat1<'_>,
        firsts: &CoverageTable<'_>,
        flag: LookupFlag,
    ) -> Option<()> {
        if skips_glyphs(flag) {
            insert_covered(&mut self.busy, firsts);
            return Some(());
        }
        let sets = format.ligature_sets();
        for (at, first) in firsts.iter().enumerate() {
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
        firsts: &CoverageTable<'_>,
        flag: LookupFlag,
        read: impl FnOnce(&mut Self) -> Option<()>,
    ) -> Option<()> {
        let (added, room) = (self.pairs.len(), self.room);
        if skips_glyphs(flag) || read(self).is_none() {
            self.pairs.truncate(added);
            self.room = room;
            insert_covered(&mut self.busy, firsts);
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

    fn context(&mut self, context: &SequenceContext<'_>) -> Option<()> {
        match context {
            SequenceContext::Format1(format) => self.glyph_rules(format),
            SequenceContext::Format2(format) => self.class_rules(format),
            SequenceContext::Format3(format) => self.coverage_rule(format),
            _ => None,
        }
    }

    fn chained_context(&mut self, context: &ChainedSequenceContext<'_>) -> Option<()> {
        match context {
            ChainedSequenceContext::Format1(format) => self.chained_glyph_rules(format),
            ChainedSequenceContext::Format2(format) => self.chained_class_rules(format),
            ChainedSequenceContext::Format3(format) => self.chained_coverage_rule(format),
            _ => None,
        }
    }

    fn glyph_rules(&mut self, format: &SequenceContextFormat1<'_>) -> Option<()> {
        let sets = format.seq_rule_sets();
        for (first, set) in format.coverage().ok()?.iter().zip(sets.iter()) {
            let Some(set) = set else {
                continue;
            };
            for rule in set.ok()?.seq_rules().iter() {
                let after = rule.ok()?.input_sequence().first().copied().map(one);
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
                    .map(|next| class(&classes, *next));
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
                let after = next
                    .or_else(|| rule.lookahead_sequence().first())
                    .copied()
                    .map(one);
                let before = rule.backtrack_sequence().first().copied().map(one);
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
                    Some(next) => Some(class(&input, *next)),
                    None => rule
                        .lookahead_sequence()
                        .first()
                        .map(|next| class(&lookahead, *next)),
                };
                let before = rule
                    .backtrack_sequence()
                    .first()
                    .map(|previous| class(&backtrack, *previous));
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
#[inline(never)]
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
#[inline(never)]
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
        insert_class(after, &seconds, u32::try_from(class).ok()?);
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

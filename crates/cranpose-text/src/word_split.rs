use std::cmp::Ordering;

use read_fonts::{
    FontRef, ReadError, TableProvider,
    array::ComputedArray,
    collections::IntSet,
    tables::{
        gpos::{
            CursivePos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PairPosFormat1,
            PairPosFormat2, PairValueRecord, ValueRecord,
        },
        gsub::{
            AlternateSubst, LigatureSubst, LigatureSubstFormat1, MultipleSubst,
            ReverseChainSingleSubst, SingleSubst,
        },
        layout::{
            ChainedSequenceContext, ChainedSequenceContextFormat2, ClassDef, CoverageTable,
            SequenceContext,
        },
    },
    types::{BigEndian, GlyphId, GlyphId16, Tag},
};

use crate::script_lookups::{LookupSubtable, ScriptLookups, everything, insert_class};

// Where a run of text in a font may be cut into words that shape apart and
// join back unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WordSplit {
    pub(crate) cut: SpaceCut,
    // The glyphs a lookup reads with the space where the cut falls: a word
    // whose glyph meets that space shapes together with its neighbor.
    pub(crate) exceptions: GlyphSet,
}

// Which side of the spaces words are cut on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpaceCut {
    // A word starts at the space before it.
    Before,
    // A word ends with the spaces after it.
    After,
}

// A set of glyphs, sorted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct GlyphSet(Box<[u32]>);

impl GlyphSet {
    pub(crate) fn contains(&self, glyph: u32) -> bool {
        self.0.binary_search(&glyph).is_ok()
    }

    fn of(glyphs: &IntSet<GlyphId>) -> Self {
        Self(glyphs.iter().map(GlyphId::to_u32).collect())
    }
}

// The glyphs some lookup reads together with a space on one side of it.
#[derive(Default)]
struct Side {
    any: bool,
    glyphs: IntSet<GlyphId>,
}

// The glyphs read with a space after them, and with a space before them.
// A lookup that reads the space with any glyph on both sides, or in a
// context, ends the walk: runs then shape whole.
#[derive(Default)]
struct Sides {
    before: Side,
    after: Side,
}

// How `font` lets runs of `scripts`, its OpenType script tags in order of
// preference, split into words at spaces; `None` when lookups read the space
// with any glyph on both sides, or in a context, and runs shape whole. It
// reads the lookups of the script HarfBuzz picks, in every language and
// feature, so it holds for any of them; it also follows the glyphs
// substitution turns the space into.
pub(crate) fn word_split(font: &FontRef<'_>, space: GlyphId, scripts: &[Tag]) -> Option<WordSplit> {
    let unread = [*b"morx", *b"mort", *b"kerx"]
        .iter()
        .any(|tag| font.table_data(Tag::new(tag)).is_some())
        || (font.table_data(Tag::new(b"kern")).is_some() && font.gpos().is_err());
    let mut sides = Sides::default();
    if unread || space_sides(font, space, scripts, &mut sides).is_none() {
        return None;
    }
    let (cut, exceptions) = match (sides.before.any, sides.after.any) {
        (true, true) => return None,
        (false, true) => (SpaceCut::Before, sides.before.glyphs),
        (true, false) => (SpaceCut::After, sides.after.glyphs),
        (false, false) if sides.before.glyphs.len() <= sides.after.glyphs.len() => {
            (SpaceCut::Before, sides.before.glyphs)
        }
        (false, false) => (SpaceCut::After, sides.after.glyphs),
    };
    Some(WordSplit {
        cut,
        exceptions: GlyphSet::of(&exceptions),
    })
}

// The walkers below run once a plan, at the bottom of a layout. Each stays
// out of line, so the stack holds one walker's frame at a time, which a
// microcontroller's main task has room for.
#[inline(never)]
fn space_sides(
    font: &FontRef<'_>,
    space: GlyphId,
    scripts: &[Tag],
    sides: &mut Sides,
) -> Option<()> {
    let mut spaces = IntSet::empty();
    spaces.insert(space);
    let features = everything();
    if let Ok(gsub) = font.gsub()
        && let Some(lookups) = ScriptLookups::substitutions(&gsub, scripts, &features).ok()?
    {
        close_spaces(&lookups, &mut spaces)?;
        lookups.each(&mut |_, subtable| subtable_sides(subtable, &spaces, sides))?;
    }
    if let Ok(gpos) = font.gpos()
        && let Some(lookups) = ScriptLookups::positionings(&gpos, scripts, &features).ok()?
    {
        lookups.each(&mut |_, subtable| subtable_sides(subtable, &spaces, sides))?;
    }
    Some(())
}

// Adds to `spaces` the glyphs `lookups` turn them into: what a substitution
// of one glyph makes of one, and the ligatures that join spaces alone. A
// lookup that reads a space in a context keeps runs whole, as
// `subtable_sides` finds, so what its nested lookups make of a space never
// decides a split. `None` for a format this does not follow.
#[inline(never)]
fn close_spaces(lookups: &ScriptLookups<'_>, spaces: &mut IntSet<GlyphId>) -> Option<()> {
    let mut found = Vec::new();
    loop {
        lookups.each(&mut |_, subtable| {
            for space in spaces.iter() {
                substitutes(&subtable, space, spaces, &mut found)?;
            }
            Some(())
        })?;
        let known = spaces.len();
        for glyph in found.drain(..) {
            spaces.insert(glyph);
        }
        if spaces.len() == known {
            return Some(());
        }
    }
}

// Pushes to `found` what `subtable` turns `space` into on its own, or joins
// it into with other `spaces` alone; `None` for a format this does not
// follow.
fn substitutes(
    subtable: &LookupSubtable<'_>,
    space: GlyphId,
    spaces: &IntSet<GlyphId>,
    found: &mut Vec<GlyphId>,
) -> Option<()> {
    match subtable {
        LookupSubtable::Single(subtable) => single_substitutes(subtable, space, found),
        LookupSubtable::Multiple(MultipleSubst::Format1(format)) => {
            if let Some(at) = format.coverage().ok()?.get(space) {
                let sequence = format.sequences().get(class_index(at)).ok()?;
                push_glyphs(found, sequence.substitute_glyph_ids());
            }
            Some(())
        }
        LookupSubtable::Alternate(AlternateSubst::Format1(format)) => {
            if let Some(at) = format.coverage().ok()?.get(space) {
                let alternates = format.alternate_sets().get(class_index(at)).ok()?;
                push_glyphs(found, alternates.alternate_glyph_ids());
            }
            Some(())
        }
        LookupSubtable::Ligature(LigatureSubst::Format1(format)) => {
            ligature_substitutes(format, space, spaces, found)
        }
        LookupSubtable::Multiple(_)
        | LookupSubtable::Alternate(_)
        | LookupSubtable::Ligature(_) => None,
        _ => Some(()),
    }
}

fn single_substitutes(
    subtable: &SingleSubst<'_>,
    space: GlyphId,
    found: &mut Vec<GlyphId>,
) -> Option<()> {
    match subtable {
        SingleSubst::Format1(format) => {
            if format.coverage().ok()?.get(space).is_some()
                && let Ok(glyph) = u16::try_from(space.to_u32())
            {
                let glyph = glyph.wrapping_add_signed(format.delta_glyph_id());
                found.push(GlyphId::from(GlyphId16::new(glyph)));
            }
        }
        SingleSubst::Format2(format) => {
            if let Some(at) = format.coverage().ok()?.get(space) {
                let at = class_index(at);
                let glyphs = format.substitute_glyph_ids();
                push_glyphs(found, glyphs.get(at..=at).unwrap_or_default());
            }
        }
        _ => return None,
    }
    Some(())
}

// The ligatures that start with `space` and join `spaces` alone.
fn ligature_substitutes(
    format: &LigatureSubstFormat1<'_>,
    space: GlyphId,
    spaces: &IntSet<GlyphId>,
    found: &mut Vec<GlyphId>,
) -> Option<()> {
    let Some(at) = format.coverage().ok()?.get(space) else {
        return Some(());
    };
    for ligature in format
        .ligature_sets()
        .get(class_index(at))
        .ok()?
        .ligatures()
        .iter()
    {
        let ligature = ligature.ok()?;
        if ligature
            .component_glyph_ids()
            .iter()
            .all(|glyph| spaces.contains(GlyphId::from(glyph.get())))
        {
            found.push(GlyphId::from(ligature.ligature_glyph()));
        }
    }
    Some(())
}

fn push_glyphs(found: &mut Vec<GlyphId>, glyphs: &[BigEndian<GlyphId16>]) {
    found.extend(glyphs.iter().map(|glyph| GlyphId::from(glyph.get())));
}

// The sides on which `subtable` reads `spaces`; `None` when it reads them
// on both sides, in a context, or in a format this does not read.
#[inline(never)]
fn subtable_sides(
    subtable: LookupSubtable<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Option<()> {
    match subtable {
        // These change the space alone; the closure follows what it becomes.
        LookupSubtable::Single(_)
        | LookupSubtable::Multiple(_)
        | LookupSubtable::Alternate(_)
        | LookupSubtable::Adjust(_) => Some(()),
        LookupSubtable::Ligature(subtable) => ligature_sides(&subtable, spaces, sides),
        LookupSubtable::Context(context) => (!context_mentions(&context, spaces)?).then_some(()),
        LookupSubtable::ChainedContext(context) => {
            (!chained_context_mentions(&context, spaces)?).then_some(())
        }
        LookupSubtable::Reverse(subtable) => (!reverse_mentions(&subtable, spaces)?).then_some(()),
        LookupSubtable::Pair(pair) => pair_sides(&pair, spaces, sides),
        // A cursive attachment joins the space to both neighbors.
        LookupSubtable::Cursive(CursivePos::Format1(format)) => {
            (!covers(&format.coverage().ok()?, spaces)).then_some(())
        }
        LookupSubtable::MarkBase(MarkBasePos::Format1(format)) => attachment_sides(
            format.base_coverage(),
            format.mark_coverage(),
            spaces,
            sides,
        ),
        LookupSubtable::MarkLigature(MarkLigPos::Format1(format)) => attachment_sides(
            format.ligature_coverage(),
            format.mark_coverage(),
            spaces,
            sides,
        ),
        LookupSubtable::MarkMark(MarkMarkPos::Format1(format)) => attachment_sides(
            format.mark2_coverage(),
            format.mark1_coverage(),
            spaces,
            sides,
        ),
        LookupSubtable::Cursive(_)
        | LookupSubtable::MarkBase(_)
        | LookupSubtable::MarkLigature(_)
        | LookupSubtable::MarkMark(_) => None,
    }
}

// A ligature reads a space it starts with the glyphs after it, and a space
// among its other components with the glyphs before it. The other format
// names glyphs past 65,535, which this does not read.
fn ligature_sides(
    subtable: &LigatureSubst<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Option<()> {
    let LigatureSubst::Format1(subtable) = subtable else {
        return None;
    };
    let sets = subtable.ligature_sets();
    for (at, first) in subtable.coverage().ok()?.iter().enumerate() {
        for ligature in sets.get(at).ok()?.ligatures().iter() {
            let components = ligature.ok()?.component_glyph_ids();
            if components.is_empty() {
                continue;
            }
            sides.after.any |= spaces.contains(first);
            sides.before.any |= glyphs_mention(components, spaces);
        }
    }
    Some(())
}

fn reverse_mentions(
    subtable: &ReverseChainSingleSubst<'_>,
    spaces: &IntSet<GlyphId>,
) -> Option<bool> {
    let ReverseChainSingleSubst::Format1(subtable) = subtable else {
        return Some(true);
    };
    coverages_mention(
        [subtable.coverage()]
            .into_iter()
            .chain(subtable.backtrack_coverages().iter())
            .chain(subtable.lookahead_coverages().iter()),
        spaces,
    )
}

// A mark attachment reads a space as the base, of `base`, with the marks
// after it, or as a mark, of `mark`, with the base before it.
fn attachment_sides(
    base: Result<CoverageTable<'_>, ReadError>,
    mark: Result<CoverageTable<'_>, ReadError>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Option<()> {
    sides.after.any |= covers(&base.ok()?, spaces);
    sides.before.any |= covers(&mark.ok()?, spaces);
    Some(())
}

// The glyphs a pair adjustment moves with a space: a glyph first in a pair
// with the space second is read with the space after it; a glyph second in
// a pair with the space first, with the space before it.
fn pair_sides(subtable: &PairPos<'_>, spaces: &IntSet<GlyphId>, sides: &mut Sides) -> Option<()> {
    match subtable {
        PairPos::Format1(format) => glyph_pair_sides(format, spaces, sides),
        PairPos::Format2(format) => class_pair_sides(format, spaces, sides),
        _ => None,
    }
}

#[inline(never)]
fn glyph_pair_sides(
    format: &PairPosFormat1<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Option<()> {
    let sets = format.pair_sets();
    for (at, first) in format.coverage().ok()?.iter().enumerate() {
        let records = sets.get(at).ok()?.pair_value_records();
        if spaces.contains(first) {
            for record in records.iter() {
                let record = record.ok()?;
                if pair_moves(&record) {
                    sides
                        .after
                        .glyphs
                        .insert(GlyphId::from(record.second_glyph()));
                }
            }
        }
        for space in spaces.iter() {
            if pair_with(&records, space)?.is_some_and(|record| pair_moves(&record)) {
                sides.before.glyphs.insert(first);
            }
        }
    }
    Some(())
}

// The record of `records` whose second glyph is `second`, found as the
// shaper finds it, by a binary search of records in the order of their
// second glyphs. `None` inside when there is none; `None` for a record that
// does not read.
fn pair_with<'a>(
    records: &ComputedArray<'a, PairValueRecord<'a>>,
    second: GlyphId,
) -> Option<Option<PairValueRecord<'a>>> {
    let second = second.to_u32();
    let (mut low, mut high) = (0, records.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let record = records.get(middle).ok()?;
        match u32::from(record.second_glyph().to_u16()).cmp(&second) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return Some(Some(record)),
        }
    }
    Some(None)
}

// Whether a pair adjustment moves either glyph of its pair.
fn pair_moves(record: &PairValueRecord<'_>) -> bool {
    moves(record.value_record1()) || moves(record.value_record2())
}

#[inline(never)]
fn class_pair_sides(
    format: &PairPosFormat2<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Option<()> {
    let coverage = format.coverage().ok()?;
    let (firsts, seconds) = (format.class_def1().ok()?, format.class_def2().ok()?);
    let rows = format.class1_records();
    let moving = |class1: u32, class2: u32| -> Option<bool> {
        let cell = rows.get(class_index(class1)).ok()?;
        let cell = cell.class2_records().get(class_index(class2)).ok()?;
        Some(moves(cell.value_record1()) || moves(cell.value_record2()))
    };
    for space in spaces.iter() {
        // The space second: every first glyph of a row that moves with the
        // space's column.
        let column = seconds.get(space);
        firsts_moving_with(
            &coverage,
            (&firsts, format.class1_count()),
            column,
            &moving,
            sides,
        )?;
        if coverage.get(space).is_none() {
            continue;
        }
        // The space first: every second glyph of a column that moves in the
        // space's row; column 0 holds every glyph no class names.
        let row = firsts.get(space);
        seconds_moving_with(format.class2_count(), &seconds, row, &moving, sides)?;
    }
    Some(())
}

// Adds to the glyphs before the space each glyph of `coverage` whose class
// in `firsts`, among `count`, moves with `column`. Each row's cell is read
// once; a glyph of a class past the rows moves with nothing, as the shaper
// reads no cell for it.
#[inline(never)]
fn firsts_moving_with(
    coverage: &CoverageTable<'_>,
    (firsts, count): (&ClassDef<'_>, u16),
    column: u32,
    moving: &impl Fn(u32, u32) -> Option<bool>,
    sides: &mut Sides,
) -> Option<()> {
    let rows = (0..u32::from(count))
        .map(|class1| moving(class1, column))
        .collect::<Option<Vec<bool>>>()?;
    if !rows.contains(&true) {
        return Some(());
    }
    for first in coverage.iter() {
        if rows.get(class_index(firsts.get(first))) == Some(&true) {
            sides.before.glyphs.insert(first);
        }
    }
    Some(())
}

// Adds to the glyphs after the space each glyph of a class of `seconds`,
// among `count`, that moves in `row`.
#[inline(never)]
fn seconds_moving_with(
    count: u16,
    seconds: &ClassDef<'_>,
    row: u32,
    moving: &impl Fn(u32, u32) -> Option<bool>,
    sides: &mut Sides,
) -> Option<()> {
    for class2 in 0..u32::from(count) {
        if !moving(row, class2)? {
            continue;
        }
        sides.after.any |= class2 == 0;
        insert_class(&mut sides.after.glyphs, seconds, class2);
    }
    Some(())
}

// Whether a value record moves its glyph.
pub(crate) fn moves(record: &ValueRecord<'_>) -> bool {
    [
        record.x_placement(),
        record.y_placement(),
        record.x_advance(),
        record.y_advance(),
    ]
    .iter()
    .any(|value| value.is_some_and(|value| value != 0))
        || record.x_placement_device().is_some()
        || record.y_placement_device().is_some()
        || record.x_advance_device().is_some()
        || record.y_advance_device().is_some()
}

fn covers(coverage: &CoverageTable<'_>, spaces: &IntSet<GlyphId>) -> bool {
    spaces.iter().any(|glyph| coverage.get(glyph).is_some())
}

// The classes `classes` gives the spaces.
fn classes_of(classes: &ClassDef<'_>, spaces: &IntSet<GlyphId>) -> IntSet<u16> {
    let mut set = IntSet::empty();
    for space in spaces.iter() {
        set.insert(u16::try_from(classes.get(space)).unwrap_or(u16::MAX));
    }
    set
}

fn glyphs_mention<'a>(
    glyphs: impl IntoIterator<Item = &'a BigEndian<GlyphId16>>,
    spaces: &IntSet<GlyphId>,
) -> bool {
    glyphs
        .into_iter()
        .any(|glyph| spaces.contains(GlyphId::from(glyph.get())))
}

fn classes_mention<'a>(
    classes: impl IntoIterator<Item = &'a BigEndian<u16>>,
    space_classes: &IntSet<u16>,
) -> bool {
    classes
        .into_iter()
        .any(|class| space_classes.contains(class.get()))
}

// Whether a contextual lookup reads a space anywhere in its sequences.
fn context_mentions(context: &SequenceContext<'_>, spaces: &IntSet<GlyphId>) -> Option<bool> {
    match context {
        SequenceContext::Format1(format) => {
            if covers(&format.coverage().ok()?, spaces) {
                return Some(true);
            }
            for set in format.seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.seq_rules().iter() {
                    if glyphs_mention(rule.ok()?.input_sequence(), spaces) {
                        return Some(true);
                    }
                }
            }
            Some(false)
        }
        SequenceContext::Format2(format) => {
            if covers(&format.coverage().ok()?, spaces) {
                return Some(true);
            }
            let classes = classes_of(&format.class_def().ok()?, spaces);
            for set in format.class_seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.class_seq_rules().iter() {
                    if classes_mention(rule.ok()?.input_sequence(), &classes) {
                        return Some(true);
                    }
                }
            }
            Some(false)
        }
        SequenceContext::Format3(format) => coverages_mention(format.coverages().iter(), spaces),
        _ => Some(true),
    }
}

// Whether a chained contextual lookup reads a space anywhere in its
// backtrack, input or lookahead.
fn chained_context_mentions(
    context: &ChainedSequenceContext<'_>,
    spaces: &IntSet<GlyphId>,
) -> Option<bool> {
    match context {
        ChainedSequenceContext::Format1(format) => {
            if covers(&format.coverage().ok()?, spaces) {
                return Some(true);
            }
            for set in format.chained_seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.chained_seq_rules().iter() {
                    let rule = rule.ok()?;
                    if [
                        rule.backtrack_sequence(),
                        rule.input_sequence(),
                        rule.lookahead_sequence(),
                    ]
                    .into_iter()
                    .any(|sequence| glyphs_mention(sequence, spaces))
                    {
                        return Some(true);
                    }
                }
            }
            Some(false)
        }
        ChainedSequenceContext::Format2(format) => chained_classes_mention(format, spaces),
        ChainedSequenceContext::Format3(format) => coverages_mention(
            format
                .backtrack_coverages()
                .iter()
                .chain(format.input_coverages().iter())
                .chain(format.lookahead_coverages().iter()),
            spaces,
        ),
        _ => Some(true),
    }
}

fn chained_classes_mention(
    format: &ChainedSequenceContextFormat2<'_>,
    spaces: &IntSet<GlyphId>,
) -> Option<bool> {
    if covers(&format.coverage().ok()?, spaces) {
        return Some(true);
    }
    let classes = [
        classes_of(&format.backtrack_class_def().ok()?, spaces),
        classes_of(&format.input_class_def().ok()?, spaces),
        classes_of(&format.lookahead_class_def().ok()?, spaces),
    ];
    for set in format.chained_class_seq_rule_sets().iter().flatten() {
        for rule in set.ok()?.chained_class_seq_rules().iter() {
            let rule = rule.ok()?;
            let sequences = [
                rule.backtrack_sequence(),
                rule.input_sequence(),
                rule.lookahead_sequence(),
            ];
            if sequences
                .into_iter()
                .zip(&classes)
                .any(|(sequence, classes)| classes_mention(sequence, classes))
            {
                return Some(true);
            }
        }
    }
    Some(false)
}

fn coverages_mention<'a>(
    coverages: impl Iterator<Item = Result<CoverageTable<'a>, ReadError>>,
    spaces: &IntSet<GlyphId>,
) -> Option<bool> {
    for coverage in coverages {
        if covers(&coverage.ok()?, spaces) {
            return Some(true);
        }
    }
    Some(false)
}

// A class as an index; classes beyond `usize` saturate.
pub(crate) fn class_index(class: u32) -> usize {
    usize::try_from(class).unwrap_or(usize::MAX)
}

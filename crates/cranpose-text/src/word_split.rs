use read_fonts::{
    FontRef, ReadError, TableProvider,
    collections::IntSet,
    tables::{
        gpos::{
            CursivePos, Gpos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PairPosFormat1,
            PairPosFormat2, PositionSubtables, ValueRecord,
        },
        gsub::{Gsub, LigatureSubst, ReverseChainSingleSubst, SubstitutionSubtables},
        layout::{
            ChainedSequenceContext, ChainedSequenceContextFormat2, ClassDef, CoverageTable,
            ScriptList, SequenceContext,
        },
    },
    types::{GlyphId, Tag},
};

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
#[derive(Default)]
struct Sides {
    before: Side,
    after: Side,
}

impl Sides {
    fn both(&mut self) {
        self.before.any = true;
        self.after.any = true;
    }
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
    if unread || space_sides(font, space, scripts, &mut sides).is_err() {
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

fn space_sides(
    font: &FontRef<'_>,
    space: GlyphId,
    scripts: &[Tag],
    sides: &mut Sides,
) -> Result<(), ReadError> {
    let mut spaces = IntSet::empty();
    spaces.insert(space);
    if let Ok(gsub) = font.gsub() {
        substitution_lookups_sides(&gsub, scripts, &mut spaces, sides)?;
    }
    if let Ok(gpos) = font.gpos() {
        positioning_lookups_sides(&gpos, scripts, &spaces, sides)?;
    }
    Ok(())
}

// Adds to `spaces` the glyphs the script's substitutions turn them into, and
// the sides on which those substitutions read them.
fn substitution_lookups_sides(
    gsub: &Gsub<'_>,
    scripts: &[Tag],
    spaces: &mut IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    let Some(script) = chosen_script(&gsub.script_list()?, scripts) else {
        return Ok(());
    };
    let features = gsub.collect_features(&single(script), &everything(), &everything())?;
    let lookups = gsub.collect_lookups(&features)?;
    gsub.closure_glyphs(&lookups, spaces)?;
    let list = gsub.lookup_list()?;
    for index in lookups.iter() {
        let lookup = list.lookups().get(usize::from(index))?;
        substitution_sides(&lookup.subtables()?, spaces, sides)?;
    }
    Ok(())
}

fn positioning_lookups_sides(
    gpos: &Gpos<'_>,
    scripts: &[Tag],
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    let Some(script) = chosen_script(&gpos.script_list()?, scripts) else {
        return Ok(());
    };
    let features = gpos.collect_features(&single(script), &everything(), &everything())?;
    let list = gpos.lookup_list()?;
    for index in gpos.collect_lookups(&features)?.iter() {
        let lookup = list.lookups().get(usize::from(index))?;
        positioning_sides(&lookup.subtables()?, spaces, sides)?;
    }
    Ok(())
}

// The script of `list` HarfBuzz shapes with: the first of `scripts` the font
// has, else its default script, else Latin.
fn chosen_script(list: &ScriptList<'_>, scripts: &[Tag]) -> Option<Tag> {
    scripts
        .iter()
        .copied()
        .chain([Tag::new(b"DFLT"), Tag::new(b"dflt"), Tag::new(b"latn")])
        .find(|&tag| list.index_for_tag(tag).is_some())
}

fn single(tag: Tag) -> IntSet<Tag> {
    let mut set = IntSet::empty();
    set.insert(tag);
    set
}

fn everything() -> IntSet<Tag> {
    let mut set = IntSet::empty();
    set.invert();
    set
}

fn substitution_sides(
    subtables: &SubstitutionSubtables<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    match subtables {
        // These change the space alone; the closure follows what it becomes.
        SubstitutionSubtables::Single(_)
        | SubstitutionSubtables::Multiple(_)
        | SubstitutionSubtables::Alternate(_)
        | SubstitutionSubtables::EmptyExtension => Ok(()),
        SubstitutionSubtables::Ligature(subtables) => {
            ligature_sides(subtables.iter(), spaces, sides)
        }
        SubstitutionSubtables::Contextual(subtables) => {
            context_sides(subtables.iter(), spaces, sides, context_mentions)
        }
        SubstitutionSubtables::ChainContextual(subtables) => {
            context_sides(subtables.iter(), spaces, sides, chained_context_mentions)
        }
        SubstitutionSubtables::Reverse(subtables) => {
            context_sides(subtables.iter(), spaces, sides, reverse_mentions)
        }
    }
}

fn positioning_sides(
    subtables: &PositionSubtables<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    match subtables {
        PositionSubtables::Single(_) | PositionSubtables::EmptyExtension => Ok(()),
        PositionSubtables::Pair(subtables) => pairs_sides(subtables.iter(), spaces, sides),
        PositionSubtables::Cursive(subtables) => cursive_sides(subtables.iter(), spaces, sides),
        PositionSubtables::MarkToBase(subtables) => {
            attachment_sides(subtables.iter(), spaces, sides, mark_base_coverages)
        }
        PositionSubtables::MarkToLig(subtables) => {
            attachment_sides(subtables.iter(), spaces, sides, mark_ligature_coverages)
        }
        PositionSubtables::MarkToMark(subtables) => {
            attachment_sides(subtables.iter(), spaces, sides, mark_mark_coverages)
        }
        PositionSubtables::Contextual(subtables) => {
            context_sides(subtables.iter(), spaces, sides, context_mentions)
        }
        PositionSubtables::ChainContextual(subtables) => {
            context_sides(subtables.iter(), spaces, sides, chained_context_mentions)
        }
    }
}

// A ligature reads a space it starts with the glyphs after it, and a space
// among its other components with the glyphs before it.
fn ligature_sides<'a>(
    subtables: impl Iterator<Item = Result<LigatureSubst<'a>, ReadError>>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    for subtable in subtables {
        // The other format names glyphs past 65,535; a lookup this does not
        // read counts on both sides.
        let LigatureSubst::Format1(subtable) = subtable? else {
            sides.both();
            continue;
        };
        let sets = subtable.ligature_sets();
        for (at, first) in subtable.coverage()?.iter().enumerate() {
            for ligature in sets.get(at)?.ligatures().iter() {
                let ligature = ligature?;
                let components = ligature.component_glyph_ids();
                if components.is_empty() {
                    continue;
                }
                sides.after.any |= spaces.contains(first);
                sides.before.any |= glyphs_mention(components, spaces);
            }
        }
    }
    Ok(())
}

// Marks both sides when one of `subtables` reads a space in a context.
fn context_sides<T>(
    subtables: impl Iterator<Item = Result<T, ReadError>>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
    mentions: impl Fn(&T, &IntSet<GlyphId>) -> Result<bool, ReadError>,
) -> Result<(), ReadError> {
    for subtable in subtables {
        if mentions(&subtable?, spaces)? {
            sides.both();
        }
    }
    Ok(())
}

fn reverse_mentions(
    subtable: &ReverseChainSingleSubst<'_>,
    spaces: &IntSet<GlyphId>,
) -> Result<bool, ReadError> {
    let ReverseChainSingleSubst::Format1(subtable) = subtable else {
        return Ok(true);
    };
    coverages_mention(
        [subtable.coverage()]
            .into_iter()
            .chain(subtable.backtrack_coverages().iter())
            .chain(subtable.lookahead_coverages().iter()),
        spaces,
    )
}

fn pairs_sides<'a>(
    subtables: impl Iterator<Item = Result<PairPos<'a>, ReadError>>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    for subtable in subtables {
        pair_sides(&subtable?, spaces, sides)?;
    }
    Ok(())
}

// A cursive attachment joins the space to both neighbors.
fn cursive_sides<'a>(
    subtables: impl Iterator<Item = Result<CursivePos<'a>, ReadError>>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    for subtable in subtables {
        match subtable? {
            CursivePos::Format1(format) if !covers(&format.coverage()?, spaces) => {}
            _ => sides.both(),
        }
    }
    Ok(())
}

// A mark attachment reads a space as the base with the marks after it, or
// as a mark with the base before it. `coverages` gives a subtable's base and
// mark coverage, `None` for a format this does not read.
fn attachment_sides<'a, T>(
    subtables: impl Iterator<Item = Result<T, ReadError>>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
    coverages: impl Fn(&T) -> Option<CoveragePair<'a>>,
) -> Result<(), ReadError> {
    for subtable in subtables {
        let Some((base, mark)) = coverages(&subtable?) else {
            sides.both();
            continue;
        };
        sides.after.any |= covers(&base?, spaces);
        sides.before.any |= covers(&mark?, spaces);
    }
    Ok(())
}

type CoveragePair<'a> = (
    Result<CoverageTable<'a>, ReadError>,
    Result<CoverageTable<'a>, ReadError>,
);

fn mark_base_coverages<'a>(subtable: &MarkBasePos<'a>) -> Option<CoveragePair<'a>> {
    match subtable {
        MarkBasePos::Format1(format) => Some((format.base_coverage(), format.mark_coverage())),
        _ => None,
    }
}

fn mark_ligature_coverages<'a>(subtable: &MarkLigPos<'a>) -> Option<CoveragePair<'a>> {
    match subtable {
        MarkLigPos::Format1(format) => Some((format.ligature_coverage(), format.mark_coverage())),
        _ => None,
    }
}

fn mark_mark_coverages<'a>(subtable: &MarkMarkPos<'a>) -> Option<CoveragePair<'a>> {
    match subtable {
        MarkMarkPos::Format1(format) => Some((format.mark2_coverage(), format.mark1_coverage())),
        _ => None,
    }
}

// The glyphs a pair adjustment moves with a space: a glyph first in a pair
// with the space second is read with the space after it; a glyph second in
// a pair with the space first, with the space before it.
fn pair_sides(
    subtable: &PairPos<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    match subtable {
        PairPos::Format1(format) => glyph_pair_sides(format, spaces, sides),
        PairPos::Format2(format) => class_pair_sides(format, spaces, sides),
        _ => {
            sides.both();
            Ok(())
        }
    }
}

fn glyph_pair_sides(
    format: &PairPosFormat1<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    let sets = format.pair_sets();
    for (at, first) in format.coverage()?.iter().enumerate() {
        for record in sets.get(at)?.pair_value_records().iter() {
            let record = record?;
            if !moves(record.value_record1()) && !moves(record.value_record2()) {
                continue;
            }
            let second = GlyphId::from(record.second_glyph());
            if spaces.contains(second) {
                sides.before.glyphs.insert(first);
            }
            if spaces.contains(first) {
                sides.after.glyphs.insert(second);
            }
        }
    }
    Ok(())
}

fn class_pair_sides(
    format: &PairPosFormat2<'_>,
    spaces: &IntSet<GlyphId>,
    sides: &mut Sides,
) -> Result<(), ReadError> {
    let coverage = format.coverage()?;
    let (firsts, seconds) = (format.class_def1()?, format.class_def2()?);
    let rows = format.class1_records();
    let moving = |class1: u32, class2: u32| -> Result<bool, ReadError> {
        let cell = rows.get(class_index(class1))?;
        let cell = cell.class2_records().get(class_index(class2))?;
        Ok(moves(cell.value_record1()) || moves(cell.value_record2()))
    };
    for space in spaces.iter() {
        // The space second: every first glyph of a row that moves with the
        // space's column.
        let column = seconds.get(space);
        for first in coverage.iter() {
            if moving(firsts.get(first), column)? {
                sides.before.glyphs.insert(first);
            }
        }
        if coverage.get(space).is_none() {
            continue;
        }
        // The space first: every second glyph of a column that moves in the
        // space's row; column 0 holds every glyph no class names.
        let row = firsts.get(space);
        for class2 in 0..u32::from(format.class2_count()) {
            if !moving(row, class2)? {
                continue;
            }
            sides.after.any |= class2 == 0;
            for (second, class) in seconds.iter() {
                if class == class2 {
                    sides.after.glyphs.insert(second);
                }
            }
        }
    }
    Ok(())
}

// Whether a value record moves its glyph.
fn moves(record: &ValueRecord<'_>) -> bool {
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
    glyphs: impl IntoIterator<Item = &'a read_fonts::types::BigEndian<read_fonts::types::GlyphId16>>,
    spaces: &IntSet<GlyphId>,
) -> bool {
    glyphs
        .into_iter()
        .any(|glyph| spaces.contains(GlyphId::from(glyph.get())))
}

fn classes_mention<'a>(
    classes: impl IntoIterator<Item = &'a read_fonts::types::BigEndian<u16>>,
    space_classes: &IntSet<u16>,
) -> bool {
    classes
        .into_iter()
        .any(|class| space_classes.contains(class.get()))
}

// Whether a contextual lookup reads a space anywhere in its sequences.
fn context_mentions(
    context: &SequenceContext<'_>,
    spaces: &IntSet<GlyphId>,
) -> Result<bool, ReadError> {
    match context {
        SequenceContext::Format1(format) => {
            if covers(&format.coverage()?, spaces) {
                return Ok(true);
            }
            for set in format.seq_rule_sets().iter().flatten() {
                for rule in set?.seq_rules().iter() {
                    if glyphs_mention(rule?.input_sequence(), spaces) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        SequenceContext::Format2(format) => {
            if covers(&format.coverage()?, spaces) {
                return Ok(true);
            }
            let classes = classes_of(&format.class_def()?, spaces);
            for set in format.class_seq_rule_sets().iter().flatten() {
                for rule in set?.class_seq_rules().iter() {
                    if classes_mention(rule?.input_sequence(), &classes) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        SequenceContext::Format3(format) => coverages_mention(format.coverages().iter(), spaces),
        _ => Ok(true),
    }
}

// Whether a chained contextual lookup reads a space anywhere in its
// backtrack, input or lookahead.
fn chained_context_mentions(
    context: &ChainedSequenceContext<'_>,
    spaces: &IntSet<GlyphId>,
) -> Result<bool, ReadError> {
    match context {
        ChainedSequenceContext::Format1(format) => {
            if covers(&format.coverage()?, spaces) {
                return Ok(true);
            }
            for set in format.chained_seq_rule_sets().iter().flatten() {
                for rule in set?.chained_seq_rules().iter() {
                    let rule = rule?;
                    if [
                        rule.backtrack_sequence(),
                        rule.input_sequence(),
                        rule.lookahead_sequence(),
                    ]
                    .into_iter()
                    .any(|sequence| glyphs_mention(sequence, spaces))
                    {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
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
        _ => Ok(true),
    }
}

fn chained_classes_mention(
    format: &ChainedSequenceContextFormat2<'_>,
    spaces: &IntSet<GlyphId>,
) -> Result<bool, ReadError> {
    if covers(&format.coverage()?, spaces) {
        return Ok(true);
    }
    let classes = [
        classes_of(&format.backtrack_class_def()?, spaces),
        classes_of(&format.input_class_def()?, spaces),
        classes_of(&format.lookahead_class_def()?, spaces),
    ];
    for set in format.chained_class_seq_rule_sets().iter().flatten() {
        for rule in set?.chained_class_seq_rules().iter() {
            let rule = rule?;
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
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn coverages_mention<'a>(
    coverages: impl Iterator<Item = Result<CoverageTable<'a>, ReadError>>,
    spaces: &IntSet<GlyphId>,
) -> Result<bool, ReadError> {
    for coverage in coverages {
        if covers(&coverage?, spaces) {
            return Ok(true);
        }
    }
    Ok(false)
}

// A class as an index; classes beyond `usize` saturate.
fn class_index(class: u32) -> usize {
    usize::try_from(class).unwrap_or(usize::MAX)
}

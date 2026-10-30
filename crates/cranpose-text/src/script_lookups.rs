use read_fonts::{
    FontRead, ReadError,
    collections::IntSet,
    tables::{
        gpos::{
            CursivePos, Gpos, MarkBasePos, MarkLigPos, MarkMarkPos, PairPos, PositionLookupList,
            PositionSubtables, SinglePos,
        },
        gsub::{
            AlternateSubst, Gsub, LigatureSubst, MultipleSubst, ReverseChainSingleSubst,
            SingleSubst, SubstitutionLookupList, SubstitutionSubtables,
        },
        layout::{
            ChainedSequenceContext, ClassDef, CoverageTable, ExtensionLookup, FeatureList,
            FeatureVariations, LangSys, LookupFlag, ScriptList, SequenceContext, Subtables,
        },
    },
    types::{GlyphId, Tag},
};

// The lookups a shape plan's features apply in one of a font's layout
// tables, for the script HarfBuzz picks there: the first of the plan's
// script tags the table has, else its default script, else Latin. Each of
// the script's languages counts, with the features it requires.
pub(crate) struct ScriptLookups<'a> {
    list: LookupList<'a>,
    lookups: IntSet<u16>,
}

enum LookupList<'a> {
    Substitution(SubstitutionLookupList<'a>),
    Positioning(PositionLookupList<'a>),
}

// The lookup a subtable belongs to.
#[derive(Clone, Copy)]
pub(crate) struct Lookup {
    pub(crate) index: u16,
    pub(crate) flag: LookupFlag,
}

// A subtable of a lookup, of either table.
pub(crate) enum LookupSubtable<'a> {
    Single(SingleSubst<'a>),
    Multiple(MultipleSubst<'a>),
    Alternate(AlternateSubst<'a>),
    Ligature(LigatureSubst<'a>),
    Reverse(ReverseChainSingleSubst<'a>),
    Adjust(SinglePos<'a>),
    Pair(PairPos<'a>),
    Cursive(CursivePos<'a>),
    MarkBase(MarkBasePos<'a>),
    MarkLigature(MarkLigPos<'a>),
    MarkMark(MarkMarkPos<'a>),
    Context(SequenceContext<'a>),
    ChainedContext(ChainedSequenceContext<'a>),
}

// What a walk does with each subtable; `None` ends the walk.
pub(crate) type Visit<'v, 'a> = dyn FnMut(Lookup, LookupSubtable<'a>) -> Option<()> + 'v;

impl<'a> ScriptLookups<'a> {
    // The substitutions `features` apply in `gsub` for `scripts`, in order
    // of preference; `None` when no script of the table applies.
    #[inline(never)]
    pub(crate) fn substitutions(
        gsub: &Gsub<'a>,
        scripts: &[Tag],
        features: &IntSet<Tag>,
    ) -> Result<Option<Self>, ReadError> {
        let script_list = gsub.script_list()?;
        let Some(script) = chosen_script(&script_list, scripts) else {
            return Ok(None);
        };
        let languages = Languages::of(&script_list, script)?;
        let tags = languages.required(&gsub.feature_list()?, features);
        let indices = gsub.collect_features(&single(script), &everything(), &tags)?;
        let lookups = gsub.collect_lookups(&indices)?;
        let list = LookupList::Substitution(gsub.lookup_list()?);
        Ok(Some(Self { list, lookups }))
    }

    // The positionings `features` apply in `gpos` for `scripts`, in order
    // of preference; `None` when no script of the table applies.
    #[inline(never)]
    pub(crate) fn positionings(
        gpos: &Gpos<'a>,
        scripts: &[Tag],
        features: &IntSet<Tag>,
    ) -> Result<Option<Self>, ReadError> {
        let script_list = gpos.script_list()?;
        let Some(script) = chosen_script(&script_list, scripts) else {
            return Ok(None);
        };
        let languages = Languages::of(&script_list, script)?;
        let tags = languages.required(&gpos.feature_list()?, features);
        let indices = gpos.collect_features(&single(script), &everything(), &tags)?;
        let lookups = gpos.collect_lookups(&indices)?;
        let list = LookupList::Positioning(gpos.lookup_list()?);
        Ok(Some(Self { list, lookups }))
    }

    // Runs `visit` on each subtable of the lookups, in lookup order; `None`
    // when `visit` ends the walk or a subtable does not read.
    #[inline(never)]
    pub(crate) fn each(&self, visit: &mut Visit<'_, 'a>) -> Option<()> {
        for index in self.lookups.iter() {
            match &self.list {
                LookupList::Substitution(list) => {
                    let lookup = list.lookups().get(usize::from(index)).ok()?;
                    let at = Lookup {
                        index,
                        flag: lookup.lookup_flag(),
                    };
                    each_substitution(lookup.subtables().ok()?, at, visit)?;
                }
                LookupList::Positioning(list) => {
                    let lookup = list.lookups().get(usize::from(index)).ok()?;
                    let at = Lookup {
                        index,
                        flag: lookup.lookup_flag(),
                    };
                    each_positioning(lookup.subtables().ok()?, at, visit)?;
                }
            }
        }
        Some(())
    }
}

fn each_substitution<'a>(
    subtables: SubstitutionSubtables<'a>,
    at: Lookup,
    visit: &mut Visit<'_, 'a>,
) -> Option<()> {
    match subtables {
        SubstitutionSubtables::Single(all) => each_of(&all, at, LookupSubtable::Single, visit),
        SubstitutionSubtables::Multiple(all) => each_of(&all, at, LookupSubtable::Multiple, visit),
        SubstitutionSubtables::Alternate(all) => {
            each_of(&all, at, LookupSubtable::Alternate, visit)
        }
        SubstitutionSubtables::Ligature(all) => each_of(&all, at, LookupSubtable::Ligature, visit),
        SubstitutionSubtables::Contextual(all) => each_of(&all, at, LookupSubtable::Context, visit),
        SubstitutionSubtables::ChainContextual(all) => {
            each_of(&all, at, LookupSubtable::ChainedContext, visit)
        }
        SubstitutionSubtables::Reverse(all) => each_of(&all, at, LookupSubtable::Reverse, visit),
        SubstitutionSubtables::EmptyExtension => Some(()),
    }
}

fn each_positioning<'a>(
    subtables: PositionSubtables<'a>,
    at: Lookup,
    visit: &mut Visit<'_, 'a>,
) -> Option<()> {
    match subtables {
        PositionSubtables::Single(all) => each_of(&all, at, LookupSubtable::Adjust, visit),
        PositionSubtables::Pair(all) => each_of(&all, at, LookupSubtable::Pair, visit),
        PositionSubtables::Cursive(all) => each_of(&all, at, LookupSubtable::Cursive, visit),
        PositionSubtables::MarkToBase(all) => each_of(&all, at, LookupSubtable::MarkBase, visit),
        PositionSubtables::MarkToLig(all) => each_of(&all, at, LookupSubtable::MarkLigature, visit),
        PositionSubtables::MarkToMark(all) => each_of(&all, at, LookupSubtable::MarkMark, visit),
        PositionSubtables::Contextual(all) => each_of(&all, at, LookupSubtable::Context, visit),
        PositionSubtables::ChainContextual(all) => {
            each_of(&all, at, LookupSubtable::ChainedContext, visit)
        }
        PositionSubtables::EmptyExtension => Some(()),
    }
}

// The one loop over the subtables of a kind: a walk reads every kind
// through it, whatever it does with them.
fn each_of<'a, T, Ext>(
    subtables: &Subtables<'a, T, Ext>,
    at: Lookup,
    kind: fn(T) -> LookupSubtable<'a>,
    visit: &mut Visit<'_, 'a>,
) -> Option<()>
where
    T: FontRead<'a, Args = ()> + 'a,
    Ext: ExtensionLookup<'a, T> + 'a,
{
    for index in 0..subtables.len() {
        visit(at, kind(subtables.get(index).ok()?))?;
    }
    Some(())
}

impl<'a> LookupSubtable<'a> {
    // The coverage of the glyphs the subtable starts at: for a mark
    // attachment, the marks. `None` for a format this does not read.
    pub(crate) fn start(&self) -> Option<Result<CoverageTable<'a>, ReadError>> {
        Some(match self {
            Self::Single(SingleSubst::Format1(format)) => format.coverage(),
            Self::Single(SingleSubst::Format2(format)) => format.coverage(),
            Self::Multiple(MultipleSubst::Format1(format)) => format.coverage(),
            Self::Alternate(AlternateSubst::Format1(format)) => format.coverage(),
            Self::Ligature(LigatureSubst::Format1(format)) => format.coverage(),
            Self::Reverse(ReverseChainSingleSubst::Format1(format)) => format.coverage(),
            Self::Adjust(SinglePos::Format1(format)) => format.coverage(),
            Self::Adjust(SinglePos::Format2(format)) => format.coverage(),
            Self::Pair(PairPos::Format1(format)) => format.coverage(),
            Self::Pair(PairPos::Format2(format)) => format.coverage(),
            Self::Cursive(CursivePos::Format1(format)) => format.coverage(),
            Self::MarkBase(MarkBasePos::Format1(format)) => format.mark_coverage(),
            Self::MarkLigature(MarkLigPos::Format1(format)) => format.mark_coverage(),
            Self::MarkMark(MarkMarkPos::Format1(format)) => format.mark1_coverage(),
            Self::Context(context) => return context_start(context),
            Self::ChainedContext(context) => return chained_context_start(context),
            _ => return None,
        })
    }
}

fn context_start<'a>(
    context: &SequenceContext<'a>,
) -> Option<Result<CoverageTable<'a>, ReadError>> {
    match context {
        SequenceContext::Format1(format) => Some(format.coverage()),
        SequenceContext::Format2(format) => Some(format.coverage()),
        SequenceContext::Format3(format) => Some(format.coverages().get(0)),
        _ => None,
    }
}

fn chained_context_start<'a>(
    context: &ChainedSequenceContext<'a>,
) -> Option<Result<CoverageTable<'a>, ReadError>> {
    match context {
        ChainedSequenceContext::Format1(format) => Some(format.coverage()),
        ChainedSequenceContext::Format2(format) => Some(format.coverage()),
        ChainedSequenceContext::Format3(format) => Some(format.input_coverages().get(0)),
        _ => None,
    }
}

// Adds the glyphs of `coverage` to `set`, a range at a time where it lists
// ranges.
pub(crate) fn insert_covered(set: &mut IntSet<GlyphId>, coverage: &CoverageTable<'_>) {
    match coverage {
        CoverageTable::Format2(format) => {
            for range in format.range_records() {
                set.insert_range(
                    GlyphId::from(range.start_glyph_id())..=GlyphId::from(range.end_glyph_id()),
                );
            }
        }
        _ => set.extend_unsorted(coverage.iter()),
    }
}

// Adds the glyphs `classes` puts in `class` to `set`, a range at a time
// where it lists ranges.
pub(crate) fn insert_class(set: &mut IntSet<GlyphId>, classes: &ClassDef<'_>, class: u32) {
    match classes {
        ClassDef::Format2(format) => {
            for range in format.class_range_records() {
                if u32::from(range.class()) == class {
                    set.insert_range(
                        GlyphId::from(range.start_glyph_id())..=GlyphId::from(range.end_glyph_id()),
                    );
                }
            }
        }
        _ => {
            for (glyph, of) in classes.iter() {
                if of == class {
                    set.insert(glyph);
                }
            }
        }
    }
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

// The substitutions `features` apply in `gsub` for `scripts`, in order of
// preference, in every language of the script, so in a text of any
// language. None when the script has no default language, which a
// language it does not list takes, and none of a feature a variation of
// the font may swap.
#[inline(never)]
pub(crate) fn substitutions_everywhere(
    gsub: &Gsub<'_>,
    scripts: &[Tag],
    features: &IntSet<Tag>,
) -> Result<IntSet<u16>, ReadError> {
    let script_list = gsub.script_list()?;
    let Some(script) = chosen_script(&script_list, scripts) else {
        return Ok(IntSet::empty());
    };
    let languages = Languages::of(&script_list, script)?;
    let Some(default) = &languages.default else {
        return Ok(IntSet::empty());
    };
    let feature_list = gsub.feature_list()?;
    let varied = match gsub.feature_variations().transpose()? {
        Some(variations) => varied_features(&variations)?,
        None => IntSet::empty(),
    };
    let applied = |language| applied(language, &feature_list, features, &varied);
    let mut everywhere = applied(default)?;
    for language in &languages.listed {
        everywhere.intersect(&applied(language)?);
    }
    Ok(everywhere)
}

// The language systems of a script: the default one, which a language the
// script does not list takes, and the listed ones.
struct Languages<'a> {
    default: Option<LangSys<'a>>,
    listed: Vec<LangSys<'a>>,
}

impl<'a> Languages<'a> {
    fn of(scripts: &ScriptList<'a>, script: Tag) -> Result<Self, ReadError> {
        let mut languages = Self {
            default: None,
            listed: Vec::new(),
        };
        let Some(record) = scripts
            .index_for_tag(script)
            .and_then(|index| scripts.script_records().get(usize::from(index)))
        else {
            return Ok(languages);
        };
        let script = record.script(scripts.offset_data())?;
        languages.default = script.default_lang_sys().transpose()?;
        for record in script.lang_sys_records() {
            languages
                .listed
                .push(record.lang_sys(script.offset_data())?);
        }
        Ok(languages)
    }

    // `asked` with the features each language requires, which HarfBuzz
    // applies whatever features are asked.
    fn required(&self, features: &FeatureList<'_>, asked: &IntSet<Tag>) -> IntSet<Tag> {
        let mut tags = asked.clone();
        for language in self.default.iter().chain(&self.listed) {
            let required = usize::from(language.required_feature_index());
            if let Some(feature) = features.feature_records().get(required) {
                tags.insert(feature.feature_tag());
            }
        }
        tags
    }
}

// The lookups HarfBuzz applies in `language` for `asked`: those of the
// feature it requires and of the first feature it lists of each tag asked,
// leaving out the `varied` features.
fn applied(
    language: &LangSys<'_>,
    features: &FeatureList<'_>,
    asked: &IntSet<Tag>,
    varied: &IntSet<u16>,
) -> Result<IntSet<u16>, ReadError> {
    let mut found = IntSet::empty();
    let firsts = language.feature_indices().iter().filter(|index| {
        features
            .feature_records()
            .get(usize::from(index.get()))
            .is_some_and(|record| {
                asked.contains(record.feature_tag()) && found.insert(record.feature_tag())
            })
    });
    let mut lookups = IntSet::empty();
    for index in std::iter::once(language.required_feature_index())
        .chain(firsts.map(|index| index.get()))
        .filter(|index| !varied.contains(*index))
    {
        let Some(record) = features.feature_records().get(usize::from(index)) else {
            continue;
        };
        if record.feature_offset().is_null() {
            continue;
        }
        let feature = record.feature(features.offset_data())?;
        lookups.extend_unsorted(
            feature
                .lookup_list_indices()
                .iter()
                .map(|index| index.get()),
        );
    }
    Ok(lookups)
}

// The features a variation of the font may swap or add lookups to.
fn varied_features(variations: &FeatureVariations<'_>) -> Result<IntSet<u16>, ReadError> {
    let mut varied = IntSet::empty();
    for record in variations.feature_variation_records() {
        if let Some(swaps) = record
            .feature_table_substitution(variations.offset_data())
            .transpose()?
        {
            varied.extend_unsorted(
                swaps
                    .substitutions()
                    .iter()
                    .map(|swap| swap.feature_index()),
            );
        }
    }
    for record in variations.lookup_variation_records().unwrap_or_default() {
        varied.insert(record.feature_index());
    }
    Ok(varied)
}

fn single(tag: Tag) -> IntSet<Tag> {
    let mut set = IntSet::empty();
    set.insert(tag);
    set
}

// Every feature: a walk that must hold for any plan reads them all.
pub(crate) fn everything() -> IntSet<Tag> {
    let mut set = IntSet::empty();
    set.invert();
    set
}

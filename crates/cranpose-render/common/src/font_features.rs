use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, PoisonError},
};

use cranpose_core::hash::default as default_hash;
use ttf_parser::{
    Face, GlyphId, Tag,
    gdef::{GlyphClass, Table as GlyphDefinitions},
    gsub::{SingleSubstitution, SubstitutionSubtable},
    opentype_layout::{Coverage, Feature, LayoutTable, Lookup},
};

use crate::ascii_glyphs::AsciiGlyphs;

const MAX_FEATURE_VARIANTS: usize = 16;
const DEFAULT_SCRIPTS: [&[u8; 4]; 2] = [b"DFLT", b"latn"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FontFeature {
    pub(crate) tag: [u8; 4],
    pub(crate) value: u32,
}

pub(crate) fn font_feature_settings(settings: &str) -> impl Iterator<Item = FontFeature> + '_ {
    settings.split(',').filter_map(parse_font_feature)
}

pub(crate) fn enabled_feature_tags(settings: &str) -> Vec<[u8; 4]> {
    let mut features: Vec<FontFeature> = Vec::new();
    for feature in font_feature_settings(settings) {
        match features.iter_mut().find(|known| known.tag == feature.tag) {
            Some(known) => known.value = feature.value,
            None => features.push(feature),
        }
    }
    let mut tags: Vec<[u8; 4]> = features
        .into_iter()
        .filter(|feature| feature.value != 0)
        .map(|feature| feature.tag)
        .collect();
    tags.sort_unstable();
    tags
}

fn parse_font_feature(entry: &str) -> Option<FontFeature> {
    let (tag, rest) = split_feature_tag(entry.trim())?;
    let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
    if !tag.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return None;
    }
    Some(FontFeature {
        tag,
        value: parse_feature_value(rest.trim_start())?,
    })
}

fn split_feature_tag(entry: &str) -> Option<(&str, &str)> {
    match *entry.as_bytes().first()? {
        quote @ (b'"' | b'\'') => {
            if entry.as_bytes().get(5) != Some(&quote) {
                return None;
            }
            Some((entry.get(1..5)?, entry.get(6..)?))
        }
        _ => {
            let length = entry
                .bytes()
                .position(|byte| !byte.is_ascii_alphanumeric())
                .unwrap_or(entry.len());
            if length != 4 {
                return None;
            }
            entry.split_at_checked(length)
        }
    }
}

fn parse_feature_value(value: &str) -> Option<u32> {
    if value.is_empty() || value.eq_ignore_ascii_case("on") {
        return Some(1);
    }
    if value.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| value.parse().ok())
        .flatten()
}

pub(crate) struct GlyphSubstitutions {
    tags: Box<[[u8; 4]]>,
    key: u64,
    pairs: Box<[(u16, u16)]>,
}

impl GlyphSubstitutions {
    pub(crate) fn build(face: &Face<'_>, tags: &[[u8; 4]]) -> Option<Self> {
        let gsub = face.tables().gsub?;
        let definitions = face.tables().gdef;
        let lookups: Vec<Box<[(u16, u16)]>> = feature_lookup_indices(&gsub, tags)
            .into_iter()
            .filter_map(|index| single_substitutions(gsub.lookups.get(index)?, definitions))
            .collect();
        let mut covered: Vec<u16> = lookups
            .iter()
            .flat_map(|lookup| lookup.iter().map(|&(glyph, _)| glyph))
            .collect();
        covered.sort_unstable();
        covered.dedup();
        let pairs: Box<[(u16, u16)]> = covered
            .into_iter()
            .filter_map(|glyph| {
                let substitute = lookups
                    .iter()
                    .fold(glyph, |glyph, lookup| substitute(lookup, glyph));
                (substitute != glyph).then_some((glyph, substitute))
            })
            .collect();
        if pairs.is_empty() {
            return None;
        }
        let mut state = default_hash::new();
        tags.hash(&mut state);
        Some(Self {
            tags: tags.into(),
            key: state.finish().max(1),
            pairs,
        })
    }

    pub(crate) fn apply(&self, glyph: ab_glyph::GlyphId) -> ab_glyph::GlyphId {
        ab_glyph::GlyphId(substitute(&self.pairs, glyph.0))
    }

    pub(crate) fn key(&self) -> u64 {
        self.key
    }

    pub(crate) fn tags(&self) -> &[[u8; 4]] {
        &self.tags
    }
}

fn substitute(pairs: &[(u16, u16)], glyph: u16) -> u16 {
    pairs
        .binary_search_by_key(&glyph, |&(covered, _)| covered)
        .ok()
        .and_then(|index| pairs.get(index))
        .map_or(glyph, |&(_, substitute)| substitute)
}

fn feature_lookup_indices(gsub: &LayoutTable<'_>, tags: &[[u8; 4]]) -> Vec<u16> {
    let mut indices: Vec<u16> = tags
        .iter()
        .filter_map(|tag| find_feature(gsub, Tag::from_bytes(tag)))
        .filter(|feature| {
            feature
                .lookup_indices
                .into_iter()
                .all(|index| is_single_substitution(gsub, index))
        })
        .flat_map(|feature| feature.lookup_indices)
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

fn is_single_substitution(gsub: &LayoutTable<'_>, index: u16) -> bool {
    gsub.lookups.get(index).is_some_and(|lookup| {
        lookup
            .subtables
            .into_iter::<SubstitutionSubtable<'_>>()
            .all(|subtable| matches!(subtable, SubstitutionSubtable::Single(_)))
    })
}

fn find_feature<'a>(gsub: &LayoutTable<'a>, tag: Tag) -> Option<Feature<'a>> {
    DEFAULT_SCRIPTS
        .iter()
        .filter_map(|script| gsub.scripts.find(Tag::from_bytes(script))?.default_language)
        .flat_map(|language| language.feature_indices)
        .filter_map(|index| gsub.features.get(index))
        .find(|feature| feature.tag == tag)
        .or_else(|| gsub.features.into_iter().find(|feature| feature.tag == tag))
}

fn single_substitutions(
    lookup: Lookup<'_>,
    definitions: Option<GlyphDefinitions<'_>>,
) -> Option<Box<[(u16, u16)]>> {
    let mut pairs = Vec::new();
    for subtable in lookup.subtables.into_iter::<SubstitutionSubtable<'_>>() {
        let SubstitutionSubtable::Single(single) = subtable else {
            continue;
        };
        for_each_covered(single.coverage(), |glyph, coverage_index| {
            let substitute = match single {
                SingleSubstitution::Format1 { delta, .. } => {
                    Some(GlyphId(glyph.0.wrapping_add_signed(delta)))
                }
                SingleSubstitution::Format2 { substitutes, .. } => substitutes.get(coverage_index),
            };
            if let Some(substitute) = substitute
                && !lookup_skips(&lookup, definitions.as_ref(), glyph)
            {
                pairs.push((glyph.0, substitute.0));
            }
        });
    }
    pairs.sort_by_key(|&(glyph, _)| glyph);
    pairs.dedup_by_key(|&mut (glyph, _)| glyph);
    (!pairs.is_empty()).then(|| pairs.into_boxed_slice())
}

fn for_each_covered(coverage: Coverage<'_>, mut visit: impl FnMut(GlyphId, u16)) {
    match coverage {
        Coverage::Format1 { glyphs } => {
            for (index, glyph) in glyphs.into_iter().enumerate() {
                if let Ok(index) = u16::try_from(index) {
                    visit(glyph, index);
                }
            }
        }
        Coverage::Format2 { records } => {
            for record in records {
                for glyph in record.start.0..=record.end.0 {
                    if let Some(index) = record.value.checked_add(glyph - record.start.0) {
                        visit(GlyphId(glyph), index);
                    }
                }
            }
        }
    }
}

fn lookup_skips(
    lookup: &Lookup<'_>,
    definitions: Option<&GlyphDefinitions<'_>>,
    glyph: GlyphId,
) -> bool {
    let Some(definitions) = definitions else {
        return false;
    };
    let flags = lookup.flags;
    match definitions.glyph_class(glyph) {
        Some(GlyphClass::Base) => flags.ignore_base_glyphs(),
        Some(GlyphClass::Ligature) => flags.ignore_ligatures(),
        Some(GlyphClass::Mark) => {
            flags.ignore_marks() || mark_filtered_out(lookup, definitions, glyph)
        }
        _ => false,
    }
}

fn mark_filtered_out(
    lookup: &Lookup<'_>,
    definitions: &GlyphDefinitions<'_>,
    glyph: GlyphId,
) -> bool {
    match lookup.mark_filtering_set {
        Some(set) => !definitions.is_mark_glyph(glyph, Some(set)),
        None => {
            let class = lookup.flags.mark_attachment_type();
            class != 0 && u16::from(class) != definitions.glyph_mark_attachment_class(glyph)
        }
    }
}

#[derive(Clone)]
pub(crate) struct FeatureGlyphs {
    pub(crate) substitutions: Arc<GlyphSubstitutions>,
    pub(crate) ascii: Arc<AsciiGlyphs>,
}

pub(crate) struct FeatureVariants {
    base_ascii: Arc<AsciiGlyphs>,
    variants: Mutex<Vec<(Box<str>, Option<FeatureGlyphs>)>>,
}

impl FeatureVariants {
    pub(crate) fn new(base_ascii: Arc<AsciiGlyphs>) -> Self {
        Self {
            base_ascii,
            variants: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn base_ascii(&self) -> &Arc<AsciiGlyphs> {
        &self.base_ascii
    }

    pub(crate) fn get(&self, settings: &str, font_data: &[u8]) -> Option<FeatureGlyphs> {
        let mut variants = self.variants.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, glyphs)) = variants.iter().find(|(known, _)| **known == *settings) {
            return glyphs.clone();
        }
        let glyphs = build_feature_glyphs(settings, font_data, &variants);
        if variants.len() >= MAX_FEATURE_VARIANTS {
            variants.remove(0);
        }
        variants.push((settings.into(), glyphs.clone()));
        glyphs
    }
}

fn build_feature_glyphs(
    settings: &str,
    font_data: &[u8],
    known: &[(Box<str>, Option<FeatureGlyphs>)],
) -> Option<FeatureGlyphs> {
    let tags = enabled_feature_tags(settings);
    if tags.is_empty() {
        return None;
    }
    if let Some(glyphs) = known
        .iter()
        .filter_map(|(_, glyphs)| glyphs.as_ref())
        .find(|glyphs| glyphs.substitutions.tags() == tags.as_slice())
    {
        return Some(glyphs.clone());
    }
    let face = Face::parse(font_data, 0).ok()?;
    Some(FeatureGlyphs {
        substitutions: Arc::new(GlyphSubstitutions::build(&face, &tags)?),
        ascii: Arc::new(AsciiGlyphs::new()),
    })
}

#[cfg(test)]
#[path = "tests/font_features_tests.rs"]
mod tests;

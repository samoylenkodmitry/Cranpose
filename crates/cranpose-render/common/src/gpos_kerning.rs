//! Pair kerning read out of a font's `GPOS` table.
//!
//! `ab_glyph`'s `kern()` reads the TrueType `kern` table and nothing else. Neither
//! font this framework actually draws with has one: Android's
//! `/system/fonts/Roboto-Regular.ttf` carries its pair kerning in `GPOS` only,
//! and so does the embedded `NotoSansMerged` fallback. Every string Cranpose
//! measured or drew was therefore unkerned while the platform's own text stack
//! kerned it — `LY` by −239 font units, `To` by −99, `AV` by −87 on Roboto.
//!
//! This reads the `kern` feature's pair-adjustment lookups directly, because
//! that is the whole of the defect. It is deliberately *not* a shaper: no glyph
//! substitution, no mark attachment, no cursive joining, no script-aware
//! reordering. Those are `GSUB`, `mark`/`mkmk` and `curs`, and adopting them
//! means adopting a shaping engine. What is implemented here is the one thing
//! the measurement named, and the boundary is stated in
//! [`GposKerning::parse`].
//!
//! Two behaviours are copied from HarfBuzz rather than from the OpenType prose,
//! because HarfBuzz is what Android runs and therefore what fonts are compiled
//! against:
//!
//! * a `PairPosFormat1` value record's device offset is measured from its
//!   `PairSet`, not from the subtable. The specification says subtable; every
//!   `VariationIndex` in Roboto only resolves with `PairSet`, so the ambiguity
//!   is settled by the font.
//! * within one lookup the first subtable that covers a pair wins and the rest
//!   are skipped — including when the value it found is zero. Separate lookups
//!   accumulate. Dropping zero-valued pairs would let a later subtable answer
//!   for a pair an earlier one deliberately pinned at zero.

use std::sync::Arc;

use ab_glyph::{CodepointIdIter, Font, FontArc, GlyphId, GlyphSvg, Outline, v2};
use ttf_parser::{Face, NormalizedCoordinate, Tag};

const VALUE_FIELDS: [u16; 8] = [
    0x0001, 0x0002, 0x0004, 0x0008, 0x0010, 0x0020, 0x0040, 0x0080,
];
const X_ADVANCE: u16 = 0x0004;
const X_ADVANCE_DEVICE: u16 = 0x0040;

const VARIATION_INDEX: u16 = 0x8000;

#[derive(Debug, Clone)]
struct PairLookup {
    subtables: Vec<PairSubtable>,
}

#[derive(Debug, Clone)]
enum PairSubtable {
    Specific {
        pairs: Vec<(u32, f32)>,
    },
    Class {
        coverage: Vec<u16>,
        first: ClassDef,
        second: ClassDef,
        second_count: u16,
        matrix: Vec<f32>,
    },
}

#[derive(Debug, Clone, Default)]
struct ClassDef {
    ranges: Vec<(u16, u16, u16)>,
}

impl ClassDef {
    fn class_of(&self, glyph: u16) -> u16 {
        match self.ranges.binary_search_by(|(start, end, _)| {
            if glyph < *start {
                std::cmp::Ordering::Greater
            } else if glyph > *end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        }) {
            Ok(index) => self.ranges[index].2,
            Err(_) => 0,
        }
    }

    fn from_pairs(mut pairs: Vec<(u16, u16)>) -> Self {
        pairs.sort_unstable();
        let mut ranges: Vec<(u16, u16, u16)> = Vec::new();
        for (glyph, class) in pairs {
            if class == 0 {
                continue;
            }
            match ranges.last_mut() {
                Some((_, end, last_class))
                    if *last_class == class && glyph == end.saturating_add(1) =>
                {
                    *end = glyph;
                }
                _ => ranges.push((glyph, glyph, class)),
            }
        }
        Self { ranges }
    }
}

/// Every `kern`-feature pair adjustment in a face, resolved for one instance.
///
/// "One instance" is load-bearing: on a variable face the values carry
/// `VariationIndex` deltas, and Roboto's are worth up to 136 font units at the
/// weights a Wear app asks for. They are resolved once here against the face's
/// normalized coordinates, so a query is a lookup rather than a delta
/// evaluation — and two instances of one file get two tables, which is already
/// how the rest of the font cache treats them.
#[derive(Debug, Clone, Default)]
pub struct GposKerning {
    lookups: Vec<PairLookup>,
}

impl GposKerning {
    /// Reads the `kern` feature's pair adjustments out of `face`.
    ///
    /// Returns `None` when the face has no `GPOS`, no `kern` feature, or no
    /// pair-adjustment lookup under it — all of which mean "nothing to add",
    /// not "something went wrong".
    ///
    /// **Feature selection is the union over every script's default language
    /// system**, rather than the script of the run being measured. HarfBuzz
    /// picks one script per run, which needs the run's script, which needs
    /// itemization — the first piece of a shaper. The union is safe in
    /// practice because a lookup can only fire on a pair it covers, and it is
    /// exact for the fonts in play: Roboto lists the identical feature set
    /// under `DFLT`, `latn`, `cyrl` and `grek`. A face that kerned one script
    /// differently from another through *different* `kern` lookups would need
    /// the real thing; nothing here does.
    ///
    /// Language-specific systems are ignored for the same reason, and so is
    /// `lookupFlag`: its glyph filters (`IgnoreMarks`, mark attachment classes)
    /// only bite on runs with marks in them, which is again shaping.
    pub fn parse(face: &Face<'_>) -> Option<Self> {
        let gpos = face.raw_face().table(Tag::from_bytes(b"GPOS"))?;
        let coordinates = face.variation_coordinates();
        let gdef = face.tables().gdef;

        let script_list = u16_at(gpos, 4)? as usize;
        let feature_list = u16_at(gpos, 6)? as usize;
        let lookup_list = u16_at(gpos, 8)? as usize;

        let feature_indices = kern_feature_indices(gpos, script_list, feature_list)?;
        let mut lookup_indices = Vec::new();
        for index in feature_indices {
            for lookup in feature_lookups(gpos, feature_list, index)? {
                if !lookup_indices.contains(&lookup) {
                    lookup_indices.push(lookup);
                }
            }
        }
        lookup_indices.sort_unstable();

        let mut lookups = Vec::new();
        for index in lookup_indices {
            let Some(subtables) = pair_lookup(gpos, lookup_list, index, gdef, coordinates) else {
                continue;
            };
            if !subtables.is_empty() {
                lookups.push(PairLookup { subtables });
            }
        }

        (!lookups.is_empty()).then_some(Self { lookups })
    }

    /// The pair adjustment for `first` followed by `second`, in font units.
    pub fn kern_unscaled(&self, first: u16, second: u16) -> f32 {
        let mut total = 0.0;
        for lookup in &self.lookups {
            for subtable in &lookup.subtables {
                if let Some(value) = subtable.lookup(first, second) {
                    total += value;
                    break;
                }
            }
        }
        total
    }

    pub fn is_empty(&self) -> bool {
        self.lookups.is_empty()
    }
}

impl PairSubtable {
    fn lookup(&self, first: u16, second: u16) -> Option<f32> {
        match self {
            Self::Specific { pairs } => {
                let key = (u32::from(first) << 16) | u32::from(second);
                pairs
                    .binary_search_by_key(&key, |(pair, _)| *pair)
                    .ok()
                    .map(|index| pairs[index].1)
            }
            Self::Class {
                coverage,
                first: first_classes,
                second: second_classes,
                second_count,
                matrix,
            } => {
                coverage.binary_search(&first).ok()?;
                let row = first_classes.class_of(first);
                let column = second_classes.class_of(second);
                if column >= *second_count {
                    return None;
                }
                let index = usize::from(row) * usize::from(*second_count) + usize::from(column);
                matrix.get(index).copied()
            }
        }
    }
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    let bytes = data.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn i16_at(data: &[u8], offset: usize) -> Option<i16> {
    u16_at(data, offset).map(|value| value as i16)
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn kern_feature_indices(gpos: &[u8], script_list: usize, feature_list: usize) -> Option<Vec<u16>> {
    let script_count = u16_at(gpos, script_list)?;
    let mut reachable = Vec::new();
    for index in 0..script_count {
        let record = script_list + 2 + usize::from(index) * 6;
        let script = script_list + usize::from(u16_at(gpos, record + 4)?);
        let default_lang_sys = u16_at(gpos, script)?;
        if default_lang_sys == 0 {
            continue;
        }
        let lang_sys = script + usize::from(default_lang_sys);
        let count = u16_at(gpos, lang_sys + 4)?;
        for slot in 0..count {
            let feature = u16_at(gpos, lang_sys + 6 + usize::from(slot) * 2)?;
            if !reachable.contains(&feature) {
                reachable.push(feature);
            }
        }
    }

    let feature_count = u16_at(gpos, feature_list)?;
    let mut kern = Vec::new();
    for index in reachable {
        if index >= feature_count {
            continue;
        }
        let record = feature_list + 2 + usize::from(index) * 6;
        let tag = gpos.get(record..record + 4)?;
        if tag == b"kern" {
            kern.push(index);
        }
    }
    Some(kern)
}

fn feature_lookups(gpos: &[u8], feature_list: usize, index: u16) -> Option<Vec<u16>> {
    let record = feature_list + 2 + usize::from(index) * 6;
    let feature = feature_list + usize::from(u16_at(gpos, record + 4)?);
    let count = u16_at(gpos, feature + 2)?;
    let mut lookups = Vec::with_capacity(usize::from(count));
    for slot in 0..count {
        lookups.push(u16_at(gpos, feature + 4 + usize::from(slot) * 2)?);
    }
    Some(lookups)
}

fn pair_lookup(
    gpos: &[u8],
    lookup_list: usize,
    index: u16,
    gdef: Option<ttf_parser::gdef::Table<'_>>,
    coordinates: &[NormalizedCoordinate],
) -> Option<Vec<PairSubtable>> {
    let count = u16_at(gpos, lookup_list)?;
    if index >= count {
        return None;
    }
    let lookup = lookup_list + usize::from(u16_at(gpos, lookup_list + 2 + usize::from(index) * 2)?);
    let kind = u16_at(gpos, lookup)?;
    let subtable_count = u16_at(gpos, lookup + 4)?;

    let mut subtables = Vec::new();
    for slot in 0..subtable_count {
        let mut offset = lookup + usize::from(u16_at(gpos, lookup + 6 + usize::from(slot) * 2)?);
        let mut resolved = kind;
        if resolved == 9 {
            resolved = u16_at(gpos, offset + 2)?;
            offset += u32_at(gpos, offset + 4)? as usize;
        }
        if resolved != 2 {
            continue;
        }
        if let Some(subtable) = pair_subtable(gpos, offset, gdef, coordinates) {
            subtables.push(subtable);
        }
    }
    Some(subtables)
}

fn pair_subtable(
    gpos: &[u8],
    offset: usize,
    gdef: Option<ttf_parser::gdef::Table<'_>>,
    coordinates: &[NormalizedCoordinate],
) -> Option<PairSubtable> {
    let format = u16_at(gpos, offset)?;
    let coverage_offset = offset + usize::from(u16_at(gpos, offset + 2)?);
    let value_format_1 = u16_at(gpos, offset + 4)?;
    let value_format_2 = u16_at(gpos, offset + 6)?;
    let coverage = coverage_glyphs(gpos, coverage_offset)?;

    match format {
        1 => {
            let set_count = u16_at(gpos, offset + 8)?;
            let record_len = 2 + value_size(value_format_1) + value_size(value_format_2);
            let mut pairs = Vec::new();
            for slot in 0..set_count {
                let Some(&first) = coverage.get(usize::from(slot)) else {
                    continue;
                };
                let set = offset + usize::from(u16_at(gpos, offset + 10 + usize::from(slot) * 2)?);
                let count = u16_at(gpos, set)?;
                for record in 0..count {
                    let at = set + 2 + usize::from(record) * record_len;
                    let Some(second) = u16_at(gpos, at) else {
                        continue;
                    };
                    let value = x_advance(gpos, at + 2, value_format_1, set, gdef, coordinates);
                    pairs.push(((u32::from(first) << 16) | u32::from(second), value));
                }
            }
            pairs.sort_by_key(|(key, _)| *key);
            pairs.dedup_by_key(|(key, _)| *key);
            (!pairs.is_empty()).then_some(PairSubtable::Specific { pairs })
        }
        2 => {
            let first = class_def(gpos, offset + usize::from(u16_at(gpos, offset + 8)?))?;
            let second = class_def(gpos, offset + usize::from(u16_at(gpos, offset + 10)?))?;
            let first_count = u16_at(gpos, offset + 12)?;
            let second_count = u16_at(gpos, offset + 14)?;
            let record_len = value_size(value_format_1) + value_size(value_format_2);
            let cells = usize::from(first_count) * usize::from(second_count);
            let mut matrix = Vec::with_capacity(cells);
            for cell in 0..cells {
                let at = offset + 16 + cell * record_len;
                matrix.push(x_advance(
                    gpos,
                    at,
                    value_format_1,
                    offset,
                    gdef,
                    coordinates,
                ));
            }
            Some(PairSubtable::Class {
                coverage,
                first,
                second,
                second_count,
                matrix,
            })
        }
        _ => None,
    }
}

fn value_size(format: u16) -> usize {
    usize::try_from((format & 0xFF).count_ones()).unwrap_or(0) * 2
}

fn x_advance(
    gpos: &[u8],
    at: usize,
    format: u16,
    device_base: usize,
    gdef: Option<ttf_parser::gdef::Table<'_>>,
    coordinates: &[NormalizedCoordinate],
) -> f32 {
    let mut cursor = at;
    let mut advance = 0.0f32;
    for field in VALUE_FIELDS {
        if format & field == 0 {
            continue;
        }
        if field == X_ADVANCE {
            advance += f32::from(i16_at(gpos, cursor).unwrap_or(0));
        }
        if field == X_ADVANCE_DEVICE {
            let device = u16_at(gpos, cursor).unwrap_or(0);
            if device != 0 && !coordinates.is_empty() {
                let table = device_base + usize::from(device);
                if u16_at(gpos, table + 4) == Some(VARIATION_INDEX) {
                    let outer = u16_at(gpos, table).unwrap_or(0);
                    let inner = u16_at(gpos, table + 2).unwrap_or(0);
                    advance += gdef
                        .and_then(|gdef| gdef.glyph_variation_delta(outer, inner, coordinates))
                        .unwrap_or(0.0);
                }
            }
        }
        cursor += 2;
    }
    advance
}

fn coverage_glyphs(gpos: &[u8], offset: usize) -> Option<Vec<u16>> {
    match u16_at(gpos, offset)? {
        1 => {
            let count = u16_at(gpos, offset + 2)?;
            let mut glyphs = Vec::with_capacity(usize::from(count));
            for index in 0..count {
                glyphs.push(u16_at(gpos, offset + 4 + usize::from(index) * 2)?);
            }
            Some(glyphs)
        }
        2 => {
            let count = u16_at(gpos, offset + 2)?;
            let mut glyphs = Vec::new();
            for index in 0..count {
                let record = offset + 4 + usize::from(index) * 6;
                let start = u16_at(gpos, record)?;
                let end = u16_at(gpos, record + 2)?;
                for glyph in start..=end {
                    glyphs.push(glyph);
                }
            }
            Some(glyphs)
        }
        _ => None,
    }
}

fn class_def(gpos: &[u8], offset: usize) -> Option<ClassDef> {
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    match u16_at(gpos, offset)? {
        1 => {
            let start = u16_at(gpos, offset + 2)?;
            let count = u16_at(gpos, offset + 4)?;
            for index in 0..count {
                let class = u16_at(gpos, offset + 6 + usize::from(index) * 2)?;
                pairs.push((start.saturating_add(index), class));
            }
        }
        2 => {
            let count = u16_at(gpos, offset + 2)?;
            for index in 0..count {
                let record = offset + 4 + usize::from(index) * 6;
                let start = u16_at(gpos, record)?;
                let end = u16_at(gpos, record + 2)?;
                let class = u16_at(gpos, record + 4)?;
                for glyph in start..=end {
                    pairs.push((glyph, class));
                }
            }
        }
        _ => return Some(ClassDef::default()),
    }
    Some(ClassDef::from_pairs(pairs))
}

/// A face paired with the pair kerning `ab_glyph` cannot read for itself.
///
/// This is a `Font` rather than a lookup the call sites reach for, because
/// there are nine of them across measuring, line breaking, alignment and
/// drawing, several behind `impl Font` generics that never see a
/// `SoftwareTextFont`. Overriding `kern_unscaled` puts the rule under every
/// existing `scaled_font.kern(..)` at once — including any this change did not
/// go looking for — and keeps a caller from having to remember which width is
/// the kerned one.
#[derive(Clone)]
pub struct KernedFont {
    font: FontArc,
    kerning: Option<Arc<GposKerning>>,
}

impl KernedFont {
    pub fn new(font: FontArc, kerning: Option<Arc<GposKerning>>) -> Self {
        Self { font, kerning }
    }

    /// Reads the GPOS pair kerning for a face instanced at `variations`.
    ///
    /// `variations` are the `(tag, user value)` pairs actually applied to the
    /// face, in the same units the caller handed `set_variation`; normalizing
    /// them is `ttf_parser`'s job and doing it a second way here would be a
    /// second answer.
    pub fn read_kerning(bytes: &[u8], variations: &[([u8; 4], f32)]) -> Option<Arc<GposKerning>> {
        let mut face = Face::parse(bytes, 0).ok()?;
        for (tag, value) in variations {
            face.set_variation(Tag::from_bytes(tag), *value);
        }
        GposKerning::parse(&face).map(Arc::new)
    }
}

impl Font for KernedFont {
    fn units_per_em(&self) -> Option<f32> {
        self.font.units_per_em()
    }

    fn ascent_unscaled(&self) -> f32 {
        self.font.ascent_unscaled()
    }

    fn descent_unscaled(&self) -> f32 {
        self.font.descent_unscaled()
    }

    fn line_gap_unscaled(&self) -> f32 {
        self.font.line_gap_unscaled()
    }

    fn italic_angle(&self) -> f32 {
        self.font.italic_angle()
    }

    fn glyph_id(&self, c: char) -> GlyphId {
        self.font.glyph_id(c)
    }

    fn h_advance_unscaled(&self, id: GlyphId) -> f32 {
        self.font.h_advance_unscaled(id)
    }

    fn h_side_bearing_unscaled(&self, id: GlyphId) -> f32 {
        self.font.h_side_bearing_unscaled(id)
    }

    fn v_advance_unscaled(&self, id: GlyphId) -> f32 {
        self.font.v_advance_unscaled(id)
    }

    fn v_side_bearing_unscaled(&self, id: GlyphId) -> f32 {
        self.font.v_side_bearing_unscaled(id)
    }

    fn kern_unscaled(&self, first: GlyphId, second: GlyphId) -> f32 {
        match &self.kerning {
            Some(kerning) => kerning.kern_unscaled(first.0, second.0),
            None => self.font.kern_unscaled(first, second),
        }
    }

    fn outline(&self, id: GlyphId) -> Option<Outline> {
        self.font.outline(id)
    }

    fn glyph_count(&self) -> usize {
        self.font.glyph_count()
    }

    fn codepoint_ids(&self) -> CodepointIdIter<'_> {
        self.font.codepoint_ids()
    }

    fn glyph_raster_image2(&self, id: GlyphId, pixel_size: u16) -> Option<v2::GlyphImage<'_>> {
        self.font.glyph_raster_image2(id, pixel_size)
    }

    fn glyph_svg_image(&self, id: GlyphId) -> Option<GlyphSvg<'_>> {
        self.font.glyph_svg_image(id)
    }

    fn font_data(&self) -> &[u8] {
        self.font.font_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTO: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");

    fn face() -> Face<'static> {
        Face::parse(NOTO, 0).expect("font")
    }

    fn glyph(face: &Face<'_>, ch: char) -> u16 {
        face.glyph_index(ch).expect("glyph").0
    }

    #[test]
    fn embedded_font_has_no_truetype_kern_table() {
        assert!(face().raw_face().table(Tag::from_bytes(b"kern")).is_none());
    }

    #[test]
    fn gpos_kerning_is_found_and_negative_for_a_kerning_pair() {
        let face = face();
        let kerning = GposKerning::parse(&face).expect("GPOS kerning");
        assert!(!kerning.is_empty());
        let value = kerning.kern_unscaled(glyph(&face, 'A'), glyph(&face, 'V'));
        assert!(value < 0.0, "AV should tuck, got {value}");
    }

    #[test]
    fn unkerned_pairs_are_zero() {
        let face = face();
        let kerning = GposKerning::parse(&face).expect("GPOS kerning");
        assert_eq!(
            kerning.kern_unscaled(glyph(&face, 'n'), glyph(&face, 'n')),
            0.0
        );
    }

    #[test]
    fn kerning_is_directional() {
        let face = face();
        let kerning = GposKerning::parse(&face).expect("GPOS kerning");
        let forward = kerning.kern_unscaled(glyph(&face, 'A'), glyph(&face, 'V'));
        let backward = kerning.kern_unscaled(glyph(&face, 'V'), glyph(&face, 'A'));
        assert!(forward < 0.0 && backward < 0.0);
    }

    #[test]
    fn class_def_collapses_runs_and_defaults_to_zero() {
        let classes = ClassDef::from_pairs(vec![(4, 1), (5, 1), (6, 1), (9, 2), (12, 0)]);
        assert_eq!(classes.ranges, vec![(4, 6, 1), (9, 9, 2)]);
        assert_eq!(classes.class_of(5), 1);
        assert_eq!(classes.class_of(9), 2);
        assert_eq!(classes.class_of(12), 0);
        assert_eq!(classes.class_of(0), 0);
    }

    #[test]
    fn value_size_counts_only_the_low_byte() {
        assert_eq!(value_size(0x0000), 0);
        assert_eq!(value_size(0x0004), 2);
        assert_eq!(value_size(0x0044), 4);
        assert_eq!(value_size(0x00FF), 16);
    }
}

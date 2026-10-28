use std::{
    hash::{BuildHasher, Hash, Hasher},
    sync::{Arc, OnceLock},
};

use foldhash::fast::FixedState;
use read_fonts::{
    FontRef, TableProvider,
    model::{Blob, Font, Source, Variation, attribute::Style, name::NameId},
    types::{GlyphId as FontGlyphId, Tag},
};
use smallvec::SmallVec;

use crate::{
    coverage::Coverage,
    font::{FontFamily, FontStyle, FontWeight},
    glyph_id::GlyphId,
};

/// The bytes a typeface is read from.
pub enum FontBytes {
    /// Bytes that live as long as the process, such as an embedded font or a
    /// font file read once: the typeface reads them in place.
    Static(&'static [u8]),
    /// Bytes the typeface takes over.
    Owned(Vec<u8>),
}

impl FontBytes {
    /// The font file's bytes.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(bytes) => bytes,
            Self::Owned(bytes) => bytes,
        }
    }
}

impl From<&'static [u8]> for FontBytes {
    fn from(bytes: &'static [u8]) -> Self {
        Self::Static(bytes)
    }
}

impl<const N: usize> From<&'static [u8; N]> for FontBytes {
    fn from(bytes: &'static [u8; N]) -> Self {
        Self::Static(bytes)
    }
}

impl From<Vec<u8>> for FontBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Owned(bytes)
    }
}

/// The identity an app-supplied typeface was registered under.
///
/// `FontFamily::FileBacked` and `FontFamily::LoadedTypeface` name a typeface by
/// its files rather than by a name inside the font, so resolution cannot
/// compare strings from the `name` table. Hashing the `FontFamily` value once
/// at registration and once per resolve keeps the two sides in step without
/// walking path lists on every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFamilyKey(u64);

impl FontFamilyKey {
    /// Returns the key of `family`, the same for equal families.
    pub fn of(family: &FontFamily) -> Self {
        Self(HASH_STATE.hash_one(family))
    }
}

/// Why bytes could not become a typeface.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum TypefaceError {
    /// The bytes are not a font this crate reads.
    #[error("the bytes are not a readable font")]
    InvalidFont,
    /// A variation names an axis the font lacks, or a value outside its range.
    #[error("variation {tag:?} is not an axis of the font, or lies outside its range")]
    InvalidVariation {
        /// The axis tag of the variation.
        tag: [u8; 4],
    },
}

/// A font loaded for text at one location in its design space, with the
/// family names, weight and style text chooses it by: Compose's `Typeface`.
/// Clones share one load.
#[derive(Clone)]
pub struct Typeface(Arc<Loaded>);

struct Loaded {
    font: Font,
    families: Box<[String]>,
    registered_family: Option<FontFamilyKey>,
    weight: FontWeight,
    style: FontStyle,
    // The weights the typeface draws: its `wght` axis range when it was read
    // without a declaration, otherwise its one weight.
    weights: (f32, f32),
    // Whether an `ital` or `slnt` axis lets the typeface draw italic as well.
    italic_axis: bool,
    is_color: bool,
    // The axis positions the typeface is instanced at, sorted by tag.
    variations: Box<[([u8; 4], f32)]>,
    content_hash: u64,
    coverage: OnceLock<Coverage>,
}

impl Typeface {
    /// Reads `bytes` as a typeface no family was declared for: its weight,
    /// style and family names come from the font.
    pub fn from_bytes(bytes: impl Into<FontBytes>) -> Result<Self, TypefaceError> {
        let font = Font::new(blob(bytes.into()), 0).ok_or(TypefaceError::InvalidFont)?;
        let content_hash = content_hash(&font, &[]);
        let weight = (&font.tables())
            .os2()
            .ok()
            .and_then(|os2| FontWeight::try_new(os2.us_weight_class()))
            .unwrap_or_default();
        let style = match font.default_attributes().style {
            Style::Normal => FontStyle::Normal,
            Style::Italic | Style::Oblique(_) => FontStyle::Italic,
        };
        let axis = |tag: &[u8; 4]| font.axes().iter().find(|axis| axis.tag() == Tag::new(tag));
        let default_weight = f32::from(weight.value());
        let weights = axis(b"wght").map_or((default_weight, default_weight), |axis| {
            (axis.min_value().max(1.0), axis.max_value().min(1000.0))
        });
        let italic_axis = axis(b"ital").is_some_and(|axis| axis.max_value() >= 1.0)
            || axis(b"slnt").is_some_and(|axis| axis.min_value() < 0.0);
        Ok(Self(Arc::new(Loaded {
            families: family_names(&font),
            registered_family: None,
            weight,
            style,
            weights,
            italic_axis,
            is_color: is_color(&font),
            variations: Box::default(),
            content_hash,
            coverage: OnceLock::new(),
            font,
        })))
    }

    /// Reads `bytes` as a typeface an app registered under `family`,
    /// declaring `weight` and `style` for it, as a Compose `Font(resId,
    /// weight, style)` entry does: the declaration wins over the font's own
    /// values, and a variable font is instanced on its `wght`, `ital` and
    /// `slnt` axes, so one file backs a whole family. The explicit
    /// `variations` then win over the declared positions; a variation of an
    /// axis the font lacks, or outside that axis's range, fails.
    pub fn from_registered(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<FontBytes>,
        variations: &[([u8; 4], f32)],
    ) -> Result<Self, TypefaceError> {
        let font = Font::new(blob(bytes.into()), 0).ok_or(TypefaceError::InvalidFont)?;
        let mut applied = declared_variations(&font, weight, style);
        for &(tag, value) in variations {
            let valid = value.is_finite()
                && font.axes().iter().any(|axis| {
                    axis.tag() == Tag::new(&tag)
                        && (axis.min_value()..=axis.max_value()).contains(&value)
                });
            if !valid {
                return Err(TypefaceError::InvalidVariation { tag });
            }
            applied.retain(|(existing, _)| *existing != tag);
            applied.push((tag, value));
        }
        applied.sort_unstable_by_key(|(tag, _)| *tag);
        let content_hash = content_hash(&font, &applied);
        let font = if applied.is_empty() {
            font
        } else {
            font.instance_builder()
                .variations(
                    applied
                        .iter()
                        .map(|(tag, value)| Variation::new(Tag::new(tag), *value)),
                )
                .build()
        };
        let declared_weight = f32::from(weight.value());
        Ok(Self(Arc::new(Loaded {
            families: family_names(&font),
            registered_family: Some(FontFamilyKey::of(family)),
            weight,
            style,
            weights: (declared_weight, declared_weight),
            italic_axis: false,
            is_color: is_color(&font),
            variations: applied.into_boxed_slice(),
            content_hash,
            coverage: OnceLock::new(),
            font,
        })))
    }

    /// The family names the font's `name` table gives it, typographic family
    /// first.
    pub fn family_names(&self) -> &[String] {
        &self.0.families
    }

    /// The family the typeface was registered under, `None` for one read
    /// with [`Typeface::from_bytes`].
    pub fn registered_family(&self) -> Option<FontFamilyKey> {
        self.0.registered_family
    }

    /// The weight text chooses the typeface by.
    pub fn weight(&self) -> FontWeight {
        self.0.weight
    }

    /// The style text chooses the typeface by.
    pub fn style(&self) -> FontStyle {
        self.0.style
    }

    /// Whether the font draws glyphs in color (a `COLR`, `CBDT`, `sbix` or
    /// `SVG` table), as emoji fonts do.
    pub fn is_color(&self) -> bool {
        self.0.is_color
    }

    // The weights the typeface draws, lowest and highest.
    pub(crate) fn weights(&self) -> (f32, f32) {
        self.0.weights
    }

    // Whether the typeface draws `style`.
    pub(crate) fn draws(&self, style: FontStyle) -> bool {
        self.0.style == style || self.0.italic_axis
    }

    // The characters the font maps to glyphs, read from the `cmap` on first use.
    pub(crate) fn coverage(&self) -> &Coverage {
        self.0.coverage.get_or_init(|| {
            Coverage::from_codepoints(
                self.0
                    .font
                    .charmap()
                    .unicodes()
                    .filter(|(_, glyph)| *glyph != FontGlyphId::NOTDEF)
                    .map(|(codepoint, _)| codepoint),
            )
        })
    }

    /// The axis positions the typeface is instanced at, sorted by tag.
    pub fn variations(&self) -> &[([u8; 4], f32)] {
        &self.0.variations
    }

    /// A hash of the font's bytes and its axis positions: equal for two
    /// loads of one instance.
    pub fn content_hash(&self) -> u64 {
        self.0.content_hash
    }

    /// The font's design units in an em.
    pub fn units_per_em(&self) -> u16 {
        self.0.font.units_per_em()
    }

    /// The font's line metrics at the instance, in design units: the
    /// typographic line when the font asks for it with `USE_TYPO_METRICS`,
    /// otherwise the `hhea` line, as Skia and FreeType read them.
    pub fn line_metrics(&self) -> LineMetrics {
        let metrics = self.0.font.metrics();
        metrics
            .h_line()
            .map_or_else(LineMetrics::default, |line| LineMetrics {
                ascent: line.ascender.to_f32(),
                descent: line.descender.to_f32(),
                line_gap: line.line_gap.to_f32(),
            })
    }

    /// The glyph the font maps `ch` to, `None` when it has none.
    pub fn glyph(&self, ch: char) -> Option<GlyphId> {
        self.0
            .font
            .charmap()
            .map_unicode(ch)
            .map(|glyph| GlyphId(glyph.to_u32()))
    }

    /// Whether the font maps `ch` to a glyph.
    pub fn covers(&self, ch: char) -> bool {
        self.coverage().contains(ch)
    }

    /// The horizontal advance of `glyph` at the instance, in design units.
    pub fn advance(&self, glyph: GlyphId) -> f32 {
        self.0
            .font
            .glyph_metrics()
            .h_advance(FontGlyphId::new(glyph.0))
    }

    // Whether `other` is a clone of this typeface.
    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    // The font model at the typeface's instance.
    pub(crate) fn font(&self) -> &Font {
        &self.0.font
    }

    // The axis positions that draw the typeface at `weight` and `style`: for
    // a variable font read without a declaration, its `wght`, `ital` and
    // `slnt` positions; none for a registered typeface, which its declaration
    // already instanced.
    pub(crate) fn axis_positions(
        &self,
        weight: FontWeight,
        style: FontStyle,
    ) -> SmallVec<[([u8; 4], f32); 3]> {
        if self.0.registered_family.is_some() {
            return SmallVec::new();
        }
        declared_variations(&self.0.font, weight, style)
    }
}

impl std::fmt::Debug for Typeface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Typeface")
            .field("family_names", &self.0.families)
            .field("weight", &self.0.weight)
            .field("style", &self.0.style)
            .field("variations", &self.0.variations)
            .finish_non_exhaustive()
    }
}

/// A font's vertical line metrics in design units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineMetrics {
    /// Height above the baseline.
    pub ascent: f32,
    /// Depth below the baseline, negative.
    pub descent: f32,
    /// Space between one line's bottom and the next line's top.
    pub line_gap: f32,
}

fn blob(bytes: FontBytes) -> Blob {
    match bytes {
        FontBytes::Static(bytes) => Blob::from(bytes),
        FontBytes::Owned(bytes) => Blob::from(bytes),
    }
}

// The model's tables expose no color tables, so this reads the table
// directory of the bytes.
fn is_color(font: &Font) -> bool {
    let Source::Blob(bytes) = font.source() else {
        return false;
    };
    FontRef::from_index(bytes.as_ref(), 0).is_ok_and(|font| {
        [*b"COLR", *b"CBDT", *b"sbix", *b"SVG "]
            .iter()
            .any(|tag| font.table_data(Tag::new(tag)).is_some())
    })
}

// The state every hash of this module starts from, the same each run, so a
// family key and a content hash stay put for the process's life.
const HASH_STATE: FixedState = FixedState::with_seed(0);

// A hash of the font's bytes and the axis positions it is instanced at.
fn content_hash(font: &Font, variations: &[([u8; 4], f32)]) -> u64 {
    let mut state = HASH_STATE.build_hasher();
    if let Source::Blob(bytes) = font.source() {
        bytes.as_ref().hash(&mut state);
    }
    for (tag, value) in variations {
        tag.hash(&mut state);
        value.to_bits().hash(&mut state);
    }
    state.finish()
}

fn family_names(font: &Font) -> Box<[String]> {
    let names = font.names();
    let mut families: Vec<String> = Vec::new();
    for id in [NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME] {
        let Some(name) = names.get(id) else {
            continue;
        };
        for localized in name.iter() {
            let value: String = localized.name().chars().collect();
            if !value.is_empty()
                && !families
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&value))
            {
                families.push(value);
            }
        }
    }
    families.into_boxed_slice()
}

// The positions a declared weight and style ask of the font's axes, clamped
// into each axis's range.
fn declared_variations(
    font: &Font,
    weight: FontWeight,
    style: FontStyle,
) -> SmallVec<[([u8; 4], f32); 3]> {
    const OBLIQUE_DEGREES: f32 = -12.0;
    font.axes()
        .iter()
        .filter_map(|axis| {
            let tag = axis.tag().to_be_bytes();
            let requested = match &tag {
                b"wght" => f32::from(weight.value()),
                b"ital" if style == FontStyle::Italic => 1.0,
                b"slnt" if style == FontStyle::Italic => OBLIQUE_DEGREES,
                _ => return None,
            };
            Some((tag, requested.clamp(axis.min_value(), axis.max_value())))
        })
        .collect()
}

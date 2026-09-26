use std::{
    hash::{Hash, Hasher},
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use ab_glyph::{
    Font, FontArc, FontVec, Glyph, GlyphId, OutlinedGlyph, PxScale, ScaleFont, VariableFont, point,
};
use cranpose_core::hash::default as default_hash;
use cranpose_ui::{
    TextLinePrefixWidths, TextMeasurer, TextMetrics,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontSynthesis, FontWeight, RangeStyle,
        RenderString, Shadow, SpanStyle, TextDrawStyle, TextMotion, TextShaping, TextStyle,
    },
    text_layout_result::{GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult},
};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect};
use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

#[cfg(test)]
use crate::font_layout::layout_line_glyphs;
#[cfg(feature = "text-hyphenation")]
use crate::text_hyphenation::HyphenationDictionaryError;
use crate::{
    Brush,
    bounded_lru_cache::BoundedLruCache,
    brush_sampling::{color_to_rgba, sample_brush_rgba},
    font_layout::{
        GlyphPixelBounds, align_glyph_to_pixel_grid, line_advance_width,
        pixel_bounds_from_outlined, vertical_metrics,
    },
    font_tracking::FontTracking,
    gpos_kerning::KernedFont,
    text_hyphenation::HyphenationDictionaryStore,
};

const COMPOSE_STROKE_MITER_LIMIT: f32 = 4.0;
const SHADOW_SIGMA_SCALE: f32 = 0.57735;
const SHADOW_SIGMA_BIAS: f32 = 0.5;
const MAX_GAUSSIAN_KERNEL_HALF: i32 = 128;
const SOFTWARE_TEXT_GLYPH_METRICS_CACHE_CAPACITY: usize = 8_192;
const SOFTWARE_TEXT_KERN_METRICS_CACHE_CAPACITY: usize = 16_384;
const SOFTWARE_TEXT_PREFIX_WIDTH_CACHE_CAPACITY: usize = 512;
#[cfg(feature = "embedded-default-font")]
#[doc(hidden)]
pub const DEFAULT_SOFTWARE_TEXT_FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SoftwareTextFontError {
    #[error("invalid software text font bytes")]
    InvalidFont,
    /// An explicit axis is unknown, nonfinite, or outside the font’s supported range.
    #[error("invalid font variation axis {tag:?}")]
    InvalidVariation {
        /// OpenType axis tag that could not be applied.
        tag: [u8; 4],
    },
    #[error("embedded default font disabled (feature `embedded-default-font` is off)")]
    EmbeddedFontDisabled,
}

#[derive(Clone)]
pub struct SoftwareTextFont {
    font: KernedFont,
    metadata: SoftwareTextFontMetadata,
    score: TextFontScore,
    content_hash: u64,
}

#[derive(Clone)]
struct SoftwareTextFontMetadata {
    families: Arc<[String]>,
    registered_family: Option<FontFamilyKey>,
    weight: FontWeight,
    style: FontStyle,
    ab_glyph_scale_factor: f32,
    tracking: FontTracking,
}

/// Identity an app-supplied face was registered under.
///
/// `FontFamily::FileBacked` and `FontFamily::LoadedTypeface` name a face by its
/// files rather than by a name inside the font, so resolution cannot compare
/// strings from the `name` table. Hashing the `FontFamily` value once at
/// registration and once per resolve keeps the two sides in step without
/// walking path lists on every frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFamilyKey(u64);

impl FontFamilyKey {
    pub fn of(family: &FontFamily) -> Self {
        let mut state = default_hash::new();
        family.hash(&mut state);
        Self(state.finish())
    }
}

impl SoftwareTextFont {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, SoftwareTextFontError> {
        let bytes = bytes.into();
        let mut hasher = default_hash::new();
        bytes.hash(&mut hasher);
        let content_hash = hasher.finish();
        let metadata = software_text_font_metadata(bytes.as_slice());
        let kerning = KernedFont::read_kerning(bytes.as_slice(), &[]);
        let font = FontArc::try_from_vec(bytes).map_err(|_| SoftwareTextFontError::InvalidFont)?;
        let score = text_font_score_from_parts(&font, &metadata);
        Ok(Self {
            font: KernedFont::new(font, kerning),
            metadata,
            score,
            content_hash,
        })
    }

    /// Parse `bytes` as a face an app registered under `family`, declaring
    /// `weight` and `style` for it.
    ///
    /// The declaration wins over the face's own `OS/2` values, the way a
    /// Compose `Font(resId, FontWeight.Medium)` entry does, and a variable face
    /// is instanced on its `wght`/`ital` axes so one file can back a whole
    /// family. That last part is what makes Android's `sans-serif` reachable:
    /// the platform ships a single variable `Roboto-Regular.ttf` and describes
    /// every weight of the family as an axis position on it.
    pub fn from_registered_bytes(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, SoftwareTextFontError> {
        Self::from_registered_bytes_with_variations(family, weight, style, bytes, &[])
    }

    /// Register a face with explicit OpenType axis coordinates shared by measurement,
    /// kerning, and rasterization. Coordinates override the declared weight/style axes;
    /// repeated tags use the last value. Unknown, nonfinite, or out-of-range values fail.
    pub fn from_registered_bytes_with_variations(
        family: &FontFamily,
        weight: FontWeight,
        style: FontStyle,
        bytes: impl Into<Vec<u8>>,
        variations: &[([u8; 4], f32)],
    ) -> Result<Self, SoftwareTextFontError> {
        let bytes = bytes.into();
        let mut hasher = default_hash::new();
        bytes.hash(&mut hasher);
        let mut metadata = software_text_font_metadata(bytes.as_slice());
        metadata.registered_family = Some(FontFamilyKey::of(family));
        metadata.weight = weight;
        metadata.style = style;

        let mut font =
            FontVec::try_from_vec(bytes).map_err(|_| SoftwareTextFontError::InvalidFont)?;
        let mut applied = apply_declared_variations(&mut font, weight, style);
        for &(tag, value) in variations {
            let valid = value.is_finite()
                && font.variations().iter().any(|axis| {
                    axis.tag == tag && (axis.min_value..=axis.max_value).contains(&value)
                });
            if !valid || !font.set_variation(&tag, value) {
                return Err(SoftwareTextFontError::InvalidVariation { tag });
            }
            applied.retain(|(existing, _)| *existing != tag);
            applied.push((tag, value));
        }
        applied.sort_unstable_by_key(|(tag, _)| *tag);
        let variations = applied;
        for (tag, value) in &variations {
            tag.hash(&mut hasher);
            value.to_bits().hash(&mut hasher);
        }
        let content_hash = hasher.finish();

        let kerning = KernedFont::read_kerning(font.font_data(), &variations);

        let font = FontArc::from(font);
        let score = text_font_score_from_parts(&font, &metadata);
        Ok(Self {
            font: KernedFont::new(font, kerning),
            metadata,
            score,
            content_hash,
        })
    }

    pub fn family_names(&self) -> &[String] {
        &self.metadata.families
    }

    /// The family an app registered this face under, if any.
    pub fn registered_family(&self) -> Option<FontFamilyKey> {
        self.metadata.registered_family
    }

    pub fn weight(&self) -> FontWeight {
        self.metadata.weight
    }

    pub fn style(&self) -> FontStyle {
        self.metadata.style
    }

    fn ab_glyph_px_size(&self, logical_font_size: f32) -> f32 {
        logical_font_size * self.metadata.ab_glyph_scale_factor
    }

    /// Stable hash of the font binary — cache-key component wherever
    /// rasterized output depends on which font served the run.
    pub fn content_hash(&self) -> u64 {
        self.content_hash
    }

    fn raster_ref(&self) -> RasterFontRef<'_, KernedFont> {
        RasterFontRef {
            font: &self.font,
            ab_glyph_scale_factor: self.metadata.ab_glyph_scale_factor,
            weight: self.weight(),
            style: self.style(),
            tracking: &self.metadata.tracking,
        }
    }
}

pub fn try_default_software_text_font() -> Result<SoftwareTextFont, SoftwareTextFontError> {
    #[cfg(feature = "embedded-default-font")]
    {
        SoftwareTextFont::from_bytes(DEFAULT_SOFTWARE_TEXT_FONT_BYTES.to_vec())
    }
    #[cfg(not(feature = "embedded-default-font"))]
    {
        Err(SoftwareTextFontError::EmbeddedFontDisabled)
    }
}

pub fn default_software_text_font() -> Option<SoftwareTextFont> {
    try_default_software_text_font().ok()
}

#[derive(Clone)]
pub struct SoftwareTextFontSet {
    fonts: Arc<[SoftwareTextFont]>,
    registered_families: Arc<[FontFamilyKey]>,
    default_index: Option<usize>,
}

impl SoftwareTextFontSet {
    pub fn empty() -> Self {
        Self::from_faces(Vec::new())
    }

    pub fn from_font(font: SoftwareTextFont) -> Self {
        Self::from_faces(vec![font])
    }

    /// Build a set from already-parsed faces, keeping the default-face choice
    /// and the registered-family index in one place.
    pub fn from_faces(fonts: Vec<SoftwareTextFont>) -> Self {
        let mut registered_families: Vec<FontFamilyKey> = Vec::new();
        for family in fonts.iter().filter_map(SoftwareTextFont::registered_family) {
            if !registered_families.contains(&family) {
                registered_families.push(family);
            }
        }
        let default_index = (!fonts.is_empty()).then(|| default_font_index(&fonts));
        Self {
            fonts: Arc::from(fonts),
            registered_families: Arc::from(registered_families),
            default_index,
        }
    }

    pub fn from_fonts_or_default(fonts: &[&[u8]]) -> Self {
        let mut parsed = Vec::with_capacity(fonts.len().max(1));
        for font in fonts {
            if let Ok(candidate) = SoftwareTextFont::from_bytes((*font).to_vec()) {
                parsed.push(candidate);
            }
        }
        if parsed.is_empty()
            && let Some(default_font) = default_software_text_font()
        {
            parsed.push(default_font);
        }

        Self::from_faces(parsed)
    }

    pub fn default_font(&self) -> Option<&SoftwareTextFont> {
        self.default_index.and_then(|index| self.fonts.get(index))
    }

    /// Every face in the set, in registration order.
    pub fn faces(&self) -> &[SoftwareTextFont] {
        &self.fonts
    }

    /// Whether any face in the set was registered under `family`.
    pub fn has_registered_family(&self, family: &FontFamily) -> bool {
        self.registered_families
            .contains(&FontFamilyKey::of(family))
    }

    pub fn resolve(&self, style: &TextStyle) -> Option<&SoftwareTextFont> {
        let target_weight = style.span_style.font_weight.unwrap_or_default();
        let target_style = style.span_style.font_style.unwrap_or_default();
        let request = FontFamilyRequest::resolve(
            style.span_style.font_family.as_ref(),
            &self.registered_families,
        );

        let mut best: Option<(usize, u32)> = None;
        for (index, font) in self.fonts.iter().enumerate() {
            let Some(score) = font_match_score(font, target_weight, target_style, request) else {
                continue;
            };
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some((index, score));
            }
        }

        let index = best.map(|(index, _)| index).or(self.default_index);
        index.and_then(|index| self.fonts.get(index))
    }
}

pub fn software_text_font_from_fonts_or_default(fonts: &[&[u8]]) -> Option<SoftwareTextFont> {
    SoftwareTextFontSet::from_fonts_or_default(fonts)
        .default_font()
        .cloned()
}

pub fn software_text_font_set_from_fonts_or_default(fonts: &[&[u8]]) -> SoftwareTextFontSet {
    SoftwareTextFontSet::from_fonts_or_default(fonts)
}

#[derive(Clone, Copy)]
struct TextFontScore {
    supported_latin_chars: usize,
    latin_sample_width: f32,
}

impl TextFontScore {
    fn is_complete_default_face(self) -> bool {
        const LATIN_SAMPLE_CHAR_COUNT: usize = 21;
        self.supported_latin_chars == LATIN_SAMPLE_CHAR_COUNT && self.latin_sample_width > 1.0
    }

    fn is_better_than(self, other: Self) -> bool {
        self.supported_latin_chars > other.supported_latin_chars
            || (self.supported_latin_chars == other.supported_latin_chars
                && self.latin_sample_width > other.latin_sample_width)
    }
}

fn text_font_score(font: &SoftwareTextFont) -> TextFontScore {
    font.score
}

fn text_font_score_from_parts(
    font: &FontArc,
    metadata: &SoftwareTextFontMetadata,
) -> TextFontScore {
    const SAMPLE: &str = "UNDER The quick brown fox";
    let glyph_font_size = 18.0 * metadata.ab_glyph_scale_factor;
    let scaled_font = font.as_scaled(PxScale::from(glyph_font_size));
    let supported_latin_chars = SAMPLE
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .filter(|ch| scaled_font.glyph_id(*ch).0 != 0)
        .count();
    let latin_sample_width = measure_text_impl(
        SAMPLE,
        &TextStyle::default(),
        18.0,
        RasterFontRef {
            font,
            ab_glyph_scale_factor: metadata.ab_glyph_scale_factor,
            style: metadata.style,
            weight: metadata.weight,
            tracking: &metadata.tracking,
        },
    )
    .width;
    TextFontScore {
        supported_latin_chars,
        latin_sample_width,
    }
}

fn default_font_index(fonts: &[SoftwareTextFont]) -> usize {
    let mut best: Option<(usize, TextFontScore)> = None;
    for (index, font) in fonts.iter().enumerate() {
        let score = text_font_score(font);
        if font.style() == FontStyle::Normal
            && font.weight() == FontWeight::NORMAL
            && score.is_complete_default_face()
        {
            return index;
        }
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score.is_better_than(*best_score))
        {
            best = Some((index, score));
        }
    }
    best.map_or(0, |(index, _)| index)
}

#[derive(Clone, Copy)]
enum FontFamilyRequest<'a> {
    Any,
    Named { name: &'a str, key: FontFamilyKey },
    Registered(FontFamilyKey),
}

impl<'a> FontFamilyRequest<'a> {
    fn resolve(font_family: Option<&'a FontFamily>, registered: &[FontFamilyKey]) -> Self {
        match font_family {
            None | Some(FontFamily::Default) => Self::Any,
            Some(FontFamily::Named(name)) => Self::Named {
                name: name.as_str(),
                key: FontFamilyKey::of(&FontFamily::Named(name.clone())),
            },
            Some(family @ (FontFamily::FileBacked(_) | FontFamily::LoadedTypeface(_))) => {
                Self::Registered(FontFamilyKey::of(family))
            }
            Some(family) => {
                let key = FontFamilyKey::of(family);
                if registered.contains(&key) {
                    Self::Registered(key)
                } else {
                    Self::Any
                }
            }
        }
    }

    fn matches(self, font: &SoftwareTextFont) -> bool {
        match self {
            Self::Any => true,
            Self::Named { name, key } => {
                font_family_matches(font, name) || font.registered_family() == Some(key)
            }
            Self::Registered(key) => font.registered_family() == Some(key),
        }
    }
}

fn font_match_score(
    font: &SoftwareTextFont,
    target_weight: FontWeight,
    target_style: FontStyle,
    request: FontFamilyRequest<'_>,
) -> Option<u32> {
    if !request.matches(font) {
        return None;
    }
    let style_penalty = if font.style() == target_style {
        0
    } else {
        10_000
    };
    let weight_penalty = (i32::from(font.weight().0) - i32::from(target_weight.0)).unsigned_abs();
    let coverage_penalty =
        (21usize.saturating_sub(text_font_score(font).supported_latin_chars) as u32) * 1_000;

    Some(style_penalty + weight_penalty + coverage_penalty)
}

fn font_family_matches(font: &SoftwareTextFont, requested: &str) -> bool {
    font.family_names()
        .iter()
        .any(|family| family.eq_ignore_ascii_case(requested))
}

fn apply_declared_variations(
    font: &mut FontVec,
    weight: FontWeight,
    style: FontStyle,
) -> Vec<([u8; 4], f32)> {
    const OBLIQUE_DEGREES: f32 = -12.0;

    let mut applied = Vec::new();
    for axis in font.variations() {
        let requested = match &axis.tag {
            b"wght" => f32::from(weight.value()),
            b"ital" if style == FontStyle::Italic => 1.0,
            b"slnt" if style == FontStyle::Italic => OBLIQUE_DEGREES,
            _ => continue,
        };
        let value = requested.clamp(axis.min_value, axis.max_value);
        if font.set_variation(&axis.tag, value) {
            applied.push((axis.tag, value));
        }
    }
    applied
}

fn software_text_font_metadata(bytes: &[u8]) -> SoftwareTextFontMetadata {
    let Some(face) = ttf_parser::Face::parse(bytes, 0).ok() else {
        return SoftwareTextFontMetadata {
            families: Arc::from(Vec::<String>::new()),
            registered_family: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            ab_glyph_scale_factor: 1.0,
            tracking: FontTracking::default(),
        };
    };

    let mut families = Vec::new();
    for name in face.names() {
        if matches!(
            name.name_id,
            ttf_parser::name_id::TYPOGRAPHIC_FAMILY | ttf_parser::name_id::FAMILY
        ) && let Some(value) = name.to_string().filter(|value| !value.is_empty())
            && !families
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
        {
            families.push(value);
        }
    }
    let weight = FontWeight::try_new(face.weight().to_number()).unwrap_or(FontWeight::NORMAL);
    let style = if face.is_italic() {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };
    let units_per_em = face.units_per_em() as f32;
    let height = (face.ascender() as f32 - face.descender() as f32).abs();
    let ab_glyph_scale_factor =
        if units_per_em.is_finite() && units_per_em > 0.0 && height.is_finite() && height > 0.0 {
            height / units_per_em
        } else {
            1.0
        };

    SoftwareTextFontMetadata {
        families: Arc::from(families),
        registered_family: None,
        weight,
        style,
        ab_glyph_scale_factor,
        tracking: FontTracking::from_face(&face),
    }
}

#[derive(Clone)]
struct TextMetricsKey {
    text: Rc<str>,
    font_size_bits: u32,
    style_hash: u64,
    span_styles_hash: u64,
}

impl PartialEq for TextMetricsKey {
    fn eq(&self, other: &Self) -> bool {
        (Rc::ptr_eq(&self.text, &other.text) || *self.text == *other.text)
            && self.font_size_bits == other.font_size_bits
            && self.style_hash == other.style_hash
            && self.span_styles_hash == other.span_styles_hash
    }
}

impl Eq for TextMetricsKey {}

impl Hash for TextMetricsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.font_size_bits.hash(state);
        self.style_hash.hash(state);
        self.span_styles_hash.hash(state);
    }
}

struct SoftwareTextMetricsCache {
    map: BoundedLruCache<TextMetricsKey, TextMetrics>,
    line_prefix_widths: BoundedLruCache<LinePrefixWidthsKey, TextLinePrefixWidths>,
    glyph_metrics: SoftwareTextGlyphMetricsCache,
}

impl SoftwareTextMetricsCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: BoundedLruCache::with_capacity_at_least_one(capacity),
            line_prefix_widths: BoundedLruCache::with_capacity_at_least_one(
                capacity.max(SOFTWARE_TEXT_PREFIX_WIDTH_CACHE_CAPACITY),
            ),
            glyph_metrics: SoftwareTextGlyphMetricsCache::new(),
        }
    }

    fn get_or_measure(
        &mut self,
        fonts: &SoftwareTextFontSet,
        text: &AnnotatedString,
        style: &TextStyle,
    ) -> TextMetrics {
        let font_size = resolve_font_size(style);
        let key = TextMetricsKey {
            text: Rc::from(text.text.as_str()),
            font_size_bits: font_size.to_bits(),
            style_hash: style.measurement_hash(),
            span_styles_hash: text.span_styles_hash(),
        };
        if let Some(metrics) = self.map.get(&key).copied() {
            return metrics;
        }

        let metrics =
            measure_annotated_text_with_font_set_cached(text, style, font_size, fonts, self);
        self.map.put(key, metrics);
        metrics
    }

    fn get_or_measure_line_prefix_widths(
        &mut self,
        fonts: &SoftwareTextFontSet,
        text: &AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        let key = line_prefix_widths_key(text, line_range.clone(), style)?;
        if let Some(widths) = self.line_prefix_widths.get(&key) {
            return Some(widths.clone());
        }

        let widths = annotated_line_prefix_widths_with_font_set_cached(
            text, line_range, style, fonts, self,
        )?;
        self.line_prefix_widths.put(key, widths.clone());
        Some(widths)
    }

    fn get_or_measure_line_width(
        &mut self,
        fonts: &SoftwareTextFontSet,
        text: &AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        let key = line_prefix_widths_key(text, line_range.clone(), style)?;
        if let Some(widths) = self.line_prefix_widths.get(&key) {
            return widths.width_for_char_range(0, widths.char_count());
        }

        let widths = annotated_line_prefix_widths_with_font_set_cached(
            text, line_range, style, fonts, self,
        )?;
        let width = widths.width_for_char_range(0, widths.char_count());
        self.line_prefix_widths.put(key, widths);
        width
    }
}

#[derive(Clone)]
struct LinePrefixWidthsKey {
    text: Rc<str>,
    start: usize,
    end: usize,
    style_hash: u64,
    span_styles_hash: u64,
}

impl PartialEq for LinePrefixWidthsKey {
    fn eq(&self, other: &Self) -> bool {
        (Rc::ptr_eq(&self.text, &other.text) || *self.text == *other.text)
            && self.start == other.start
            && self.end == other.end
            && self.style_hash == other.style_hash
            && self.span_styles_hash == other.span_styles_hash
    }
}

impl Eq for LinePrefixWidthsKey {}

impl Hash for LinePrefixWidthsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
        self.start.hash(state);
        self.end.hash(state);
        self.style_hash.hash(state);
        self.span_styles_hash.hash(state);
    }
}

fn line_prefix_widths_key(
    text: &AnnotatedString,
    line_range: std::ops::Range<usize>,
    style: &TextStyle,
) -> Option<LinePrefixWidthsKey> {
    if !style_allows_prefix_widths(style)
        || line_range.start > line_range.end
        || line_range.end > text.text.len()
        || !text.text.is_char_boundary(line_range.start)
        || !text.text.is_char_boundary(line_range.end)
        || text.text[line_range.clone()].contains('\n')
    {
        return None;
    }

    Some(LinePrefixWidthsKey {
        text: Rc::from(text.text.as_str()),
        start: line_range.start,
        end: line_range.end,
        style_hash: style.measurement_hash(),
        span_styles_hash: text.span_styles_hash(),
    })
}

#[derive(Clone, Copy, Debug)]
struct CachedGlyphMetrics {
    glyph_id: GlyphId,
    advance_unscaled: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphMetricsKey {
    font_hash: u64,
    ch: char,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct KernMetricsKey {
    font_hash: u64,
    previous_id: u32,
    glyph_id: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SoftwareTextGlyphMetricsStats {
    glyph_hits: u64,
    glyph_misses: u64,
    kern_hits: u64,
    kern_misses: u64,
}

struct SoftwareTextGlyphMetricsCache {
    glyphs: BoundedLruCache<GlyphMetricsKey, CachedGlyphMetrics>,
    kerns: BoundedLruCache<KernMetricsKey, f32>,
    stats: SoftwareTextGlyphMetricsStats,
}

impl SoftwareTextGlyphMetricsCache {
    fn new() -> Self {
        Self {
            glyphs: BoundedLruCache::with_capacity_at_least_one(
                SOFTWARE_TEXT_GLYPH_METRICS_CACHE_CAPACITY,
            ),
            kerns: BoundedLruCache::with_capacity_at_least_one(
                SOFTWARE_TEXT_KERN_METRICS_CACHE_CAPACITY,
            ),
            stats: SoftwareTextGlyphMetricsStats::default(),
        }
    }

    #[cfg(test)]
    fn stats(&self) -> SoftwareTextGlyphMetricsStats {
        self.stats
    }

    fn glyph_metrics<F, S>(
        &mut self,
        font: &SoftwareTextFont,
        scaled_font: &S,
        ch: char,
    ) -> CachedGlyphMetrics
    where
        F: Font,
        S: ScaleFont<F>,
    {
        let key = GlyphMetricsKey {
            font_hash: font.content_hash(),
            ch,
        };
        if let Some(metrics) = self.glyphs.get(&key).copied() {
            self.stats.glyph_hits = self.stats.glyph_hits.saturating_add(1);
            return metrics;
        }

        let glyph_id = scaled_font.font().glyph_id(ch);
        let metrics = CachedGlyphMetrics {
            glyph_id,
            advance_unscaled: scaled_font.font().h_advance_unscaled(glyph_id).max(0.0),
        };
        self.glyphs.put(key, metrics);
        self.stats.glyph_misses = self.stats.glyph_misses.saturating_add(1);
        metrics
    }

    fn kern<F, S>(
        &mut self,
        font: &SoftwareTextFont,
        scaled_font: &S,
        previous_id: GlyphId,
        glyph_id: GlyphId,
    ) -> f32
    where
        F: Font,
        S: ScaleFont<F>,
    {
        let key = KernMetricsKey {
            font_hash: font.content_hash(),
            previous_id: previous_id.0.into(),
            glyph_id: glyph_id.0.into(),
        };
        if let Some(kern) = self.kerns.get(&key).copied() {
            self.stats.kern_hits = self.stats.kern_hits.saturating_add(1);
            return kern;
        }

        let kern = scaled_font.font().kern_unscaled(previous_id, glyph_id);
        self.kerns.put(key, kern);
        self.stats.kern_misses = self.stats.kern_misses.saturating_add(1);
        kern
    }
}

pub struct SoftwareTextMeasurer {
    fonts: SoftwareTextFontSet,
    cache: Mutex<SoftwareTextMetricsCache>,
    hyphenation: HyphenationDictionaryStore,
}

impl SoftwareTextMeasurer {
    pub fn new(font: SoftwareTextFont, cache_capacity: usize) -> Self {
        Self::from_font_set(SoftwareTextFontSet::from_font(font), cache_capacity)
    }

    pub fn from_font_set(fonts: SoftwareTextFontSet, cache_capacity: usize) -> Self {
        Self {
            fonts,
            cache: Mutex::new(SoftwareTextMetricsCache::new(cache_capacity)),
            hyphenation: HyphenationDictionaryStore::new(),
        }
    }

    pub fn from_fonts_or_default(fonts: &[&[u8]], cache_capacity: usize) -> Self {
        Self::from_font_set(
            software_text_font_set_from_fonts_or_default(fonts),
            cache_capacity,
        )
    }

    fn lock_cache(&self) -> MutexGuard<'_, SoftwareTextMetricsCache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(feature = "text-hyphenation")]
    pub fn register_hyphenation_dictionary_path(
        &self,
        locale: &str,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), HyphenationDictionaryError> {
        self.hyphenation.register_dictionary_path(locale, path)
    }

    #[cfg(feature = "text-hyphenation")]
    pub fn register_hyphenation_dictionary_reader(
        &self,
        locale: &str,
        reader: &mut impl std::io::Read,
    ) -> Result<(), HyphenationDictionaryError> {
        self.hyphenation.register_dictionary_reader(locale, reader)
    }
}

impl TextMeasurer for SoftwareTextMeasurer {
    fn measure(&self, text: &cranpose_ui::text::AnnotatedString, style: &TextStyle) -> TextMetrics {
        self.lock_cache().get_or_measure(&self.fonts, text, style)
    }

    fn measure_subsequence(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        let text = text.subsequence(range);
        self.lock_cache().get_or_measure(&self.fonts, &text, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        self.lock_cache()
            .get_or_measure_line_prefix_widths(&self.fonts, text, line_range, style)
    }

    fn measure_line_width(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        self.lock_cache()
            .get_or_measure_line_width(&self.fonts, text, line_range, style)
    }

    fn line_height(&self, text: &cranpose_ui::text::AnnotatedString, style: &TextStyle) -> f32 {
        let font_size = resolve_font_size(style);
        max_line_height_for_annotated_text_with_resolver(text, style, font_size, &self.fonts)
    }

    fn glyph_line_box(&self, style: &TextStyle) -> Option<(f32, f32)> {
        let font = self.fonts.resolve(style)?;
        let font_size = resolve_font_size(style);
        let metrics = crate::font_layout::vertical_metrics(&font.font, font_size);
        let asked = line_height_for_render_style(style, font_size);
        let resolved = line_box_for(style, metrics, asked, measure_grid());
        let height = metrics.natural_line_height.min(resolved.height).max(1.0);
        Some((((resolved.height - height) * 0.5).max(0.0), height))
    }

    fn first_baseline(&self, style: &TextStyle) -> Option<f32> {
        Some(self.line_box(style)?.baseline)
    }

    fn line_box(&self, style: &TextStyle) -> Option<cranpose_ui::text::LineBox> {
        let font = self.fonts.resolve(style)?;
        let font_size = resolve_font_size(style);
        let metrics =
            crate::font_layout::vertical_metrics(&font.font, font.ab_glyph_px_size(font_size));
        Some(line_box_for(
            style,
            metrics,
            line_height_for_render_style(style, font_size),
            measure_grid(),
        ))
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        if let Some(font) = self.fonts.resolve(style) {
            text_offset_for_position_with_font(text.text.as_str(), style, x, y, font)
        } else {
            fallback_text_offset_for_position(text.text.as_str(), style, x, y)
        }
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        if let Some(font) = self.fonts.resolve(style) {
            cursor_x_for_offset_with_font(text.text.as_str(), style, offset, font)
        } else {
            fallback_cursor_x_for_offset(text.text.as_str(), style, offset)
        }
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextLayoutResult {
        if let Some(font) = self.fonts.resolve(style) {
            layout_text_with_font(text.text.as_str(), style, font)
        } else {
            fallback_layout_text(text.text.as_str(), style)
        }
    }

    fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        self.hyphenation.choose_auto_hyphen_break(
            line,
            style,
            segment_start_char,
            measured_break_char,
        )
    }
}

pub fn software_text_content_hash(text: &cranpose_ui::text::AnnotatedString) -> u64 {
    let mut state = default_hash::new();
    text.text.hash(&mut state);
    text.span_styles_hash().hash(&mut state);
    state.finish()
}

#[derive(Clone, Copy)]
enum GlyphRasterStyle {
    Fill,
    Stroke { width_px: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoftwareGlyphAtlasKey {
    pub font_hash: u64,
    pub glyph_id: u32,
    pub scale_x_bits: u32,
    pub scale_y_bits: u32,
    pub embolden_px_bits: u32,
    pub slant_bits: u32,
}

#[derive(Clone)]
pub struct SoftwareGlyphAtlasMask {
    pub alpha: Arc<[f32]>,
    pub width: usize,
    pub height: usize,
}

#[derive(Clone)]
pub struct SoftwareGlyphAtlasGlyph {
    pub key: SoftwareGlyphAtlasKey,
    pub mask: SoftwareGlyphAtlasMask,
    pub x: i32,
    pub y: i32,
    pub color: Color,
}

#[derive(Clone, Copy)]
pub struct SoftwareGlyphAtlasPlacement {
    pub key: SoftwareGlyphAtlasKey,
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    pub color: Color,
}

#[derive(Clone)]
pub enum SoftwareGlyphAtlasRunGlyph {
    Cached(SoftwareGlyphAtlasPlacement),
    New(SoftwareGlyphAtlasGlyph),
}

impl SoftwareGlyphAtlasRunGlyph {
    pub fn placement(&self) -> SoftwareGlyphAtlasPlacement {
        match self {
            Self::Cached(placement) => *placement,
            Self::New(glyph) => SoftwareGlyphAtlasPlacement {
                key: glyph.key,
                x: glyph.x,
                y: glyph.y,
                width: glyph.mask.width,
                height: glyph.mask.height,
                color: glyph.color,
            },
        }
    }
}

#[derive(Clone)]
struct GlyphMask {
    alpha: Arc<[f32]>,
    width: usize,
    height: usize,
    origin_x: i32,
    origin_y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoftwareGlyphRasterCacheStats {
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
}

const RUN_GLYPH_METRICS_CACHE_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GlyphRasterStyleKey {
    Fill,
    Stroke { width_px_bits: u32 },
}

impl GlyphRasterStyleKey {
    fn from_style(style: GlyphRasterStyle) -> Self {
        match style {
            GlyphRasterStyle::Fill => Self::Fill,
            GlyphRasterStyle::Stroke { width_px } => Self::Stroke {
                width_px_bits: width_px.to_bits(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphMaskCacheKey {
    font_hash: u64,
    glyph_id: u32,
    scale_x_bits: u32,
    scale_y_bits: u32,
    raster_style: GlyphRasterStyleKey,
    embolden_px_bits: u32,
    slant_bits: u32,
}

#[derive(Clone)]
struct CachedGlyphMask {
    alpha: Arc<[f32]>,
    width: usize,
    height: usize,
    origin_offset_x: i32,
    origin_offset_y: i32,
}

impl CachedGlyphMask {
    fn from_mask(mask: GlyphMask, glyph: &Glyph) -> Self {
        let (glyph_x, glyph_y) = static_glyph_pixel_origin(glyph);
        Self {
            alpha: mask.alpha,
            width: mask.width,
            height: mask.height,
            origin_offset_x: mask.origin_x - glyph_x,
            origin_offset_y: mask.origin_y - glyph_y,
        }
    }

    fn instantiate(&self, glyph: &Glyph) -> GlyphMask {
        let (glyph_x, glyph_y) = static_glyph_pixel_origin(glyph);
        GlyphMask {
            alpha: Arc::clone(&self.alpha),
            width: self.width,
            height: self.height,
            origin_x: glyph_x + self.origin_offset_x,
            origin_y: glyph_y + self.origin_offset_y,
        }
    }

    fn placement(&self, glyph: &Glyph) -> (i32, i32, usize, usize) {
        let (glyph_x, glyph_y) = static_glyph_pixel_origin(glyph);
        (
            glyph_x + self.origin_offset_x,
            glyph_y + self.origin_offset_y,
            self.width,
            self.height,
        )
    }

    fn atlas_metrics(&self, key: SoftwareGlyphAtlasKey) -> CachedAtlasGlyphMetrics {
        CachedAtlasGlyphMetrics {
            key,
            width: self.width,
            height: self.height,
            origin_offset_x: self.origin_offset_x,
            origin_offset_y: self.origin_offset_y,
        }
    }
}

#[derive(Clone, Copy)]
struct CachedAtlasGlyphMetrics {
    key: SoftwareGlyphAtlasKey,
    width: usize,
    height: usize,
    origin_offset_x: i32,
    origin_offset_y: i32,
}

impl CachedAtlasGlyphMetrics {
    fn placement(self, glyph: &Glyph, color: Color) -> SoftwareGlyphAtlasPlacement {
        let (glyph_x, glyph_y) = static_glyph_pixel_origin(glyph);
        SoftwareGlyphAtlasPlacement {
            key: self.key,
            x: glyph_x + self.origin_offset_x,
            y: glyph_y + self.origin_offset_y,
            width: self.width,
            height: self.height,
            color,
        }
    }
}

pub struct SoftwareGlyphRasterCache {
    masks: BoundedLruCache<GlyphMaskCacheKey, CachedGlyphMask>,
    hits: u64,
    misses: u64,
}

impl SoftwareGlyphRasterCache {
    pub fn with_capacity_at_least_one(capacity: usize) -> Self {
        Self {
            masks: BoundedLruCache::with_capacity_at_least_one(capacity),
            hits: 0,
            misses: 0,
        }
    }

    pub fn stats(&self) -> SoftwareGlyphRasterCacheStats {
        SoftwareGlyphRasterCacheStats {
            entries: self.masks.len(),
            hits: self.hits,
            misses: self.misses,
        }
    }

    fn get(&mut self, key: &GlyphMaskCacheKey, glyph: &Glyph) -> Option<GlyphMask> {
        let mask = self.masks.get(key)?.instantiate(glyph);
        self.hits = self.hits.saturating_add(1);
        Some(mask)
    }

    fn get_atlas_placement(
        &mut self,
        key: &GlyphMaskCacheKey,
        glyph: &Glyph,
    ) -> Option<(SoftwareGlyphAtlasKey, i32, i32, usize, usize)> {
        let atlas_key = glyph_atlas_key_from_mask_key(*key)?;
        let (x, y, width, height) = self.masks.get(key)?.placement(glyph);
        self.hits = self.hits.saturating_add(1);
        Some((atlas_key, x, y, width, height))
    }

    fn get_atlas_metrics(&mut self, key: &GlyphMaskCacheKey) -> Option<CachedAtlasGlyphMetrics> {
        let atlas_key = glyph_atlas_key_from_mask_key(*key)?;
        let metrics = self.masks.get(key)?.atlas_metrics(atlas_key);
        self.hits = self.hits.saturating_add(1);
        Some(metrics)
    }

    pub fn atlas_glyph_for_placement(
        &mut self,
        placement: &SoftwareGlyphAtlasPlacement,
    ) -> Option<SoftwareGlyphAtlasGlyph> {
        let key = GlyphMaskCacheKey {
            font_hash: placement.key.font_hash,
            glyph_id: placement.key.glyph_id,
            scale_x_bits: placement.key.scale_x_bits,
            scale_y_bits: placement.key.scale_y_bits,
            raster_style: GlyphRasterStyleKey::Fill,
            embolden_px_bits: placement.key.embolden_px_bits,
            slant_bits: placement.key.slant_bits,
        };
        let mask = self.masks.get(&key)?;
        self.hits = self.hits.saturating_add(1);
        Some(SoftwareGlyphAtlasGlyph {
            key: placement.key,
            mask: SoftwareGlyphAtlasMask {
                alpha: Arc::clone(&mask.alpha),
                width: mask.width,
                height: mask.height,
            },
            x: placement.x,
            y: placement.y,
            color: placement.color,
        })
    }

    fn put(&mut self, key: GlyphMaskCacheKey, glyph: &Glyph, mask: GlyphMask) -> GlyphMask {
        let cached = CachedGlyphMask::from_mask(mask, glyph);
        let mask = cached.instantiate(glyph);
        self.masks.put(key, cached);
        self.misses = self.misses.saturating_add(1);
        mask
    }
}

struct RasterFontRef<'a, F> {
    font: &'a F,
    ab_glyph_scale_factor: f32,
    weight: FontWeight,
    style: FontStyle,
    tracking: &'a FontTracking,
}

#[derive(Clone, Copy)]
struct TextWeightSynthesis {
    embolden_px: f32,
    advance_scale: f32,
}

impl TextWeightSynthesis {
    fn none() -> Self {
        Self {
            embolden_px: 0.0,
            advance_scale: 1.0,
        }
    }

    const FAKE_BOLD_MIN_WEIGHT: u16 = 600;
    const FAKE_BOLD_MIN_DELTA: u16 = 200;

    fn for_style(
        style: &TextStyle,
        resolved_weight: FontWeight,
        font_size: f32,
        scale: f32,
    ) -> Self {
        let requested_weight = style.span_style.font_weight.unwrap_or_default();
        if requested_weight <= resolved_weight {
            return Self::none();
        }
        if requested_weight.value() < Self::FAKE_BOLD_MIN_WEIGHT
            || requested_weight.value() - resolved_weight.value() < Self::FAKE_BOLD_MIN_DELTA
        {
            return Self::none();
        }

        let synthesis = style
            .span_style
            .font_synthesis
            .unwrap_or(FontSynthesis::All);
        if !matches!(synthesis, FontSynthesis::All | FontSynthesis::Weight) {
            return Self::none();
        }

        let weight_delta = (requested_weight.value() - resolved_weight.value()) as f32;
        let strength = (weight_delta / 300.0).clamp(0.0, 1.5);
        Self {
            embolden_px: (font_size * scale * 0.055 * strength).clamp(0.0, 3.0 * scale),
            advance_scale: 1.0 + 0.085 * strength.min(1.0),
        }
    }

    fn apply_width(self, width: f32) -> f32 {
        width * self.advance_scale
    }
}

#[derive(Clone, Copy)]
struct TextStyleSynthesis {
    slant: f32,
    font_size: f32,
    scale: f32,
}

impl TextStyleSynthesis {
    fn none() -> Self {
        Self {
            slant: 0.0,
            font_size: 0.0,
            scale: 1.0,
        }
    }

    fn for_style(style: &TextStyle, resolved_style: FontStyle, font_size: f32, scale: f32) -> Self {
        let requested_style = style.span_style.font_style.unwrap_or_default();
        if requested_style != FontStyle::Italic || resolved_style == FontStyle::Italic {
            return Self::none();
        }

        let synthesis = style
            .span_style
            .font_synthesis
            .unwrap_or(FontSynthesis::All);
        if !matches!(synthesis, FontSynthesis::All | FontSynthesis::Style) {
            return Self::none();
        }

        Self {
            slant: 0.22,
            font_size,
            scale,
        }
    }

    fn visual_overhang_px(self) -> f32 {
        if self.slant <= 0.0 || !self.font_size.is_finite() || !self.scale.is_finite() {
            return 0.0;
        }
        (self.font_size * self.scale * self.slant).ceil().max(0.0)
    }
}

pub fn rasterize_text_to_image(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
) -> Option<ImageBitmap> {
    rasterize_text_to_image_impl(
        TextRasterImageRequest {
            text,
            rect,
            style,
            fallback_color,
            font_size,
            scale,
        },
        font.raster_ref(),
        font.content_hash(),
        None,
    )
}

#[expect(clippy::too_many_arguments)]
pub fn rasterize_text_to_image_with_glyph_cache(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: &mut SoftwareGlyphRasterCache,
) -> Option<ImageBitmap> {
    rasterize_text_to_image_impl(
        TextRasterImageRequest {
            text,
            rect,
            style,
            fallback_color,
            font_size,
            scale,
        },
        font.raster_ref(),
        font.content_hash(),
        Some(glyph_cache),
    )
}

/// The slice of a text payload that solid-run rasterization reads: content
/// plus span styles. Borrowable from both an [`AnnotatedString`] (UI-side
/// text) and a [`RenderString`] (a lowered scene's link-handler-free view),
/// so the run collectors serve both without copying either.
#[derive(Clone, Copy)]
pub struct StyledTextRef<'a> {
    pub text: &'a str,
    pub span_styles: &'a [RangeStyle<SpanStyle>],
}

impl StyledTextRef<'_> {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn span_boundaries(&self) -> Vec<usize> {
        let mut boundaries = vec![0, self.text.len()];
        for span in self.span_styles {
            boundaries.push(span.range.start);
            boundaries.push(span.range.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries
            .into_iter()
            .filter(|&b| b <= self.text.len() && self.text.is_char_boundary(b))
            .collect()
    }
}

impl<'a> From<&'a AnnotatedString> for StyledTextRef<'a> {
    fn from(text: &'a AnnotatedString) -> Self {
        Self {
            text: text.text.as_str(),
            span_styles: &text.span_styles,
        }
    }
}

impl<'a> From<&'a RenderString> for StyledTextRef<'a> {
    fn from(text: &'a RenderString) -> Self {
        Self {
            text: text.text.as_str(),
            span_styles: &text.span_styles,
        }
    }
}

fn annotated_line_alignment_offsets(
    text: &StyledTextRef<'_>,
    style: &TextStyle,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
) -> Option<Vec<f32>> {
    let align_fraction = cranpose_ui::text::text_align_fraction(style, text.text);
    if align_fraction == 0.0 || !text.text.contains('\n') {
        return None;
    }

    let mut advances = vec![0.0f32];
    for range in annotated_segment_boundaries(text).windows(2) {
        let (start, end) = (range[0], range[1]);
        if start == end {
            continue;
        }
        let segment_style = effective_style_for_range(text.span_styles, style, start, end);
        for part in text.text[start..end].split_inclusive('\n') {
            let has_newline = part.ends_with('\n');
            let content = if has_newline {
                &part[..part.len().saturating_sub(1)]
            } else {
                part
            };
            if !content.is_empty() {
                let segment_font_size = segment_style.resolve_font_size(font_size);
                let font = fonts.resolve(&segment_style)?;
                let font_px_size = font.ab_glyph_px_size(segment_font_size) * scale;
                let letter_spacing = font
                    .metadata
                    .tracking
                    .resolve(&segment_style, segment_font_size)
                    * scale;
                if let Some(last) = advances.last_mut() {
                    *last += segment_advance_px(&font.font, content, font_px_size, letter_spacing);
                }
            }
            if has_newline {
                advances.push(0.0);
            }
        }
    }

    let block = advances.iter().copied().fold(0.0f32, f32::max);
    Some(
        advances
            .iter()
            .map(|advance| ((block - advance) * align_fraction).max(0.0))
            .collect(),
    )
}

fn annotated_segment_boundaries(text: &StyledTextRef<'_>) -> Vec<usize> {
    let mut boundaries = text.span_boundaries();
    for (offset, ch) in text.text.char_indices() {
        if ch == '\n' {
            boundaries.push(offset);
            boundaries.push(offset + ch.len_utf8());
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.retain(|offset| *offset <= text.text.len() && text.text.is_char_boundary(*offset));
    boundaries
}

fn segment_advance_px(
    font: &impl Font,
    content: &str,
    font_px_size: f32,
    letter_spacing: f32,
) -> f32 {
    let scaled_font = font.as_scaled(PxScale::from(font_px_size));
    let mut caret = 0.0f32;
    let mut previous = None;
    for ch in content.chars() {
        let glyph_id = scaled_font.glyph_id(ch);
        if let Some(previous_id) = previous {
            caret += scaled_font.kern(previous_id, glyph_id);
        }
        caret += letter_spacing + scaled_font.h_advance(glyph_id);
        previous = Some(glyph_id);
    }
    caret.max(0.0)
}

fn text_render_request_is_degenerate(
    text_is_empty: bool,
    rect: Rect,
    font_size: f32,
    scale: f32,
) -> bool {
    text_is_empty
        || rect.width <= 0.0
        || rect.height <= 0.0
        || !font_size.is_finite()
        || font_size <= 0.0
        || !scale.is_finite()
        || scale <= 0.0
}

#[derive(Clone, Copy)]
struct TextSegmentMetrics {
    font_px_size: f32,
    letter_spacing: f32,
    align_fraction: f32,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    line_height: f32,
    first_baseline_y: f32,
}

fn text_segment_metrics(
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
) -> TextSegmentMetrics {
    let font_px_size = font.ab_glyph_px_size(font_size) * scale;
    let letter_spacing = font.metadata.tracking.resolve(style, font_size) * scale;
    let align_fraction = cranpose_ui::text::text_align_fraction(style, text);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font.weight(), font_size, scale);
    let style_synthesis = TextStyleSynthesis::for_style(style, font.style(), font_size, scale);
    let metrics = vertical_metrics(&font.font, font_px_size);
    let line_box = line_box_for(
        style,
        metrics,
        (style.resolve_line_height(14.0, font_size * 1.4) * scale).max(1.0),
        1.0,
    );
    TextSegmentMetrics {
        font_px_size,
        letter_spacing,
        align_fraction,
        weight_synthesis,
        style_synthesis,
        line_height: line_box.height,
        first_baseline_y: local_rect.y + line_box.baseline,
    }
}

fn text_segment_supports_solid_atlas(style: &TextStyle) -> bool {
    style_can_atlas_solid_fill(style)
        && style
            .paragraph_style
            .text_motion
            .unwrap_or(TextMotion::Static)
            == TextMotion::Static
}

#[expect(clippy::too_many_arguments)]
pub fn rasterize_annotated_text_to_image_with_glyph_cache<'a>(
    text: impl Into<StyledTextRef<'a>>,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
) -> Option<ImageBitmap> {
    let text: StyledTextRef<'a> = text.into();
    if text.span_styles.is_empty() {
        let font = fonts.resolve(style)?;
        return rasterize_text_to_image_with_glyph_cache(
            text.text,
            rect,
            style,
            fallback_color,
            font_size,
            scale,
            font,
            glyph_cache,
        );
    }
    if text_render_request_is_degenerate(text.is_empty(), rect, font_size, scale) {
        return None;
    }

    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;
    let boundaries = text.span_boundaries();
    let mut segment_plan = Vec::with_capacity(boundaries.len().saturating_sub(1));
    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }
        let segment_style = effective_style_for_range(text.span_styles, style, start, end);
        if !style_can_rasterize_direct_solid(&segment_style) {
            return None;
        }
        let static_text_motion = segment_style
            .paragraph_style
            .text_motion
            .unwrap_or(TextMotion::Static)
            == TextMotion::Static;
        if !static_text_motion {
            return None;
        }
        segment_plan.push((start, end, segment_style));
    }

    let mut canvas = vec![0_u8; (width as usize) * (height as usize) * 4];
    let base_line_height = line_height_for_render_style(style, font_size);
    let mut current_line_height = base_line_height;
    let line_offsets = annotated_line_alignment_offsets(&text, style, font_size, scale, fonts);
    let mut line_idx = 0usize;
    let mut cursor_x = rect.x + line_offset(&line_offsets, 0);
    let mut cursor_y = rect.y;

    for (start, end, segment_style) in segment_plan {
        let segment = &text.text[start..end];
        for part in segment.split_inclusive('\n') {
            let has_newline = part.ends_with('\n');
            let content = if has_newline {
                &part[..part.len().saturating_sub(1)]
            } else {
                part
            };

            if !content.is_empty() {
                let segment_font_size = segment_style.resolve_font_size(font_size);
                if let Some(font) = fonts.resolve(&segment_style) {
                    let local_rect = Rect {
                        x: (cursor_x - rect.x).round(),
                        y: (cursor_y - rect.y).round(),
                        width: width as f32,
                        height: height as f32,
                    };
                    let color = segment_style.resolve_text_color(fallback_color);
                    let advance_px = draw_text_segment_solid_to_rgba(
                        &mut canvas,
                        width,
                        height,
                        content,
                        local_rect,
                        &segment_style,
                        color,
                        segment_font_size,
                        scale,
                        font,
                        glyph_cache,
                    );
                    cursor_x += advance_px;
                    current_line_height = current_line_height.max(line_height_for_render_style(
                        &segment_style,
                        segment_font_size,
                    ));
                }
            }

            if has_newline {
                line_idx += 1;
                cursor_x = rect.x + line_offset(&line_offsets, line_idx);
                cursor_y += current_line_height * scale;
                current_line_height = base_line_height;
            }
        }
    }

    ImageBitmap::from_rgba8(width, height, canvas).ok()
}

#[expect(clippy::too_many_arguments)]
fn walk_solid_text_atlas_segments<'a, T>(
    text: impl Into<StyledTextRef<'a>>,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<T>,
    mut collect_segment: impl FnMut(
        &str,
        Rect,
        &TextStyle,
        Color,
        f32,
        f32,
        &SoftwareTextFont,
        &mut SoftwareGlyphRasterCache,
        &mut Vec<T>,
    ) -> Option<f32>,
) -> Option<()> {
    let text: StyledTextRef<'a> = text.into();
    if text_render_request_is_degenerate(text.is_empty(), rect, font_size, scale) {
        return Some(());
    }

    let base_line_height = line_height_for_render_style(style, font_size);
    let mut current_line_height = base_line_height;
    let line_offsets = annotated_line_alignment_offsets(&text, style, font_size, scale, fonts);
    let mut line_idx = 0usize;
    let mut cursor_x = rect.x + line_offset(&line_offsets, 0);
    let mut cursor_y = rect.y;
    let initial_len = out.len();

    let boundaries = annotated_segment_boundaries(&text);

    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let segment_style = effective_style_for_range(text.span_styles, style, start, end);
        if !text_segment_supports_solid_atlas(&segment_style) {
            out.truncate(initial_len);
            return None;
        }

        let segment = &text.text[start..end];
        for part in segment.split_inclusive('\n') {
            let has_newline = part.ends_with('\n');
            let content = if has_newline {
                &part[..part.len().saturating_sub(1)]
            } else {
                part
            };

            if !content.is_empty() {
                let segment_font_size = segment_style.resolve_font_size(font_size);
                let Some(font) = fonts.resolve(&segment_style) else {
                    out.truncate(initial_len);
                    return None;
                };
                let local_rect = Rect {
                    x: (cursor_x - rect.x).round(),
                    y: (cursor_y - rect.y).round(),
                    width: rect.width,
                    height: rect.height,
                };
                let color = segment_style.resolve_text_color(fallback_color);
                let Some(advance_px) = collect_segment(
                    content,
                    local_rect,
                    &segment_style,
                    color,
                    segment_font_size,
                    scale,
                    font,
                    glyph_cache,
                    out,
                ) else {
                    out.truncate(initial_len);
                    return None;
                };
                cursor_x += advance_px;
                current_line_height = current_line_height.max(line_height_for_render_style(
                    &segment_style,
                    segment_font_size,
                ));
            }

            if has_newline {
                line_idx += 1;
                cursor_x = rect.x + line_offset(&line_offsets, line_idx);
                cursor_y += current_line_height * scale;
                current_line_height = base_line_height;
            }
        }
    }

    Some(())
}

#[expect(clippy::too_many_arguments)]
pub fn collect_solid_text_atlas_glyphs(
    text: &AnnotatedString,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasGlyph>,
) -> Option<()> {
    walk_solid_text_atlas_segments(
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
        fonts,
        glyph_cache,
        out,
        collect_text_segment_solid_atlas_glyphs,
    )
}

#[expect(clippy::too_many_arguments)]
pub fn collect_cached_solid_text_atlas_placements(
    text: &AnnotatedString,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasPlacement>,
) -> Option<()> {
    walk_solid_text_atlas_segments(
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
        fonts,
        glyph_cache,
        out,
        collect_text_segment_cached_solid_atlas_placements,
    )
}

#[expect(clippy::too_many_arguments)]
pub fn collect_solid_text_atlas_run<'a>(
    text: impl Into<StyledTextRef<'a>>,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasRunGlyph>,
) -> Option<()> {
    walk_solid_text_atlas_segments(
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
        fonts,
        glyph_cache,
        out,
        collect_text_segment_solid_atlas_run,
    )
}

pub fn measure_text_with_font(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
) -> TextMetrics {
    measure_text_impl(text, style, font_size, font.raster_ref())
}

fn measure_text_with_font_cached(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
    cache: &mut SoftwareTextMetricsCache,
) -> TextMetrics {
    measure_text_impl_cached(text, style, font_size, font, cache)
}

pub fn measure_annotated_text_with_font(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
) -> TextMetrics {
    if text.span_styles.is_empty() {
        return measure_text_with_font(text.text.as_str(), style, font_size, font);
    }
    measure_annotated_text_with_resolver(
        text,
        style,
        font_size,
        &SoftwareTextFontSet::from_font(font.clone()),
        None,
    )
}

pub fn measure_annotated_text_with_font_set(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
) -> TextMetrics {
    if text.span_styles.is_empty() {
        if let Some(font) = fonts.resolve(style) {
            return measure_text_with_font(text.text.as_str(), style, font_size, font);
        }
        return fallback_text_metrics(text.text.as_str(), style, font_size);
    }
    measure_annotated_text_with_resolver(text, style, font_size, fonts, None)
}

fn measure_annotated_text_with_font_set_cached(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
    cache: &mut SoftwareTextMetricsCache,
) -> TextMetrics {
    if text.span_styles.is_empty() {
        if let Some(font) = fonts.resolve(style) {
            return measure_text_with_font_cached(
                text.text.as_str(),
                style,
                font_size,
                font,
                cache,
            );
        }
        return fallback_text_metrics(text.text.as_str(), style, font_size);
    }
    measure_annotated_text_with_resolver(text, style, font_size, fonts, Some(cache))
}

pub fn text_offset_for_position_with_font(
    text: &str,
    style: &TextStyle,
    x: f32,
    y: f32,
    font: &SoftwareTextFont,
) -> usize {
    if text.is_empty() {
        return 0;
    }

    let font_size = resolve_font_size(style);
    let line_height = resolve_line_height(style, font_size * 1.4);

    let line_index = (y / line_height).floor().max(0.0) as usize;
    let lines: Vec<&str> = text.split('\n').collect();
    let target_line = line_index.min(lines.len().saturating_sub(1));

    let mut line_start_byte = 0;
    for line in lines.iter().take(target_line) {
        line_start_byte += line.len() + 1;
    }

    let line_text = lines.get(target_line).unwrap_or(&"");
    if line_text.is_empty() {
        return line_start_byte;
    }

    let mut best_offset = 0;
    let mut best_distance = f32::INFINITY;
    let mut current_byte_offset = 0;

    for c in line_text.chars() {
        let prefix = &line_text[..current_byte_offset];
        let glyph_x = measure_text_impl(prefix, style, font_size, font.raster_ref()).width;

        let char_str = &line_text[current_byte_offset..current_byte_offset + c.len_utf8()];
        let char_width = measure_text_impl(char_str, style, font_size, font.raster_ref())
            .width
            .max(font_size * 0.5);

        let left_dist = (x - glyph_x).abs();
        if left_dist < best_distance {
            best_distance = left_dist;
            best_offset = current_byte_offset;
        }

        let right_x = glyph_x + char_width;
        let right_dist = (x - right_x).abs();
        if right_dist < best_distance {
            best_distance = right_dist;
            best_offset = current_byte_offset + c.len_utf8();
        }

        current_byte_offset += c.len_utf8();
    }

    let total_width = measure_text_impl(line_text, style, font_size, font.raster_ref()).width;
    let end_dist = (x - total_width).abs();
    if end_dist < best_distance {
        best_offset = line_text.len();
    }

    line_start_byte + best_offset.min(line_text.len())
}

pub fn cursor_x_for_offset_with_font(
    text: &str,
    style: &TextStyle,
    offset: usize,
    font: &SoftwareTextFont,
) -> f32 {
    let clamped_offset = clamp_to_char_boundary(text, offset.min(text.len()));
    if clamped_offset == 0 {
        return 0.0;
    }

    let font_size = resolve_font_size(style);
    measure_text_impl(&text[..clamped_offset], style, font_size, font.raster_ref()).width
}

pub fn layout_text_with_font(
    text: &str,
    style: &TextStyle,
    font: &SoftwareTextFont,
) -> TextLayoutResult {
    let font_size = resolve_font_size(style);
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let resolved_weight = font.weight();
    let font_ref = font.raster_ref();
    let letter_spacing = font.metadata.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, resolved_weight, font_size, 1.0);
    let font = &font.font;
    let line_height = resolve_line_height(style, font_size * 1.4);
    let scaled_font = font.as_scaled(PxScale::from(glyph_font_size));

    let mut glyph_x_positions = Vec::new();
    let mut char_to_byte = Vec::new();
    let mut glyph_layouts = Vec::new();
    let mut lines = Vec::new();
    let mut current_x = 0.0f32;
    let mut line_start = 0;
    let mut y = 0.0f32;

    let mut iter = text.char_indices().peekable();
    while let Some((byte_offset, c)) = iter.next() {
        glyph_x_positions.push(current_x);
        char_to_byte.push(byte_offset);

        if c == '\n' {
            lines.push(LineLayout {
                start_offset: line_start,
                end_offset: byte_offset,
                y,
                height: line_height,
            });
            line_start = byte_offset + 1;
            y += line_height;
            current_x = 0.0;
        } else {
            let glyph_id = scaled_font.glyph_id(c);
            let glyph_width =
                weight_synthesis.apply_width(scaled_font.h_advance(glyph_id).max(0.0));
            let glyph_end = byte_offset + c.len_utf8();
            if glyph_end > byte_offset {
                glyph_layouts.push(GlyphLayout {
                    line_index: lines.len(),
                    start_offset: byte_offset,
                    end_offset: glyph_end,
                    x: current_x,
                    y,
                    width: glyph_width,
                    height: line_height,
                });
            }
            current_x += glyph_width;
            if let Some((_, next)) = iter.peek()
                && *next != '\n'
            {
                current_x += letter_spacing;
            }
        }
    }

    glyph_x_positions.push(current_x);
    char_to_byte.push(text.len());

    lines.push(LineLayout {
        start_offset: line_start,
        end_offset: text.len(),
        y,
        height: line_height,
    });

    let metrics = measure_text_impl(text, style, font_size, font_ref);
    TextLayoutResult::new(
        text,
        TextLayoutData {
            width: metrics.width,
            height: metrics.height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            glyph_layouts,
        },
    )
}

/// Rasterize a raw font at its ab_glyph pixel scale with explicit style spacing.
/// Use [`rasterize_text_to_image`] for registered font sizing and automatic tracking.
pub fn rasterize_text_to_image_with_font(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    font: &impl Font,
) -> Option<ImageBitmap> {
    rasterize_text_to_image_impl(
        TextRasterImageRequest {
            text,
            rect,
            style,
            fallback_color,
            font_size,
            scale,
        },
        RasterFontRef {
            font,
            ab_glyph_scale_factor: 1.0,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            tracking: &FontTracking::default(),
        },
        0,
        None,
    )
}

struct TextRasterImageRequest<'a> {
    text: &'a str,
    rect: Rect,
    style: &'a TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
}

fn rasterize_text_to_image_impl(
    request: TextRasterImageRequest<'_>,
    font_ref: RasterFontRef<'_, impl Font>,
    font_cache_key: u64,
    mut glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
) -> Option<ImageBitmap> {
    let TextRasterImageRequest {
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
    } = request;

    if text_render_request_is_degenerate(text.is_empty(), rect, font_size, scale) {
        return None;
    }

    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;

    let fallback_brush = Brush::solid(fallback_color);
    let (brush, brush_alpha_multiplier) = match style.span_style.brush.as_ref() {
        Some(brush) => (brush, style.span_style.alpha.unwrap_or(1.0).clamp(0.0, 1.0)),
        None => (&fallback_brush, 1.0),
    };
    let raster_style = match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Fill => GlyphRasterStyle::Fill,
        TextDrawStyle::Stroke { width } => {
            if width.is_finite() && width > 0.0 {
                GlyphRasterStyle::Stroke {
                    width_px: width * scale,
                }
            } else {
                GlyphRasterStyle::Fill
            }
        }
    };
    let shadow = style
        .span_style
        .shadow
        .filter(|shadow| shadow.color.3 > 0.0);
    let static_text_motion = style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;

    let origin_x = if static_text_motion {
        0.0
    } else {
        rect.x.fract()
    };
    let origin_y = if static_text_motion {
        0.0
    } else {
        rect.y.fract()
    };

    let font = font_ref.font;
    let font_px_size = font_size * scale * font_ref.ab_glyph_scale_factor;
    let letter_spacing = font_ref.tracking.resolve(style, font_size) * scale;
    let align_fraction = cranpose_ui::text::text_align_fraction(style, text);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font_ref.weight, font_size, scale);
    let style_synthesis = TextStyleSynthesis::for_style(style, font_ref.style, font_size, scale);
    let metrics = vertical_metrics(font, font_px_size);
    let line_box = line_box_for(
        style,
        metrics,
        (style.resolve_line_height(14.0, font_size * 1.4) * scale).max(1.0),
        1.0,
    );
    let line_height = line_box.height;
    let first_baseline_y = line_box.baseline;

    if let Brush::Solid(color) = brush
        && shadow.is_none()
    {
        let color = color_to_rgba(*color);
        let mut rgba = vec![0u8; (width * height * 4) as usize];
        visit_text_glyph_masks(
            text,
            font,
            font_cache_key,
            font_px_size,
            line_height,
            first_baseline_y,
            origin_x,
            origin_y,
            letter_spacing,
            align_fraction,
            static_text_motion,
            raster_style,
            weight_synthesis,
            style_synthesis,
            glyph_cache.as_deref_mut(),
            |mask| {
                draw_mask_glyph_solid_u8(
                    &mut rgba,
                    width,
                    height,
                    mask,
                    color,
                    brush_alpha_multiplier,
                );
            },
        );

        return ImageBitmap::from_rgba8(width, height, rgba).ok();
    }

    let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];
    visit_text_glyph_masks(
        text,
        font,
        font_cache_key,
        font_px_size,
        line_height,
        first_baseline_y,
        origin_x,
        origin_y,
        letter_spacing,
        align_fraction,
        static_text_motion,
        raster_style,
        weight_synthesis,
        style_synthesis,
        glyph_cache,
        |mask| {
            if let Some(shadow) = shadow {
                draw_shadow_mask(
                    &mut canvas,
                    width,
                    height,
                    mask,
                    shadow,
                    scale,
                    static_text_motion,
                );
            }

            draw_mask_glyph(
                &mut canvas,
                width,
                height,
                mask,
                brush,
                brush_alpha_multiplier,
                rect,
            );
        },
    );

    let mut rgba = vec![0u8; canvas.len() * 4];
    for (index, pixel) in canvas.iter().enumerate() {
        let base = index * 4;
        rgba[base] = (pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 1] = (pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 2] = (pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        rgba[base + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    ImageBitmap::from_rgba8(width, height, rgba).ok()
}

fn style_can_rasterize_direct_solid(style: &TextStyle) -> bool {
    if style
        .span_style
        .shadow
        .is_some_and(|shadow| shadow.color.3 > 0.0)
    {
        return false;
    }
    matches!(
        style.span_style.brush.as_ref(),
        None | Some(Brush::Solid(_))
    )
}

fn style_can_atlas_solid_fill(style: &TextStyle) -> bool {
    if !style_can_rasterize_direct_solid(style) {
        return false;
    }
    match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Fill => true,
        TextDrawStyle::Stroke { width } => !width.is_finite() || width <= 0.0,
    }
}

#[expect(clippy::too_many_arguments)]
fn draw_text_segment_solid_to_rgba(
    canvas: &mut [u8],
    canvas_width: u32,
    canvas_height: u32,
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: &mut SoftwareGlyphRasterCache,
) -> f32 {
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return 0.0;
    }

    let raster_style = match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Fill => GlyphRasterStyle::Fill,
        TextDrawStyle::Stroke { width } => {
            if width.is_finite() && width > 0.0 {
                GlyphRasterStyle::Stroke {
                    width_px: width * scale,
                }
            } else {
                GlyphRasterStyle::Fill
            }
        }
    };
    let text_motion_static = style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;
    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    let origin_x = if text_motion_static {
        local_rect.x.round()
    } else {
        local_rect.x + local_rect.x.fract()
    };
    let color = color_to_rgba(color);

    visit_text_glyph_masks(
        text,
        &font.font,
        font.content_hash(),
        m.font_px_size,
        m.line_height,
        m.first_baseline_y,
        origin_x,
        0.0,
        m.letter_spacing,
        m.align_fraction,
        text_motion_static,
        raster_style,
        m.weight_synthesis,
        m.style_synthesis,
        Some(glyph_cache),
        |mask| draw_mask_glyph_solid_u8(canvas, canvas_width, canvas_height, mask, color, 1.0),
    )
}

#[expect(clippy::too_many_arguments)]
fn collect_text_segment_solid_atlas_glyphs(
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasGlyph>,
) -> Option<f32> {
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return Some(0.0);
    }
    if !text_segment_supports_solid_atlas(style) {
        return None;
    }

    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    let origin_x = local_rect.x.round();
    let initial_len = out.len();

    let advance = visit_text_glyph_masks_with_key(
        text,
        &font.font,
        font.content_hash(),
        m.font_px_size,
        m.line_height,
        m.first_baseline_y,
        origin_x,
        0.0,
        m.letter_spacing,
        m.align_fraction,
        true,
        GlyphRasterStyle::Fill,
        m.weight_synthesis,
        m.style_synthesis,
        Some(glyph_cache),
        |key, mask| {
            if mask.width == 0 || mask.height == 0 {
                return;
            }
            out.push(SoftwareGlyphAtlasGlyph {
                key,
                mask: SoftwareGlyphAtlasMask {
                    alpha: Arc::clone(&mask.alpha),
                    width: mask.width,
                    height: mask.height,
                },
                x: mask.origin_x,
                y: mask.origin_y,
                color,
            });
        },
    );

    if advance.is_finite() {
        Some(advance)
    } else {
        out.truncate(initial_len);
        None
    }
}

#[expect(clippy::too_many_arguments)]
fn collect_text_segment_cached_solid_atlas_placements(
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasPlacement>,
) -> Option<f32> {
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return Some(0.0);
    }
    if !text_segment_supports_solid_atlas(style) {
        return None;
    }

    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    let origin_x = local_rect.x.round();
    let initial_len = out.len();

    let advance = visit_cached_text_glyph_atlas_placements(
        text,
        &font.font,
        font.content_hash(),
        m.font_px_size,
        m.line_height,
        m.first_baseline_y,
        origin_x,
        0.0,
        m.letter_spacing,
        m.align_fraction,
        GlyphRasterStyle::Fill,
        m.weight_synthesis,
        m.style_synthesis,
        glyph_cache,
        |placement| {
            if placement.width == 0 || placement.height == 0 {
                return;
            }
            out.push(SoftwareGlyphAtlasPlacement { color, ..placement });
        },
    );

    if advance.is_finite() {
        Some(advance)
    } else {
        out.truncate(initial_len);
        None
    }
}

#[expect(clippy::too_many_arguments)]
fn collect_text_segment_solid_atlas_run(
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    color: Color,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<SoftwareGlyphAtlasRunGlyph>,
) -> Option<f32> {
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return Some(0.0);
    }
    if !text_segment_supports_solid_atlas(style) {
        return None;
    }

    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    let origin_x = local_rect.x.round();
    let initial_len = out.len();

    let advance = visit_text_glyph_atlas_run(
        text,
        &font.font,
        font.content_hash(),
        m.font_px_size,
        m.line_height,
        m.first_baseline_y,
        origin_x,
        0.0,
        m.letter_spacing,
        m.align_fraction,
        GlyphRasterStyle::Fill,
        m.weight_synthesis,
        m.style_synthesis,
        glyph_cache,
        |run_glyph| {
            let run_glyph = match run_glyph {
                SoftwareGlyphAtlasRunGlyph::Cached(mut placement) => {
                    if placement.width == 0 || placement.height == 0 {
                        return;
                    }
                    placement.color = color;
                    SoftwareGlyphAtlasRunGlyph::Cached(placement)
                }
                SoftwareGlyphAtlasRunGlyph::New(mut glyph) => {
                    if glyph.mask.width == 0 || glyph.mask.height == 0 {
                        return;
                    }
                    glyph.color = color;
                    SoftwareGlyphAtlasRunGlyph::New(glyph)
                }
            };
            out.push(run_glyph);
        },
    );

    if advance.is_finite() {
        Some(advance)
    } else {
        out.truncate(initial_len);
        None
    }
}

fn resolve_font_size(style: &TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

fn line_box_for(
    style: &TextStyle,
    metrics: crate::font_layout::FontVerticalMetrics,
    line_height: f32,
    grid: f32,
) -> cranpose_ui::text::LineBox {
    cranpose_ui::text::line_box(
        style,
        cranpose_ui::text::FontExtent::new(metrics.ascent, -metrics.descent, metrics.line_gap),
        line_height,
        grid,
    )
}

fn measure_grid() -> f32 {
    if cranpose_ui::has_current_app_context() {
        cranpose_ui::current_density()
    } else {
        1.0
    }
}

fn resolve_line_height(style: &TextStyle, font_size: f32) -> f32 {
    style.resolve_line_height(14.0, font_size)
}

fn line_height_for_render_style(style: &TextStyle, font_size: f32) -> f32 {
    resolve_line_height(style, font_size * 1.4).max(1.0)
}

fn resolve_letter_spacing(style: &TextStyle, font_size: f32) -> f32 {
    let _ = font_size;
    style.resolve_letter_spacing(14.0)
}

fn run_tracking(char_count: usize, letter_spacing: f32) -> f32 {
    char_count as f32 * letter_spacing
}

fn run_lead_in(char_count: usize, letter_spacing: f32) -> f32 {
    if char_count == 0 {
        0.0
    } else {
        letter_spacing * 0.5
    }
}

fn fallback_char_width(font_size: f32) -> f32 {
    font_size.max(1.0) * 0.55
}

fn fallback_line_height(style: &TextStyle, font_size: f32) -> f32 {
    resolve_line_height(style, font_size.max(1.0) * 1.2)
}

fn fallback_line_heights(text: &str, style: &TextStyle, font_size: f32) -> Vec<f32> {
    let line_count = text.split('\n').count().max(1);
    vec![fallback_line_height(style, font_size); line_count]
}

fn fallback_text_metrics(text: &str, style: &TextStyle, font_size: f32) -> TextMetrics {
    let line_height = fallback_line_height(style, font_size);
    let char_width = fallback_char_width(font_size);
    let letter_spacing = resolve_letter_spacing(style, font_size);
    let mut line_count = 0usize;
    let mut max_width = 0.0f32;

    for line in text.split('\n') {
        line_count += 1;
        let char_count = line.chars().count();
        let spacing = run_tracking(char_count, letter_spacing);
        max_width = max_width.max(char_count as f32 * char_width + spacing);
    }

    let line_count = line_count.max(1);
    TextMetrics {
        width: max_width,
        height: line_count as f32 * line_height,
        line_height,
        line_count,
    }
}

fn fallback_cursor_x_for_offset(text: &str, style: &TextStyle, offset: usize) -> f32 {
    let font_size = resolve_font_size(style);
    let clamped = clamp_to_char_boundary(text, offset.min(text.len()));
    let line_start = text[..clamped].rfind('\n').map_or(0, |index| index + 1);
    let char_count = text[line_start..clamped].chars().count();
    let spacing = run_tracking(char_count, resolve_letter_spacing(style, font_size));
    char_count as f32 * fallback_char_width(font_size) + spacing
}

fn fallback_text_offset_for_position(text: &str, style: &TextStyle, x: f32, y: f32) -> usize {
    if text.is_empty() {
        return 0;
    }

    let font_size = resolve_font_size(style);
    let line_height = fallback_line_height(style, font_size);
    let line_index = (y / line_height).floor().max(0.0) as usize;
    let lines: Vec<&str> = text.split('\n').collect();
    let target_line = line_index.min(lines.len().saturating_sub(1));

    let mut line_start_byte = 0;
    for line in lines.iter().take(target_line) {
        line_start_byte += line.len() + 1;
    }

    let line_text = lines.get(target_line).copied().unwrap_or("");
    if line_text.is_empty() {
        return line_start_byte;
    }

    let advance =
        (fallback_char_width(font_size) + resolve_letter_spacing(style, font_size)).max(1.0);
    let target_char = (x / advance).round().max(0.0) as usize;
    line_start_byte + byte_offset_for_char_index(line_text, target_char)
}

fn fallback_layout_text(text: &str, style: &TextStyle) -> TextLayoutResult {
    let font_size = resolve_font_size(style);
    let line_height = fallback_line_height(style, font_size);
    let char_width = fallback_char_width(font_size);
    let letter_spacing = resolve_letter_spacing(style, font_size);

    let mut glyph_x_positions = Vec::new();
    let mut char_to_byte = Vec::new();
    let mut glyph_layouts = Vec::new();
    let mut lines = Vec::new();
    let mut current_x = 0.0f32;
    let mut line_start = 0;
    let mut y = 0.0f32;

    let mut iter = text.char_indices().peekable();
    while let Some((byte_offset, ch)) = iter.next() {
        glyph_x_positions.push(current_x);
        char_to_byte.push(byte_offset);

        if ch == '\n' {
            lines.push(LineLayout {
                start_offset: line_start,
                end_offset: byte_offset,
                y,
                height: line_height,
            });
            line_start = byte_offset + 1;
            y += line_height;
            current_x = 0.0;
        } else {
            glyph_layouts.push(GlyphLayout {
                line_index: lines.len(),
                start_offset: byte_offset,
                end_offset: byte_offset + ch.len_utf8(),
                x: current_x,
                y,
                width: char_width,
                height: line_height,
            });
            current_x += char_width;
            if let Some((_, next)) = iter.peek()
                && *next != '\n'
            {
                current_x += letter_spacing;
            }
        }
    }

    glyph_x_positions.push(current_x);
    char_to_byte.push(text.len());
    lines.push(LineLayout {
        start_offset: line_start,
        end_offset: text.len(),
        y,
        height: line_height,
    });

    let metrics = fallback_text_metrics(text, style, font_size);
    TextLayoutResult::new(
        text,
        TextLayoutData {
            width: metrics.width,
            height: metrics.height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            glyph_layouts,
            lines,
        },
    )
}

fn style_allows_prefix_widths(style: &TextStyle) -> bool {
    !matches!(
        style
            .paragraph_style
            .platform_style
            .and_then(|platform| platform.shaping),
        Some(TextShaping::Advanced)
    )
}

fn cached_line_advance_width(
    font: &SoftwareTextFont,
    text: &str,
    glyph_font_size: f32,
    glyph_metrics: &mut SoftwareTextGlyphMetricsCache,
) -> f32 {
    let scaled_font = font.font.as_scaled(PxScale::from(glyph_font_size));
    let h_scale = scaled_font.h_scale_factor();
    let mut width = 0.0f32;
    let mut previous = None;

    for ch in text.chars() {
        let metrics = glyph_metrics.glyph_metrics(font, &scaled_font, ch);
        if let Some(previous_id) = previous {
            width +=
                glyph_metrics.kern(font, &scaled_font, previous_id, metrics.glyph_id) * h_scale;
        }
        width += metrics.advance_unscaled * h_scale;
        previous = Some(metrics.glyph_id);
    }

    width.max(0.0)
}

fn annotated_line_prefix_widths_with_font_set_cached(
    text: &AnnotatedString,
    line_range: std::ops::Range<usize>,
    style: &TextStyle,
    fonts: &SoftwareTextFontSet,
    cache: &mut SoftwareTextMetricsCache,
) -> Option<TextLinePrefixWidths> {
    let mut boundaries = text.span_boundaries();
    boundaries.push(line_range.start);
    boundaries.push(line_range.end);
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.retain(|offset| {
        *offset >= line_range.start
            && *offset <= line_range.end
            && text.text.is_char_boundary(*offset)
    });

    let char_count = text.text[line_range.clone()].chars().count();
    let mut prefix_widths = Vec::with_capacity(char_count + 1);
    let mut separator_before = Vec::with_capacity(char_count);
    let non_empty_overhang = {
        let mut sink = PrefixWidthSegmentSink {
            prefix_widths: &mut prefix_widths,
            separator_before: &mut separator_before,
            width: 0.0,
            non_empty_overhang: 0.0,
        };
        sink.prefix_widths.push(sink.width);

        for range in boundaries.windows(2) {
            let start = range[0];
            let end = range[1];
            if start >= end {
                continue;
            }
            let segment = &text.text[start..end];
            let segment_style = effective_style_for_range(&text.span_styles, style, start, end);
            append_prefix_width_segment_cached(segment, &segment_style, fonts, cache, &mut sink);
        }

        sink.non_empty_overhang
    };

    TextLinePrefixWidths::from_parts(prefix_widths, separator_before, non_empty_overhang)
}

struct PrefixWidthSegmentSink<'a> {
    prefix_widths: &'a mut Vec<f32>,
    separator_before: &'a mut Vec<f32>,
    width: f32,
    non_empty_overhang: f32,
}

fn append_prefix_width_segment_cached(
    segment: &str,
    style: &TextStyle,
    fonts: &SoftwareTextFontSet,
    cache: &mut SoftwareTextMetricsCache,
    sink: &mut PrefixWidthSegmentSink<'_>,
) {
    if segment.is_empty() {
        return;
    }

    let font_size = resolve_font_size(style);
    if let Some(font) = fonts.resolve(style) {
        append_font_prefix_width_segment_cached(segment, style, font_size, font, cache, sink);
    } else {
        append_fallback_prefix_width_segment(segment, style, font_size, sink);
    }
}

fn append_font_prefix_width_segment_cached(
    segment: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
    cache: &mut SoftwareTextMetricsCache,
    sink: &mut PrefixWidthSegmentSink<'_>,
) {
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let scaled_font = font.font.as_scaled(PxScale::from(glyph_font_size));
    let letter_spacing = font.metadata.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font.weight(), font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, font.style(), font_size, 1.0);
    sink.non_empty_overhang = sink
        .non_empty_overhang
        .max(style_synthesis.visual_overhang_px());

    let mut previous = None;
    let h_scale = scaled_font.h_scale_factor();

    for (index, ch) in segment.chars().enumerate() {
        let metrics = cache.glyph_metrics.glyph_metrics(font, &scaled_font, ch);
        let separator = if index == 0 {
            0.0
        } else {
            previous.map_or(0.0, |previous_id| {
                weight_synthesis.apply_width(
                    cache
                        .glyph_metrics
                        .kern(font, &scaled_font, previous_id, metrics.glyph_id)
                        * h_scale,
                )
            })
        };
        sink.separator_before.push(separator);
        sink.width += separator
            + letter_spacing
            + weight_synthesis.apply_width(metrics.advance_unscaled * h_scale);
        sink.prefix_widths.push(sink.width.max(0.0));
        previous = Some(metrics.glyph_id);
    }
}

fn append_fallback_prefix_width_segment(
    segment: &str,
    style: &TextStyle,
    font_size: f32,
    sink: &mut PrefixWidthSegmentSink<'_>,
) {
    let char_width = fallback_char_width(font_size);
    let letter_spacing = resolve_letter_spacing(style, font_size);
    for _ in segment.chars() {
        sink.separator_before.push(0.0);
        sink.width += letter_spacing + char_width;
        sink.prefix_widths.push(sink.width.max(0.0));
    }
}

fn byte_offset_for_char_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .map(|(index, _)| index)
        .nth(char_index)
        .unwrap_or(text.len())
}

fn measure_text_impl(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font_ref: RasterFontRef<'_, impl Font>,
) -> TextMetrics {
    let font = font_ref.font;
    let glyph_font_size = font_size * font_ref.ab_glyph_scale_factor;
    let line_height = line_box_for(
        style,
        vertical_metrics(font, glyph_font_size),
        resolve_line_height(style, font_size * 1.4),
        measure_grid(),
    )
    .height;
    let letter_spacing = font_ref.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font_ref.weight, font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, font_ref.style, font_size, 1.0);

    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1);

    let mut max_width: f32 = 0.0;
    for line in &lines {
        let line_width = line_advance_width(font, line, glyph_font_size);
        let char_spacing = run_tracking(line.chars().count(), letter_spacing);
        let line_width = (weight_synthesis.apply_width(line_width) + char_spacing).max(0.0);
        let line_width = if line.is_empty() {
            line_width
        } else {
            line_width + style_synthesis.visual_overhang_px()
        };
        max_width = max_width.max(line_width);
    }

    TextMetrics {
        width: max_width,
        height: line_count as f32 * line_height,
        line_height,
        line_count,
    }
}

fn measure_text_impl_cached(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
    cache: &mut SoftwareTextMetricsCache,
) -> TextMetrics {
    let letter_spacing = font.metadata.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font.weight(), font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, font.style(), font_size, 1.0);
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let line_height = line_box_for(
        style,
        vertical_metrics(&font.font, glyph_font_size),
        resolve_line_height(style, font_size * 1.4),
        measure_grid(),
    )
    .height;

    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1);

    let mut max_width: f32 = 0.0;
    for line in &lines {
        let line_width =
            cached_line_advance_width(font, line, glyph_font_size, &mut cache.glyph_metrics);
        let char_spacing = run_tracking(line.chars().count(), letter_spacing);
        let line_width = (weight_synthesis.apply_width(line_width) + char_spacing).max(0.0);
        let line_width = if line.is_empty() {
            line_width
        } else {
            line_width + style_synthesis.visual_overhang_px()
        };
        max_width = max_width.max(line_width);
    }

    TextMetrics {
        width: max_width,
        height: line_count as f32 * line_height,
        line_height,
        line_count,
    }
}

fn measure_annotated_text_with_resolver(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
    mut cache: Option<&mut SoftwareTextMetricsCache>,
) -> TextMetrics {
    let Some(base_font) = fonts.resolve(style) else {
        return fallback_text_metrics(text.text.as_str(), style, font_size);
    };
    let base_line_height = line_height_for_style(style, font_size, base_font);
    let mut boundaries = text.span_boundaries();
    for (offset, ch) in text.text.char_indices() {
        if ch == '\n' {
            boundaries.push(offset);
            boundaries.push(offset + ch.len_utf8());
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.retain(|offset| *offset <= text.text.len() && text.text.is_char_boundary(*offset));

    let mut line_count = 1usize;
    let mut max_width = 0.0f32;
    let mut current_line_width = 0.0f32;

    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let segment = &text.text[start..end];
        let segment_style = effective_style_for_range(&text.span_styles, style, start, end);
        let segment_font_size = resolve_font_size(&segment_style);
        let Some(segment_font) = fonts.resolve(&segment_style) else {
            let mut remaining = segment;
            loop {
                if let Some(newline_offset) = remaining.find('\n') {
                    let before_newline = &remaining[..newline_offset];
                    if !before_newline.is_empty() {
                        current_line_width += fallback_text_metrics(
                            before_newline,
                            &segment_style,
                            segment_font_size,
                        )
                        .width;
                    }
                    max_width = max_width.max(current_line_width);
                    current_line_width = 0.0;
                    line_count += 1;
                    remaining = &remaining[newline_offset + 1..];
                    if remaining.is_empty() {
                        break;
                    }
                } else {
                    if !remaining.is_empty() {
                        current_line_width +=
                            fallback_text_metrics(remaining, &segment_style, segment_font_size)
                                .width;
                    }
                    break;
                }
            }
            continue;
        };

        let mut remaining = segment;
        loop {
            if let Some(newline_offset) = remaining.find('\n') {
                let before_newline = &remaining[..newline_offset];
                if !before_newline.is_empty() {
                    let metrics = if let Some(cache) = cache.as_deref_mut() {
                        measure_text_with_font_cached(
                            before_newline,
                            &segment_style,
                            segment_font_size,
                            segment_font,
                            cache,
                        )
                    } else {
                        measure_text_with_font(
                            before_newline,
                            &segment_style,
                            segment_font_size,
                            segment_font,
                        )
                    };
                    current_line_width += metrics.width;
                }
                max_width = max_width.max(current_line_width);
                current_line_width = 0.0;
                line_count += 1;
                remaining = &remaining[newline_offset + 1..];
                if remaining.is_empty() {
                    break;
                }
            } else {
                if !remaining.is_empty() {
                    let metrics = if let Some(cache) = cache.as_deref_mut() {
                        measure_text_with_font_cached(
                            remaining,
                            &segment_style,
                            segment_font_size,
                            segment_font,
                            cache,
                        )
                    } else {
                        measure_text_with_font(
                            remaining,
                            &segment_style,
                            segment_font_size,
                            segment_font,
                        )
                    };
                    current_line_width += metrics.width;
                }
                break;
            }
        }
    }

    max_width = max_width.max(current_line_width);

    let line_heights = annotated_line_heights_with_resolver(text, style, font_size, fonts);
    let total_height = line_heights.iter().sum();
    let max_line_height = line_heights.into_iter().fold(base_line_height, f32::max);

    TextMetrics {
        width: max_width,
        height: total_height,
        line_height: max_line_height,
        line_count,
    }
}

fn annotated_line_heights_with_resolver(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
) -> Vec<f32> {
    let Some(base_font) = fonts.resolve(style) else {
        return fallback_line_heights(text.text.as_str(), style, font_size);
    };
    let base_line_height = line_height_for_style(style, font_size, base_font);
    let mut line_heights = vec![base_line_height];
    let mut boundaries = text.span_boundaries();
    for (offset, ch) in text.text.char_indices() {
        if ch == '\n' {
            boundaries.push(offset);
            boundaries.push(offset + ch.len_utf8());
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries.retain(|offset| *offset <= text.text.len() && text.text.is_char_boundary(*offset));

    let mut line_index = 0usize;
    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let segment = &text.text[start..end];
        let segment_style = effective_style_for_range(&text.span_styles, style, start, end);
        let segment_font_size = resolve_font_size(&segment_style);
        let segment_line_height = if let Some(segment_font) = fonts.resolve(&segment_style) {
            line_height_for_style(&segment_style, segment_font_size, segment_font)
        } else {
            fallback_line_height(&segment_style, segment_font_size)
        };
        for ch in segment.chars() {
            line_heights[line_index] = line_heights[line_index].max(segment_line_height);
            if ch == '\n' {
                line_index += 1;
                if line_heights.len() <= line_index {
                    line_heights.push(base_line_height);
                }
            }
        }
    }

    line_heights
}

fn max_line_height_for_annotated_text_with_resolver(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
) -> f32 {
    let base_line_height = fonts.resolve(style).map_or_else(
        || fallback_line_height(style, font_size),
        |font| line_height_for_style(style, font_size, font),
    );
    if text.span_styles.is_empty() {
        return base_line_height;
    }

    let mut max_line_height = base_line_height;
    for range in text.span_boundaries().windows(2) {
        let start = range[0];
        let end = range[1];
        if start == end {
            continue;
        }
        let segment_style = effective_style_for_range(&text.span_styles, style, start, end);
        let segment_font_size = resolve_font_size(&segment_style);
        let segment_line_height = fonts.resolve(&segment_style).map_or_else(
            || fallback_line_height(&segment_style, segment_font_size),
            |font| line_height_for_style(&segment_style, segment_font_size, font),
        );
        max_line_height = max_line_height.max(segment_line_height);
    }
    max_line_height
}

fn effective_style_for_range(
    span_styles: &[RangeStyle<SpanStyle>],
    style: &TextStyle,
    start: usize,
    end: usize,
) -> TextStyle {
    let mut effective = style.clone();
    for span in span_styles {
        if span.range.start < end && span.range.end > start {
            effective.span_style = effective.span_style.merge(&span.item);
        }
    }
    effective
}

fn line_height_for_style(style: &TextStyle, font_size: f32, font: &SoftwareTextFont) -> f32 {
    let asked = resolve_line_height(style, font_size * 1.4);
    if style.paragraph_style.line_height_style.is_none() {
        return asked;
    }
    let metrics =
        crate::font_layout::vertical_metrics(&font.font, font.ab_glyph_px_size(font_size));
    line_box_for(style, metrics, asked, measure_grid()).height
}

fn clamp_to_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn align_glyph_for_text_motion(glyph: Glyph, static_text_motion: bool) -> Glyph {
    align_glyph_to_pixel_grid(glyph, static_text_motion)
}

fn static_glyph_pixel_origin(glyph: &Glyph) -> (i32, i32) {
    (
        glyph.position.x.round() as i32,
        glyph.position.y.round() as i32,
    )
}

fn glyph_mask_cache_key(
    font_hash: u64,
    glyph: &Glyph,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
) -> GlyphMaskCacheKey {
    GlyphMaskCacheKey {
        font_hash,
        glyph_id: u32::from(glyph.id.0),
        scale_x_bits: glyph.scale.x.to_bits(),
        scale_y_bits: glyph.scale.y.to_bits(),
        raster_style: GlyphRasterStyleKey::from_style(raster_style),
        embolden_px_bits: weight_synthesis.embolden_px.to_bits(),
        slant_bits: style_synthesis.slant.to_bits(),
    }
}

fn glyph_atlas_key_from_mask_key(key: GlyphMaskCacheKey) -> Option<SoftwareGlyphAtlasKey> {
    if !matches!(key.raster_style, GlyphRasterStyleKey::Fill) {
        return None;
    }
    Some(SoftwareGlyphAtlasKey {
        font_hash: key.font_hash,
        glyph_id: key.glyph_id,
        scale_x_bits: key.scale_x_bits,
        scale_y_bits: key.scale_y_bits,
        embolden_px_bits: key.embolden_px_bits,
        slant_bits: key.slant_bits,
    })
}

fn build_complete_glyph_mask(
    font: &impl Font,
    glyph: &Glyph,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
) -> Option<GlyphMask> {
    let (outlined, bounds) = outline_glyph_with_bounds(font, glyph)?;
    let mask = build_glyph_mask(font, glyph, &outlined, bounds, raster_style)?;
    let mask = synthesize_glyph_weight(mask, weight_synthesis);
    Some(synthesize_glyph_style(mask, style_synthesis))
}

fn cached_static_glyph_mask_with_key(
    cache: &mut SoftwareGlyphRasterCache,
    font_hash: u64,
    font: &impl Font,
    glyph: &Glyph,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
) -> Option<(GlyphMaskCacheKey, GlyphMask)> {
    let key = glyph_mask_cache_key(
        font_hash,
        glyph,
        raster_style,
        weight_synthesis,
        style_synthesis,
    );
    if let Some(mask) = cache.get(&key, glyph) {
        return Some((key, mask));
    }
    let mask =
        build_complete_glyph_mask(font, glyph, raster_style, weight_synthesis, style_synthesis)?;
    Some((key, cache.put(key, glyph, mask)))
}

fn cached_static_glyph_mask(
    cache: &mut SoftwareGlyphRasterCache,
    font_hash: u64,
    font: &impl Font,
    glyph: &Glyph,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
) -> Option<GlyphMask> {
    cached_static_glyph_mask_with_key(
        cache,
        font_hash,
        font,
        glyph,
        raster_style,
        weight_synthesis,
        style_synthesis,
    )
    .map(|(_, mask)| mask)
}

fn line_alignment_offsets<F: Font, S: ScaleFont<F>>(
    scaled_font: &S,
    text: &str,
    letter_spacing: f32,
    align_fraction: f32,
) -> Option<Vec<f32>> {
    if align_fraction == 0.0 || !text.contains('\n') {
        return None;
    }
    let advances: Vec<f32> = text
        .split('\n')
        .map(|line| {
            let mut advance = 0.0f32;
            let mut previous = None;
            for ch in line.chars() {
                let glyph_id = scaled_font.glyph_id(ch);
                if let Some(previous_id) = previous {
                    advance += scaled_font.kern(previous_id, glyph_id);
                }
                advance += letter_spacing + scaled_font.h_advance(glyph_id);
                previous = Some(glyph_id);
            }
            advance.max(0.0)
        })
        .collect();
    let block = advances.iter().copied().fold(0.0f32, f32::max);
    Some(
        advances
            .iter()
            .map(|advance| ((block - advance) * align_fraction).max(0.0))
            .collect(),
    )
}

fn line_offset(offsets: &Option<Vec<f32>>, line_idx: usize) -> f32 {
    offsets
        .as_ref()
        .and_then(|offsets| offsets.get(line_idx).copied())
        .unwrap_or(0.0)
}

#[expect(clippy::too_many_arguments)]
fn visit_text_glyph_masks(
    text: &str,
    font: &impl Font,
    font_hash: u64,
    font_px_size: f32,
    line_height: f32,
    first_baseline_y: f32,
    origin_x: f32,
    origin_y: f32,
    letter_spacing: f32,
    align_fraction: f32,
    static_text_motion: bool,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    mut glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
    mut visit: impl FnMut(&GlyphMask),
) -> f32 {
    let scale = PxScale::from(font_px_size);
    let scaled_font = font.as_scaled(scale);
    let line_offsets = line_alignment_offsets(&scaled_font, text, letter_spacing, align_fraction);
    let mut max_advance = 0.0f32;
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let lead_in = run_lead_in(line.chars().count(), letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous = None;
        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret_x += scaled_font.kern(previous_id, glyph_id) + letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(caret_x, baseline_y));
            caret_x += scaled_font.h_advance(glyph_id);
            previous = Some(glyph_id);
            let glyph = align_glyph_for_text_motion(glyph, static_text_motion);
            let Some(mask) = (if static_text_motion {
                glyph_cache.as_deref_mut().and_then(|cache| {
                    cached_static_glyph_mask(
                        cache,
                        font_hash,
                        font,
                        &glyph,
                        raster_style,
                        weight_synthesis,
                        style_synthesis,
                    )
                })
            } else {
                None
            })
            .or_else(|| {
                build_complete_glyph_mask(
                    font,
                    &glyph,
                    raster_style,
                    weight_synthesis,
                    style_synthesis,
                )
            }) else {
                continue;
            };
            visit(&mask);
        }
        max_advance = max_advance.max((caret_x - origin_x + lead_in).max(0.0));
    }
    max_advance
}

#[expect(clippy::too_many_arguments)]
fn visit_text_glyph_masks_with_key(
    text: &str,
    font: &impl Font,
    font_hash: u64,
    font_px_size: f32,
    line_height: f32,
    first_baseline_y: f32,
    origin_x: f32,
    origin_y: f32,
    letter_spacing: f32,
    align_fraction: f32,
    static_text_motion: bool,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    mut glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
    mut visit: impl FnMut(SoftwareGlyphAtlasKey, &GlyphMask),
) -> f32 {
    if !static_text_motion {
        return 0.0;
    }

    let scale = PxScale::from(font_px_size);
    let scaled_font = font.as_scaled(scale);
    let line_offsets = line_alignment_offsets(&scaled_font, text, letter_spacing, align_fraction);
    let mut max_advance = 0.0f32;
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let lead_in = run_lead_in(line.chars().count(), letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous = None;
        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret_x += scaled_font.kern(previous_id, glyph_id) + letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(caret_x, baseline_y));
            caret_x += scaled_font.h_advance(glyph_id);
            previous = Some(glyph_id);
            let glyph = align_glyph_for_text_motion(glyph, true);
            let Some((cache_key, mask)) = glyph_cache.as_deref_mut().and_then(|cache| {
                cached_static_glyph_mask_with_key(
                    cache,
                    font_hash,
                    font,
                    &glyph,
                    raster_style,
                    weight_synthesis,
                    style_synthesis,
                )
            }) else {
                continue;
            };
            let Some(atlas_key) = glyph_atlas_key_from_mask_key(cache_key) else {
                continue;
            };
            visit(atlas_key, &mask);
        }
        max_advance = max_advance.max((caret_x - origin_x + lead_in).max(0.0));
    }
    max_advance
}

#[expect(clippy::too_many_arguments)]
fn visit_cached_text_glyph_atlas_placements(
    text: &str,
    font: &impl Font,
    font_hash: u64,
    font_px_size: f32,
    line_height: f32,
    first_baseline_y: f32,
    origin_x: f32,
    origin_y: f32,
    letter_spacing: f32,
    align_fraction: f32,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    mut visit: impl FnMut(SoftwareGlyphAtlasPlacement),
) -> f32 {
    let scale = PxScale::from(font_px_size);
    let scaled_font = font.as_scaled(scale);
    let line_offsets = line_alignment_offsets(&scaled_font, text, letter_spacing, align_fraction);
    let mut max_advance = 0.0f32;
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let lead_in = run_lead_in(line.chars().count(), letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous = None;
        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret_x += scaled_font.kern(previous_id, glyph_id) + letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(caret_x, baseline_y));
            caret_x += scaled_font.h_advance(glyph_id);
            previous = Some(glyph_id);
            let glyph = align_glyph_for_text_motion(glyph, true);
            let cache_key = glyph_mask_cache_key(
                font_hash,
                &glyph,
                raster_style,
                weight_synthesis,
                style_synthesis,
            );
            let Some((key, x, y, width, height)) =
                glyph_cache.get_atlas_placement(&cache_key, &glyph)
            else {
                if font.outline(glyph.id).is_none() {
                    continue;
                }
                return f32::NAN;
            };
            visit(SoftwareGlyphAtlasPlacement {
                key,
                x,
                y,
                width,
                height,
                color: Color::WHITE,
            });
        }
        max_advance = max_advance.max((caret_x - origin_x + lead_in).max(0.0));
    }
    max_advance
}

#[expect(clippy::too_many_arguments)]
fn visit_text_glyph_atlas_run(
    text: &str,
    font: &impl Font,
    font_hash: u64,
    font_px_size: f32,
    line_height: f32,
    first_baseline_y: f32,
    origin_x: f32,
    origin_y: f32,
    letter_spacing: f32,
    align_fraction: f32,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    mut visit: impl FnMut(SoftwareGlyphAtlasRunGlyph),
) -> f32 {
    let scale = PxScale::from(font_px_size);
    let scaled_font = font.as_scaled(scale);
    let line_offsets = line_alignment_offsets(&scaled_font, text, letter_spacing, align_fraction);
    let mut max_advance = 0.0f32;
    let mut run_metrics_cache: Vec<(GlyphMaskCacheKey, CachedAtlasGlyphMetrics)> = Vec::new();
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let lead_in = run_lead_in(line.chars().count(), letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous = None;
        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret_x += scaled_font.kern(previous_id, glyph_id) + letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(caret_x, baseline_y));
            caret_x += scaled_font.h_advance(glyph_id);
            previous = Some(glyph_id);
            let glyph = align_glyph_for_text_motion(glyph, true);
            let cache_key = glyph_mask_cache_key(
                font_hash,
                &glyph,
                raster_style,
                weight_synthesis,
                style_synthesis,
            );
            if let Some((_, metrics)) = run_metrics_cache
                .iter()
                .find(|(cached_key, _)| *cached_key == cache_key)
            {
                visit(SoftwareGlyphAtlasRunGlyph::Cached(
                    metrics.placement(&glyph, Color::WHITE),
                ));
                continue;
            }
            if let Some(metrics) = glyph_cache.get_atlas_metrics(&cache_key) {
                if run_metrics_cache.len() < RUN_GLYPH_METRICS_CACHE_LIMIT {
                    run_metrics_cache.push((cache_key, metrics));
                }
                visit(SoftwareGlyphAtlasRunGlyph::Cached(
                    metrics.placement(&glyph, Color::WHITE),
                ));
                continue;
            }

            if font.outline(glyph.id).is_none() {
                continue;
            }
            let Some(mask) = build_complete_glyph_mask(
                font,
                &glyph,
                raster_style,
                weight_synthesis,
                style_synthesis,
            ) else {
                continue;
            };
            let mask = glyph_cache.put(cache_key, &glyph, mask);
            let Some(key) = glyph_atlas_key_from_mask_key(cache_key) else {
                continue;
            };
            let (glyph_x, glyph_y) = static_glyph_pixel_origin(&glyph);
            if run_metrics_cache.len() < RUN_GLYPH_METRICS_CACHE_LIMIT {
                run_metrics_cache.push((
                    cache_key,
                    CachedAtlasGlyphMetrics {
                        key,
                        width: mask.width,
                        height: mask.height,
                        origin_offset_x: mask.origin_x - glyph_x,
                        origin_offset_y: mask.origin_y - glyph_y,
                    },
                ));
            }
            visit(SoftwareGlyphAtlasRunGlyph::New(SoftwareGlyphAtlasGlyph {
                key,
                mask: SoftwareGlyphAtlasMask {
                    alpha: Arc::clone(&mask.alpha),
                    width: mask.width,
                    height: mask.height,
                },
                x: mask.origin_x,
                y: mask.origin_y,
                color: Color::WHITE,
            }));
        }
        max_advance = max_advance.max((caret_x - origin_x + lead_in).max(0.0));
    }
    max_advance
}

fn blend_src_over(dst: &mut [f32; 4], src: [f32; 4]) {
    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }

    let dst_alpha = dst[3].clamp(0.0, 1.0);
    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);

    if out_alpha <= f32::EPSILON {
        *dst = [0.0, 0.0, 0.0, 0.0];
        return;
    }

    for channel in 0..3 {
        let src_premult = src[channel].clamp(0.0, 1.0) * src_alpha;
        let dst_premult = dst[channel].clamp(0.0, 1.0) * dst_alpha;
        dst[channel] =
            ((src_premult + dst_premult * (1.0 - src_alpha)) / out_alpha).clamp(0.0, 1.0);
    }
    dst[3] = out_alpha;
}

fn draw_mask_glyph(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    mask: &GlyphMask,
    brush: &Brush,
    brush_alpha_multiplier: f32,
    brush_rect: Rect,
) {
    for y in 0..mask.height {
        let py = mask.origin_y + y as i32;
        if py < 0 || py >= height as i32 {
            continue;
        }

        for x in 0..mask.width {
            let px = mask.origin_x + x as i32;
            if px < 0 || px >= width as i32 {
                continue;
            }

            let coverage = mask.alpha[y * mask.width + x];
            if coverage <= 0.0 {
                continue;
            }

            let sample = sample_brush_rgba(
                brush,
                brush_rect,
                brush_rect.x + px as f32 + 0.5,
                brush_rect.y + py as f32 + 0.5,
                cranpose_ui_graphics::Point::default(),
            );
            let alpha = coverage * sample[3] * brush_alpha_multiplier;
            if alpha <= 0.0 {
                continue;
            }
            let idx = (py as u32 * width + px as u32) as usize;
            blend_src_over(
                &mut canvas[idx],
                [sample[0], sample[1], sample[2], alpha.clamp(0.0, 1.0)],
            );
        }
    }
}

fn blend_src_over_u8(dst: &mut [u8], src: [f32; 4]) {
    let src_alpha = src[3].clamp(0.0, 1.0);
    if src_alpha <= 0.0 {
        return;
    }

    let dst_alpha = dst[3] as f32 / 255.0;
    if dst_alpha <= 0.0 {
        dst[0] = (src[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst[1] = (src[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst[2] = (src[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        dst[3] = (src_alpha * 255.0).round() as u8;
        return;
    }

    let out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha);
    if out_alpha <= f32::EPSILON {
        dst.fill(0);
        return;
    }

    for channel in 0..3 {
        let src_premult = src[channel].clamp(0.0, 1.0) * src_alpha;
        let dst_premult = (dst[channel] as f32 / 255.0) * dst_alpha;
        dst[channel] =
            ((src_premult + dst_premult * (1.0 - src_alpha)) / out_alpha * 255.0).round() as u8;
    }
    dst[3] = (out_alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
}

fn draw_mask_glyph_solid_u8(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    mask: &GlyphMask,
    color: [f32; 4],
    alpha_multiplier: f32,
) {
    let red = (color[0].clamp(0.0, 1.0) * 255.0).round() as u8;
    let green = (color[1].clamp(0.0, 1.0) * 255.0).round() as u8;
    let blue = (color[2].clamp(0.0, 1.0) * 255.0).round() as u8;
    let alpha_scale = color[3].clamp(0.0, 1.0) * alpha_multiplier.clamp(0.0, 1.0);
    if alpha_scale <= 0.0 {
        return;
    }

    for y in 0..mask.height {
        let py = mask.origin_y + y as i32;
        if py < 0 || py >= height as i32 {
            continue;
        }

        for x in 0..mask.width {
            let px = mask.origin_x + x as i32;
            if px < 0 || px >= width as i32 {
                continue;
            }

            let coverage = mask.alpha[y * mask.width + x];
            if coverage <= 0.0 {
                continue;
            }

            let alpha = (coverage * alpha_scale).clamp(0.0, 1.0);
            let alpha_u8 = (alpha * 255.0).round() as u8;
            if alpha_u8 == 0 {
                continue;
            }
            let idx = ((py as u32 * width + px as u32) * 4) as usize;
            let dst = &mut canvas[idx..idx + 4];
            if dst[3] == 0 {
                dst[0] = red;
                dst[1] = green;
                dst[2] = blue;
                dst[3] = alpha_u8;
            } else {
                blend_src_over_u8(dst, [color[0], color[1], color[2], alpha]);
            }
        }
    }
}

fn draw_shadow_mask(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    mask: &GlyphMask,
    shadow: Shadow,
    text_scale: f32,
    static_text_motion: bool,
) {
    if mask.width == 0 || mask.height == 0 {
        return;
    }

    let shadow_dx = shadow.offset.x * text_scale;
    let shadow_dy = shadow.offset.y * text_scale;
    let blur_radius = (shadow.blur_radius * text_scale).max(0.0);
    let sigma = shadow_blur_sigma(blur_radius);
    let blur_margin = if sigma > 0.0 {
        (sigma * 3.0).ceil() as i32
    } else {
        0
    };

    let padded_width = mask.width + (blur_margin as usize) * 2;
    let padded_height = mask.height + (blur_margin as usize) * 2;
    let mut padded_mask = vec![0.0f32; padded_width * padded_height];

    for y in 0..mask.height {
        let src_offset = y * mask.width;
        let dst_offset = (y + blur_margin as usize) * padded_width + blur_margin as usize;
        padded_mask[dst_offset..dst_offset + mask.width]
            .copy_from_slice(&mask.alpha[src_offset..src_offset + mask.width]);
    }

    let blurred = if sigma > 0.0 {
        gaussian_blur_alpha(&padded_mask, padded_width, padded_height, sigma)
    } else {
        padded_mask
    };

    let shadow_rgba = color_to_rgba(shadow.color);
    let shadow_origin_x = mask.origin_x - blur_margin;
    let shadow_origin_y = mask.origin_y - blur_margin;

    for y in 0..padded_height {
        for x in 0..padded_width {
            let alpha = blurred[y * padded_width + x] * shadow_rgba[3];
            if alpha <= 0.0 {
                continue;
            }

            let target_x = shadow_origin_x as f32 + x as f32 + shadow_dx;
            let target_y = shadow_origin_y as f32 + y as f32 + shadow_dy;
            if static_text_motion {
                blend_shadow_pixel(
                    canvas,
                    width,
                    height,
                    target_x.round() as i32,
                    target_y.round() as i32,
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            } else {
                blend_shadow_pixel_subpixel(
                    canvas,
                    width,
                    height,
                    target_x,
                    target_y,
                    shadow_rgba,
                    alpha.clamp(0.0, 1.0),
                );
            }
        }
    }
}

fn blend_shadow_pixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    px: i32,
    py: i32,
    color: [f32; 4],
    alpha: f32,
) {
    if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 || alpha <= 0.0 {
        return;
    }
    let idx = (py as u32 * width + px as u32) as usize;
    blend_src_over(
        &mut canvas[idx],
        [color[0], color[1], color[2], alpha.clamp(0.0, 1.0)],
    );
}

fn blend_shadow_pixel_subpixel(
    canvas: &mut [[f32; 4]],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    color: [f32; 4],
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }

    let base_x = x.floor();
    let base_y = y.floor();
    let frac_x = x - base_x;
    let frac_y = y - base_y;
    let base_x_i32 = base_x as i32;
    let base_y_i32 = base_y as i32;
    let weights = [
        ((1.0 - frac_x) * (1.0 - frac_y), 0i32, 0i32),
        (frac_x * (1.0 - frac_y), 1, 0),
        ((1.0 - frac_x) * frac_y, 0, 1),
        (frac_x * frac_y, 1, 1),
    ];

    for (weight, dx, dy) in weights {
        if weight <= 0.0 {
            continue;
        }
        blend_shadow_pixel(
            canvas,
            width,
            height,
            base_x_i32 + dx,
            base_y_i32 + dy,
            color,
            alpha * weight,
        );
    }
}

fn shadow_blur_sigma(blur_radius: f32) -> f32 {
    if blur_radius <= 0.0 {
        0.0
    } else {
        (blur_radius * SHADOW_SIGMA_SCALE + SHADOW_SIGMA_BIAS).max(0.5)
    }
}

fn gaussian_blur_alpha(src: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let kernel = gaussian_kernel_1d(sigma);
    if kernel.len() == 1 {
        return src.to_vec();
    }
    let half = (kernel.len() / 2) as i32;

    let mut horizontal = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_x = (x as i32 + offset).clamp(0, width as i32 - 1) as usize;
                sum += src[y * width + sample_x] * *weight;
            }
            horizontal[y * width + x] = sum;
        }
    }

    let mut output = vec![0.0f32; src.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for (index, weight) in kernel.iter().enumerate() {
                let offset = index as i32 - half;
                let sample_y = (y as i32 + offset).clamp(0, height as i32 - 1) as usize;
                sum += horizontal[sample_y * width + x] * *weight;
            }
            output[y * width + x] = sum;
        }
    }

    output
}

fn gaussian_kernel_1d(sigma: f32) -> Vec<f32> {
    let half = ((sigma * 3.0).ceil() as i32).clamp(1, MAX_GAUSSIAN_KERNEL_HALF);
    if half <= 0 {
        return vec![1.0];
    }

    let mut kernel = Vec::with_capacity((half * 2 + 1) as usize);
    let mut sum = 0.0f32;
    for offset in -half..=half {
        let distance = offset as f32;
        let weight = (-0.5 * (distance / sigma).powi(2)).exp();
        kernel.push(weight);
        sum += weight;
    }

    if sum > f32::EPSILON {
        for weight in &mut kernel {
            *weight /= sum;
        }
    }

    kernel
}

fn outline_glyph_with_bounds(
    font: &impl Font,
    glyph: &Glyph,
) -> Option<(OutlinedGlyph, GlyphPixelBounds)> {
    let outlined = font.outline_glyph(glyph.clone())?;
    let bounds = pixel_bounds_from_outlined(&outlined);
    Some((outlined, bounds))
}

fn build_glyph_mask(
    font: &impl Font,
    glyph: &Glyph,
    outlined: &OutlinedGlyph,
    bounds: GlyphPixelBounds,
    style: GlyphRasterStyle,
) -> Option<GlyphMask> {
    match style {
        GlyphRasterStyle::Fill => build_fill_mask(outlined, bounds),
        GlyphRasterStyle::Stroke { width_px } => {
            build_stroke_mask(font, glyph, outlined, bounds, width_px)
        }
    }
}

fn build_fill_mask(outlined: &OutlinedGlyph, bounds: GlyphPixelBounds) -> Option<GlyphMask> {
    let mask_width = bounds.width();
    let mask_height = bounds.height();
    if mask_width == 0 || mask_height == 0 {
        return None;
    }

    let mut alpha = vec![0.0f32; mask_width * mask_height];
    outlined.draw(|gx, gy, value| {
        let idx = gy as usize * mask_width + gx as usize;
        alpha[idx] = value;
    });

    Some(GlyphMask {
        alpha: Arc::from(alpha),
        width: mask_width,
        height: mask_height,
        origin_x: bounds.min_x,
        origin_y: bounds.min_y,
    })
}

fn build_stroke_mask(
    font: &impl Font,
    glyph: &Glyph,
    outlined: &OutlinedGlyph,
    bounds: GlyphPixelBounds,
    stroke_width_px: f32,
) -> Option<GlyphMask> {
    if !stroke_width_px.is_finite() || stroke_width_px <= 0.0 {
        return build_fill_mask(outlined, bounds);
    }

    let mask_width = bounds.max_x - bounds.min_x;
    let mask_height = bounds.max_y - bounds.min_y;
    if mask_width <= 0 || mask_height <= 0 {
        return None;
    }

    let half_width = stroke_width_px * 0.5;
    let miter_pad = (half_width * COMPOSE_STROKE_MITER_LIMIT).ceil();
    let pad = miter_pad.max(1.0) as i32 + 1;
    let path = build_outline_path(font, glyph, bounds, pad)?;
    let raster_width = mask_width + pad * 2;
    let raster_height = mask_height + pad * 2;
    if raster_width <= 0 || raster_height <= 0 {
        return None;
    }

    let mut pixmap = Pixmap::new(raster_width as u32, raster_height as u32)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    let stroke = Stroke {
        width: stroke_width_px,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        miter_limit: COMPOSE_STROKE_MITER_LIMIT,
        ..Stroke::default()
    };

    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    let alpha: Vec<f32> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3] as f32 / 255.0)
        .collect();

    Some(GlyphMask {
        alpha: Arc::from(alpha),
        width: raster_width as usize,
        height: raster_height as usize,
        origin_x: bounds.min_x - pad,
        origin_y: bounds.min_y - pad,
    })
}

fn synthesize_glyph_weight(mask: GlyphMask, synthesis: TextWeightSynthesis) -> GlyphMask {
    let horizontal_shift = synthetic_weight_shift_px(synthesis.embolden_px);
    if horizontal_shift == 0 || mask.width == 0 || mask.height == 0 {
        return mask;
    }

    let vertical_shift = (horizontal_shift / 2).min(1);
    let output_width = mask.width + horizontal_shift;
    let output_height = mask.height + vertical_shift * 2;
    let mut alpha = vec![0.0f32; output_width * output_height];
    for y in 0..mask.height {
        for x in 0..mask.width {
            let coverage = mask.alpha[y * mask.width + x];
            if coverage <= 0.0 {
                continue;
            }
            for dy in 0..=(vertical_shift * 2) {
                let output_y = y + dy;
                for dx in 0..=horizontal_shift {
                    let output_x = x + dx;
                    let output_index = output_y * output_width + output_x;
                    if coverage > alpha[output_index] {
                        alpha[output_index] = coverage;
                    }
                }
            }
        }
    }

    GlyphMask {
        alpha: Arc::from(alpha),
        width: output_width,
        height: output_height,
        origin_x: mask.origin_x,
        origin_y: mask.origin_y - vertical_shift as i32,
    }
}

fn synthesize_glyph_style(mask: GlyphMask, synthesis: TextStyleSynthesis) -> GlyphMask {
    if synthesis.slant <= 0.0 || mask.width == 0 || mask.height == 0 {
        return mask;
    }

    let max_shift = ((mask.height.saturating_sub(1)) as f32 * synthesis.slant).ceil() as usize;
    if max_shift == 0 {
        return mask;
    }

    let output_width = mask.width + max_shift + 1;
    let mut alpha = vec![0.0f32; output_width * mask.height];
    for y in 0..mask.height {
        let shift = (mask.height.saturating_sub(1) - y) as f32 * synthesis.slant;
        let shift_floor = shift.floor() as usize;
        let shift_fraction = shift - shift.floor();
        for x in 0..mask.width {
            let coverage = mask.alpha[y * mask.width + x];
            if coverage <= 0.0 {
                continue;
            }

            let output_x = x + shift_floor;
            let left_index = y * output_width + output_x;
            let left_coverage = coverage * (1.0 - shift_fraction);
            if left_coverage > alpha[left_index] {
                alpha[left_index] = left_coverage;
            }

            if shift_fraction > 0.0 {
                let right_index = left_index + 1;
                let right_coverage = coverage * shift_fraction;
                if right_coverage > alpha[right_index] {
                    alpha[right_index] = right_coverage;
                }
            }
        }
    }

    GlyphMask {
        alpha: Arc::from(alpha),
        width: output_width,
        height: mask.height,
        origin_x: mask.origin_x,
        origin_y: mask.origin_y,
    }
}

fn synthetic_weight_shift_px(embolden_px: f32) -> usize {
    if !embolden_px.is_finite() || embolden_px < 0.35 {
        return 0;
    }
    embolden_px.ceil().max(1.0) as usize
}

fn build_outline_path(
    font: &impl Font,
    glyph: &Glyph,
    bounds: GlyphPixelBounds,
    pad: i32,
) -> Option<Path> {
    let outline = font.outline(glyph.id)?;
    let scale_factor = font.as_scaled(glyph.scale).scale_factor();
    let mut builder = PathBuilder::new();
    let mut has_segments = false;
    let mut current_end = None;
    let mut subpath_start = None;

    for curve in outline.curves {
        match curve {
            ab_glyph::OutlineCurve::Line(p0, p1) => {
                let start = transform_outline_point(p0, scale_factor, glyph, bounds, pad);
                let end = transform_outline_point(p1, scale_factor, glyph, bounds, pad);
                if current_end != Some(start) {
                    if current_end.is_some() {
                        builder.close();
                    }
                    builder.move_to(start.0, start.1);
                    subpath_start = Some(start);
                }
                builder.line_to(end.0, end.1);
                if subpath_start == Some(end) {
                    builder.close();
                    current_end = None;
                    subpath_start = None;
                } else {
                    current_end = Some(end);
                }
            }
            ab_glyph::OutlineCurve::Quad(p0, p1, p2) => {
                let start = transform_outline_point(p0, scale_factor, glyph, bounds, pad);
                let control = transform_outline_point(p1, scale_factor, glyph, bounds, pad);
                let end = transform_outline_point(p2, scale_factor, glyph, bounds, pad);
                if current_end != Some(start) {
                    if current_end.is_some() {
                        builder.close();
                    }
                    builder.move_to(start.0, start.1);
                    subpath_start = Some(start);
                }
                builder.quad_to(control.0, control.1, end.0, end.1);
                if subpath_start == Some(end) {
                    builder.close();
                    current_end = None;
                    subpath_start = None;
                } else {
                    current_end = Some(end);
                }
            }
            ab_glyph::OutlineCurve::Cubic(p0, p1, p2, p3) => {
                let start = transform_outline_point(p0, scale_factor, glyph, bounds, pad);
                let control1 = transform_outline_point(p1, scale_factor, glyph, bounds, pad);
                let control2 = transform_outline_point(p2, scale_factor, glyph, bounds, pad);
                let end = transform_outline_point(p3, scale_factor, glyph, bounds, pad);
                if current_end != Some(start) {
                    if current_end.is_some() {
                        builder.close();
                    }
                    builder.move_to(start.0, start.1);
                    subpath_start = Some(start);
                }
                builder.cubic_to(control1.0, control1.1, control2.0, control2.1, end.0, end.1);
                if subpath_start == Some(end) {
                    builder.close();
                    current_end = None;
                    subpath_start = None;
                } else {
                    current_end = Some(end);
                }
            }
        }
        has_segments = true;
    }

    if !has_segments {
        return None;
    }

    if current_end.is_some() {
        builder.close();
    }

    builder.finish()
}

fn transform_outline_point(
    point: ab_glyph::Point,
    scale_factor: ab_glyph::PxScaleFactor,
    glyph: &Glyph,
    bounds: GlyphPixelBounds,
    pad: i32,
) -> (f32, f32) {
    (
        point.x * scale_factor.horizontal + glyph.position.x - bounds.min_x as f32 + pad as f32,
        point.y * -scale_factor.vertical + glyph.position.y - bounds.min_y as f32 + pad as f32,
    )
}

#[cfg(test)]
#[path = "tests/software_text_raster_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/software_text_raster_line_alignment_tests.rs"]
mod line_alignment_tests;

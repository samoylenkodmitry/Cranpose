use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
    ops::ControlFlow,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use ab_glyph::{
    Font, FontArc, FontRef, FontVec, Glyph, GlyphId, InvalidFont, OutlinedGlyph, PxScale,
    ScaleFont, VariableFont, point,
};
use cranpose_core::{
    collections::{bounded_lru::BoundedLruCache, pass_aged::PassAgedCache},
    hash::default as default_hash,
};
use cranpose_ui::{
    TextLinePrefixWidths, TextMeasurer, TextMetrics,
    text::{
        AnnotatedString, FontFamily, FontStyle, FontSynthesis, FontWeight, RangeStyle,
        RenderString, Shadow, SpanStyle, TextDrawStyle, TextMotion, TextShaping, TextStyle,
    },
    text_layout_result::{GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult},
};
use cranpose_ui_graphics::{Color, ImageBitmap, Point, Rect};
use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(test)]
use crate::font_layout::layout_line_glyphs;
#[cfg(feature = "text-hyphenation")]
use crate::text_hyphenation::HyphenationDictionaryError;
use crate::{
    Brush,
    annotated_text::{
        AnnotatedBrushExtent, AnnotatedTextLayout, AnnotatedTextSegment, FontResolver,
    },
    ascii_glyphs::{ASCII_COUNT, ascii_slot},
    brush_sampling::{color_to_rgba, sample_brush_rgba},
    direct_mapped_cache::DirectMappedCache,
    font_layout::{
        GlyphPixelBounds, align_glyph_to_pixel_grid, line_advance_width,
        pixel_bounds_from_outlined, vertical_metrics,
    },
    font_tracking::FontTracking,
    gpos_kerning::KernedFont,
    text_cache_key::{TextCacheKey, TextProbe},
    text_hyphenation::HyphenationDictionaryStore,
    text_mask_gamma::TextLuminance,
};

const COMPOSE_STROKE_MITER_LIMIT: f32 = 4.0;
const SHADOW_SIGMA_SCALE: f32 = 0.57735;
const SHADOW_SIGMA_BIAS: f32 = 0.5;
const MAX_GAUSSIAN_KERNEL_HALF: i32 = 128;
/// Slots of the per-character glyph metrics cache, as a power of two: text
/// rarely uses more than a few hundred characters per font.
const SOFTWARE_TEXT_GLYPH_METRICS_SLOTS_LOG2: u32 = 11;
/// Slots of the per-pair kerning cache, as a power of two.
const SOFTWARE_TEXT_KERN_METRICS_SLOTS_LOG2: u32 = 13;
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
    glyph_map_hash: u64,
    #[cfg(feature = "text-shaping")]
    shaping: Arc<Mutex<crate::text_shaping::ShapingCache>>,
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

/// The bytes a face is parsed from.
pub enum FontBytes {
    /// Bytes that live as long as the process, such as an embedded font or a
    /// font file read once: the face reads them in place.
    Static(&'static [u8]),
    /// Bytes the face takes over.
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

    fn into_font(self) -> Result<FontArc, InvalidFont> {
        match self {
            Self::Static(bytes) => FontArc::try_from_slice(bytes),
            Self::Owned(bytes) => FontArc::try_from_vec(bytes),
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

/// A face instanced at its declared and explicit axis positions.
struct InstancedFace {
    font: FontArc,
    kerning: Option<Arc<crate::gpos_kerning::GposKerning>>,
    variations: Vec<([u8; 4], f32)>,
}

impl SoftwareTextFont {
    pub fn from_bytes(bytes: impl Into<FontBytes>) -> Result<Self, SoftwareTextFontError> {
        let bytes = bytes.into();
        let mut hasher = default_hash::new();
        bytes.as_slice().hash(&mut hasher);
        let content_hash = hasher.finish();
        let metadata = software_text_font_metadata(bytes.as_slice());
        let kerning = KernedFont::read_kerning(bytes.as_slice(), &[]);
        let font = bytes
            .into_font()
            .map_err(|_| SoftwareTextFontError::InvalidFont)?;
        let score = text_font_score_from_parts(&font, &metadata);
        Ok(Self {
            font: KernedFont::new(font, kerning),
            metadata,
            score,
            content_hash,
            glyph_map_hash: content_hash,
            #[cfg(feature = "text-shaping")]
            shaping: Arc::new(Mutex::new(crate::text_shaping::ShapingCache::new(&[]))),
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
        bytes: impl Into<FontBytes>,
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
        bytes: impl Into<FontBytes>,
        variations: &[([u8; 4], f32)],
    ) -> Result<Self, SoftwareTextFontError> {
        let bytes = bytes.into();
        let mut hasher = default_hash::new();
        bytes.as_slice().hash(&mut hasher);
        let mut metadata = software_text_font_metadata(bytes.as_slice());
        metadata.registered_family = Some(FontFamilyKey::of(family));
        metadata.weight = weight;
        metadata.style = style;

        let invalid = |_| SoftwareTextFontError::InvalidFont;
        let InstancedFace {
            font,
            kerning,
            variations,
        } = match bytes {
            FontBytes::Static(bytes) => instance_face(
                FontRef::try_from_slice(bytes).map_err(invalid)?,
                weight,
                style,
                variations,
            ),
            FontBytes::Owned(bytes) => instance_face(
                FontVec::try_from_vec(bytes).map_err(invalid)?,
                weight,
                style,
                variations,
            ),
        }?;
        for (tag, value) in &variations {
            tag.hash(&mut hasher);
            value.to_bits().hash(&mut hasher);
        }
        let content_hash = hasher.finish();

        let score = text_font_score_from_parts(&font, &metadata);
        Ok(Self {
            font: KernedFont::new(font, kerning),
            metadata,
            score,
            content_hash,
            glyph_map_hash: content_hash,
            #[cfg(feature = "text-shaping")]
            shaping: Arc::new(Mutex::new(crate::text_shaping::ShapingCache::new(
                &variations,
            ))),
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

    pub(crate) fn shaped_for(&self, style: &TextStyle) -> Cow<'_, Self> {
        match self
            .font
            .with_feature_settings(style.span_style.font_feature_settings.as_deref())
        {
            None => Cow::Borrowed(self),
            Some(font) => Cow::Owned(Self {
                glyph_map_hash: self.content_hash ^ font.feature_key(),
                font,
                metadata: self.metadata.clone(),
                score: self.score,
                content_hash: self.content_hash,
                #[cfg(feature = "text-shaping")]
                shaping: Arc::clone(&self.shaping),
            }),
        }
    }

    /// Whether `text` is ASCII and this face supports each of its graphemes
    /// as [`font_supports_grapheme`] decides: each character is a control
    /// character or has a glyph. An ASCII grapheme is one character, or
    /// `\r\n`, two control characters. The face's glyph table answers each
    /// character with one load, so no grapheme split or cmap search runs.
    fn takes_ascii(&self, text: &str) -> bool {
        text.is_ascii()
            && text.bytes().map(char::from).all(|ch| {
                ch.is_ascii_control()
                    || self
                        .font
                        .ascii_glyph(ch)
                        .is_some_and(|(glyph, _)| glyph.0 != 0)
            })
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

    #[cfg(feature = "text-shaping")]
    fn shaped_run(
        &self,
        text: &str,
        style: &TextStyle,
    ) -> Option<Arc<crate::text_shaping::ShapedRun>> {
        self.shaping
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .shape(self.font.font_data(), text, style)
    }
}

pub fn try_default_software_text_font() -> Result<SoftwareTextFont, SoftwareTextFontError> {
    #[cfg(feature = "embedded-default-font")]
    {
        SoftwareTextFont::from_bytes(DEFAULT_SOFTWARE_TEXT_FONT_BYTES)
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

    pub fn from_fonts_or_default(fonts: &[&'static [u8]]) -> Self {
        let mut parsed = Vec::with_capacity(fonts.len().max(1));
        for font in fonts {
            if let Ok(candidate) = SoftwareTextFont::from_bytes(*font) {
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

    /// Resolves the family and style, then uses Compose's directional CSS weight
    /// matching: lighter first below 400, heavier first above 500, and weights
    /// up to 500 before lighter or heavier alternatives in the 400–500 interval.
    pub fn resolve(&self, style: &TextStyle) -> Option<&SoftwareTextFont> {
        self.resolve_for_request(FontSelectionRequest::resolve(
            style,
            &self.registered_families,
        ))
    }

    fn resolve_for_request(&self, request: FontSelectionRequest<'_>) -> Option<&SoftwareTextFont> {
        let mut best: Option<(usize, FontMatchScore)> = None;
        for (index, font) in self.fonts.iter().enumerate() {
            let Some(score) = font_match_score(font, request) else {
                continue;
            };
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some((index, score));
            }
        }

        let index = best.map(|(index, _)| index).or(self.default_index);
        index.and_then(|index| self.fonts.get(index))
    }

    fn resolve_grapheme<'a>(
        &'a self,
        primary: &'a SoftwareTextFont,
        grapheme: &str,
        request: FontSelectionRequest<'_>,
    ) -> &'a SoftwareTextFont {
        if font_supports_grapheme(primary, grapheme) {
            return primary;
        }

        self.fonts
            .iter()
            .enumerate()
            .filter(|(_, font)| font_supports_grapheme(font, grapheme))
            .min_by_key(|(index, font)| {
                (
                    u8::from(!request.family.matches(font)),
                    font_match_score_unfiltered(font, request.target_weight, request.target_style),
                    *index,
                )
            })
            .map_or(primary, |(_, font)| font)
    }

    pub(crate) fn visit_font_runs<'a>(
        &'a self,
        text: &'a str,
        style: &TextStyle,
        mut visit: impl FnMut(std::ops::Range<usize>, &'a SoftwareTextFont),
    ) -> Option<()> {
        let request = FontSelectionRequest::resolve(style, &self.registered_families);
        let primary = self.resolve_for_request(request)?;
        if self.fonts.len() == 1 || primary.takes_ascii(text) {
            if !text.is_empty() {
                visit(0..text.len(), primary);
            }
            return Some(());
        }
        let mut run_start = 0;
        let mut run_font = None;
        for (start, grapheme) in text.grapheme_indices(true) {
            let font = self.resolve_grapheme(primary, grapheme, request);
            match run_font {
                None => {
                    run_start = start;
                    run_font = Some(font);
                }
                Some(previous) if !std::ptr::eq(previous, font) => {
                    visit(run_start..start, previous);
                    run_start = start;
                    run_font = Some(font);
                }
                Some(_) => {}
            }
        }
        if let Some(font) = run_font {
            visit(run_start..text.len(), font);
        }
        Some(())
    }

    pub(crate) fn single_font_for_text(
        &self,
        text: &str,
        style: &TextStyle,
    ) -> Option<&SoftwareTextFont> {
        self.resolve_covering(text, style)
            .and_then(|(primary, covers)| covers.then_some(primary))
    }

    fn resolve_covering(&self, text: &str, style: &TextStyle) -> Option<(&SoftwareTextFont, bool)> {
        let request = FontSelectionRequest::resolve(style, &self.registered_families);
        let primary = self.resolve_for_request(request)?;
        let covers = self.fonts.len() == 1
            || primary.takes_ascii(text)
            || text.graphemes(true).all(|grapheme| {
                std::ptr::eq(primary, self.resolve_grapheme(primary, grapheme, request))
            });
        Some((primary, covers))
    }
}

fn font_supports_grapheme(font: &SoftwareTextFont, grapheme: &str) -> bool {
    grapheme.chars().all(|ch| {
        ch.is_control()
            || matches!(ch, '\u{200c}' | '\u{200d}' | '\u{2066}'..='\u{2069}' | '\u{fe00}'..='\u{fe0f}')
            || font.font.glyph_id(ch).0 != 0
    })
}

pub fn software_text_font_from_fonts_or_default(
    fonts: &[&'static [u8]],
) -> Option<SoftwareTextFont> {
    SoftwareTextFontSet::from_fonts_or_default(fonts)
        .default_font()
        .cloned()
}

pub fn software_text_font_set_from_fonts_or_default(
    fonts: &[&'static [u8]],
) -> SoftwareTextFontSet {
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
            Some(family @ FontFamily::Named(name)) => Self::Named {
                name: name.as_str(),
                key: FontFamilyKey::of(family),
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

#[derive(Clone, Copy)]
struct FontSelectionRequest<'a> {
    family: FontFamilyRequest<'a>,
    target_weight: FontWeight,
    target_style: FontStyle,
}

impl<'a> FontSelectionRequest<'a> {
    fn resolve(style: &'a TextStyle, registered: &[FontFamilyKey]) -> Self {
        Self {
            family: FontFamilyRequest::resolve(style.span_style.font_family.as_ref(), registered),
            target_weight: style.span_style.font_weight.unwrap_or_default(),
            target_style: style.span_style.font_style.unwrap_or_default(),
        }
    }
}

type FontMatchScore = (u32, u8, u16);

fn font_match_score(
    font: &SoftwareTextFont,
    request: FontSelectionRequest<'_>,
) -> Option<FontMatchScore> {
    if !request.family.matches(font) {
        return None;
    }
    Some(font_match_score_unfiltered(
        font,
        request.target_weight,
        request.target_style,
    ))
}

fn font_match_score_unfiltered(
    font: &SoftwareTextFont,
    target_weight: FontWeight,
    target_style: FontStyle,
) -> FontMatchScore {
    let style_penalty = if font.style() == target_style {
        0
    } else {
        10_000
    };
    let (weight_group, weight_distance) = font_weight_rank(font.weight(), target_weight);
    let coverage_penalty =
        (21usize.saturating_sub(text_font_score(font).supported_latin_chars) as u32) * 1_000;

    (
        style_penalty + coverage_penalty,
        weight_group,
        weight_distance,
    )
}

fn font_weight_rank(candidate: FontWeight, requested: FontWeight) -> (u8, u16) {
    let candidate = candidate.value();
    let requested = requested.value();
    let group = if requested < 400 {
        u8::from(candidate > requested)
    } else if requested > 500 {
        u8::from(candidate < requested)
    } else if candidate < requested {
        1
    } else if candidate <= 500 {
        0
    } else {
        2
    };
    (group, candidate.abs_diff(requested))
}

fn font_family_matches(font: &SoftwareTextFont, requested: &str) -> bool {
    font.family_names()
        .iter()
        .any(|family| family.eq_ignore_ascii_case(requested))
}

/// Instances `font` at the axis positions `weight` and `style` declare, then
/// at the explicit `variations`, which win over them.
fn instance_face<F: Font + VariableFont + Send + Sync + 'static>(
    mut font: F,
    weight: FontWeight,
    style: FontStyle,
    variations: &[([u8; 4], f32)],
) -> Result<InstancedFace, SoftwareTextFontError> {
    let mut applied = apply_declared_variations(&mut font, weight, style);
    for &(tag, value) in variations {
        let valid = value.is_finite()
            && font
                .variations()
                .iter()
                .any(|axis| axis.tag == tag && (axis.min_value..=axis.max_value).contains(&value));
        if !valid || !font.set_variation(&tag, value) {
            return Err(SoftwareTextFontError::InvalidVariation { tag });
        }
        applied.retain(|(existing, _)| *existing != tag);
        applied.push((tag, value));
    }
    applied.sort_unstable_by_key(|(tag, _)| *tag);
    let kerning = KernedFont::read_kerning(font.font_data(), &applied);
    Ok(InstancedFace {
        font: FontArc::new(font),
        kerning,
        variations: applied,
    })
}

fn apply_declared_variations(
    font: &mut impl VariableFont,
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

/// A text metrics lookup's parameters besides the text: font size bits,
/// style hash and span styles hash.
type TextMetricsParams = (u32, u64, u64, usize);

struct SoftwareTextMetricsCache {
    map: PassAgedCache<TextCacheKey<TextMetricsParams>, TextMetrics>,
    glyph_metrics: SoftwareTextGlyphMetricsCache,
}

impl SoftwareTextMetricsCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: PassAgedCache::with_capacity_at_least_one(capacity),
            glyph_metrics: SoftwareTextGlyphMetricsCache::new(),
        }
    }

    fn get_or_measure(
        &mut self,
        fonts: &SoftwareTextFontSet,
        text: &AnnotatedString,
        style: &TextStyle,
    ) -> TextMetrics {
        self.get_or_measure_range(fonts, text, 0..text.text.len(), style)
    }

    fn get_or_measure_range(
        &mut self,
        fonts: &SoftwareTextFontSet,
        text: &AnnotatedString,
        range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        let font_size = resolve_font_size(style);
        let probe = TextProbe::new(
            &text.text[range.clone()],
            (
                font_size.to_bits(),
                style.measurement_hash(),
                text.span_measurement_hash(),
                range.start,
            ),
        );
        if let Some(metrics) = self.map.get(probe.key()).copied() {
            return metrics;
        }

        let metrics = if range == (0..text.text.len()) {
            measure_annotated_text_with_font_set_cached(text, style, font_size, fonts, self)
        } else {
            let layout = AnnotatedTextLayout::new_range(
                text.into(),
                range.clone(),
                style,
                font_size,
                1.0,
                measure_grid(),
                fonts,
            );
            layout.map_or_else(
                || fallback_text_metrics(&text.text[range], style, font_size),
                |layout| measure_resolved_text(&layout, Some(self)),
            )
        };
        self.map.push(probe.to_owned_key(), metrics);
        metrics
    }

    /// Drops the measurements the last layout passes did not use.
    fn begin_layout_pass(&mut self) {
        self.map.begin_pass(|_| {});
    }
}

/// Whether `line_range` is one line of `text` that prefix widths can
/// measure: a range on character boundaries, without a line break, in a
/// style shaped glyph by glyph.
fn measures_line_prefixes(
    text: &AnnotatedString,
    line_range: &std::ops::Range<usize>,
    style: &TextStyle,
) -> bool {
    !requires_font_layout(&text.text, style)
        && style_allows_prefix_widths(style)
        && is_single_line_range(text, line_range)
}

fn is_single_line_range(text: &AnnotatedString, line_range: &std::ops::Range<usize>) -> bool {
    line_range.start <= line_range.end
        && line_range.end <= text.text.len()
        && text.text.is_char_boundary(line_range.start)
        && text.text.is_char_boundary(line_range.end)
        && !text.text[line_range.clone()].contains('\n')
}

pub(crate) fn requires_font_layout(text: &str, style: &TextStyle) -> bool {
    #[cfg(feature = "text-shaping")]
    {
        crate::text_shaping::required(text, style)
    }
    #[cfg(not(feature = "text-shaping"))]
    {
        let _ = (text, style);
        false
    }
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
    glyphs: DirectMappedCache<GlyphMetricsKey, CachedGlyphMetrics>,
    kerns: DirectMappedCache<KernMetricsKey, f32>,
    stats: SoftwareTextGlyphMetricsStats,
}

impl SoftwareTextGlyphMetricsCache {
    fn new() -> Self {
        Self {
            glyphs: DirectMappedCache::with_slots_log2(SOFTWARE_TEXT_GLYPH_METRICS_SLOTS_LOG2),
            kerns: DirectMappedCache::with_slots_log2(SOFTWARE_TEXT_KERN_METRICS_SLOTS_LOG2),
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
        // The face's ASCII table answers, and the font was read at most
        // once for it, so it counts as a hit.
        if let Some((glyph_id, advance)) = font.font.ascii_glyph(ch) {
            self.stats.glyph_hits = self.stats.glyph_hits.saturating_add(1);
            return CachedGlyphMetrics {
                glyph_id,
                advance_unscaled: advance.max(0.0),
            };
        }
        let key = GlyphMetricsKey {
            font_hash: font.glyph_map_hash,
            ch,
        };
        if let Some(metrics) = self.glyphs.get(&key) {
            self.stats.glyph_hits = self.stats.glyph_hits.saturating_add(1);
            return metrics;
        }

        let glyph_id = scaled_font.font().glyph_id(ch);
        let metrics = CachedGlyphMetrics {
            glyph_id,
            advance_unscaled: scaled_font.font().h_advance_unscaled(glyph_id).max(0.0),
        };
        self.glyphs.insert(key, metrics);
        self.stats.glyph_misses = self.stats.glyph_misses.saturating_add(1);
        metrics
    }

    fn kern<F, S>(
        &mut self,
        font: &SoftwareTextFont,
        scaled_font: &S,
        previous: (char, GlyphId),
        next: (char, GlyphId),
    ) -> f32
    where
        F: Font,
        S: ScaleFont<F>,
    {
        if let Some(kern) = font.font.ascii_kern(previous, next) {
            self.stats.kern_hits = self.stats.kern_hits.saturating_add(1);
            return kern;
        }
        let ((_, previous_id), (_, glyph_id)) = (previous, next);
        let key = KernMetricsKey {
            font_hash: font.content_hash(),
            previous_id: previous_id.0.into(),
            glyph_id: glyph_id.0.into(),
        };
        if let Some(kern) = self.kerns.get(&key) {
            self.stats.kern_hits = self.stats.kern_hits.saturating_add(1);
            return kern;
        }

        let kern = scaled_font.font().kern_unscaled(previous_id, glyph_id);
        self.kerns.insert(key, kern);
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

    pub fn from_fonts_or_default(fonts: &[&'static [u8]], cache_capacity: usize) -> Self {
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

    fn begin_layout_pass(&self) {
        self.lock_cache().begin_layout_pass();
    }

    fn measure_subsequence(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> TextMetrics {
        let start = range.start.min(text.text.len());
        let end = range.end.max(start).min(text.text.len());
        self.lock_cache()
            .get_or_measure_range(&self.fonts, text, start..end, style)
    }

    fn measure_line_prefix_widths(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<TextLinePrefixWidths> {
        if !measures_line_prefixes(text, &line_range, style) {
            return None;
        }
        let char_count = text.text[line_range.clone()].chars().count();
        let mut sink = PrefixWidthVecs {
            prefix_widths: Vec::with_capacity(char_count + 1),
            separator_before: Vec::with_capacity(char_count),
        };
        sink.prefix_widths.push(0.0);
        let overhang = walk_line_prefix_widths(
            text,
            line_range,
            style,
            &self.fonts,
            &mut self.lock_cache().glyph_metrics,
            &mut sink,
        );
        TextLinePrefixWidths::from_parts(sink.prefix_widths, sink.separator_before, overhang)
    }

    fn measure_line_width(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        line_range: std::ops::Range<usize>,
        style: &TextStyle,
    ) -> Option<f32> {
        if requires_font_layout(&text.text, style) && is_single_line_range(text, &line_range) {
            return Some(self.measure_subsequence(text, line_range, style).width);
        }
        if !measures_line_prefixes(text, &line_range, style) {
            return None;
        }
        let mut sink = LineWidthSink::default();
        let overhang = walk_line_prefix_widths(
            text,
            line_range,
            style,
            &self.fonts,
            &mut self.lock_cache().glyph_metrics,
            &mut sink,
        );
        sink.width(overhang)
    }

    fn line_height(&self, text: &cranpose_ui::text::AnnotatedString, style: &TextStyle) -> f32 {
        let font_size = resolve_font_size(style);
        max_line_height_for_annotated_text_with_resolver(text, style, font_size, &self.fonts)
    }

    fn glyph_line_box(&self, style: &TextStyle) -> Option<(f32, f32)> {
        Some(font_glyph_line_box(style, self.fonts.resolve(style)?))
    }

    fn first_baseline(&self, style: &TextStyle) -> Option<f32> {
        Some(self.line_box(style)?.baseline)
    }

    fn line_box(&self, style: &TextStyle) -> Option<cranpose_ui::text::LineBox> {
        let font = self.fonts.resolve(style)?;
        Some(font_line_box(style, font, resolve_font_size(style)))
    }

    fn line_box_and_height(
        &self,
        _node_id: Option<cranpose_core::NodeId>,
        text: &AnnotatedString,
        style: &TextStyle,
    ) -> (Option<cranpose_ui::text::LineBox>, f32) {
        let Some((font, covers)) = self.fonts.resolve_covering(&text.text, style) else {
            return (None, self.line_height(text, style));
        };
        let line_box = font_line_box(style, font, resolve_font_size(style));
        let height = if covers && text.span_styles.is_empty() {
            line_box.height
        } else {
            self.line_height(text, style)
        };
        (Some(line_box), height)
    }

    fn visit_line_boxes(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        visit: &mut dyn FnMut(cranpose_ui::text::LineBox),
    ) -> Option<()> {
        visit_annotated_line_boxes(text, style, &self.fonts, visit)
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        annotated_offset_for_position(text, style, x, y, &self.fonts)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        annotated_cursor_x_for_offset(text, style, offset, &self.fonts)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &TextStyle,
    ) -> TextLayoutResult {
        layout_annotated_text_with_font_set(text, style, &self.fonts)
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

/// Atlas metrics of the glyphs a segment's walk has met, by printable ASCII
/// character: a segment repeats its letters, and the mask cache answers each
/// lookup with a hash and a recency update. Valid for one segment, whose
/// glyphs share a face, size and synthesis.
struct SegmentGlyphMetrics {
    /// Per character, its index in `metrics` plus one; 0 while unmet.
    slots: [u8; ASCII_COUNT],
    metrics: Vec<CachedAtlasGlyphMetrics>,
}

impl SegmentGlyphMetrics {
    fn new() -> Self {
        Self {
            slots: [0; ASCII_COUNT],
            metrics: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.slots.fill(0);
        self.metrics.clear();
    }

    fn get(&self, ch: char) -> Option<CachedAtlasGlyphMetrics> {
        let index = usize::from(self.slots[ascii_slot(ch)?]).checked_sub(1)?;
        self.metrics.get(index).copied()
    }

    fn insert(&mut self, ch: char, metrics: CachedAtlasGlyphMetrics) {
        let Some(slot) = ascii_slot(ch) else {
            return;
        };
        let Ok(index) = u8::try_from(self.metrics.len() + 1) else {
            return;
        };
        self.slots[slot] = index;
        self.metrics.push(metrics);
    }
}

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
    kerns: DirectMappedCache<KernMetricsKey, f32>,
    segment_metrics: SegmentGlyphMetrics,
    hits: u64,
    misses: u64,
}

impl SoftwareGlyphRasterCache {
    pub fn with_capacity_at_least_one(capacity: usize) -> Self {
        Self {
            masks: BoundedLruCache::with_capacity_at_least_one(capacity),
            kerns: DirectMappedCache::with_slots_log2(SOFTWARE_TEXT_KERN_METRICS_SLOTS_LOG2),
            segment_metrics: SegmentGlyphMetrics::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// The kerning between two glyphs of the font hashed `font_hash`, in
    /// font units. A GPOS lookup is a binary search per pair, and the runs a
    /// frame lays out repeat the same pairs.
    fn kern_unscaled(
        &mut self,
        font_hash: u64,
        font: &impl Font,
        previous: GlyphId,
        glyph: GlyphId,
    ) -> f32 {
        let key = KernMetricsKey {
            font_hash,
            previous_id: previous.0.into(),
            glyph_id: glyph.0.into(),
        };
        if let Some(kern) = self.kerns.get(&key) {
            return kern;
        }
        let kern = font.kern_unscaled(previous, glyph);
        self.kerns.insert(key, kern);
        kern
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
    if requires_font_layout(text, style) {
        return rasterize_styled_text_region(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            rect,
            Point::new(rect.x, rect.y),
            style,
            fallback_color,
            font_size,
            scale,
            font.into(),
            None,
        );
    }
    let shaped = font.shaped_for(style);
    let font = &*shaped;
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
    if requires_font_layout(text, style) {
        return rasterize_styled_text_region(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            rect,
            Point::new(rect.x, rect.y),
            style,
            fallback_color,
            font_size,
            scale,
            font.into(),
            Some(glyph_cache),
        );
    }
    let shaped = font.shaped_for(style);
    let font = &*shaped;
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

    pub(crate) fn span_boundaries(&self) -> cranpose_ui::text::SpanBoundaries {
        let mut boundaries: cranpose_ui::text::SpanBoundaries =
            smallvec::smallvec![0, self.text.len()];
        for span in self.span_styles {
            boundaries.push(span.range.start);
            boundaries.push(span.range.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries.retain(|b| *b <= self.text.len() && self.text.is_char_boundary(*b));
        boundaries
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
            text: text.text(),
            span_styles: text.span_styles(),
        }
    }
}

fn annotated_line_alignment_offsets(
    layout: &AnnotatedTextLayout<'_>,
    text: &StyledTextRef<'_>,
    style: &TextStyle,
    scale: f32,
) -> Option<Vec<f32>> {
    if !text.text.contains('\n') {
        return None;
    }
    let align_fraction = cranpose_ui::text::text_align_fraction(style, text.text);
    if align_fraction == 0.0 {
        return None;
    }
    let mut offsets = Vec::with_capacity(layout.lines.len());
    let mut block = 0.0_f32;
    for index in 0..layout.lines.len() {
        let advance = layout.walk_line(index, 0.0, |segment| {
            Some(annotated_raster_segment_advance(&segment, scale))
        })?;
        block = block.max(advance);
        offsets.push(advance);
    }
    for offset in &mut offsets {
        *offset = ((block - *offset) * align_fraction).max(0.0);
    }
    Some(offsets)
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

fn annotated_raster_segment_advance(segment: &AnnotatedTextSegment<'_>, scale: f32) -> f32 {
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(segment.text, segment.style)
        && let Some(run) = segment.font.shaped_run(segment.text, segment.style)
    {
        return shaped_run_width(&run, segment.font, segment.style, segment.font_size, scale);
    }
    let font = segment.font.shaped_for(segment.style);
    segment_advance_px(
        &font.font,
        segment.text,
        font.ab_glyph_px_size(segment.font_size) * scale,
        font.metadata
            .tracking
            .resolve(segment.style, segment.font_size)
            * scale,
    )
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
    let line_box = line_box_for(style, metrics, asked_line_height(style, scale), 1.0);
    TextSegmentMetrics {
        font_px_size,
        letter_spacing,
        align_fraction,
        weight_synthesis,
        style_synthesis,
        line_height: line_box.height,
        first_baseline_y: local_rect.y + line_box.first_baseline(),
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

fn style_can_rasterize_direct_solid(style: &TextStyle) -> bool {
    !style
        .span_style
        .shadow
        .is_some_and(|shadow| shadow.color.3 > 0.0)
        && matches!(
            style.span_style.brush.as_ref(),
            None | Some(Brush::Solid(_))
        )
}

fn style_can_atlas_solid_fill(style: &TextStyle) -> bool {
    style_can_rasterize_direct_solid(style)
        && match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
            TextDrawStyle::Fill => true,
            TextDrawStyle::Stroke { width } => !width.is_finite() || width <= 0.0,
        }
}

fn visit_annotated_text_segments<'a>(
    text: impl Into<StyledTextRef<'a>>,
    style: &TextStyle,
    font_size: f32,
    scale: f32,
    fonts: &'a SoftwareTextFontSet,
    visit: impl FnMut(AnnotatedTextSegment<'_>) -> Option<f32>,
) -> Option<TextMetrics> {
    let text = text.into();
    let layout = AnnotatedTextLayout::new(text, style, font_size, scale, 1.0, fonts)?;
    let offsets = annotated_line_alignment_offsets(&layout, &text, style, scale);
    layout.walk(offsets.as_deref(), visit)
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
    let text = text.into();
    rasterize_annotated_text_region(
        text,
        rect,
        Point::new(rect.x, rect.y),
        style,
        fallback_color,
        font_size,
        scale,
        fonts,
        Some(glyph_cache),
    )
}

/// Rasterizes an annotated paragraph into `rect`, preserving its original
/// `text_origin` when the image covers only a clipped portion of the paragraph.
/// Glyphs from every span share line baselines and may extend across line boxes.
#[expect(clippy::too_many_arguments)]
pub fn rasterize_annotated_text_region<'a>(
    text: impl Into<StyledTextRef<'a>>,
    rect: Rect,
    text_origin: Point,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: &SoftwareTextFontSet,
    glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
) -> Option<ImageBitmap> {
    rasterize_styled_text_region(
        text.into(),
        rect,
        text_origin,
        style,
        fallback_color,
        font_size,
        scale,
        fonts.into(),
        glyph_cache,
    )
}

#[expect(clippy::too_many_arguments)]
fn rasterize_styled_text_region(
    text: StyledTextRef<'_>,
    rect: Rect,
    text_origin: Point,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    fonts: FontResolver<'_>,
    mut glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
) -> Option<ImageBitmap> {
    if text_render_request_is_degenerate(text.is_empty(), rect, font_size, scale) {
        return None;
    }
    if text.span_styles.is_empty()
        && !requires_font_layout(text.text, style)
        && text_origin.x == rect.x
        && text_origin.y == rect.y
        && let Some(font) = fonts.single_font_for_text(text.text, style)
    {
        return match glyph_cache.as_deref_mut() {
            Some(cache) => rasterize_text_to_image_with_glyph_cache(
                text.text,
                rect,
                style,
                fallback_color,
                font_size,
                scale,
                font,
                cache,
            ),
            None => rasterize_text_to_image(
                text.text,
                rect,
                style,
                fallback_color,
                font_size,
                scale,
                font,
            ),
        };
    }
    let layout = AnnotatedTextLayout::new(text, style, font_size, scale, 1.0, fonts)?;
    let solid = layout.all_styles(style_can_rasterize_direct_solid);
    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;
    let mut canvas = TextRasterCanvas::new(width, height, solid);
    let offsets = annotated_line_alignment_offsets(&layout, &text, style, scale);
    let brush_extents = layout.has_non_solid_brush().then(|| {
        layout.brush_extents(offsets.as_deref(), scale, |segment| {
            annotated_raster_segment_advance(segment, scale)
        })
    });
    layout.walk(offsets.as_deref(), |segment| {
        let static_text_motion = segment
            .style
            .paragraph_style
            .text_motion
            .unwrap_or(TextMotion::Static)
            == TextMotion::Static;
        let origin = Point::new(
            text_origin.x - rect.x + segment.origin.x,
            text_origin.y - rect.y + segment.origin.y,
        );
        let local_rect = Rect {
            x: if static_text_motion {
                origin.x.round()
            } else {
                origin.x + rect.x.fract()
            },
            y: if static_text_motion {
                origin.y
            } else {
                origin.y + rect.y.fract()
            },
            width: width as f32,
            height: height as f32,
        };
        let fallback = Brush::solid(segment.style.resolve_text_color(fallback_color));
        let (brush, alpha) =
            segment
                .style
                .span_style
                .brush
                .as_ref()
                .map_or((&fallback, 1.0), |brush| {
                    (
                        brush,
                        segment
                            .style
                            .span_style
                            .alpha
                            .unwrap_or(1.0)
                            .clamp(0.0, 1.0),
                    )
                });
        let brush_rect = annotated_brush_rect(
            brush,
            brush_extents.as_deref().unwrap_or_default(),
            &segment,
            text_origin,
            rect,
        )?;
        let paint = GlyphPaint {
            brush,
            alpha,
            shadow: segment
                .style
                .span_style
                .shadow
                .filter(|shadow| shadow.color.3 > 0.0),
            brush_rect,
            canvas_origin: Point::new(rect.x, rect.y),
            scale,
            static_text_motion,
        };
        Some(visit_text_segment_masks(
            segment.text,
            local_rect,
            segment.style,
            segment.font_size,
            scale,
            segment.font,
            glyph_cache.as_deref_mut(),
            &GlyphRasterClip::new(width, height, &paint),
            |mask| canvas.paint(mask, &paint),
        ))
    })?;
    canvas.into_image()
}

fn annotated_brush_rect(
    brush: &Brush,
    extents: &[AnnotatedBrushExtent],
    segment: &AnnotatedTextSegment<'_>,
    text_origin: Point,
    fallback: Rect,
) -> Option<Rect> {
    if matches!(brush, Brush::Solid(_)) {
        return Some(fallback);
    }
    let index = extents
        .binary_search_by_key(&(segment.line_index, segment.style_index), |extent| {
            (extent.line_index, extent.style_index)
        })
        .ok()?;
    Some(extents[index].rect.translate(text_origin.x, text_origin.y))
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
    let text = text.into();
    if text_render_request_is_degenerate(text.is_empty(), rect, font_size, scale) {
        return Some(());
    }
    if text.span_styles.is_empty() && !requires_font_layout(text.text, style) {
        if !text_segment_supports_solid_atlas(style) {
            return None;
        }
        if let Some(font) = fonts.single_font_for_text(text.text, style) {
            return collect_segment(
                text.text,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: rect.width,
                    height: rect.height,
                },
                style,
                style.resolve_text_color(fallback_color),
                font_size,
                scale,
                font,
                glyph_cache,
                out,
            )
            .map(|_| ());
        }
    }
    let initial_len = out.len();
    let result = visit_annotated_text_segments(text, style, font_size, scale, fonts, |segment| {
        if !text_segment_supports_solid_atlas(segment.style) {
            return None;
        }
        collect_segment(
            segment.text,
            Rect {
                x: segment.origin.x.round(),
                y: segment.origin.y,
                width: rect.width,
                height: rect.height,
            },
            segment.style,
            segment.style.resolve_text_color(fallback_color),
            segment.font_size,
            scale,
            segment.font,
            glyph_cache,
            out,
        )
    });
    if result.is_none() {
        out.truncate(initial_len);
    }
    result.map(|_| ())
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

/// Appends the glyphs of `text` that put pixels down to `out`: each either
/// placed from the mask cache or rasterized into it.
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
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(text, style) {
        return measure_annotated_text_with_resolver(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            style,
            font_size,
            font,
            None,
        );
    }
    measure_text_impl(text, style, font_size, font.shaped_for(style).raster_ref())
}

fn measure_text_with_font_cached(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
    cache: &mut SoftwareTextMetricsCache,
) -> TextMetrics {
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(text, style) {
        return measure_shaped_text(text, style, font_size, font);
    }
    measure_text_impl_cached(text, style, font_size, &font.shaped_for(style), cache)
}

pub fn measure_annotated_text_with_font(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
) -> TextMetrics {
    if text.span_styles.is_empty() && !requires_font_layout(&text.text, style) {
        return measure_text_with_font(text.text.as_str(), style, font_size, font);
    }
    measure_annotated_text_with_resolver(text, style, font_size, font, None)
}

pub fn measure_annotated_text_with_font_set(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
) -> TextMetrics {
    if text.span_styles.is_empty()
        && let Some(font) = fonts.single_font_for_text(text.text.as_str(), style)
    {
        return measure_text_with_font(text.text.as_str(), style, font_size, font);
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
    if text.span_styles.is_empty()
        && let Some(font) = fonts.single_font_for_text(text.text.as_str(), style)
    {
        return measure_text_with_font_cached(text.text.as_str(), style, font_size, font, cache);
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
    if requires_font_layout(text, style) {
        return annotated_offset_for_position(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            style,
            x,
            y,
            font,
        );
    }
    if text.is_empty() {
        return 0;
    }

    let shaped = font.shaped_for(style);
    let font = &*shaped;
    let font_size = resolve_font_size(style);
    let line_box = font_line_box(style, font, font_size);
    let line_height = line_box.height;

    let line_index = ((y + line_box.trim_top) / line_height).floor().max(0.0) as usize;
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
    if requires_font_layout(text, style) {
        return annotated_cursor_x_for_offset(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            style,
            offset,
            font,
        );
    }
    let clamped_offset = clamp_to_char_boundary(text, offset.min(text.len()));
    if clamped_offset == 0 {
        return 0.0;
    }

    let font_size = resolve_font_size(style);
    let shaped = font.shaped_for(style);
    let font = &*shaped;
    measure_text_impl(&text[..clamped_offset], style, font_size, font.raster_ref()).width
}

pub fn layout_text_with_font(
    text: &str,
    style: &TextStyle,
    font: &SoftwareTextFont,
) -> TextLayoutResult {
    if requires_font_layout(text, style) {
        return layout_annotated_text_with_font_set(
            StyledTextRef {
                text,
                span_styles: &[],
            },
            style,
            font,
        );
    }
    let shaped = font.shaped_for(style);
    let font = &*shaped;
    let font_size = resolve_font_size(style);
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let resolved_weight = font.weight();
    let font_ref = font.raster_ref();
    let letter_spacing = font.metadata.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, resolved_weight, font_size, 1.0);
    let line_box = font_line_box(style, font, font_size);
    let line_height = line_box.height;
    let font = &font.font;
    let scaled_font = font.as_scaled(PxScale::from(glyph_font_size));

    let mut glyph_x_positions = Vec::new();
    let mut char_to_byte = Vec::new();
    let mut glyph_layouts = Vec::new();
    let mut lines = Vec::new();
    let mut current_x = 0.0f32;
    let mut line_start = 0;
    let mut y = -line_box.trim_top;

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

fn walk_annotated_glyphs(
    segment: &AnnotatedTextSegment<'_>,
    mut visit: impl FnMut(GlyphLayout),
) -> f32 {
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(segment.text, segment.style)
        && let Some(run) = segment.font.shaped_run(segment.text, segment.style)
    {
        let font = segment.font;
        let weight =
            TextWeightSynthesis::for_style(segment.style, font.weight(), segment.font_size, 1.0);
        let natural = font
            .font
            .as_scaled(font.ab_glyph_px_size(segment.font_size))
            .h_scale_factor();
        let scale = run.horizontal_scale(natural, weight.apply_width(natural));
        let spacing = run.spacing(
            font.metadata
                .tracking
                .resolve(segment.style, segment.font_size),
        );
        for (index, cluster) in run.clusters.iter().enumerate() {
            visit(GlyphLayout {
                line_index: segment.line_index,
                start_offset: segment.range.start + cluster.range.start,
                end_offset: segment.range.start + cluster.range.end,
                x: segment.origin.x + cluster.x * scale + (index as f32 + 0.5) * spacing,
                y: segment.line_top,
                width: cluster.advance * scale,
                height: segment.line_height,
            });
        }
        return run.width(scale, spacing);
    }
    let shaped = segment.font.shaped_for(segment.style);
    let font = &*shaped;
    let weight =
        TextWeightSynthesis::for_style(segment.style, font.weight(), segment.font_size, 1.0);
    let glyph_size = font.ab_glyph_px_size(segment.font_size);
    let scaled = font.font.as_scaled(PxScale {
        x: weight.apply_width(glyph_size),
        y: glyph_size,
    });
    let spacing = font
        .metadata
        .tracking
        .resolve(segment.style, segment.font_size);
    let lead = run_lead_in(segment.text, spacing);
    let mut x = segment.origin.x + lead;
    let mut previous = None;
    for (offset, ch) in segment.text.char_indices() {
        let glyph = scaled.glyph_id(ch);
        if let Some(previous) = previous {
            x += scaled.kern(previous, glyph) + spacing;
        }
        let width = scaled.h_advance(glyph);
        visit(GlyphLayout {
            line_index: segment.line_index,
            start_offset: segment.range.start + offset,
            end_offset: segment.range.start + offset + ch.len_utf8(),
            x,
            y: segment.line_top,
            width,
            height: segment.line_height,
        });
        x += width;
        previous = Some(glyph);
    }
    (x - segment.origin.x + lead).max(0.0)
}

fn segment_is_rtl(segment: &AnnotatedTextSegment<'_>) -> bool {
    requires_font_layout(segment.text, segment.style)
        && segment
            .style
            .paragraph_style
            .text_direction
            .resolve(segment.text)
            == cranpose_ui::text::ResolvedTextDirection::Rtl
}

fn cursor_edges(glyph: &GlyphLayout, rtl: bool) -> [(f32, usize); 2] {
    let (start, end) = if rtl {
        (glyph.x + glyph.width, glyph.x)
    } else {
        (glyph.x, glyph.x + glyph.width)
    };
    [(start, glyph.start_offset), (end, glyph.end_offset)]
}

pub(crate) fn layout_annotated_text_with_font_set<'a>(
    text: impl Into<StyledTextRef<'a>>,
    style: &TextStyle,
    fonts: impl Into<FontResolver<'a>>,
) -> TextLayoutResult {
    let text = text.into();
    let fonts = fonts.into();
    if text.span_styles.is_empty()
        && !requires_font_layout(text.text, style)
        && let Some(font) = fonts.single_font_for_text(text.text, style)
    {
        return layout_text_with_font(text.text, style, font);
    }
    let Some(layout) = AnnotatedTextLayout::new(
        text,
        style,
        resolve_font_size(style),
        1.0,
        measure_grid(),
        fonts,
    ) else {
        return fallback_layout_text(text.text, style);
    };
    let char_to_byte: Vec<usize> = text
        .text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(text.text.len()))
        .collect();
    let mut glyph_x_positions = vec![0.0; char_to_byte.len()];
    let mut glyph_layouts = Vec::new();
    let mut lines = Vec::with_capacity(layout.lines.len());
    let mut width = 0.0_f32;
    for (index, line) in layout.lines.iter().enumerate() {
        let mut end_cursor = None;
        let advance = layout
            .walk_line(index, 0.0, |segment| {
                let rtl = segment_is_rtl(&segment);
                Some(walk_annotated_glyphs(&segment, |glyph| {
                    let [(start, offset), (end, end_offset)] = cursor_edges(&glyph, rtl);
                    let first = char_to_byte.partition_point(|byte| *byte < offset);
                    let last = char_to_byte.partition_point(|byte| *byte < end_offset);
                    glyph_x_positions[first..last].fill(start);
                    if end_offset == line.range.end {
                        end_cursor = Some(end);
                    }
                    glyph_layouts.push(glyph);
                }))
            })
            .unwrap_or(0.0);
        width = width.max(advance);
        if let Ok(index) = char_to_byte.binary_search(&line.range.end) {
            glyph_x_positions[index] = end_cursor.unwrap_or(advance);
        }
        lines.push(LineLayout {
            start_offset: line.range.start,
            end_offset: line.range.end,
            y: line.top,
            height: line.line_box.height,
        });
    }
    let metrics = layout.metrics(width);
    TextLayoutResult::new(
        text.text,
        TextLayoutData {
            width: metrics.width,
            height: metrics.height,
            line_height: metrics.line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            glyph_layouts,
        },
    )
}

pub(crate) fn annotated_cursor_x_for_offset<'a>(
    text: impl Into<StyledTextRef<'a>>,
    style: &TextStyle,
    offset: usize,
    fonts: impl Into<FontResolver<'a>>,
) -> f32 {
    let text = text.into();
    let fonts = fonts.into();
    let offset = clamp_to_char_boundary(text.text, offset);
    if text.span_styles.is_empty()
        && !requires_font_layout(text.text, style)
        && let Some(font) = fonts.single_font_for_text(text.text, style)
    {
        return cursor_x_for_offset_with_font(text.text, style, offset, font);
    }
    let Some(layout) = AnnotatedTextLayout::new(
        text,
        style,
        resolve_font_size(style),
        1.0,
        measure_grid(),
        fonts,
    ) else {
        return fallback_cursor_x_for_offset(text.text, style, offset);
    };
    let line_index = layout
        .lines
        .iter()
        .position(|line| offset <= line.range.end)
        .unwrap_or_else(|| layout.lines.len().saturating_sub(1));
    let mut x = 0.0;
    let mut best = (0, false);
    let advance = layout
        .walk_line(line_index, 0.0, |segment| {
            let rtl = segment_is_rtl(&segment);
            Some(walk_annotated_glyphs(&segment, |glyph| {
                for (index, (position, boundary)) in
                    cursor_edges(&glyph, rtl).into_iter().enumerate()
                {
                    let rank = (boundary, index == 0);
                    if boundary <= offset && rank >= best {
                        best = rank;
                        x = position;
                    }
                }
            }))
        })
        .unwrap_or(0.0);
    if !requires_font_layout(text.text, style)
        && layout
            .lines
            .get(line_index)
            .is_some_and(|line| offset >= line.range.end)
    {
        advance
    } else {
        x
    }
}

pub(crate) fn annotated_offset_for_position<'a>(
    text: impl Into<StyledTextRef<'a>>,
    style: &TextStyle,
    x: f32,
    y: f32,
    fonts: impl Into<FontResolver<'a>>,
) -> usize {
    let text = text.into();
    let fonts = fonts.into();
    if text.span_styles.is_empty()
        && !requires_font_layout(text.text, style)
        && let Some(font) = fonts.single_font_for_text(text.text, style)
    {
        return text_offset_for_position_with_font(text.text, style, x, y, font);
    }
    let Some(layout) = AnnotatedTextLayout::new(
        text,
        style,
        resolve_font_size(style),
        1.0,
        measure_grid(),
        fonts,
    ) else {
        return fallback_text_offset_for_position(text.text, style, x, y);
    };
    let line_index = layout
        .lines
        .iter()
        .position(|line| y < line.top + line.line_box.height)
        .unwrap_or_else(|| layout.lines.len().saturating_sub(1));
    let Some(line) = layout.lines.get(line_index) else {
        return 0;
    };
    let mut best = line.range.start;
    let mut distance = f32::INFINITY;
    let advance = layout
        .walk_line(line_index, 0.0, |segment| {
            let rtl = segment_is_rtl(&segment);
            Some(walk_annotated_glyphs(&segment, |glyph| {
                for (position, offset) in cursor_edges(&glyph, rtl) {
                    let delta = (x - position).abs();
                    if delta < distance {
                        distance = delta;
                        best = offset;
                    }
                }
            }))
        })
        .unwrap_or(0.0);
    if !requires_font_layout(text.text, style) && (x - advance).abs() < distance {
        line.range.end
    } else {
        best
    }
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

struct GlyphPaint<'a> {
    brush: &'a Brush,
    alpha: f32,
    shadow: Option<Shadow>,
    brush_rect: Rect,
    canvas_origin: Point,
    scale: f32,
    static_text_motion: bool,
}

struct GlyphRasterClip {
    width: f32,
    height: f32,
    shadow: Option<(Point, f32)>,
}

impl GlyphRasterClip {
    fn new(width: u32, height: u32, paint: &GlyphPaint<'_>) -> Self {
        Self {
            width: width as f32,
            height: height as f32,
            shadow: paint.shadow.map(|shadow| {
                (
                    Point::new(shadow.offset.x * paint.scale, shadow.offset.y * paint.scale),
                    (shadow_blur_sigma((shadow.blur_radius * paint.scale).max(0.0)) * 3.0).ceil()
                        + 1.0,
                )
            }),
        }
    }

    fn intersects(&self, bounds: Rect) -> bool {
        let overlaps = |rect: Rect| {
            rect.x < self.width
                && rect.y < self.height
                && rect.x + rect.width > 0.0
                && rect.y + rect.height > 0.0
        };
        overlaps(bounds)
            || self.shadow.is_some_and(|(offset, margin)| {
                overlaps(Rect {
                    x: bounds.x + offset.x - margin,
                    y: bounds.y + offset.y - margin,
                    width: bounds.width + 2.0 * margin,
                    height: bounds.height + 2.0 * margin,
                })
            })
    }
}

enum TextRasterPixels {
    Solid(Vec<u8>),
    Blended(Vec<[f32; 4]>),
}

struct TextRasterCanvas {
    width: u32,
    height: u32,
    pixels: TextRasterPixels,
}

impl TextRasterCanvas {
    fn new(width: u32, height: u32, solid: bool) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            pixels: if solid {
                TextRasterPixels::Solid(vec![0; len * 4])
            } else {
                TextRasterPixels::Blended(vec![[0.0; 4]; len])
            },
        }
    }

    fn paint(&mut self, mask: &GlyphMask, paint: &GlyphPaint<'_>) {
        match &mut self.pixels {
            TextRasterPixels::Solid(pixels) => {
                if let Brush::Solid(color) = paint.brush {
                    draw_mask_glyph_solid_u8(
                        pixels,
                        self.width,
                        self.height,
                        mask,
                        color_to_rgba(*color),
                        paint.alpha,
                    );
                }
            }
            TextRasterPixels::Blended(pixels) => {
                if let Some(shadow) = paint.shadow {
                    draw_shadow_mask(
                        pixels,
                        self.width,
                        self.height,
                        mask,
                        shadow,
                        paint.scale,
                        paint.static_text_motion,
                    );
                }
                draw_mask_glyph(pixels, self.width, self.height, mask, paint);
            }
        }
    }

    fn into_image(self) -> Option<ImageBitmap> {
        let pixels = match self.pixels {
            TextRasterPixels::Solid(pixels) => pixels,
            TextRasterPixels::Blended(canvas) => {
                let mut pixels = Vec::with_capacity(canvas.len() * 4);
                for pixel in canvas {
                    pixels.extend(
                        pixel.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8),
                    );
                }
                pixels
            }
        };
        ImageBitmap::from_rgba8(self.width, self.height, pixels).ok()
    }
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
    glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
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
    let line_box = line_box_for(style, metrics, asked_line_height(style, scale), 1.0);
    let line_height = line_box.height;
    let first_baseline_y = line_box.first_baseline();

    let mut canvas = TextRasterCanvas::new(
        width,
        height,
        matches!(brush, Brush::Solid(_)) && shadow.is_none(),
    );
    let paint = GlyphPaint {
        brush,
        alpha: brush_alpha_multiplier,
        shadow,
        brush_rect: rect,
        canvas_origin: Point::new(rect.x, rect.y),
        scale,
        static_text_motion,
    };
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
        None,
        |mask| canvas.paint(mask, &paint),
    );
    canvas.into_image()
}

#[expect(clippy::too_many_arguments)]
fn visit_text_segment_masks(
    text: &str,
    local_rect: Rect,
    style: &TextStyle,
    font_size: f32,
    scale: f32,
    font: &SoftwareTextFont,
    glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
    clip: &GlyphRasterClip,
    visit: impl FnMut(&GlyphMask),
) -> f32 {
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return 0.0;
    }
    let raster_style = match style.span_style.draw_style.unwrap_or(TextDrawStyle::Fill) {
        TextDrawStyle::Stroke { width } if width.is_finite() && width > 0.0 => {
            GlyphRasterStyle::Stroke {
                width_px: width * scale,
            }
        }
        _ => GlyphRasterStyle::Fill,
    };
    let static_text_motion = style
        .paragraph_style
        .text_motion
        .unwrap_or(TextMotion::Static)
        == TextMotion::Static;
    let shaped = font.shaped_for(style);
    let font = &*shaped;
    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(text, style) {
        let mut glyph_cache = glyph_cache;
        let mut visit = visit;
        return visit_shaped_glyphs(text, style, font, &m, local_rect.x, |glyph| {
            let glyph = align_glyph_for_text_motion(glyph, static_text_motion);
            if let Some(mask) = raster_glyph_mask(
                if static_text_motion {
                    glyph_cache.as_deref_mut()
                } else {
                    None
                },
                font.content_hash(),
                &font.font,
                &glyph,
                raster_style,
                m.weight_synthesis,
                m.style_synthesis,
                Some(clip),
            ) {
                visit(&mask);
            }
            ControlFlow::Continue(())
        })
        .unwrap_or(0.0);
    }
    visit_text_glyph_masks(
        text,
        &font.font,
        font.content_hash(),
        m.font_px_size,
        m.line_height,
        m.first_baseline_y,
        local_rect.x,
        0.0,
        m.letter_spacing,
        m.align_fraction,
        static_text_motion,
        raster_style,
        m.weight_synthesis,
        m.style_synthesis,
        glyph_cache,
        Some(clip),
        visit,
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
    let request = AtlasSegmentRequest {
        text,
        local_rect,
        style,
        font_size,
        scale,
        font,
    };
    collect_atlas_segment(request, glyph_cache, out, |cache, glyph, out| {
        let mask = match cache.get(&glyph.key, &glyph.glyph) {
            Some(mask) => mask,
            None => {
                let Some(mask) = glyph.build_mask() else {
                    return ControlFlow::Continue(());
                };
                cache.put(glyph.key, &glyph.glyph, mask)
            }
        };
        if let Some(key) = glyph_atlas_key_from_mask_key(glyph.key)
            && mask.width != 0
            && mask.height != 0
        {
            out.push(SoftwareGlyphAtlasGlyph {
                key,
                mask: SoftwareGlyphAtlasMask {
                    alpha: mask.alpha,
                    width: mask.width,
                    height: mask.height,
                },
                x: mask.origin_x,
                y: mask.origin_y,
                color,
            });
        }
        ControlFlow::Continue(())
    })
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
    let request = AtlasSegmentRequest {
        text,
        local_rect,
        style,
        font_size,
        scale,
        font,
    };
    collect_atlas_segment(request, glyph_cache, out, |cache, glyph, out| {
        let Some((key, x, y, width, height)) = cache.get_atlas_placement(&glyph.key, &glyph.glyph)
        else {
            // A glyph with an outline but no cached mask cannot be placed
            // without rasterizing it, which this walk does not do.
            return if glyph.has_outline() {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            };
        };
        if width != 0 && height != 0 {
            out.push(SoftwareGlyphAtlasPlacement {
                key,
                x,
                y,
                width,
                height,
                color,
            });
        }
        ControlFlow::Continue(())
    })
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
    let request = AtlasSegmentRequest {
        text,
        local_rect,
        style,
        font_size,
        scale,
        font,
    };
    glyph_cache.segment_metrics.clear();
    let transparent = color.3 <= 0.0;
    collect_atlas_segment(request, glyph_cache, out, |cache, glyph, out| {
        if transparent {
            return ControlFlow::Continue(());
        }
        let metrics = match cache.segment_metrics.get(glyph.ch) {
            Some(metrics) => metrics,
            None => match cache.get_atlas_metrics(&glyph.key) {
                Some(metrics) => {
                    cache.segment_metrics.insert(glyph.ch, metrics);
                    metrics
                }
                None => {
                    if let Some(new) = rasterize_atlas_run_glyph(cache, glyph)
                        && new.mask.width != 0
                        && new.mask.height != 0
                    {
                        out.push(SoftwareGlyphAtlasRunGlyph::New(SoftwareGlyphAtlasGlyph {
                            color,
                            ..new
                        }));
                    }
                    return ControlFlow::Continue(());
                }
            },
        };
        if metrics.width != 0 && metrics.height != 0 {
            out.push(SoftwareGlyphAtlasRunGlyph::Cached(
                metrics.placement(&glyph.glyph, color),
            ));
        }
        ControlFlow::Continue(())
    })
}

/// Rasterizes a glyph no cache holds yet into the mask cache and the
/// segment's metrics; `None` for a glyph that draws nothing.
fn rasterize_atlas_run_glyph(
    cache: &mut SoftwareGlyphRasterCache,
    glyph: &AtlasSegmentGlyph<'_>,
) -> Option<SoftwareGlyphAtlasGlyph> {
    if !glyph.has_outline() {
        return None;
    }
    let mask = glyph.build_mask()?;
    let mask = cache.put(glyph.key, &glyph.glyph, mask);
    let key = glyph_atlas_key_from_mask_key(glyph.key)?;
    let (glyph_x, glyph_y) = static_glyph_pixel_origin(&glyph.glyph);
    cache.segment_metrics.insert(
        glyph.ch,
        CachedAtlasGlyphMetrics {
            key,
            width: mask.width,
            height: mask.height,
            origin_offset_x: mask.origin_x - glyph_x,
            origin_offset_y: mask.origin_y - glyph_y,
        },
    );
    Some(SoftwareGlyphAtlasGlyph {
        key,
        mask: SoftwareGlyphAtlasMask {
            alpha: mask.alpha,
            width: mask.width,
            height: mask.height,
        },
        x: mask.origin_x,
        y: mask.origin_y,
        color: Color::WHITE,
    })
}

/// One styled segment of a text, laid out for the solid glyph atlas.
#[derive(Clone, Copy)]
struct AtlasSegmentRequest<'a> {
    text: &'a str,
    local_rect: Rect,
    style: &'a TextStyle,
    font_size: f32,
    scale: f32,
    font: &'a SoftwareTextFont,
}

/// A glyph of a segment walked for the solid glyph atlas: where it draws
/// and its mask's cache key.
struct AtlasSegmentGlyph<'a> {
    ch: char,
    glyph: Glyph,
    key: GlyphMaskCacheKey,
    font: &'a KernedFont,
    metrics: &'a TextSegmentMetrics,
}

impl AtlasSegmentGlyph<'_> {
    fn has_outline(&self) -> bool {
        self.font.outline(self.glyph.id).is_some()
    }

    fn build_mask(&self) -> Option<GlyphMask> {
        build_complete_glyph_mask(
            self.font,
            &self.glyph,
            GlyphRasterStyle::Fill,
            self.metrics.weight_synthesis,
            self.metrics.style_synthesis,
            None,
        )
    }
}

impl<'a> AtlasSegmentGlyph<'a> {
    fn new(
        ch: char,
        glyph: Glyph,
        font: &'a SoftwareTextFont,
        metrics: &'a TextSegmentMetrics,
    ) -> Self {
        let glyph = align_glyph_for_text_motion(glyph, true);
        Self {
            ch,
            key: glyph_mask_cache_key(
                font.content_hash(),
                &glyph,
                GlyphRasterStyle::Fill,
                metrics.weight_synthesis,
                metrics.style_synthesis,
            ),
            glyph,
            font: &font.font,
            metrics,
        }
    }
}

/// Walks a segment's glyphs where they draw, handing each to `visit` with
/// `out`: each line from its aligned start, each glyph on the pixel grid
/// after the previous one's advance, kerning and letter spacing. The widest
/// line's advance, or `None` with `out` as it was when the style cannot draw
/// from the atlas or `visit` breaks.
fn collect_atlas_segment<T>(
    request: AtlasSegmentRequest<'_>,
    glyph_cache: &mut SoftwareGlyphRasterCache,
    out: &mut Vec<T>,
    mut visit: impl FnMut(
        &mut SoftwareGlyphRasterCache,
        &AtlasSegmentGlyph<'_>,
        &mut Vec<T>,
    ) -> ControlFlow<()>,
) -> Option<f32> {
    let AtlasSegmentRequest {
        text,
        local_rect,
        style,
        font_size,
        scale,
        font,
    } = request;
    if text_render_request_is_degenerate(text.is_empty(), local_rect, font_size, scale) {
        return Some(0.0);
    }
    if !text_segment_supports_solid_atlas(style) {
        return None;
    }

    let shaped = font.shaped_for(style);
    let font = &*shaped;
    let m = text_segment_metrics(text, local_rect, style, font_size, scale, font);
    let font_hash = font.content_hash();
    let kerned = &font.font;
    let origin_x = local_rect.x.round();
    let initial_len = out.len();
    #[cfg(feature = "text-shaping")]
    if requires_font_layout(text, style) {
        let result = visit_shaped_glyphs(text, style, font, &m, origin_x, |glyph| {
            let glyph = AtlasSegmentGlyph::new('\0', glyph, font, &m);
            visit(glyph_cache, &glyph, out)
        });
        if result.is_none() {
            out.truncate(initial_len);
        }
        return result;
    }
    let px_scale = PxScale::from(m.font_px_size);
    let scaled_font = kerned.as_scaled(px_scale);
    let h_scale = scaled_font.h_scale_factor();
    let line_offsets =
        line_alignment_offsets(&scaled_font, text, m.letter_spacing, m.align_fraction);
    let mut max_advance = 0.0f32;
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = m.first_baseline_y + line_idx as f32 * m.line_height;
        let lead_in = run_lead_in(line, m.letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous: Option<(char, GlyphId)> = None;
        for ch in line.chars() {
            let (glyph_id, advance) = kerned.glyph_advance(ch);
            if let Some(previous) = previous {
                let kern = kerned
                    .ascii_kern(previous, (ch, glyph_id))
                    .unwrap_or_else(|| {
                        glyph_cache.kern_unscaled(font_hash, kerned, previous.1, glyph_id)
                    });
                caret_x += kern * h_scale + m.letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(px_scale, point(caret_x, baseline_y));
            caret_x += advance * h_scale;
            previous = Some((ch, glyph_id));
            let segment_glyph = AtlasSegmentGlyph::new(ch, glyph, font, &m);
            if visit(glyph_cache, &segment_glyph, out).is_break() {
                out.truncate(initial_len);
                return None;
            }
        }
        max_advance = max_advance.max((caret_x - origin_x + lead_in).max(0.0));
    }
    Some(max_advance)
}

fn resolve_font_size(style: &TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

#[cfg(feature = "text-shaping")]
fn shaped_run_width(
    run: &crate::text_shaping::ShapedRun,
    font: &SoftwareTextFont,
    style: &TextStyle,
    font_size: f32,
    scale: f32,
) -> f32 {
    let scaled = font
        .font
        .as_scaled(font.ab_glyph_px_size(font_size) * scale);
    let weight = TextWeightSynthesis::for_style(style, font.weight(), font_size, scale);
    run.width(
        run.horizontal_scale(
            scaled.h_scale_factor(),
            weight.apply_width(scaled.h_scale_factor()),
        ),
        font.metadata.tracking.resolve(style, font_size) * scale,
    )
}

#[cfg(feature = "text-shaping")]
fn measure_shaped_text(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
) -> TextMetrics {
    let line_box = font_line_box(style, font, font_size);
    let mut width = 0.0_f32;
    let mut count = 0;
    for line in text.split('\n') {
        count += 1;
        if let Some(run) = font.shaped_run(line, style) {
            let overhang = if line.is_empty() {
                0.0
            } else {
                TextStyleSynthesis::for_style(style, font.style(), font_size, 1.0)
                    .visual_overhang_px()
            };
            width = width.max(shaped_run_width(&run, font, style, font_size, 1.0) + overhang);
        }
    }
    TextMetrics {
        width,
        height: line_box.block_height(count),
        line_height: line_box.height,
        line_count: count,
    }
}

#[cfg(feature = "text-shaping")]
fn visit_shaped_glyphs(
    text: &str,
    style: &TextStyle,
    font: &SoftwareTextFont,
    m: &TextSegmentMetrics,
    origin_x: f32,
    mut visit: impl FnMut(Glyph) -> ControlFlow<()>,
) -> Option<f32> {
    let scaled = font.font.as_scaled(m.font_px_size);
    let natural_scale = scaled.h_scale_factor();
    let synthesized_scale = m.weight_synthesis.apply_width(natural_scale);
    let scale_y = scaled.v_scale_factor();
    let mut width = 0.0_f32;
    for line in text.split('\n') {
        let run = font.shaped_run(line, style)?;
        width = width.max(run.width(
            run.horizontal_scale(natural_scale, synthesized_scale),
            m.letter_spacing,
        ));
    }
    for (index, line) in text.split('\n').enumerate() {
        let run = font.shaped_run(line, style)?;
        let scale_x = run.horizontal_scale(natural_scale, synthesized_scale);
        let x = origin_x + (width - run.width(scale_x, m.letter_spacing)) * m.align_fraction;
        let y = m.first_baseline_y + index as f32 * m.line_height;
        let mut result = ControlFlow::Continue(());
        run.visit(scale_x, scale_y, m.letter_spacing, |id, dx, dy| {
            if result.is_continue() {
                result = visit(id.with_scale_and_position(m.font_px_size, point(x + dx, y + dy)));
            }
        });
        if result.is_break() {
            return None;
        }
    }
    Some(width)
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

pub(crate) fn font_extent(
    font: &SoftwareTextFont,
    font_size: f32,
) -> cranpose_ui::text::FontExtent {
    let metrics = vertical_metrics(&font.font, font.ab_glyph_px_size(font_size));
    cranpose_ui::text::FontExtent::new(metrics.ascent, -metrics.descent, metrics.line_gap)
}

/// The paragraph line box `font` gives `style` at `font_size`.
pub(crate) fn font_line_box(
    style: &TextStyle,
    font: &SoftwareTextFont,
    font_size: f32,
) -> cranpose_ui::text::LineBox {
    line_box_for(
        style,
        crate::font_layout::vertical_metrics(&font.font, font.ab_glyph_px_size(font_size)),
        asked_line_height(style, 1.0),
        measure_grid(),
    )
}

/// The font's own ascent-to-descent box inside a line of `style`, as
/// `(top_offset, height)` from the paragraph's line grid: the first line's
/// glyphs start `top_offset` below the paragraph's top, and every later line's
/// a whole advance further down.
pub(crate) fn font_glyph_line_box(style: &TextStyle, font: &SoftwareTextFont) -> (f32, f32) {
    let font_size = resolve_font_size(style);
    let metrics =
        crate::font_layout::vertical_metrics(&font.font, font.ab_glyph_px_size(font_size));
    let resolved = font_line_box(style, font, font_size);
    let height = metrics.natural_line_height.min(resolved.height).max(1.0);
    (resolved.first_baseline() - metrics.ascent, height)
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

/// The line height `style` asks for, times `scale`. A style that asks for none
/// is laid out at its font's own extent by `line_box`, whatever this returns.
pub(crate) fn asked_line_height(style: &TextStyle, scale: f32) -> f32 {
    (style.resolve_line_height(14.0, f32::NAN) * scale).max(1.0)
}

fn resolve_letter_spacing(style: &TextStyle, font_size: f32) -> f32 {
    let _ = font_size;
    style.resolve_letter_spacing(14.0)
}

fn run_tracking(char_count: usize, letter_spacing: f32) -> f32 {
    char_count as f32 * letter_spacing
}

fn run_lead_in(line: &str, letter_spacing: f32) -> f32 {
    if line.is_empty() {
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
        if let Some(previous) = previous {
            width +=
                glyph_metrics.kern(font, &scaled_font, previous, (ch, metrics.glyph_id)) * h_scale;
        }
        width += metrics.advance_unscaled * h_scale;
        previous = Some((ch, metrics.glyph_id));
    }

    width.max(0.0)
}

/// What a walk over a line reports for each character: the separator
/// before it, which a range starting at it leaves out, and the line's width
/// through it.
trait PrefixWidthSink {
    fn push(&mut self, separator: f32, prefix_width: f32);
}

/// Every prefix width and separator of a line, as
/// [`TextLinePrefixWidths::from_parts`] takes them.
struct PrefixWidthVecs {
    prefix_widths: Vec<f32>,
    separator_before: Vec<f32>,
}

impl PrefixWidthSink for PrefixWidthVecs {
    fn push(&mut self, separator: f32, prefix_width: f32) {
        self.separator_before.push(separator);
        self.prefix_widths.push(prefix_width);
    }
}

/// A line's whole width as its prefix widths would give it, kept without
/// the widths themselves.
#[derive(Default)]
struct LineWidthSink {
    first_separator: Option<f32>,
    last_width: f32,
    non_finite: bool,
}

impl PrefixWidthSink for LineWidthSink {
    fn push(&mut self, separator: f32, prefix_width: f32) {
        self.first_separator.get_or_insert(separator);
        self.last_width = prefix_width;
        self.non_finite |= !separator.is_finite() || !prefix_width.is_finite();
    }
}

impl LineWidthSink {
    /// [`TextLinePrefixWidths::width_for_char_range`] over every character,
    /// or `None` where [`TextLinePrefixWidths::from_parts`] refuses the line.
    fn width(&self, non_empty_overhang: f32) -> Option<f32> {
        let overhang = non_empty_overhang.max(0.0);
        if self.non_finite || !overhang.is_finite() {
            return None;
        }
        Some(self.first_separator.map_or(0.0, |separator| {
            (self.last_width - separator).max(0.0) + overhang
        }))
    }
}

/// Walks the characters of `line_range` span by span, reporting each to
/// `sink`, and answers the widest visual overhang of its non-empty runs.
fn walk_line_prefix_widths(
    text: &AnnotatedString,
    line_range: std::ops::Range<usize>,
    style: &TextStyle,
    fonts: &SoftwareTextFontSet,
    glyph_metrics: &mut SoftwareTextGlyphMetricsCache,
    sink: &mut impl PrefixWidthSink,
) -> f32 {
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

    let mut walk = PrefixWidthWalk {
        sink,
        width: 0.0,
        non_empty_overhang: 0.0,
    };
    for range in boundaries.windows(2) {
        let start = range[0];
        let end = range[1];
        if start >= end {
            continue;
        }
        let segment = &text.text[start..end];
        let segment_style = effective_style_for_range(&text.span_styles, style, start, end);
        append_prefix_width_segment(segment, &segment_style, fonts, glyph_metrics, &mut walk);
    }
    walk.non_empty_overhang
}

struct PrefixWidthWalk<'a, S> {
    sink: &'a mut S,
    width: f32,
    non_empty_overhang: f32,
}

impl<S: PrefixWidthSink> PrefixWidthWalk<'_, S> {
    /// Adds a character `step` wide, `separator` of it before the character.
    fn advance(&mut self, separator: f32, step: f32) {
        self.width += step;
        self.sink.push(separator, self.width.max(0.0));
    }
}

fn append_prefix_width_segment<S: PrefixWidthSink>(
    segment: &str,
    style: &TextStyle,
    fonts: &SoftwareTextFontSet,
    glyph_metrics: &mut SoftwareTextGlyphMetricsCache,
    walk: &mut PrefixWidthWalk<'_, S>,
) {
    if segment.is_empty() {
        return;
    }

    let font_size = resolve_font_size(style);
    if fonts
        .visit_font_runs(segment, style, |range, font| {
            append_font_prefix_width_segment(
                &segment[range],
                style,
                font_size,
                font,
                glyph_metrics,
                walk,
            );
        })
        .is_none()
    {
        let char_width = fallback_char_width(font_size);
        let letter_spacing = resolve_letter_spacing(style, font_size);
        for _ in segment.chars() {
            walk.advance(0.0, letter_spacing + char_width);
        }
    }
}

fn append_font_prefix_width_segment<S: PrefixWidthSink>(
    segment: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
    glyph_metrics: &mut SoftwareTextGlyphMetricsCache,
    walk: &mut PrefixWidthWalk<'_, S>,
) {
    let shaped = font.shaped_for(style);
    let font = &*shaped;
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let scaled_font = font.font.as_scaled(PxScale::from(glyph_font_size));
    let letter_spacing = font.metadata.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font.weight(), font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, font.style(), font_size, 1.0);
    walk.non_empty_overhang = walk
        .non_empty_overhang
        .max(style_synthesis.visual_overhang_px());

    let mut previous = None;
    let h_scale = scaled_font.h_scale_factor();

    for (index, ch) in segment.chars().enumerate() {
        let metrics = glyph_metrics.glyph_metrics(font, &scaled_font, ch);
        let separator = if index == 0 {
            0.0
        } else {
            previous.map_or(0.0, |previous| {
                weight_synthesis.apply_width(
                    glyph_metrics.kern(font, &scaled_font, previous, (ch, metrics.glyph_id))
                        * h_scale,
                )
            })
        };
        walk.advance(
            separator,
            separator
                + letter_spacing
                + weight_synthesis.apply_width(metrics.advance_unscaled * h_scale),
        );
        previous = Some((ch, metrics.glyph_id));
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
    let line_box = line_box_for(
        style,
        vertical_metrics(font, glyph_font_size),
        asked_line_height(style, 1.0),
        measure_grid(),
    );
    let line_height = line_box.height;
    let letter_spacing = font_ref.tracking.resolve(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, font_ref.weight, font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, font_ref.style, font_size, 1.0);

    let mut max_width: f32 = 0.0;
    let mut line_count = 0usize;
    for line in text.split('\n') {
        line_count += 1;
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
        height: line_box.block_height(line_count),
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
    let line_box = font_line_box(style, font, font_size);
    let line_height = line_box.height;

    let mut max_width: f32 = 0.0;
    let mut line_count = 0usize;
    for line in text.split('\n') {
        line_count += 1;
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
        height: line_box.block_height(line_count),
        line_height,
        line_count,
    }
}

fn measure_annotated_text_with_resolver<'a>(
    text: impl Into<StyledTextRef<'a>>,
    style: &TextStyle,
    font_size: f32,
    fonts: impl Into<FontResolver<'a>>,
    cache: Option<&mut SoftwareTextMetricsCache>,
) -> TextMetrics {
    let text = text.into();
    let fonts = fonts.into();
    let Some(layout) = AnnotatedTextLayout::new(text, style, font_size, 1.0, measure_grid(), fonts)
    else {
        return fallback_text_metrics(text.text, style, font_size);
    };
    measure_resolved_text(&layout, cache)
}

fn measure_resolved_text(
    layout: &AnnotatedTextLayout<'_>,
    mut cache: Option<&mut SoftwareTextMetricsCache>,
) -> TextMetrics {
    layout
        .walk(None, |segment| {
            #[cfg(feature = "text-shaping")]
            if requires_font_layout(segment.text, segment.style) {
                return Some(
                    measure_shaped_text(
                        segment.text,
                        segment.style,
                        segment.font_size,
                        segment.font,
                    )
                    .width,
                );
            }
            let metrics = if let Some(cache) = cache.as_deref_mut() {
                measure_text_with_font_cached(
                    segment.text,
                    segment.style,
                    segment.font_size,
                    segment.font,
                    cache,
                )
            } else {
                measure_text_with_font(segment.text, segment.style, segment.font_size, segment.font)
            };
            Some(metrics.width)
        })
        .unwrap_or_else(|| layout.metrics(0.0))
}

fn max_line_height_for_annotated_text_with_resolver(
    text: &AnnotatedString,
    style: &TextStyle,
    font_size: f32,
    fonts: &SoftwareTextFontSet,
) -> f32 {
    if text.span_styles.is_empty()
        && let Some(font) = fonts.single_font_for_text(text.text.as_str(), style)
    {
        return font_line_box(style, font, font_size).height;
    }
    AnnotatedTextLayout::new(text.into(), style, font_size, 1.0, measure_grid(), fonts).map_or_else(
        || fallback_line_height(style, font_size),
        |layout| layout.metrics(0.0).line_height,
    )
}

pub(crate) fn visit_annotated_line_boxes(
    text: &AnnotatedString,
    style: &TextStyle,
    fonts: &SoftwareTextFontSet,
    visit: &mut dyn FnMut(cranpose_ui::text::LineBox),
) -> Option<()> {
    let layout = AnnotatedTextLayout::new(
        text.into(),
        style,
        resolve_font_size(style),
        1.0,
        measure_grid(),
        fonts,
    )?;
    for line in &layout.lines {
        visit(line.line_box);
    }
    Some(())
}

pub(crate) fn effective_style_for_range<'a>(
    span_styles: &[RangeStyle<SpanStyle>],
    style: &'a TextStyle,
    start: usize,
    end: usize,
) -> Cow<'a, TextStyle> {
    let mut effective = Cow::Borrowed(style);
    for span in span_styles {
        if span.range.start < end && span.range.end > start {
            let merged = effective.span_style.merge(&span.item);
            effective.to_mut().span_style = merged;
        }
    }
    effective
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
    clip: Option<&GlyphRasterClip>,
) -> Option<GlyphMask> {
    let (outlined, bounds) = outline_glyph_with_bounds(font, glyph)?;
    if clip.is_some_and(|clip| {
        !clip.intersects(complete_glyph_bounds(
            bounds,
            raster_style,
            weight_synthesis,
            style_synthesis,
        ))
    }) {
        return None;
    }
    let mask = build_glyph_mask(font, glyph, &outlined, bounds, raster_style)?;
    let mask = synthesize_glyph_weight(mask, weight_synthesis);
    Some(synthesize_glyph_style(mask, style_synthesis))
}

fn complete_glyph_bounds(
    bounds: GlyphPixelBounds,
    raster_style: GlyphRasterStyle,
    weight: TextWeightSynthesis,
    style: TextStyleSynthesis,
) -> Rect {
    let pad = match raster_style {
        GlyphRasterStyle::Fill => 0.0,
        GlyphRasterStyle::Stroke { width_px } => stroke_mask_padding(width_px) as f32,
    };
    let horizontal = synthetic_weight_shift_px(weight.embolden_px);
    let vertical = (horizontal / 2).min(1) as f32;
    let height = bounds.height() as f32 + 2.0 * (pad + vertical);
    let slant = if style.slant > 0.0 {
        ((height - 1.0).max(0.0) * style.slant).ceil() + 1.0
    } else {
        0.0
    };
    Rect {
        x: bounds.min_x as f32 - pad,
        y: bounds.min_y as f32 - pad - vertical,
        width: bounds.width() as f32 + 2.0 * pad + horizontal as f32 + slant,
        height,
    }
}

#[expect(clippy::too_many_arguments)]
fn raster_glyph_mask(
    cache: Option<&mut SoftwareGlyphRasterCache>,
    font_hash: u64,
    font: &impl Font,
    glyph: &Glyph,
    raster_style: GlyphRasterStyle,
    weight_synthesis: TextWeightSynthesis,
    style_synthesis: TextStyleSynthesis,
    clip: Option<&GlyphRasterClip>,
) -> Option<GlyphMask> {
    if let Some(cache) = cache {
        let key = glyph_mask_cache_key(
            font_hash,
            glyph,
            raster_style,
            weight_synthesis,
            style_synthesis,
        );
        if let Some(mask) = cache.get(&key, glyph) {
            return clip
                .is_none_or(|clip| {
                    clip.intersects(Rect {
                        x: mask.origin_x as f32,
                        y: mask.origin_y as f32,
                        width: mask.width as f32,
                        height: mask.height as f32,
                    })
                })
                .then_some(mask);
        }
        let mask = build_complete_glyph_mask(
            font,
            glyph,
            raster_style,
            weight_synthesis,
            style_synthesis,
            clip,
        )?;
        return Some(cache.put(key, glyph, mask));
    }
    build_complete_glyph_mask(
        font,
        glyph,
        raster_style,
        weight_synthesis,
        style_synthesis,
        clip,
    )
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

/// `scaled_font`'s kerning between two glyphs, through `glyph_cache` when a
/// caller has one.
fn cached_kern<F: Font, S: ScaleFont<F>>(
    glyph_cache: Option<&mut SoftwareGlyphRasterCache>,
    font_hash: u64,
    scaled_font: &S,
    previous: GlyphId,
    glyph: GlyphId,
) -> f32 {
    match glyph_cache {
        Some(cache) => {
            cache.kern_unscaled(font_hash, scaled_font.font(), previous, glyph)
                * scaled_font.h_scale_factor()
        }
        None => scaled_font.kern(previous, glyph),
    }
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
    clip: Option<&GlyphRasterClip>,
    mut visit: impl FnMut(&GlyphMask),
) -> f32 {
    let scale = PxScale::from(font_px_size);
    let scaled_font = font.as_scaled(scale);
    let line_offsets = line_alignment_offsets(&scaled_font, text, letter_spacing, align_fraction);
    let mut max_advance = 0.0f32;
    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let lead_in = run_lead_in(line, letter_spacing);
        let mut caret_x = origin_x + line_offset(&line_offsets, line_idx) + lead_in;
        let mut previous = None;
        for ch in line.chars() {
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(previous_id) = previous {
                caret_x += cached_kern(
                    glyph_cache.as_deref_mut(),
                    font_hash,
                    &scaled_font,
                    previous_id,
                    glyph_id,
                ) + letter_spacing;
            }
            let glyph = glyph_id.with_scale_and_position(scale, point(caret_x, baseline_y));
            caret_x += scaled_font.h_advance(glyph_id);
            previous = Some(glyph_id);
            let glyph = align_glyph_for_text_motion(glyph, static_text_motion);
            let Some(mask) = raster_glyph_mask(
                if static_text_motion {
                    glyph_cache.as_deref_mut()
                } else {
                    None
                },
                font_hash,
                font,
                &glyph,
                raster_style,
                weight_synthesis,
                style_synthesis,
                clip,
            ) else {
                continue;
            };
            visit(&mask);
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
    paint: &GlyphPaint<'_>,
) {
    let correction = TextLuminance::of_brush(paint.brush).correction();
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

            let coverage = correction.apply_unit(mask.alpha[y * mask.width + x]);
            if coverage <= 0.0 {
                continue;
            }

            let sample = sample_brush_rgba(
                paint.brush,
                paint.brush_rect,
                paint.canvas_origin.x + px as f32 + 0.5,
                paint.canvas_origin.y + py as f32 + 0.5,
                cranpose_ui_graphics::Point::default(),
            );
            let alpha = coverage * sample[3] * paint.alpha;
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
    let correction = TextLuminance::of_color(Color(color[0], color[1], color[2], 1.0)).correction();

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

            let coverage = correction.apply_unit(mask.alpha[y * mask.width + x]);
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

    // An unblurred shadow is text in the shadow color and gets its mask
    // gamma; Skia never corrects a mask a blur filters.
    let correction = (sigma <= 0.0).then(|| TextLuminance::of_color(shadow.color).correction());
    for y in 0..mask.height {
        let src_offset = y * mask.width;
        let dst_offset = (y + blur_margin as usize) * padded_width + blur_margin as usize;
        let source = &mask.alpha[src_offset..src_offset + mask.width];
        let target = &mut padded_mask[dst_offset..dst_offset + mask.width];
        match correction {
            Some(correction) => target
                .iter_mut()
                .zip(source)
                .for_each(|(target, &coverage)| *target = correction.apply_unit(coverage)),
            None => target.copy_from_slice(source),
        }
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

    let pad = stroke_mask_padding(stroke_width_px);
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

fn stroke_mask_padding(width: f32) -> i32 {
    if width.is_finite() && width > 0.0 {
        (width * 0.5 * COMPOSE_STROKE_MITER_LIMIT).ceil().max(1.0) as i32 + 1
    } else {
        0
    }
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

#[cfg(test)]
#[path = "tests/software_text_features_tests.rs"]
mod feature_tests;

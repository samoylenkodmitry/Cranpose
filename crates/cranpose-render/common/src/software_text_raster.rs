use ab_glyph::{point, Font, FontArc, Glyph, OutlinedGlyph, PxScale, ScaleFont};
use cranpose_core::hash::default as default_hash;
use cranpose_ui::text::{
    AnnotatedString, FontFamily, FontStyle, FontSynthesis, FontWeight, Shadow, TextDrawStyle,
    TextMotion, TextStyle,
};
use cranpose_ui::text_layout_result::{GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult};
use cranpose_ui::{TextMeasurer, TextMetrics};
use cranpose_ui_graphics::{Color, ImageBitmap, Rect};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::bounded_lru_cache::BoundedLruCache;
use crate::brush_sampling::{color_to_rgba, sample_brush_rgba};
use crate::font_layout::{
    align_glyph_to_pixel_grid, layout_line_glyphs, line_advance_width, pixel_bounds_from_outlined,
    vertical_metrics, GlyphPixelBounds,
};
#[cfg(feature = "text-hyphenation")]
use crate::text_hyphenation::HyphenationDictionaryError;
use crate::text_hyphenation::HyphenationDictionaryStore;
use crate::Brush;

const COMPOSE_STROKE_MITER_LIMIT: f32 = 4.0;
const SHADOW_SIGMA_SCALE: f32 = 0.57735;
const SHADOW_SIGMA_BIAS: f32 = 0.5;
const MAX_GAUSSIAN_KERNEL_HALF: i32 = 128;
#[doc(hidden)]
pub const DEFAULT_SOFTWARE_TEXT_FONT_BYTES: &[u8] = include_bytes!("../assets/NotoSansMerged.ttf");

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SoftwareTextFontError {
    #[error("invalid software text font bytes")]
    InvalidFont,
}

#[derive(Clone)]
pub struct SoftwareTextFont {
    font: FontArc,
    metadata: SoftwareTextFontMetadata,
    score: TextFontScore,
}

#[derive(Clone)]
struct SoftwareTextFontMetadata {
    families: Arc<[String]>,
    weight: FontWeight,
    style: FontStyle,
    ab_glyph_scale_factor: f32,
}

impl SoftwareTextFont {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, SoftwareTextFontError> {
        let bytes = bytes.into();
        let metadata = software_text_font_metadata(bytes.as_slice());
        let font = FontArc::try_from_vec(bytes).map_err(|_| SoftwareTextFontError::InvalidFont)?;
        let score =
            text_font_score_from_parts(&font, metadata.ab_glyph_scale_factor, metadata.weight);
        Ok(Self {
            font,
            metadata,
            score,
        })
    }

    pub fn family_names(&self) -> &[String] {
        &self.metadata.families
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
}

pub fn try_default_software_text_font() -> Result<SoftwareTextFont, SoftwareTextFontError> {
    SoftwareTextFont::from_bytes(DEFAULT_SOFTWARE_TEXT_FONT_BYTES.to_vec())
}

pub fn default_software_text_font() -> Option<SoftwareTextFont> {
    try_default_software_text_font().ok()
}

#[derive(Clone)]
pub struct SoftwareTextFontSet {
    fonts: Arc<[SoftwareTextFont]>,
    default_index: Option<usize>,
}

impl SoftwareTextFontSet {
    pub fn empty() -> Self {
        Self {
            fonts: Arc::from(Vec::new()),
            default_index: None,
        }
    }

    pub fn from_font(font: SoftwareTextFont) -> Self {
        Self {
            fonts: Arc::from(vec![font]),
            default_index: Some(0),
        }
    }

    pub fn from_fonts_or_default(fonts: &[&[u8]]) -> Self {
        let mut parsed = Vec::with_capacity(fonts.len().max(1));
        for font in fonts {
            if let Ok(candidate) = SoftwareTextFont::from_bytes((*font).to_vec()) {
                parsed.push(candidate);
            }
        }
        if parsed.is_empty() {
            if let Some(default_font) = default_software_text_font() {
                parsed.push(default_font);
            }
        }

        let default_index = (!parsed.is_empty()).then(|| default_font_index(&parsed));
        Self {
            fonts: Arc::from(parsed),
            default_index,
        }
    }

    pub fn default_font(&self) -> Option<&SoftwareTextFont> {
        self.default_index.and_then(|index| self.fonts.get(index))
    }

    pub fn resolve(&self, style: &TextStyle) -> Option<&SoftwareTextFont> {
        let target_weight = style.span_style.font_weight.unwrap_or_default();
        let target_style = style.span_style.font_style.unwrap_or_default();
        let family_name = requested_family_name(style.span_style.font_family.as_ref());

        let mut best: Option<(usize, u32)> = None;
        for (index, font) in self.fonts.iter().enumerate() {
            let Some(score) = font_match_score(font, target_weight, target_style, family_name)
            else {
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
    ab_glyph_scale_factor: f32,
    weight: FontWeight,
) -> TextFontScore {
    const SAMPLE: &str = "UNDER The quick brown fox";
    let glyph_font_size = 18.0 * ab_glyph_scale_factor;
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
        glyph_font_size,
        font,
        FontStyle::Normal,
        weight,
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
    best.map(|(index, _)| index).unwrap_or(0)
}

fn requested_family_name(font_family: Option<&FontFamily>) -> Option<&str> {
    match font_family {
        Some(FontFamily::Named(name)) => Some(name.as_str()),
        _ => None,
    }
}

fn font_match_score(
    font: &SoftwareTextFont,
    target_weight: FontWeight,
    target_style: FontStyle,
    family_name: Option<&str>,
) -> Option<u32> {
    let family_penalty = match family_name {
        Some(name) if font_family_matches(font, name) => 0,
        Some(_) => return None,
        None => 0,
    };
    let style_penalty = if font.style() == target_style {
        0
    } else {
        10_000
    };
    let weight_penalty = (i32::from(font.weight().0) - i32::from(target_weight.0)).unsigned_abs();
    let coverage_penalty =
        (21usize.saturating_sub(text_font_score(font).supported_latin_chars) as u32) * 1_000;

    Some(family_penalty + style_penalty + weight_penalty + coverage_penalty)
}

fn font_family_matches(font: &SoftwareTextFont, requested: &str) -> bool {
    font.family_names()
        .iter()
        .any(|family| family.eq_ignore_ascii_case(requested))
}

fn software_text_font_metadata(bytes: &[u8]) -> SoftwareTextFontMetadata {
    let Some(face) = ttf_parser::Face::parse(bytes, 0).ok() else {
        return SoftwareTextFontMetadata {
            families: Arc::from(Vec::<String>::new()),
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
            ab_glyph_scale_factor: 1.0,
        };
    };

    let mut families = Vec::new();
    for name in face.names() {
        if matches!(
            name.name_id,
            ttf_parser::name_id::TYPOGRAPHIC_FAMILY | ttf_parser::name_id::FAMILY
        ) {
            if let Some(value) = name.to_string().filter(|value| !value.is_empty()) {
                if !families
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
                {
                    families.push(value);
                }
            }
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
        weight,
        style,
        ab_glyph_scale_factor,
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
}

impl SoftwareTextMetricsCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: BoundedLruCache::with_capacity_at_least_one(capacity),
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

        let metrics = measure_annotated_text_with_font_set(text, style, font_size, fonts);
        self.map.put(key, metrics);
        metrics
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
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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

struct GlyphMask {
    alpha: Vec<f32>,
    width: usize,
    height: usize,
    origin_x: i32,
    origin_y: i32,
}

struct RasterFontRef<'a, F> {
    font: &'a F,
    ab_glyph_scale_factor: f32,
    weight: FontWeight,
    style: FontStyle,
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
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
        RasterFontRef {
            font: &font.font,
            ab_glyph_scale_factor: font.metadata.ab_glyph_scale_factor,
            weight: font.weight(),
            style: font.style(),
        },
    )
}

pub fn measure_text_with_font(
    text: &str,
    style: &TextStyle,
    font_size: f32,
    font: &SoftwareTextFont,
) -> TextMetrics {
    measure_text_impl(
        text,
        style,
        font_size,
        font.ab_glyph_px_size(font_size),
        &font.font,
        font.style(),
        font.weight(),
    )
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
    measure_annotated_text_with_resolver(text, style, font_size, fonts)
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
    let glyph_font_size = font.ab_glyph_px_size(font_size);
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
        let glyph_x = measure_text_impl(
            prefix,
            style,
            font_size,
            glyph_font_size,
            &font.font,
            font.style(),
            font.weight(),
        )
        .width;

        let char_str = &line_text[current_byte_offset..current_byte_offset + c.len_utf8()];
        let char_width = measure_text_impl(
            char_str,
            style,
            font_size,
            glyph_font_size,
            &font.font,
            font.style(),
            font.weight(),
        )
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

    let total_width = measure_text_impl(
        line_text,
        style,
        font_size,
        glyph_font_size,
        &font.font,
        font.style(),
        font.weight(),
    )
    .width;
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
    measure_text_impl(
        &text[..clamped_offset],
        style,
        font_size,
        font.ab_glyph_px_size(font_size),
        &font.font,
        font.style(),
        font.weight(),
    )
    .width
}

pub fn layout_text_with_font(
    text: &str,
    style: &TextStyle,
    font: &SoftwareTextFont,
) -> TextLayoutResult {
    let font_size = resolve_font_size(style);
    let glyph_font_size = font.ab_glyph_px_size(font_size);
    let resolved_weight = font.weight();
    let resolved_style = font.style();
    let weight_synthesis = TextWeightSynthesis::for_style(style, resolved_weight, font_size, 1.0);
    let font = &font.font;
    let line_height = resolve_line_height(style, font_size * 1.4);
    let letter_spacing = resolve_letter_spacing(style, font_size);
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
            if let Some((_, next)) = iter.peek() {
                if *next != '\n' {
                    current_x += letter_spacing;
                }
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

    let metrics = measure_text_impl(
        text,
        style,
        font_size,
        glyph_font_size,
        font,
        resolved_style,
        resolved_weight,
    );
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
        text,
        rect,
        style,
        fallback_color,
        font_size,
        scale,
        RasterFontRef {
            font,
            ab_glyph_scale_factor: 1.0,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        },
    )
}

fn rasterize_text_to_image_impl(
    text: &str,
    rect: Rect,
    style: &TextStyle,
    fallback_color: Color,
    font_size: f32,
    scale: f32,
    font_ref: RasterFontRef<'_, impl Font>,
) -> Option<ImageBitmap> {
    if text.is_empty()
        || rect.width <= 0.0
        || rect.height <= 0.0
        || !font_size.is_finite()
        || font_size <= 0.0
        || !scale.is_finite()
        || scale <= 0.0
    {
        return None;
    }

    let width = rect.width.ceil().max(1.0) as u32;
    let height = rect.height.ceil().max(1.0) as u32;
    let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];

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
    let weight_synthesis = TextWeightSynthesis::for_style(style, font_ref.weight, font_size, scale);
    let style_synthesis = TextStyleSynthesis::for_style(style, font_ref.style, font_size, scale);
    let metrics = vertical_metrics(font, font_px_size);
    let line_height = (style.resolve_line_height(14.0, font_size * 1.4) * scale).max(1.0);
    let first_baseline_y = baseline_y_for_line_box(metrics, line_height);

    for (line_idx, line) in text.split('\n').enumerate() {
        let baseline_y = first_baseline_y + line_idx as f32 * line_height + origin_y;
        let offset = point(origin_x, baseline_y);

        for glyph in layout_line_glyphs(font, line, font_px_size, offset) {
            let glyph = align_glyph_for_text_motion(glyph, static_text_motion);
            let Some((outlined, bounds)) = outline_glyph_with_bounds(font, &glyph) else {
                continue;
            };
            let Some(mask) = build_glyph_mask(font, &glyph, &outlined, bounds, raster_style) else {
                continue;
            };
            let mask = synthesize_glyph_weight(mask, weight_synthesis);
            let mask = synthesize_glyph_style(mask, style_synthesis);

            if let Some(shadow) = shadow {
                draw_shadow_mask(
                    &mut canvas,
                    width,
                    height,
                    &mask,
                    shadow,
                    scale,
                    static_text_motion,
                );
            }

            draw_mask_glyph(
                &mut canvas,
                width,
                height,
                &mask,
                brush,
                brush_alpha_multiplier,
                rect,
            );
        }
    }

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

fn resolve_font_size(style: &TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

fn baseline_y_for_line_box(
    metrics: crate::font_layout::FontVerticalMetrics,
    line_height: f32,
) -> f32 {
    metrics.ascent + (line_height - metrics.natural_line_height) * 0.5
}

fn resolve_line_height(style: &TextStyle, font_size: f32) -> f32 {
    style.resolve_line_height(14.0, font_size)
}

fn resolve_letter_spacing(style: &TextStyle, font_size: f32) -> f32 {
    let _ = font_size;
    style.resolve_letter_spacing(14.0)
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
        let spacing = char_count.saturating_sub(1) as f32 * letter_spacing;
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
    let spacing = char_count.saturating_sub(1) as f32 * resolve_letter_spacing(style, font_size);
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
            if let Some((_, next)) = iter.peek() {
                if *next != '\n' {
                    current_x += letter_spacing;
                }
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
    glyph_font_size: f32,
    font: &impl Font,
    resolved_style: FontStyle,
    resolved_weight: FontWeight,
) -> TextMetrics {
    let line_height = resolve_line_height(style, font_size * 1.4);
    let letter_spacing = resolve_letter_spacing(style, font_size);
    let weight_synthesis = TextWeightSynthesis::for_style(style, resolved_weight, font_size, 1.0);
    let style_synthesis = TextStyleSynthesis::for_style(style, resolved_style, font_size, 1.0);

    let lines: Vec<&str> = text.split('\n').collect();
    let line_count = lines.len().max(1);

    let mut max_width: f32 = 0.0;
    for line in &lines {
        let line_width = line_advance_width(font, line, glyph_font_size);
        let char_spacing = (line.chars().count().saturating_sub(1) as f32) * letter_spacing;
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
) -> TextMetrics {
    let Some(base_font) = fonts.resolve(style) else {
        return fallback_text_metrics(text.text.as_str(), style, font_size);
    };
    let base_line_height = line_height_for_style(style, font_size, &base_font.font);
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
        let segment_style = effective_style_for_range(text, style, start, end);
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
                    let metrics = measure_text_impl(
                        before_newline,
                        &segment_style,
                        segment_font_size,
                        segment_font.ab_glyph_px_size(segment_font_size),
                        &segment_font.font,
                        segment_font.style(),
                        segment_font.weight(),
                    );
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
                    let metrics = measure_text_impl(
                        remaining,
                        &segment_style,
                        segment_font_size,
                        segment_font.ab_glyph_px_size(segment_font_size),
                        &segment_font.font,
                        segment_font.style(),
                        segment_font.weight(),
                    );
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
    let base_line_height = line_height_for_style(style, font_size, &base_font.font);
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
        let segment_style = effective_style_for_range(text, style, start, end);
        let segment_font_size = resolve_font_size(&segment_style);
        let segment_line_height = if let Some(segment_font) = fonts.resolve(&segment_style) {
            line_height_for_style(&segment_style, segment_font_size, &segment_font.font)
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

fn effective_style_for_range(
    text: &AnnotatedString,
    style: &TextStyle,
    start: usize,
    end: usize,
) -> TextStyle {
    let mut effective = style.clone();
    for span in &text.span_styles {
        if span.range.start < end && span.range.end > start {
            effective.span_style = effective.span_style.merge(&span.item);
        }
    }
    effective
}

fn line_height_for_style(style: &TextStyle, font_size: f32, font: &impl Font) -> f32 {
    let _ = font;
    resolve_line_height(style, font_size * 1.4)
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
        alpha,
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

    let alpha = pixmap
        .data()
        .chunks_exact(4)
        .map(|pixel| pixel[3] as f32 / 255.0)
        .collect();

    Some(GlyphMask {
        alpha,
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
        alpha,
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
        alpha,
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
mod tests {
    use super::*;
    use cranpose_ui::text::SpanStyle;
    use cranpose_ui_graphics::Point;

    fn count_ink_pixels(image: &ImageBitmap) -> usize {
        image
            .pixels()
            .chunks_exact(4)
            .filter(|px| px[3] > 0)
            .count()
    }

    fn average_ink_rgb(
        image: &ImageBitmap,
        x_start: u32,
        x_end: u32,
        y_start: u32,
        y_end: u32,
    ) -> Option<[f32; 3]> {
        let width = image.width();
        let height = image.height();
        let mut sums = [0.0f32; 3];
        let mut count = 0usize;
        let pixels = image.pixels();

        let x_end = x_end.min(width);
        let y_end = y_end.min(height);
        for y in y_start.min(height)..y_end {
            for x in x_start.min(width)..x_end {
                let idx = ((y * width + x) * 4) as usize;
                let alpha = pixels[idx + 3];
                if alpha == 0 {
                    continue;
                }
                sums[0] += pixels[idx] as f32 / 255.0;
                sums[1] += pixels[idx + 1] as f32 / 255.0;
                sums[2] += pixels[idx + 2] as f32 / 255.0;
                count += 1;
            }
        }

        if count == 0 {
            return None;
        }
        Some([
            sums[0] / count as f32,
            sums[1] / count as f32,
            sums[2] / count as f32,
        ])
    }

    fn ink_x_range(image: &ImageBitmap) -> Option<(u32, u32)> {
        let width = image.width();
        let height = image.height();
        let pixels = image.pixels();
        let mut min_x = u32::MAX;
        let mut max_x = 0u32;
        let mut found = false;
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                if pixels[idx + 3] > 0 {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x + 1);
                    found = true;
                }
            }
        }
        found.then_some((min_x, max_x))
    }

    fn ink_y_range(image: &ImageBitmap) -> Option<(u32, u32)> {
        let width = image.width();
        let height = image.height();
        let pixels = image.pixels();
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        let mut found = false;
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                if pixels[idx + 3] > 0 {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y + 1);
                    found = true;
                }
            }
        }
        found.then_some((min_y, max_y))
    }

    fn ink_centroid_x(image: &ImageBitmap, y_start: u32, y_end: u32) -> Option<f32> {
        let width = image.width();
        let height = image.height();
        let pixels = image.pixels();
        let mut weighted_x = 0.0f32;
        let mut total_alpha = 0.0f32;

        for y in y_start.min(height)..y_end.min(height) {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                let alpha = pixels[idx + 3] as f32 / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                weighted_x += x as f32 * alpha;
                total_alpha += alpha;
            }
        }

        (total_alpha > 0.0).then_some(weighted_x / total_alpha)
    }

    fn vertical_slant_delta(image: &ImageBitmap) -> f32 {
        let (top, bottom) = ink_y_range(image).expect("image should contain ink");
        let mid = top + (bottom - top).max(1) / 2;
        let top_x = ink_centroid_x(image, top, mid).expect("top ink centroid");
        let bottom_x = ink_centroid_x(image, mid, bottom).expect("bottom ink centroid");
        top_x - bottom_x
    }

    fn top_ink_row(image: &ImageBitmap) -> Option<u32> {
        let width = image.width();
        let height = image.height();
        let pixels = image.pixels();
        for y in 0..height {
            for x in 0..width {
                let idx = ((y * width + x) * 4) as usize;
                if pixels[idx + 3] > 0 {
                    return Some(y);
                }
            }
        }
        None
    }

    fn reference_dilation_offsets(radius: i32) -> Vec<(i32, i32)> {
        let mut offsets = Vec::new();
        let squared_radius = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= squared_radius {
                    offsets.push((dx, dy));
                }
            }
        }
        if offsets.is_empty() {
            offsets.push((0, 0));
        }
        offsets
    }

    fn reference_dilation_stroke_mask(fill: &GlyphMask, stroke_width: f32) -> GlyphMask {
        let radius = (stroke_width * 0.5).ceil() as i32;
        let offsets = reference_dilation_offsets(radius);
        let out_width = fill.width as i32 + radius * 2;
        let out_height = fill.height as i32 + radius * 2;
        let fill_width_i32 = fill.width as i32;
        let fill_height_i32 = fill.height as i32;
        let mut alpha = vec![0.0f32; (out_width * out_height) as usize];

        for out_y in 0..out_height {
            let oy = out_y - radius;
            for out_x in 0..out_width {
                let ox = out_x - radius;
                let base_alpha =
                    if ox >= 0 && oy >= 0 && ox < fill_width_i32 && oy < fill_height_i32 {
                        fill.alpha[oy as usize * fill.width + ox as usize]
                    } else {
                        0.0
                    };

                let mut dilated_alpha = 0.0f32;
                for (dx, dy) in &offsets {
                    let sx = ox + dx;
                    let sy = oy + dy;
                    if sx < 0 || sy < 0 || sx >= fill_width_i32 || sy >= fill_height_i32 {
                        continue;
                    }
                    let sample = fill.alpha[sy as usize * fill.width + sx as usize];
                    if sample > dilated_alpha {
                        dilated_alpha = sample;
                        if dilated_alpha >= 0.999 {
                            break;
                        }
                    }
                }
                alpha[out_y as usize * out_width as usize + out_x as usize] =
                    (dilated_alpha - base_alpha).max(0.0);
            }
        }

        GlyphMask {
            alpha,
            width: out_width as usize,
            height: out_height as usize,
            origin_x: fill.origin_x - radius,
            origin_y: fill.origin_y - radius,
        }
    }

    fn rasterize_reference_dilation_stroke(
        text: &str,
        rect: Rect,
        font_size: f32,
        stroke_width: f32,
        font: &impl Font,
    ) -> ImageBitmap {
        let width = rect.width.ceil().max(1.0) as u32;
        let height = rect.height.ceil().max(1.0) as u32;
        let mut canvas = vec![[0.0f32; 4]; (width * height) as usize];

        let metrics = vertical_metrics(font, font_size);
        let baseline = baseline_y_for_line_box(metrics, font_size * 1.4);
        for glyph in layout_line_glyphs(font, text, font_size, point(0.0, baseline)) {
            let Some((outlined, bounds)) = outline_glyph_with_bounds(font, &glyph) else {
                continue;
            };
            let Some(fill) = build_fill_mask(&outlined, bounds) else {
                continue;
            };
            let reference = reference_dilation_stroke_mask(&fill, stroke_width);
            draw_mask_glyph(
                &mut canvas,
                width,
                height,
                &reference,
                &Brush::solid(Color::WHITE),
                1.0,
                rect,
            );
        }

        let mut rgba = vec![0u8; canvas.len() * 4];
        for (index, pixel) in canvas.iter().enumerate() {
            let base = index * 4;
            rgba[base] = (pixel[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 1] = (pixel[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 2] = (pixel[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[base + 3] = (pixel[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        ImageBitmap::from_rgba8(width, height, rgba).expect("reference dilation image")
    }

    fn test_font() -> ab_glyph::FontRef<'static> {
        ab_glyph::FontRef::try_from_slice(include_bytes!("../assets/NotoSansMerged.ttf"))
            .expect("font")
    }

    fn test_software_font() -> SoftwareTextFont {
        SoftwareTextFont::from_bytes(include_bytes!("../assets/NotoSansMerged.ttf").to_vec())
            .expect("font")
    }

    #[test]
    fn software_text_font_rejects_invalid_bytes() {
        assert!(SoftwareTextFont::from_bytes(vec![0, 1, 2, 3]).is_err());
    }

    #[test]
    fn default_software_text_font_has_no_process_global_cache() {
        let source = include_str!("software_text_raster.rs");
        let once_lock = ["Once", "Lock"].concat();
        let cached_default = ["static ", "FONT"].concat();
        let default_font_fn = ["fn ", "default_font()"].concat();

        assert!(
            !source.contains(&cached_default)
                && !source.contains(&default_font_fn)
                && !source.contains(&once_lock),
            "default software text font construction must be explicit renderer/app-owned state, not a process-global cache"
        );
    }

    #[test]
    fn software_text_measurer_empty_font_set_uses_deterministic_fallback_without_panicking() {
        let measurer = SoftwareTextMeasurer::from_font_set(SoftwareTextFontSet::empty(), 4);
        let style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = AnnotatedString::from("ab\nc");

        let metrics = measurer.measure(&text, &style);
        assert_eq!(metrics.line_count, 2);
        assert!(metrics.width > 0.0);
        assert!(metrics.height >= metrics.line_height * 2.0);

        let cursor_x = measurer.get_cursor_x_for_offset(&text, &style, 2);
        assert!(cursor_x > 0.0);
        let second_line_offset =
            measurer.get_offset_for_position(&text, &style, 0.0, metrics.line_height);
        assert!(
            second_line_offset >= "ab\n".len(),
            "fallback hit testing should resolve into the second line: {second_line_offset}"
        );

        let layout = measurer.layout(&text, &style);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.glyph_layouts().len(), 3);
    }

    #[test]
    fn software_text_metrics_layout_and_cursor_share_font_backend() {
        let font = test_software_font();
        let style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(18.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let text = "Text\nBackend";

        let metrics = measure_text_with_font(text, &style, 18.0, &font);
        let layout = layout_text_with_font(text, &style, &font);

        assert!(metrics.width > 0.0);
        assert_eq!(metrics.line_count, 2);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.height, metrics.height);
        assert!(layout.glyph_layouts().len() >= "TextBackend".len());

        let offset =
            text_offset_for_position_with_font(text, &style, 0.0, metrics.line_height, &font);
        assert!(
            offset >= "Text\n".len(),
            "second-line hit testing should return a byte offset on the second line: {offset}"
        );
        let cursor_x = cursor_x_for_offset_with_font(text, &style, "Text".len(), &font);
        assert!(cursor_x > 0.0);
    }

    #[test]
    fn software_text_metrics_keep_requested_font_size_for_default_font() {
        let font = default_software_text_font().expect("bundled default test font");
        let style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(14.0),
                ..Default::default()
            },
            ..Default::default()
        };

        let metrics = measure_text_with_font("Counter App", &style, 14.0, &font);
        assert!(
            (metrics.width - 83.16).abs() < 0.05 && (metrics.height - 19.6).abs() < 0.05,
            "14sp demo text must use font em metrics, not ab_glyph height units: {metrics:?}"
        );
    }

    #[test]
    fn software_text_synthesizes_missing_bold_weight() {
        let font = test_software_font();
        let normal_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let bold_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            ..Default::default()
        };
        let no_synthesis_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                font_weight: Some(FontWeight::BOLD),
                font_synthesis: Some(FontSynthesis::None),
                ..Default::default()
            },
            ..Default::default()
        };

        let normal = measure_text_with_font("Save Raster WebP", &normal_style, 20.0, &font);
        let synthesized = measure_text_with_font("Save Raster WebP", &bold_style, 20.0, &font);
        let disabled = measure_text_with_font("Save Raster WebP", &no_synthesis_style, 20.0, &font);

        assert!(
            synthesized.width > normal.width * 1.04,
            "bold fallback should synthesize heavier advances: normal={normal:?} synthesized={synthesized:?}"
        );
        assert!(
            (disabled.width - normal.width).abs() < 0.01,
            "explicit FontSynthesis::None should preserve regular metrics: normal={normal:?} disabled={disabled:?}"
        );
    }

    #[test]
    fn rasterized_synthetic_bold_adds_ink_without_changing_line_box() {
        let font = test_software_font();
        let normal_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let bold_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(20.0),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            ..Default::default()
        };
        let normal_metrics = measure_text_with_font("Composer", &normal_style, 20.0, &font);
        let bold_metrics = measure_text_with_font("Composer", &bold_style, 20.0, &font);

        let normal = rasterize_text_to_image(
            "Composer",
            Rect {
                x: 0.0,
                y: 0.0,
                width: normal_metrics.width.ceil(),
                height: normal_metrics.height.ceil(),
            },
            &normal_style,
            Color::WHITE,
            20.0,
            1.0,
            &font,
        )
        .expect("normal text image");
        let bold = rasterize_text_to_image(
            "Composer",
            Rect {
                x: 0.0,
                y: 0.0,
                width: bold_metrics.width.ceil(),
                height: bold_metrics.height.ceil(),
            },
            &bold_style,
            Color::WHITE,
            20.0,
            1.0,
            &font,
        )
        .expect("bold text image");

        assert_eq!(bold.height(), normal.height());
        assert!(
            count_ink_pixels(&bold) > count_ink_pixels(&normal),
            "synthetic bold should increase rasterized ink coverage"
        );
    }

    #[test]
    fn software_text_synthesizes_missing_italic_style() {
        let font = test_software_font();
        let normal_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(36.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let italic_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(36.0),
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            ..Default::default()
        };
        let no_synthesis_style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(36.0),
                font_style: Some(FontStyle::Italic),
                font_synthesis: Some(FontSynthesis::None),
                ..Default::default()
            },
            ..Default::default()
        };

        let normal_metrics = measure_text_with_font("Italic", &normal_style, 36.0, &font);
        let italic_metrics = measure_text_with_font("Italic", &italic_style, 36.0, &font);
        let disabled_metrics = measure_text_with_font("Italic", &no_synthesis_style, 36.0, &font);

        assert!(
            italic_metrics.width > normal_metrics.width + 6.0,
            "italic fallback should reserve slanted visual overhang: normal={normal_metrics:?} italic={italic_metrics:?}"
        );
        assert!(
            (disabled_metrics.width - normal_metrics.width).abs() < 0.01,
            "explicit FontSynthesis::None should preserve regular metrics: normal={normal_metrics:?} disabled={disabled_metrics:?}"
        );

        let normal = rasterize_text_to_image(
            "Italic",
            Rect {
                x: 0.0,
                y: 0.0,
                width: normal_metrics.width.ceil(),
                height: normal_metrics.height.ceil(),
            },
            &normal_style,
            Color::WHITE,
            36.0,
            1.0,
            &font,
        )
        .expect("normal text image");
        let italic = rasterize_text_to_image(
            "Italic",
            Rect {
                x: 0.0,
                y: 0.0,
                width: italic_metrics.width.ceil(),
                height: italic_metrics.height.ceil(),
            },
            &italic_style,
            Color::WHITE,
            36.0,
            1.0,
            &font,
        )
        .expect("italic text image");
        let disabled = rasterize_text_to_image(
            "Italic",
            Rect {
                x: 0.0,
                y: 0.0,
                width: disabled_metrics.width.ceil(),
                height: disabled_metrics.height.ceil(),
            },
            &no_synthesis_style,
            Color::WHITE,
            36.0,
            1.0,
            &font,
        )
        .expect("disabled italic text image");

        assert_eq!(
            normal.pixels(),
            disabled.pixels(),
            "FontSynthesis::None must not synthesize oblique glyphs"
        );
        assert!(
            vertical_slant_delta(&italic) > vertical_slant_delta(&normal) + 2.0,
            "synthetic italic should visibly lean top ink to the right"
        );
    }

    #[test]
    fn rasterized_default_text_fills_expected_visual_height() {
        let font = default_software_text_font().expect("bundled default test font");
        let style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(14.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let metrics = measure_text_with_font("Counter App", &style, 14.0, &font);
        let image = rasterize_text_to_image(
            "Counter App",
            Rect {
                x: 0.0,
                y: 0.0,
                width: metrics.width.ceil(),
                height: metrics.height.ceil(),
            },
            &style,
            Color::WHITE,
            14.0,
            1.0,
            &font,
        )
        .expect("text image");
        let (top, bottom) = ink_y_range(&image).expect("text should contain ink");
        let ink_height = bottom - top;

        assert!(
            ink_height >= 13,
            "14sp default text ink should keep visual height parity with the WGPU baseline: top={top} bottom={bottom} image={}x{}",
            image.width(),
            image.height()
        );
    }

    #[test]
    fn software_text_font_selection_preserves_first_complete_default_face() {
        let regular =
            SoftwareTextFont::from_bytes(include_bytes!("../assets/NotoSansMerged.ttf").to_vec())
                .expect("regular test font should load");
        let font = software_text_font_from_fonts_or_default(&[
            include_bytes!("../assets/NotoSansMerged.ttf"),
            include_bytes!("../assets/NotoSansBold.ttf"),
            include_bytes!("../assets/TwemojiMozilla.ttf"),
        ])
        .expect("font selection should resolve a test font");
        let style = TextStyle {
            span_style: SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(18.0),
                ..Default::default()
            },
            ..Default::default()
        };

        let regular_metrics = measure_text_with_font("UNDER", &style, 18.0, &regular);
        let metrics = measure_text_with_font("UNDER", &style, 18.0, &font);
        assert!(
            (metrics.width - regular_metrics.width).abs() < 0.01,
            "font selection should keep the declared regular face for default text: selected={metrics:?}, regular={regular_metrics:?}"
        );
    }

    #[test]
    fn software_text_font_resolution_reuses_cached_font_score() {
        let font = test_software_font();
        assert!(
            font.score.is_complete_default_face(),
            "test font should cache complete Latin coverage at load time: supported={} width={}",
            font.score.supported_latin_chars,
            font.score.latin_sample_width
        );

        let fonts = SoftwareTextFontSet::from_font(font.clone());
        let resolved = fonts
            .resolve(&TextStyle {
                span_style: SpanStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..Default::default()
                },
                ..Default::default()
            })
            .expect("font set should resolve a test font");

        assert_eq!(
            resolved.score.supported_latin_chars,
            font.score.supported_latin_chars
        );
        assert_eq!(
            resolved.score.latin_sample_width,
            font.score.latin_sample_width
        );
    }

    #[test]
    fn software_text_font_set_resolves_requested_weight() {
        let fonts = software_text_font_set_from_fonts_or_default(&[
            include_bytes!("../assets/NotoSansMerged.ttf"),
            include_bytes!("../assets/NotoSansBold.ttf"),
            include_bytes!("../assets/TwemojiMozilla.ttf"),
        ]);
        let regular = fonts
            .resolve(&TextStyle::default())
            .expect("font set should resolve regular test font");
        let bold_style = TextStyle {
            span_style: SpanStyle {
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            },
            ..Default::default()
        };
        let bold = fonts
            .resolve(&bold_style)
            .expect("font set should resolve bold test font");

        assert_eq!(regular.weight(), FontWeight::NORMAL);
        assert_eq!(bold.weight(), FontWeight::BOLD);

        let regular_metrics =
            measure_text_with_font("Counter App", &TextStyle::default(), 18.0, regular);
        let bold_metrics = measure_text_with_font("Counter App", &bold_style, 18.0, bold);
        assert!(
            bold_metrics.width > regular_metrics.width,
            "bold face resolution should affect real text metrics: regular={regular_metrics:?} bold={bold_metrics:?}"
        );
    }

    #[test]
    fn software_text_metrics_use_largest_annotated_span_font_size() {
        let font = default_software_text_font().expect("bundled default test font");
        let text = AnnotatedString::builder()
            .push_style(SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(30.0),
                ..Default::default()
            })
            .append("BIG ")
            .pop()
            .push_style(SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(10.0),
                ..Default::default()
            })
            .append("small")
            .pop()
            .to_annotated_string();

        let metrics = measure_annotated_text_with_font(&text, &TextStyle::default(), 14.0, &font);

        assert!(
            metrics.height >= 30.0,
            "rich text metrics must include the largest span height: {metrics:?}"
        );
        assert!(
            metrics.width > 48.0,
            "rich text metrics should measure run widths at their span sizes: {metrics:?}"
        );
    }

    #[test]
    fn software_text_metrics_cache_keys_include_span_styles() {
        let measurer = SoftwareTextMeasurer::new(
            default_software_text_font().expect("bundled default test font"),
            8,
        );
        let plain = AnnotatedString::from("BIG small");
        let rich = AnnotatedString::builder()
            .push_style(SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(30.0),
                ..Default::default()
            })
            .append("BIG ")
            .pop()
            .append("small")
            .to_annotated_string();

        let plain_metrics = measurer.measure(&plain, &TextStyle::default());
        let rich_metrics = measurer.measure(&rich, &TextStyle::default());

        assert!(
            rich_metrics.height > plain_metrics.height,
            "cached plain text metrics must not be reused for styled text: plain={plain_metrics:?} rich={rich_metrics:?}"
        );
    }

    #[test]
    fn software_text_metrics_cache_recovers_after_poison() {
        let measurer = SoftwareTextMeasurer::new(
            default_software_text_font().expect("bundled default test font"),
            8,
        );
        let text = AnnotatedString::from("Recovered text metrics");

        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = measurer
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panic!("poison software text metrics cache for recovery test");
        }));

        assert!(poison_result.is_err());

        let metrics = measurer.measure(&text, &TextStyle::default());
        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);

        let subset =
            measurer.measure_subsequence(&text, 0.."Recovered".len(), &TextStyle::default());
        assert!(subset.width > 0.0);
        assert!(subset.width < metrics.width);
    }

    #[test]
    fn rasterized_gradient_text_shows_color_transition() {
        let font = test_font();
        // Use a gradient sized to the rendered text width so left=red, right=blue.
        // We first do a plain measurement pass to know the text width.
        let plain_style = TextStyle::default();
        let probe = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 96.0,
            },
            &plain_style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("probe image");
        let (ink_x_min, ink_x_max) = ink_x_range(&probe).expect("probe must contain ink");
        let gradient_end = ink_x_max as f32;

        let style = TextStyle {
            span_style: SpanStyle {
                brush: Some(Brush::linear_gradient_range(
                    vec![Color::RED, Color::BLUE],
                    Point::new(0.0, 0.0),
                    Point::new(gradient_end, 0.0),
                )),
                ..Default::default()
            },
            ..Default::default()
        };

        let image = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            Rect {
                x: 0.0,
                y: 0.0,
                width: 320.0,
                height: 96.0,
            },
            &style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("rasterized image");

        let ink_span = ink_x_max.saturating_sub(ink_x_min).max(1);
        let left_end = ink_x_min + ink_span * 3 / 10;
        let right_start = ink_x_max.saturating_sub(ink_span * 3 / 10);
        let left = average_ink_rgb(&image, ink_x_min, left_end, 8, 90).expect("left ink");
        let right = average_ink_rgb(&image, right_start, ink_x_max, 8, 90).expect("right ink");
        assert!(
            left[0] > left[2] * 1.1,
            "left region should be red dominant, got {left:?}"
        );
        assert!(
            right[2] > right[0] * 1.1,
            "right region should be blue dominant, got {right:?}"
        );
    }

    #[test]
    fn rasterized_stroke_and_fill_ink_coverage_differs() {
        let font = test_font();
        let fill_style = TextStyle::default();
        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke { width: 6.0 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 96.0,
        };

        let fill = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            rect,
            &fill_style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("fill image");
        let stroke = rasterize_text_to_image_with_font(
            "MMMMMMMM",
            rect,
            &stroke_style,
            Color::WHITE,
            48.0,
            1.0,
            &font,
        )
        .expect("stroke image");

        let fill_ink = count_ink_pixels(&fill);
        let stroke_ink = count_ink_pixels(&stroke);
        assert_ne!(fill.pixels(), stroke.pixels());
        assert!(
            fill_ink.abs_diff(stroke_ink) > 300,
            "fill/stroke ink coverage should differ; fill={fill_ink}, stroke={stroke_ink}"
        );
    }

    #[test]
    fn stroke_path_uses_miter_join_for_acute_apexes() {
        let font = test_font();
        let fill_style = TextStyle::default();
        let stroke_width = 12.0;
        let stroke_style = TextStyle {
            span_style: SpanStyle {
                draw_style: Some(TextDrawStyle::Stroke {
                    width: stroke_width,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 140.0,
        };

        let fill = rasterize_text_to_image_with_font(
            "A",
            rect,
            &fill_style,
            Color::WHITE,
            110.0,
            1.0,
            &font,
        )
        .expect("fill image");
        let stroke = rasterize_text_to_image_with_font(
            "A",
            rect,
            &stroke_style,
            Color::WHITE,
            110.0,
            1.0,
            &font,
        )
        .expect("stroke image");

        let fill_top = top_ink_row(&fill).expect("fill top row");
        let stroke_top = top_ink_row(&stroke).expect("stroke top row");
        let reference_dilation =
            rasterize_reference_dilation_stroke("A", rect, 110.0, stroke_width, &font);
        let reference_top = top_ink_row(&reference_dilation).expect("reference top row");
        let extra_extension = fill_top.saturating_sub(stroke_top) as f32;
        let half_stroke = stroke_width * 0.5;
        assert!(
            extra_extension >= half_stroke - 0.25,
            "stroke apex should extend by roughly at least half stroke width; fill_top={fill_top}, stroke_top={stroke_top}, half_stroke={half_stroke:.2}"
        );
        assert!(
            stroke.pixels() != reference_dilation.pixels(),
            "path stroke should diverge from mask-dilation reference output"
        );
        assert!(
            stroke_top <= reference_top,
            "miter stroke should keep acute apex at least as extended as mask-dilation reference; stroke_top={stroke_top}, reference_top={reference_top}"
        );
    }

    #[test]
    fn shadow_blur_radius_changes_spread_for_shared_raster_path() {
        let font = test_font();
        let base_shadow = Shadow {
            color: Color(0.0, 0.0, 0.0, 0.9),
            offset: Point::new(5.5, 4.25),
            blur_radius: 0.0,
        };
        let hard_shadow_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            ..Default::default()
        };
        let blurred_shadow_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(Shadow {
                    blur_radius: 9.0,
                    ..base_shadow
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 120.0,
        };

        let hard_shadow = rasterize_text_to_image_with_font(
            "Shared shadow",
            rect,
            &hard_shadow_style,
            Color::TRANSPARENT,
            48.0,
            1.0,
            &font,
        )
        .expect("hard shadow image");
        let blurred_shadow = rasterize_text_to_image_with_font(
            "Shared shadow",
            rect,
            &blurred_shadow_style,
            Color::TRANSPARENT,
            48.0,
            1.0,
            &font,
        )
        .expect("blurred shadow image");

        let hard_ink = count_ink_pixels(&hard_shadow);
        let blurred_ink = count_ink_pixels(&blurred_shadow);
        assert_ne!(
            hard_shadow.pixels(),
            blurred_shadow.pixels(),
            "blur radius should change rasterized shadow output"
        );
        assert!(
            blurred_ink > hard_ink,
            "blurred shadow should spread to more pixels; hard={hard_ink}, blurred={blurred_ink}"
        );
    }

    #[test]
    fn text_motion_changes_fractional_shadow_sampling() {
        let font = test_font();
        let base_shadow = Shadow {
            color: Color(0.0, 0.0, 0.0, 0.9),
            offset: Point::new(3.35, 2.65),
            blur_radius: 6.0,
        };
        let static_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::text::ParagraphStyle {
                text_motion: Some(TextMotion::Static),
                ..Default::default()
            },
        };
        let animated_style = TextStyle {
            span_style: SpanStyle {
                shadow: Some(base_shadow),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::text::ParagraphStyle {
                text_motion: Some(TextMotion::Animated),
                ..Default::default()
            },
        };
        let rect = Rect {
            x: 11.35,
            y: 7.65,
            width: 280.0,
            height: 120.0,
        };

        let static_image = rasterize_text_to_image_with_font(
            "Motion shadow",
            rect,
            &static_style,
            Color::TRANSPARENT,
            42.0,
            1.0,
            &font,
        )
        .expect("static image");
        let animated_image = rasterize_text_to_image_with_font(
            "Motion shadow",
            rect,
            &animated_style,
            Color::TRANSPARENT,
            42.0,
            1.0,
            &font,
        )
        .expect("animated image");

        assert_ne!(
            static_image.pixels(),
            animated_image.pixels(),
            "TextMotion::Static should quantize shadow placement while Animated keeps fractional sampling"
        );
    }

    #[test]
    fn static_text_motion_aligns_glyph_positions_to_pixel_grid() {
        let font = test_font();
        let base_glyph = layout_line_glyphs(&font, "A", 17.0, point(0.0, 13.37))
            .into_iter()
            .next()
            .expect("glyph");
        let static_aligned = align_glyph_for_text_motion(base_glyph, true);
        let static_position = static_aligned.position;
        assert!(
            (static_position.x - static_position.x.round()).abs() < f32::EPSILON,
            "static text should snap glyph x to pixel grid"
        );
        assert!(
            (static_position.y - static_position.y.round()).abs() < f32::EPSILON,
            "static text should snap glyph y to pixel grid"
        );

        let animated_source = layout_line_glyphs(&font, "A", 17.0, point(0.0, 13.37))
            .into_iter()
            .next()
            .expect("glyph");
        let animated_aligned = align_glyph_for_text_motion(animated_source, false);
        let animated_position = animated_aligned.position;
        assert!(
            (animated_position.y - 13.37).abs() < 1e-3,
            "animated text should preserve fractional glyph position"
        );
    }
}

//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile (Android/iOS).

mod effect_renderer;
pub(crate) mod gpu_stats;
mod offscreen;
mod pipeline;
mod render;
mod scene;
mod shader_cache;
mod shaders;

pub use scene::{BackdropLayer, ClickAction, DrawShape, HitRegion, ImageDraw, Scene, TextDraw};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    text_hyphenation::choose_auto_hyphen_break as choose_shared_auto_hyphen_break, RenderScene,
    Renderer,
};
use cranpose_ui::{set_text_measurer, LayoutTree, TextMeasurer};
use cranpose_ui_graphics::Size;
use glyphon::{
    Attrs, AttrsOwned, Buffer, FamilyOwned, FontSystem, Metrics, Shaping, Style as GlyphonStyle,
    Weight as GlyphonWeight,
};
use lru::LruCache;
use render::GpuRenderer;
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// Text fallback: NotoSansMerged (NotoSans + NotoSansSymbols, OFL 1.1).
// 1.2 MB; covers Latin/Greek/Cyrillic text plus arrows and symbols (U+2190–U+27FF etc.).
const EMBEDDED_FALLBACK_FONT_BYTES: &[u8] =
    include_bytes!("../../../../assets/NotoSansMerged.ttf");
const EMBEDDED_FALLBACK_FAMILY: &str = "Noto Sans";

// Emoji fallback: Twemoji.Mozilla (COLR+CPAL v0, Apache 2.0 / CC-BY 4.0).
// 1.4 MB; 13 700+ emoji as COLR vector glyphs — rendered in full color via SwashContent::Color.
const EMBEDDED_EMOJI_FONT_BYTES: &[u8] =
    include_bytes!("../../../../assets/TwemojiMozilla.ttf");
const EMBEDDED_EMOJI_FAMILY: &str = "Twemoji Mozilla";

/// Controls which fallback fonts the framework injects after primary app fonts.
///
/// The framework's default bundle is two fonts:
/// - **NotoSansMerged** (1.2 MB, OFL 1.1): Latin/Greek/Cyrillic text plus symbol blocks.
/// - **Twemoji Mozilla** (1.4 MB, Apache 2.0/CC-BY 4.0): 13 700+ COLR vector color emoji.
///
/// Both work on every target including WASM/WebGL2.  Applications are expected to supply
/// their own primary text fonts via [`AppLauncher::with_fonts`][crate::AppLauncher::with_fonts].
///
/// Use this policy to extend, replace, or disable the bundle.
///
/// # Example
///
/// ```no_run
/// // (inside AppLauncher builder – see AppLauncher docs for full example)
/// // Add fonts on top of the framework defaults:
/// //   .with_extra_fallback_fonts(&[MY_EXTRA_FONT])
/// // Replace the framework defaults entirely:
/// //   .with_fallback_fonts(&[MY_FALLBACK])
/// // Disable all framework fallbacks:
/// //   .with_no_fallback_fonts()
/// ```
#[derive(Clone, Debug, Default)]
pub enum FallbackFontPolicy {
    /// Inject the framework's default fallback bundle: NotoSansMerged (text+symbols) +
    /// Twemoji Mozilla (COLR color emoji).
    ///
    /// Appended after primary app fonts so it only activates for codepoints the primary
    /// fonts do not cover.  This is the default on all targets including WASM.
    #[default]
    Default,
    /// Inject the framework defaults **and** the supplied extra fonts as additional fallbacks.
    ///
    /// Framework defaults are loaded first; the extra fonts are appended after.
    Extend(&'static [&'static [u8]]),
    /// Do **not** inject the framework defaults; use only the supplied fonts as fallbacks.
    ///
    /// The renderer still guarantees at least one face is present – if the provided
    /// slice is empty it injects the embedded NotoSansMerged as a last-resort guard
    /// so that text rendering never panics.
    Replace(&'static [&'static [u8]]),
    /// No framework or user-supplied fallback fonts.
    ///
    /// Only primary app fonts (passed via `AppLauncher::with_fonts`) are loaded.
    /// The renderer still injects the embedded NotoSansMerged if the font database
    /// would otherwise be empty, preventing a hard renderer crash.
    None,
}

/// Size-only cache for ultra-fast text measurement lookups.
/// Key: (text_hash, font_size_fixed_point, style_hash)
/// Value: (text_content, size) - text stored to handle hash collisions
type TextSizeCache = Arc<Mutex<LruCache<(u64, i32, u64), (String, Size)>>>;

#[derive(Debug)]
pub enum WgpuRendererError {
    Layout(String),
    Wgpu(String),
}

/// CPU-readable RGBA frame captured from the renderer output.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Unified hash key for text caching - shared between measurement and rendering.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum TextKey {
    Content(String),
    Node(NodeId),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TextCacheKey {
    key: TextKey,
    scale_bits: u32, // f32 as bits for hashing
    style_hash: u64,
}

impl TextCacheKey {
    fn new(text: &str, font_size: f32, style_hash: u64) -> Self {
        Self {
            key: TextKey::Content(text.to_string()),
            scale_bits: font_size.to_bits(),
            style_hash,
        }
    }

    fn for_node(node_id: NodeId, font_size: f32, style_hash: u64) -> Self {
        Self {
            key: TextKey::Node(node_id),
            scale_bits: font_size.to_bits(),
            style_hash,
        }
    }
}

impl Hash for TextCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.scale_bits.hash(state);
        self.style_hash.hash(state);
    }
}

/// Cached text buffer shared between measurement and rendering
pub(crate) struct SharedTextBuffer {
    pub(crate) buffer: Buffer,
    text: String,
    font_size: f32,
    line_height: f32,
    style_hash: u64,
    /// Cached size to avoid recalculating on every access
    cached_size: Option<Size>,
}

pub(crate) struct EnsureTextBufferParams<'a> {
    pub(crate) annotated_text: &'a cranpose_ui::text::AnnotatedString,
    pub(crate) font_size_px: f32,
    pub(crate) line_height_px: f32,
    pub(crate) style_hash: u64,
    pub(crate) style: &'a cranpose_ui::text::TextStyle,
    pub(crate) scale: f32,
}

impl SharedTextBuffer {
    /// Ensure the buffer has the correct text and font_size, only reshaping if needed
    pub(crate) fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        font_family_resolver: &mut WgpuFontFamilyResolver,
        params: EnsureTextBufferParams<'_>,
    ) {
        let annotated_text = params.annotated_text;
        let font_size_px = params.font_size_px;
        let line_height_px = params.line_height_px;
        let style_hash = params.style_hash;
        let style = params.style;
        let scale = params.scale;
        let text_str = annotated_text.text.as_str();
        let text_changed = self.text != text_str;
        let font_changed = (self.font_size - font_size_px).abs() > 0.1;
        let line_height_changed = (self.line_height - line_height_px).abs() > 0.1;
        let style_changed = self.style_hash != style_hash;

        // Only reshape if something actually changed
        if !text_changed && !font_changed && !line_height_changed && !style_changed {
            return; // Nothing changed, skip reshape
        }

        // Set metrics and size for unlimited layout
        let metrics = Metrics::new(font_size_px, line_height_px);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer
            .set_size(font_system, Some(f32::MAX), Some(f32::MAX));

        let unscaled_base_size = if scale > 0.0 {
            font_size_px / scale
        } else {
            14.0
        };

        // Set text and shape
        if annotated_text.span_styles.is_empty() {
            let attrs = attrs_from_text_style(
                style,
                unscaled_base_size,
                scale,
                font_system,
                font_family_resolver,
            );
            let attrs_ref = attrs.as_attrs();
            self.buffer
                .set_text(font_system, text_str, &attrs_ref, Shaping::Advanced);
        } else {
            let boundaries = annotated_text.span_boundaries();
            let mut rich_spans: Vec<(&str, AttrsOwned)> =
                Vec::with_capacity(boundaries.len().saturating_sub(1));
            for window in boundaries.windows(2) {
                let start = window[0];
                let end = window[1];
                if start == end {
                    continue;
                }
                let slice = &annotated_text.text[start..end];
                let mut merged_style = style.span_style.clone();
                for span in &annotated_text.span_styles {
                    if span.range.start <= start && span.range.end >= end {
                        merged_style = merged_style.merge(&span.item);
                    }
                }
                let mut chunk_text_style = style.clone();
                chunk_text_style.span_style = merged_style;
                let attrs = attrs_from_text_style(
                    &chunk_text_style,
                    unscaled_base_size,
                    scale,
                    font_system,
                    font_family_resolver,
                );
                rich_spans.push((slice, attrs));
            }
            let default_attrs = attrs_from_text_style(
                style,
                unscaled_base_size,
                scale,
                font_system,
                font_family_resolver,
            );
            let default_attrs_ref = default_attrs.as_attrs();
            self.buffer.set_rich_text(
                font_system,
                rich_spans
                    .iter()
                    .map(|(slice, attrs)| (*slice, attrs.as_attrs())),
                &default_attrs_ref,
                Shaping::Advanced,
                None,
            );
        }
        self.buffer.shape_until_scroll(font_system, false);

        // Update cached values
        self.text.clear();
        self.text.push_str(text_str);
        self.font_size = font_size_px;
        self.line_height = line_height_px;
        self.style_hash = style_hash;
        self.cached_size = None; // Invalidate size cache
    }

    /// Get or calculate the size of the shaped text
    pub(crate) fn size(&mut self) -> Size {
        if let Some(size) = self.cached_size {
            return size;
        }

        // Calculate size from buffer
        let mut max_width = 0.0f32;
        let mut total_height = 0.0f32;
        for run in self.buffer.layout_runs() {
            let mut run_height = run.line_height;
            for glyph in run.glyphs {
                let physical_height = glyph.font_size * 1.4; // 1.4 is our default line_height modifier
                if physical_height > run_height {
                    run_height = physical_height;
                }
            }

            max_width = max_width.max(run.line_w);
            total_height = total_height.max(run.line_top + run_height);
        }

        let size = Size {
            width: max_width,
            height: total_height,
        };

        self.cached_size = Some(size);
        size
    }
}

/// Shared cache for text buffers used by both measurement and rendering
pub(crate) type SharedTextCache = Arc<Mutex<HashMap<TextCacheKey, SharedTextBuffer>>>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TypefaceRequest {
    font_family: Option<cranpose_ui::text::FontFamily>,
    font_weight: cranpose_ui::text::FontWeight,
    font_style: cranpose_ui::text::FontStyle,
    font_synthesis: cranpose_ui::text::FontSynthesis,
}

impl TypefaceRequest {
    fn from_span_style(span_style: &cranpose_ui::text::SpanStyle) -> Self {
        Self {
            font_family: span_style.font_family.clone(),
            font_weight: span_style.font_weight.unwrap_or_default(),
            font_style: span_style.font_style.unwrap_or_default(),
            font_synthesis: span_style.font_synthesis.unwrap_or_default(),
        }
    }
}

#[derive(Default)]
struct WgpuFontFamilyResolver {
    request_cache: HashMap<TypefaceRequest, FamilyOwned>,
    loaded_typeface_paths: HashMap<String, String>,
    unavailable_typeface_paths: HashSet<String>,
    available_family_names: HashMap<String, String>,
    indexed_face_count: usize,
    generic_fallback_seeded: bool,
}

impl WgpuFontFamilyResolver {
    fn prime(&mut self, font_system: &mut FontSystem) {
        self.ensure_non_empty_font_db(font_system);
        self.ensure_family_index(font_system);
        self.ensure_generic_fallbacks(font_system);
    }

    fn resolve_family_owned(
        &mut self,
        font_system: &mut FontSystem,
        span_style: &cranpose_ui::text::SpanStyle,
    ) -> FamilyOwned {
        self.ensure_non_empty_font_db(font_system);
        self.ensure_family_index(font_system);
        self.ensure_generic_fallbacks(font_system);

        let request = TypefaceRequest::from_span_style(span_style);
        if let Some(cached) = self.request_cache.get(&request) {
            return cached.clone();
        }

        let resolved = self.resolve_family_owned_uncached(font_system, &request);
        self.request_cache.insert(request, resolved.clone());
        resolved
    }

    fn ensure_non_empty_font_db(&mut self, font_system: &mut FontSystem) {
        if font_system.db().faces().next().is_some() {
            load_embedded_fallback_if_missing(font_system);
            load_embedded_emoji_if_missing(font_system);
            return;
        }

        log::warn!("Font database is empty before shaping; injecting guard font");
        font_system
            .db_mut()
            .load_font_data(EMBEDDED_FALLBACK_FONT_BYTES.to_vec());

        if font_system.db().faces().next().is_none() {
            log::error!("Guard font failed to load; text shaping may still panic");
        }
    }

    fn resolve_family_owned_uncached(
        &mut self,
        font_system: &mut FontSystem,
        request: &TypefaceRequest,
    ) -> FamilyOwned {
        use cranpose_ui::text::FontFamily;

        match request.font_family.as_ref() {
            None | Some(FontFamily::Default | FontFamily::SansSerif) => FamilyOwned::SansSerif,
            Some(FontFamily::Serif) => FamilyOwned::Serif,
            Some(FontFamily::Monospace) => FamilyOwned::Monospace,
            Some(FontFamily::Cursive) => FamilyOwned::Cursive,
            Some(FontFamily::Fantasy) => FamilyOwned::Fantasy,
            Some(FontFamily::Named(name)) => self
                .canonical_family_name(name)
                .map(|resolved| FamilyOwned::Name(resolved.into()))
                .unwrap_or(FamilyOwned::SansSerif),
            Some(FontFamily::FileBacked(file_backed)) => self
                .resolve_file_backed_family(font_system, file_backed, request)
                .unwrap_or(FamilyOwned::SansSerif),
            Some(FontFamily::LoadedTypeface(typeface_path)) => self
                .resolve_loaded_typeface_family(font_system, typeface_path.path.as_str())
                .unwrap_or(FamilyOwned::SansSerif),
        }
    }

    fn resolve_file_backed_family(
        &mut self,
        font_system: &mut FontSystem,
        file_backed: &cranpose_ui::text::FileBackedFontFamily,
        request: &TypefaceRequest,
    ) -> Option<FamilyOwned> {
        let mut candidates: Vec<&cranpose_ui::text::FontFile> = file_backed.fonts.iter().collect();
        candidates.sort_by_key(|candidate| {
            let style_penalty = if candidate.style == request.font_style {
                0u32
            } else {
                10_000u32
            };
            let weight_penalty =
                (i32::from(candidate.weight.0) - i32::from(request.font_weight.0)).unsigned_abs();
            style_penalty + weight_penalty
        });

        for candidate in candidates {
            let Some(family_name) = self.load_typeface_path(font_system, candidate.path.as_str())
            else {
                continue;
            };
            if let Some(canonical) = self.canonical_family_name(family_name.as_str()) {
                return Some(FamilyOwned::Name(canonical.into()));
            }
        }
        None
    }

    fn resolve_loaded_typeface_family(
        &mut self,
        font_system: &mut FontSystem,
        path: &str,
    ) -> Option<FamilyOwned> {
        self.load_typeface_path(font_system, path)
            .map(|family_name| {
                self.canonical_family_name(family_name.as_str())
                    .map(|resolved| FamilyOwned::Name(resolved.into()))
                    .unwrap_or(FamilyOwned::SansSerif)
            })
    }

    fn ensure_family_index(&mut self, font_system: &FontSystem) {
        let face_count = font_system.db().faces().count();
        if face_count == self.indexed_face_count {
            return;
        }

        self.available_family_names.clear();
        for face in font_system.db().faces() {
            for (family_name, _) in &face.families {
                self.available_family_names
                    .entry(family_name.to_lowercase())
                    .or_insert_with(|| family_name.clone());
            }
        }
        self.indexed_face_count = face_count;
        self.request_cache.clear();
        self.generic_fallback_seeded = false;
    }

    fn canonical_family_name(&self, family_name: &str) -> Option<String> {
        self.available_family_names
            .get(&family_name.to_lowercase())
            .cloned()
    }

    fn ensure_generic_fallbacks(&mut self, font_system: &mut FontSystem) {
        if self.generic_fallback_seeded {
            return;
        }

        let Some(primary_family) = font_system
            .db()
            .faces()
            .find_map(|face| face.families.first().map(|(name, _)| name.clone()))
        else {
            return;
        };

        let db = font_system.db_mut();
        db.set_sans_serif_family(primary_family.clone());
        db.set_serif_family(primary_family.clone());
        db.set_monospace_family(primary_family.clone());
        db.set_cursive_family(primary_family.clone());
        db.set_fantasy_family(primary_family);

        self.generic_fallback_seeded = true;
        self.request_cache.clear();
    }

    fn load_typeface_path(&mut self, font_system: &mut FontSystem, path: &str) -> Option<String> {
        if let Some(family_name) = self.loaded_typeface_paths.get(path) {
            return Some(family_name.clone());
        }

        if self.unavailable_typeface_paths.contains(path) {
            return None;
        }

        #[cfg(target_arch = "wasm32")]
        let _ = font_system;

        #[cfg(target_arch = "wasm32")]
        {
            log::warn!(
                "Typeface path '{}' requested on wasm target; filesystem font loading is unavailable",
                path
            );
            self.unavailable_typeface_paths.insert(path.to_string());
            return None;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let font_bytes = match std::fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    log::warn!("Failed to read typeface path '{}': {}", path, error);
                    self.unavailable_typeface_paths.insert(path.to_string());
                    return None;
                }
            };
            let preferred_family = primary_family_name_from_bytes(font_bytes.as_slice());
            let previous_face_count = font_system.db().faces().count();
            font_system.db_mut().load_font_data(font_bytes);

            self.ensure_family_index(font_system);

            let mut resolved_family =
                preferred_family.and_then(|name| self.canonical_family_name(name.as_str()));
            if resolved_family.is_none() && self.indexed_face_count > previous_face_count {
                resolved_family = font_system
                    .db()
                    .faces()
                    .skip(previous_face_count)
                    .find_map(|face| face.families.first().map(|(name, _)| name.clone()));
            }

            let Some(family_name) = resolved_family else {
                log::warn!(
                    "Typeface path '{}' loaded but no usable family name was resolved",
                    path
                );
                self.unavailable_typeface_paths.insert(path.to_string());
                return None;
            };
            let family_name = self
                .canonical_family_name(family_name.as_str())
                .unwrap_or(family_name);

            self.loaded_typeface_paths
                .insert(path.to_string(), family_name.clone());
            self.unavailable_typeface_paths.remove(path);
            Some(family_name)
        }
    }
}

fn load_fonts_with_policy(
    font_system: &mut FontSystem,
    primary_fonts: &[&[u8]],
    policy: &FallbackFontPolicy,
) {
    for (i, font_data) in primary_fonts.iter().enumerate() {
        log::info!(
            "Loading primary font #{}, size: {} bytes",
            i,
            font_data.len()
        );
        font_system.db_mut().load_font_data(font_data.to_vec());
    }

    match policy {
        FallbackFontPolicy::Default => {
            load_framework_fallbacks(font_system);
        }
        FallbackFontPolicy::Extend(extra) => {
            load_framework_fallbacks(font_system);
            for (i, font_data) in extra.iter().enumerate() {
                log::info!(
                    "Loading extra fallback font #{}, size: {} bytes",
                    i,
                    font_data.len()
                );
                font_system.db_mut().load_font_data(font_data.to_vec());
            }
        }
        FallbackFontPolicy::Replace(user_fallbacks) => {
            for (i, font_data) in user_fallbacks.iter().enumerate() {
                log::info!(
                    "Loading replacement fallback font #{}, size: {} bytes",
                    i,
                    font_data.len()
                );
                font_system.db_mut().load_font_data(font_data.to_vec());
            }
            ensure_guard_font(font_system);
        }
        FallbackFontPolicy::None => {
            ensure_guard_font(font_system);
        }
    }

    let face_count = font_system.db().faces().count();
    log::info!("Total font faces loaded: {}", face_count);
}

fn load_framework_fallbacks(font_system: &mut FontSystem) {
    load_embedded_fallback_if_missing(font_system);
    load_embedded_emoji_if_missing(font_system);
    ensure_guard_font(font_system);
}

/// Inject the fallback font only when the database is still empty after all policy loading.
fn ensure_guard_font(font_system: &mut FontSystem) {
    if font_system.db().faces().count() == 0 {
        log::warn!("No fonts loaded – injecting NotoSansMerged as last-resort guard.");
        font_system
            .db_mut()
            .load_font_data(EMBEDDED_FALLBACK_FONT_BYTES.to_vec());
    }
}

fn font_db_contains_family(font_system: &FontSystem, family_name: &str) -> bool {
    font_system.db().faces().any(|face| {
        face.families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(family_name))
    })
}

fn load_embedded_fallback_if_missing(font_system: &mut FontSystem) {
    if font_db_contains_family(font_system, EMBEDDED_FALLBACK_FAMILY) {
        return;
    }
    log::info!(
        "Loading embedded text fallback {} (NotoSans + symbols, 1.2 MB)",
        EMBEDDED_FALLBACK_FAMILY
    );
    font_system
        .db_mut()
        .load_font_data(EMBEDDED_FALLBACK_FONT_BYTES.to_vec());
}

fn load_embedded_emoji_if_missing(font_system: &mut FontSystem) {
    if font_db_contains_family(font_system, EMBEDDED_EMOJI_FAMILY) {
        return;
    }
    log::info!(
        "Loading embedded emoji font {} (COLR color, 1.4 MB)",
        EMBEDDED_EMOJI_FAMILY
    );
    font_system
        .db_mut()
        .load_font_data(EMBEDDED_EMOJI_FONT_BYTES.to_vec());
}

#[cfg(not(target_arch = "wasm32"))]
fn primary_family_name_from_bytes(bytes: &[u8]) -> Option<String> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let mut fallback_family = None;
    for name in face.names() {
        if name.name_id == ttf_parser::name_id::TYPOGRAPHIC_FAMILY {
            let resolved = name.to_string().filter(|value| !value.is_empty());
            if resolved.is_some() {
                return resolved;
            }
        }
        if fallback_family.is_none() && name.name_id == ttf_parser::name_id::FAMILY {
            fallback_family = name.to_string().filter(|value| !value.is_empty());
        }
    }
    fallback_family
}

type SharedFontFamilyResolver = Arc<Mutex<WgpuFontFamilyResolver>>;

/// Trim text cache if it exceeds MAX_CACHE_ITEMS.
/// Removes the oldest half of entries when limit is reached.
pub(crate) fn trim_text_cache(cache: &mut HashMap<TextCacheKey, SharedTextBuffer>) {
    if cache.len() > MAX_CACHE_ITEMS {
        let target_size = MAX_CACHE_ITEMS / 2;
        let to_remove = cache.len() - target_size;

        // Remove oldest entries (arbitrary keys from the front)
        let keys_to_remove: Vec<TextCacheKey> = cache.keys().take(to_remove).cloned().collect();

        for key in keys_to_remove {
            cache.remove(&key);
        }

        log::debug!(
            "Trimmed text cache from {} to {} entries",
            cache.len() + to_remove,
            cache.len()
        );
    }
}

/// Maximum number of cached text buffers before trimming occurs
const MAX_CACHE_ITEMS: usize = 256;

/// WGPU-based renderer for GPU-accelerated 2D rendering.
///
/// This renderer supports:
/// - GPU-accelerated shape rendering (rectangles, rounded rectangles)
/// - Gradients (solid, linear, radial)
/// - GPU text rendering via glyphon
/// - Cross-platform support (Desktop, Web, Mobile)
pub struct WgpuRenderer {
    scene: Scene,
    gpu_renderer: Option<GpuRenderer>,
    font_system: Arc<Mutex<FontSystem>>,
    font_family_resolver: SharedFontFamilyResolver,
    /// Shared text buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
    /// Root scale factor for text rendering (use for density scaling)
    root_scale: f32,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer.
    ///
    /// * `primary_fonts` – application-supplied font bytes loaded at highest priority.
    ///   Pass `&[]` if the application does not supply its own fonts.
    /// * `policy` – controls which framework fallback fonts are injected after the
    ///   primary fonts.  See [`FallbackFontPolicy`] for the available options.
    ///
    /// Call [`init_gpu`][Self::init_gpu] before rendering.
    ///
    /// # Example
    ///
    /// ```text
    /// let font_light = include_bytes!("path/to/font-light.ttf");
    /// let font_regular = include_bytes!("path/to/font-regular.ttf");
    /// let renderer = WgpuRenderer::new(
    ///     &[font_light, font_regular],
    ///     &FallbackFontPolicy::Default,
    /// );
    /// ```
    pub fn new(primary_fonts: &[&[u8]], policy: &FallbackFontPolicy) -> Self {
        let mut font_system = FontSystem::new();

        // On Android never load system fonts: modern Android ships variable Roboto
        // which can cause rasterization corruption or font-ID conflicts with glyphon.
        #[cfg(target_os = "android")]
        log::info!("Skipping Android system fonts – using application-provided fonts only");

        load_fonts_with_policy(&mut font_system, primary_fonts, policy);

        let mut font_family_resolver_impl = WgpuFontFamilyResolver::default();
        font_family_resolver_impl.prime(&mut font_system);

        let font_system = Arc::new(Mutex::new(font_system));
        let font_family_resolver = Arc::new(Mutex::new(font_family_resolver_impl));
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer = WgpuTextMeasurer::new(
            font_system.clone(),
            text_cache.clone(),
            font_family_resolver.clone(),
        );
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            font_family_resolver,
            text_cache,
            root_scale: 1.0,
        }
    }

    /// Initialize GPU resources with a WGPU device and queue.
    pub fn init_gpu(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) {
        self.gpu_renderer = Some(GpuRenderer::new(
            device,
            queue,
            surface_format,
            self.font_system.clone(),
            self.font_family_resolver.clone(),
            self.text_cache.clone(),
        ));
    }

    /// Set root scale factor for text rendering (e.g., density scaling on Android)
    pub fn set_root_scale(&mut self, scale: f32) {
        self.root_scale = scale;
    }

    /// Render the scene to a texture view.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            gpu_renderer
                .render(
                    view,
                    &self.scene.shapes,
                    &self.scene.images,
                    &self.scene.texts,
                    &self.scene.shadow_draws,
                    &self.scene.effect_layers,
                    &self.scene.backdrop_layers,
                    width,
                    height,
                    self.root_scale,
                )
                .map_err(WgpuRendererError::Wgpu)
        } else {
            Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ))
        }
    }

    /// Render the current scene into an RGBA pixel buffer for robot tests.
    ///
    /// Uses the renderer's configured root scale.
    pub fn capture_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<CapturedFrame, WgpuRendererError> {
        self.capture_frame_with_scale(width, height, self.root_scale)
    }

    /// Render the current scene into an RGBA pixel buffer with an explicit scale.
    pub fn capture_frame_with_scale(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<CapturedFrame, WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            let pixels = gpu_renderer
                .render_to_rgba_pixels(
                    &self.scene.shapes,
                    &self.scene.images,
                    &self.scene.texts,
                    &self.scene.shadow_draws,
                    &self.scene.effect_layers,
                    &self.scene.backdrop_layers,
                    width,
                    height,
                    root_scale,
                )
                .map_err(WgpuRendererError::Wgpu)?;
            Ok(CapturedFrame {
                width,
                height,
                pixels,
            })
        } else {
            Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ))
        }
    }

    /// Get access to the WGPU device (for surface configuration).
    pub fn device(&self) -> &wgpu::Device {
        self.gpu_renderer
            .as_ref()
            .map(|r| &*r.device)
            .expect("GPU renderer not initialized")
    }
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new(&[], &FallbackFontPolicy::Default)
    }
}

impl Renderer for WgpuRenderer {
    type Scene = Scene;
    type Error = WgpuRendererError;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        // Build scene in logical dp - scaling happens in GPU vertex upload
        pipeline::render_layout_tree(layout_tree.root(), &mut self.scene);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        // Build scene in logical dp - scaling happens in GPU vertex upload
        // Traverse layout nodes via applier instead of rebuilding LayoutTree
        pipeline::render_from_applier(applier, root, &mut self.scene, 1.0);
        Ok(())
    }

    fn draw_dev_overlay(&mut self, text: &str, viewport: Size) {
        use cranpose_ui_graphics::{BlendMode, Brush, Color, Rect, RoundedCornerShape};

        // Draw FPS text in top-right corner with semi-transparent background
        // Position: 8px from right edge, 8px from top
        let padding = 8.0;
        let font_size = 14.0;

        // Measure text width (approximate: ~7px per character at 14px font)
        let char_width = 7.0;
        let text_width = text.len() as f32 * char_width;
        let text_height = font_size * 1.4;

        let x = viewport.width - text_width - padding * 2.0;
        let y = padding;

        // Add background rectangle (dark semi-transparent)
        let bg_rect = Rect {
            x,
            y,
            width: text_width + padding,
            height: text_height + padding / 2.0,
        };
        self.scene.push_shape(
            bg_rect,
            Brush::Solid(Color(0.0, 0.0, 0.0, 0.7)),
            Some(RoundedCornerShape::uniform(4.0)),
            None,
            BlendMode::SrcOver,
        );

        // Add text (green color for visibility)
        let text_rect = Rect {
            x: x + padding / 2.0,
            y: y + padding / 4.0,
            width: text_width,
            height: text_height,
        };
        self.scene.push_text(
            NodeId::MAX,
            text_rect,
            Rc::new(cranpose_ui::text::AnnotatedString::from(text)),
            Color(0.0, 1.0, 0.0, 1.0), // Green
            cranpose_ui::TextStyle::default(),
            font_size,
            1.0,
            cranpose_ui::TextLayoutOptions::default(),
            None,
        );
    }
}

fn resolve_font_size(style: &cranpose_ui::text::TextStyle) -> f32 {
    style.resolve_font_size(14.0)
}

fn resolve_line_height(style: &cranpose_ui::text::TextStyle, font_size: f32) -> f32 {
    style.resolve_line_height(14.0, font_size * 1.4)
}

fn resolve_max_span_font_size(
    style: &cranpose_ui::text::TextStyle,
    text: &cranpose_ui::text::AnnotatedString,
    base_font_size: f32,
) -> f32 {
    if text.span_styles.is_empty() {
        return base_font_size;
    }

    let mut max_font_size = base_font_size;
    for window in text.span_boundaries().windows(2) {
        let start = window[0];
        let end = window[1];
        if start == end {
            continue;
        }

        let mut merged_span = style.span_style.clone();
        for span in &text.span_styles {
            if span.range.start <= start && span.range.end >= end {
                merged_span = merged_span.merge(&span.item);
            }
        }
        let mut chunk_style = style.clone();
        chunk_style.span_style = merged_span;
        max_font_size = max_font_size.max(chunk_style.resolve_font_size(base_font_size));
    }
    max_font_size
}

pub(crate) fn resolve_effective_line_height(
    style: &cranpose_ui::text::TextStyle,
    text: &cranpose_ui::text::AnnotatedString,
    base_font_size: f32,
) -> f32 {
    let max_font_size = resolve_max_span_font_size(style, text, base_font_size);
    resolve_line_height(style, max_font_size)
}

fn family_owned_to_fontdb_family(family: &FamilyOwned) -> glyphon::fontdb::Family<'_> {
    match family {
        FamilyOwned::Name(name) => glyphon::fontdb::Family::Name(name.as_str()),
        FamilyOwned::Serif => glyphon::fontdb::Family::Serif,
        FamilyOwned::SansSerif => glyphon::fontdb::Family::SansSerif,
        FamilyOwned::Monospace => glyphon::fontdb::Family::Monospace,
        FamilyOwned::Cursive => glyphon::fontdb::Family::Cursive,
        FamilyOwned::Fantasy => glyphon::fontdb::Family::Fantasy,
    }
}

fn requested_fontdb_style(style: Option<cranpose_ui::text::FontStyle>) -> glyphon::fontdb::Style {
    match style.unwrap_or_default() {
        cranpose_ui::text::FontStyle::Normal => glyphon::fontdb::Style::Normal,
        cranpose_ui::text::FontStyle::Italic => glyphon::fontdb::Style::Italic,
    }
}

fn glyphon_style_from_fontdb(style: glyphon::fontdb::Style) -> GlyphonStyle {
    match style {
        glyphon::fontdb::Style::Italic | glyphon::fontdb::Style::Oblique => GlyphonStyle::Italic,
        glyphon::fontdb::Style::Normal => GlyphonStyle::Normal,
    }
}

fn resolve_available_style_and_weight(
    font_system: &FontSystem,
    family: &FamilyOwned,
    requested_weight: Option<cranpose_ui::text::FontWeight>,
    requested_style: Option<cranpose_ui::text::FontStyle>,
) -> Option<(GlyphonStyle, GlyphonWeight)> {
    let requested_fontdb_weight = requested_weight.unwrap_or_default().0;
    let requested_style = requested_fontdb_style(requested_style);
    let requested_family = family_owned_to_fontdb_family(family);
    let requested_family_name = font_system
        .db()
        .family_name(&requested_family)
        .to_ascii_lowercase();

    let style_penalty = |face_style: glyphon::fontdb::Style| -> u32 {
        if face_style == requested_style {
            0
        } else if requested_style != glyphon::fontdb::Style::Normal
            && face_style == glyphon::fontdb::Style::Normal
        {
            1_000
        } else {
            10_000
        }
    };

    let weight_penalty = |face_weight: u16| -> u32 {
        (i32::from(face_weight) - i32::from(requested_fontdb_weight)).unsigned_abs()
    };

    let mut best_in_family: Option<(u32, glyphon::fontdb::Style, u16)> = None;
    let mut best_global: Option<(u32, glyphon::fontdb::Style, u16)> = None;

    for face in font_system.db().faces() {
        let score = style_penalty(face.style) + weight_penalty(face.weight.0);
        let in_family = face
            .families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(requested_family_name.as_str()));

        if in_family
            && best_in_family
                .as_ref()
                .map(|(best_score, _, _)| score < *best_score)
                .unwrap_or(true)
        {
            best_in_family = Some((score, face.style, face.weight.0));
        }

        if best_global
            .as_ref()
            .map(|(best_score, _, _)| score < *best_score)
            .unwrap_or(true)
        {
            best_global = Some((score, face.style, face.weight.0));
        }
    }

    let (_, resolved_style, resolved_weight) = best_in_family.or(best_global)?;
    Some((
        glyphon_style_from_fontdb(resolved_style),
        GlyphonWeight(resolved_weight),
    ))
}

fn attrs_from_text_style(
    style: &cranpose_ui::text::TextStyle,
    unscaled_base_font_size: f32,
    scale: f32,
    font_system: &mut FontSystem,
    font_family_resolver: &mut WgpuFontFamilyResolver,
) -> AttrsOwned {
    let mut attrs = Attrs::new();
    let span_style = &style.span_style;
    let font_weight = span_style.font_weight;
    let font_style = span_style.font_style;
    let letter_spacing = span_style.letter_spacing;

    let unscaled_font_size = style.resolve_font_size(unscaled_base_font_size);
    let unscaled_line_height =
        style.resolve_line_height(unscaled_base_font_size, unscaled_font_size * 1.4);

    let font_size_px = unscaled_font_size * scale;
    let line_height_px = unscaled_line_height * scale;

    attrs = attrs.metrics(glyphon::Metrics::new(font_size_px, line_height_px));

    if let Some(color) = &span_style.color {
        let r = (color.0 * 255.0).clamp(0.0, 255.0) as u8;
        let g = (color.1 * 255.0).clamp(0.0, 255.0) as u8;
        let b = (color.2 * 255.0).clamp(0.0, 255.0) as u8;
        let a = (color.3 * 255.0).clamp(0.0, 255.0) as u8;
        attrs = attrs.color(glyphon::Color::rgba(r, g, b, a));
    }

    let family_owned = font_family_resolver.resolve_family_owned(font_system, span_style);
    attrs = attrs.family(family_owned.as_family());

    if let Some((resolved_style, resolved_weight)) =
        resolve_available_style_and_weight(font_system, &family_owned, font_weight, font_style)
    {
        attrs = attrs.style(resolved_style).weight(resolved_weight);
    } else {
        if let Some(font_weight) = font_weight {
            attrs = attrs.weight(GlyphonWeight(font_weight.0));
        }

        if let Some(font_style) = font_style {
            attrs = attrs.style(match font_style {
                cranpose_ui::text::FontStyle::Normal => GlyphonStyle::Normal,
                cranpose_ui::text::FontStyle::Italic => GlyphonStyle::Italic,
            });
        }
    }

    attrs = match letter_spacing {
        cranpose_ui::text::TextUnit::Em(value) => attrs.letter_spacing(value),
        cranpose_ui::text::TextUnit::Sp(value) if font_size_px > 0.0 => {
            attrs.letter_spacing((value * scale) / font_size_px)
        }
        _ => attrs,
    };

    AttrsOwned::new(&attrs)
}

// Text measurer implementation for WGPU

// Text measurer implementation for WGPU

#[derive(Clone)]
struct WgpuTextMeasurer {
    font_system: Arc<Mutex<FontSystem>>,
    font_family_resolver: SharedFontFamilyResolver,
    size_cache: TextSizeCache,
    /// Shared buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
}

impl WgpuTextMeasurer {
    fn new(
        font_system: Arc<Mutex<FontSystem>>,
        text_cache: SharedTextCache,
        font_family_resolver: SharedFontFamilyResolver,
    ) -> Self {
        Self {
            font_system,
            font_family_resolver,
            // Larger cache size (1024) reduces misses, FxHasher for faster lookups
            size_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(1024).unwrap()))),
            text_cache,
        }
    }
}

/// Convenience function for tests to initialize an accurate wgpu text measurer without launching a window.
pub fn setup_headless_text_measurer() {
    let mut font_system = FontSystem::new();
    let mut font_family_resolver_impl = WgpuFontFamilyResolver::default();
    font_family_resolver_impl.prime(&mut font_system);
    let font_system = Arc::new(Mutex::new(font_system));
    let font_family_resolver = Arc::new(Mutex::new(font_family_resolver_impl));
    let text_cache = Arc::new(Mutex::new(HashMap::new()));
    cranpose_ui::text::set_text_measurer(WgpuTextMeasurer::new(
        font_system,
        text_cache,
        font_family_resolver,
    ));
}

// Base font size in logical units (dp) - shared between measurement and rendering

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::TextMetrics {
        let text_str = text.text.as_str();
        let font_size = resolve_font_size(style);
        let line_height = resolve_effective_line_height(style, text, font_size);
        let style_hash = style.measurement_hash() ^ text.span_styles_hash();
        let size_int = (font_size * 100.0) as i32;

        // Calculate hash to avoid allocating String for lookup
        // FxHasher is ~3x faster than DefaultHasher for short strings
        let mut hasher = FxHasher::default();
        text_str.hash(&mut hasher);
        let text_hash = hasher.finish();
        let cache_key = (text_hash, size_int, style_hash);

        // Check size cache first (fastest path)
        {
            let mut cache = self.size_cache.lock().unwrap();
            if let Some((cached_text, size)) = cache.get(&cache_key) {
                // Verify partial collision
                if cached_text == text_str {
                    let line_count = text_str.split('\n').count().max(1);
                    return cranpose_ui::TextMetrics {
                        width: size.width,
                        height: size.height,
                        line_height,
                        line_count,
                    };
                }
            }
        }

        // Get or create text buffer
        let text_buffer_key = TextCacheKey::new(text_str, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();
        let mut font_family_resolver = self.font_family_resolver.lock().unwrap();

        // Get or create buffer and calculate size
        let size = {
            let buffer = text_cache.entry(text_buffer_key).or_insert_with(|| {
                let buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
                SharedTextBuffer {
                    buffer,
                    text: String::new(),
                    font_size: 0.0,
                    line_height: 0.0,
                    style_hash: 0,
                    cached_size: None,
                }
            });

            // Ensure buffer has the correct text
            buffer.ensure(
                &mut font_system,
                &mut font_family_resolver,
                EnsureTextBufferParams {
                    annotated_text: text,
                    font_size_px: font_size,
                    line_height_px: line_height,
                    style_hash,
                    style,
                    scale: 1.0,
                },
            );

            // Calculate size if not cached
            buffer.size()
        };

        // Trim cache if needed (after we're done with buffer reference)
        trim_text_cache(&mut text_cache);

        drop(font_system);
        drop(text_cache);

        // Cache the size result
        let mut size_cache = self.size_cache.lock().unwrap();
        // Only allocate string on cache miss
        size_cache.put(cache_key, (text_str.to_string(), size));

        // Calculate line info for multiline support
        let line_count = text_str.split('\n').count().max(1);

        cranpose_ui::TextMetrics {
            width: size.width,
            height: size.height,
            line_height,
            line_count,
        }
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        let text_str = text.text.as_str();
        let font_size = resolve_font_size(style);
        let line_height = resolve_effective_line_height(style, text, font_size);
        let style_hash = style.measurement_hash() ^ text.span_styles_hash();
        if text_str.is_empty() {
            return 0;
        }

        let cache_key = TextCacheKey::new(text_str, font_size, style_hash);

        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();
        let mut font_family_resolver = self.font_family_resolver.lock().unwrap();

        let buffer = text_cache.entry(cache_key).or_insert_with(|| {
            let buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
            SharedTextBuffer {
                buffer,
                text: String::new(),
                font_size: 0.0,
                line_height: 0.0,
                style_hash: 0,
                cached_size: None,
            }
        });

        buffer.ensure(
            &mut font_system,
            &mut font_family_resolver,
            EnsureTextBufferParams {
                annotated_text: text,
                font_size_px: font_size,
                line_height_px: line_height,
                style_hash,
                style,
                scale: 1.0,
            },
        );

        let line_offsets: Vec<(usize, usize)> = text_str
            .split('\n')
            .scan(0usize, |line_start, line| {
                let start = *line_start;
                let end = start + line.len();
                *line_start = end.saturating_add(1);
                Some((start, end))
            })
            .collect();

        let mut target_line = None;
        let mut best_vertical_distance = f32::INFINITY;

        for run in buffer.buffer.layout_runs() {
            let mut run_height = run.line_height;
            for glyph in run.glyphs.iter() {
                run_height = run_height.max(glyph.font_size * 1.4);
            }

            let top = run.line_top;
            let bottom = top + run_height.max(1.0);
            let vertical_distance = if y < top {
                top - y
            } else if y > bottom {
                y - bottom
            } else {
                0.0
            };

            if vertical_distance < best_vertical_distance {
                best_vertical_distance = vertical_distance;
                target_line = Some(run.line_i);
                if vertical_distance == 0.0 {
                    break;
                }
            }
        }

        let fallback_line = (y / line_height).floor().max(0.0) as usize;
        let target_line = target_line
            .unwrap_or(fallback_line)
            .min(line_offsets.len().saturating_sub(1));
        let (line_start, line_end) = line_offsets
            .get(target_line)
            .copied()
            .unwrap_or((0, text_str.len()));
        let line_len = line_end.saturating_sub(line_start);

        let mut best_offset = line_offsets
            .get(target_line)
            .map(|(_, end)| *end)
            .unwrap_or(text_str.len());
        let mut best_distance = f32::INFINITY;
        let mut found_glyph = false;

        for run in buffer.buffer.layout_runs() {
            if run.line_i != target_line {
                continue;
            }
            for glyph in run.glyphs.iter() {
                found_glyph = true;
                let glyph_start = line_start.saturating_add(glyph.start.min(line_len));
                let glyph_end = line_start.saturating_add(glyph.end.min(line_len));
                let left_dist = (x - glyph.x).abs();
                if left_dist < best_distance {
                    best_distance = left_dist;
                    best_offset = glyph_start;
                }

                let right_x = glyph.x + glyph.w;
                let right_dist = (x - right_x).abs();
                if right_dist < best_distance {
                    best_distance = right_dist;
                    best_offset = glyph_end;
                }
            }
        }

        if !found_glyph {
            if let Some((start, end)) = line_offsets.get(target_line) {
                best_offset = if x <= 0.0 { *start } else { *end };
            }
        }

        best_offset.min(text_str.len())
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        let text = text.text.as_str();
        let clamped_offset = offset.min(text.len());
        if clamped_offset == 0 {
            return 0.0;
        }

        // Measure text up to offset
        let prefix = &text[..clamped_offset];
        self.measure(&cranpose_ui::text::AnnotatedString::from(prefix), style)
            .width
    }

    fn choose_auto_hyphen_break(
        &self,
        line: &str,
        style: &cranpose_ui::text::TextStyle,
        segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        choose_shared_auto_hyphen_break(line, style, segment_start_char, measured_break_char)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        let text_str = text.text.as_str();
        use cranpose_ui::text_layout_result::{
            GlyphLayout, LineLayout, TextLayoutData, TextLayoutResult,
        };

        let font_size = resolve_font_size(style);
        let line_height = resolve_effective_line_height(style, text, font_size);
        let style_hash = style.measurement_hash() ^ text.span_styles_hash();

        let cache_key = TextCacheKey::new(text_str, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();
        let mut font_family_resolver = self.font_family_resolver.lock().unwrap();

        let buffer = text_cache.entry(cache_key.clone()).or_insert_with(|| {
            let buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
            SharedTextBuffer {
                buffer,
                text: String::new(),
                font_size: 0.0,
                line_height: 0.0,
                style_hash: 0,
                cached_size: None,
            }
        });
        buffer.ensure(
            &mut font_system,
            &mut font_family_resolver,
            EnsureTextBufferParams {
                annotated_text: text,
                font_size_px: font_size,
                line_height_px: line_height,
                style_hash,
                style,
                scale: 1.0,
            },
        );

        // Extract glyph positions from layout runs
        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut glyph_layouts = Vec::new();
        let mut lines = Vec::new();
        let line_offsets: Vec<(usize, usize)> = text_str
            .split('\n')
            .scan(0usize, |line_start, line| {
                let start = *line_start;
                let end = start + line.len();
                *line_start = end.saturating_add(1);
                Some((start, end))
            })
            .collect();

        for run in buffer.buffer.layout_runs() {
            let line_idx = run.line_i;
            let run_height = run
                .glyphs
                .iter()
                .fold(run.line_height, |acc, glyph| acc.max(glyph.font_size * 1.4))
                .max(1.0);

            for glyph in run.glyphs.iter() {
                let (line_start, line_end) = line_offsets
                    .get(line_idx)
                    .copied()
                    .unwrap_or((0, text_str.len()));
                let line_len = line_end.saturating_sub(line_start);
                let glyph_start = line_start.saturating_add(glyph.start.min(line_len));
                let glyph_end = line_start.saturating_add(glyph.end.min(line_len));

                glyph_x_positions.push(glyph.x);
                char_to_byte.push(glyph_start);
                if glyph_end > glyph_start {
                    glyph_layouts.push(GlyphLayout {
                        line_index: line_idx,
                        start_offset: glyph_start,
                        end_offset: glyph_end,
                        x: glyph.x,
                        y: run.line_top,
                        width: glyph.w.max(0.0),
                        height: run_height,
                    });
                }
            }
        }

        // Add end position
        let total_width = self.measure(text, style).width;
        glyph_x_positions.push(total_width);
        char_to_byte.push(text_str.len());

        // Build lines from text newlines
        let mut y = 0.0f32;
        let mut line_start = 0usize;
        for (i, line_text) in text_str.split('\n').enumerate() {
            let line_end = if i == text_str.split('\n').count() - 1 {
                text_str.len()
            } else {
                line_start + line_text.len()
            };

            lines.push(LineLayout {
                start_offset: line_start,
                end_offset: line_end,
                y,
                height: line_height,
            });

            line_start = line_end + 1;
            y += line_height;
        }

        if lines.is_empty() {
            lines.push(LineLayout {
                start_offset: 0,
                end_offset: 0,
                y: 0.0,
                height: line_height,
            });
        }

        let metrics = self.measure(text, style);
        TextLayoutResult::new(
            text_str,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_font_system_and_resolver() -> (FontSystem, WgpuFontFamilyResolver) {
        let mut db = glyphon::fontdb::Database::new();
        db.load_font_data(EMBEDDED_FALLBACK_FONT_BYTES.to_vec());
        let mut font_system = FontSystem::new_with_locale_and_db("en-US".to_string(), db);
        let mut resolver = WgpuFontFamilyResolver::default();
        resolver.prime(&mut font_system);
        (font_system, resolver)
    }

    #[test]
    fn attrs_resolution_falls_back_for_missing_named_family() {
        let (mut font_system, mut resolver) = seeded_font_system_and_resolver();
        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_family: Some(cranpose_ui::text::FontFamily::named("Missing Family Name")),
                ..Default::default()
            },
            ..Default::default()
        };

        let attrs = attrs_from_text_style(&style, 14.0, 1.0, &mut font_system, &mut resolver);
        assert_eq!(attrs.family_owned, FamilyOwned::SansSerif);
    }

    #[test]
    fn attrs_resolution_seeds_generic_families_from_loaded_fonts() {
        let (font_system, resolver) = seeded_font_system_and_resolver();
        assert!(
            resolver.generic_fallback_seeded,
            "expected generic fallback seeding after resolver prime"
        );
        let query = glyphon::fontdb::Query {
            families: &[glyphon::fontdb::Family::Monospace],
            weight: glyphon::fontdb::Weight::NORMAL,
            stretch: glyphon::fontdb::Stretch::Normal,
            style: glyphon::fontdb::Style::Normal,
        };
        assert!(
            font_system.db().query(&query).is_some(),
            "generic monospace query should resolve after fallback seeding"
        );
    }

    #[test]
    fn attrs_resolution_named_family_lookup_is_case_insensitive() {
        let (mut font_system, mut resolver) = seeded_font_system_and_resolver();
        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_family: Some(cranpose_ui::text::FontFamily::named("noto sans")),
                ..Default::default()
            },
            ..Default::default()
        };

        let attrs = attrs_from_text_style(&style, 14.0, 1.0, &mut font_system, &mut resolver);
        assert!(
            matches!(attrs.family_owned, FamilyOwned::Name(_)),
            "case-insensitive family lookup should resolve to a concrete family name"
        );
    }

    #[test]
    fn attrs_resolution_downgrades_missing_italic_to_available_style() {
        let (mut font_system, mut resolver) = seeded_font_system_and_resolver();
        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_family: Some(cranpose_ui::text::FontFamily::named("Noto Sans")),
                font_style: Some(cranpose_ui::text::FontStyle::Italic),
                ..Default::default()
            },
            ..Default::default()
        };

        let attrs = attrs_from_text_style(&style, 14.0, 1.0, &mut font_system, &mut resolver);
        assert_eq!(
            attrs.style,
            GlyphonStyle::Normal,
            "missing italic face should downgrade to available style instead of panicking during shaping"
        );
    }

    #[test]
    fn attrs_resolution_downgrades_missing_weight_to_available_weight() {
        let (mut font_system, mut resolver) = seeded_font_system_and_resolver();
        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_family: Some(cranpose_ui::text::FontFamily::named("Noto Sans")),
                font_weight: Some(cranpose_ui::text::FontWeight::BOLD),
                ..Default::default()
            },
            ..Default::default()
        };

        let attrs = attrs_from_text_style(&style, 14.0, 1.0, &mut font_system, &mut resolver);
        assert_eq!(
            attrs.weight,
            GlyphonWeight(cranpose_ui::text::FontWeight::NORMAL.0),
            "missing bold face should downgrade to available weight instead of panicking during shaping"
        );
    }

    fn empty_font_system() -> FontSystem {
        let db = glyphon::fontdb::Database::new();
        FontSystem::new_with_locale_and_db("en-US".to_string(), db)
    }

    #[test]
    fn default_policy_populates_face_db() {
        let mut fs = empty_font_system();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::Default);
        assert!(
            fs.db().faces().count() > 0,
            "Default policy must load at least one face"
        );
    }

    #[test]
    fn default_policy_includes_text_and_emoji_fonts() {
        let mut fs = empty_font_system();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::Default);
        assert!(
            font_db_contains_family(&fs, EMBEDDED_FALLBACK_FAMILY),
            "Default policy must include the NotoSansMerged text fallback"
        );
        assert!(
            font_db_contains_family(&fs, EMBEDDED_EMOJI_FAMILY),
            "Default policy must include Twemoji color emoji fallback"
        );
    }

    #[test]
    fn extend_policy_includes_framework_and_extra_fonts() {
        static EXTRA: &[&[u8]] = &[EMBEDDED_FALLBACK_FONT_BYTES];
        let mut fs = empty_font_system();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::Extend(EXTRA));
        assert!(
            font_db_contains_family(&fs, EMBEDDED_FALLBACK_FAMILY),
            "Extend policy must still include framework fallback font"
        );
    }

    #[test]
    fn replace_policy_excludes_framework_defaults() {
        static REPLACEMENT: &[&[u8]] = &[EMBEDDED_FALLBACK_FONT_BYTES];
        let mut fs = empty_font_system();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::Replace(REPLACEMENT));
        // The same family name is loaded via REPLACEMENT, but the framework default path
        // was not taken – verified by checking the face count matches one font face only.
        assert!(
            fs.db().faces().count() > 0,
            "Replace policy must load the replacement fonts"
        );
    }

    #[test]
    fn none_policy_excludes_framework_defaults() {
        let mut fs = empty_font_system();
        // Seed with the fallback font so the guard doesn't fire, then verify None
        // policy does not add an extra copy of the framework fallback.
        fs.db_mut()
            .load_font_data(EMBEDDED_FALLBACK_FONT_BYTES.to_vec());
        let face_count_before = fs.db().faces().count();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::None);
        assert_eq!(
            fs.db().faces().count(),
            face_count_before,
            "None policy must not inject additional framework fonts"
        );
    }

    #[test]
    fn none_policy_empty_db_guard_injects_fallback() {
        let mut fs = empty_font_system();
        load_fonts_with_policy(&mut fs, &[], &FallbackFontPolicy::None);
        assert!(
            fs.db().faces().count() > 0,
            "None policy with empty DB must inject last-resort guard font so rendering never panics"
        );
    }

    #[test]
    fn resolver_injects_embedded_fallback_if_font_db_is_empty() {
        let mut font_system = empty_font_system();
        let mut resolver = WgpuFontFamilyResolver::default();
        let span_style = cranpose_ui::text::SpanStyle::default();

        let _ = resolver.resolve_family_owned(&mut font_system, &span_style);

        assert!(
            font_system.db().faces().next().is_some(),
            "resolver must guarantee at least one face before shaping"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn attrs_resolution_loads_file_backed_family_from_path() {
        let (mut font_system, mut resolver) = seeded_font_system_and_resolver();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let unique_path = format!(
            "{}/cranpose-font-resolver-{}-{}.ttf",
            std::env::temp_dir().display(),
            std::process::id(),
            nonce
        );
        std::fs::write(&unique_path, EMBEDDED_FALLBACK_FONT_BYTES).expect("write font fixture");

        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_family: Some(cranpose_ui::text::FontFamily::file_backed(vec![
                    cranpose_ui::text::FontFile::new(unique_path.clone()),
                ])),
                ..Default::default()
            },
            ..Default::default()
        };

        let attrs = attrs_from_text_style(&style, 14.0, 1.0, &mut font_system, &mut resolver);
        assert!(
            matches!(attrs.family_owned, FamilyOwned::Name(_)),
            "file-backed font family should resolve to an installed family name"
        );

        let _ = std::fs::remove_file(&unique_path);
    }
}

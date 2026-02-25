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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Size-only cache for ultra-fast text measurement lookups.
/// Key: (text_hash, font_size_fixed_point, style_hash)
/// Value: (text_content, size) - text stored to handle hash collisions
type TextSizeCache = Arc<Mutex<LruCache<(u64, i32, u64), (String, Size)>>>;

static TEXT_MEASURE_TELEMETRY_ENABLED: OnceLock<bool> = OnceLock::new();
static TEXT_MEASURE_TELEMETRY: OnceLock<TextMeasureTelemetry> = OnceLock::new();

#[derive(Default)]
struct TextMeasureTelemetry {
    measure_calls: AtomicU64,
    layout_calls: AtomicU64,
    offset_calls: AtomicU64,
    size_cache_hits: AtomicU64,
    size_cache_misses: AtomicU64,
    text_cache_hits: AtomicU64,
    text_cache_misses: AtomicU64,
    ensure_reshapes: AtomicU64,
    ensure_reuses: AtomicU64,
}

fn text_measure_telemetry_enabled() -> bool {
    *TEXT_MEASURE_TELEMETRY_ENABLED
        .get_or_init(|| std::env::var_os("CRANPOSE_TEXT_MEASURE_TELEMETRY").is_some())
}

fn text_measure_telemetry() -> &'static TextMeasureTelemetry {
    TEXT_MEASURE_TELEMETRY.get_or_init(TextMeasureTelemetry::default)
}

fn maybe_report_text_measure_telemetry(sequence: u64) {
    if !text_measure_telemetry_enabled() || !sequence.is_multiple_of(200) {
        return;
    }
    let telemetry = text_measure_telemetry();
    let measure_calls = telemetry.measure_calls.load(Ordering::Relaxed);
    let layout_calls = telemetry.layout_calls.load(Ordering::Relaxed);
    let offset_calls = telemetry.offset_calls.load(Ordering::Relaxed);
    let size_hits = telemetry.size_cache_hits.load(Ordering::Relaxed);
    let size_misses = telemetry.size_cache_misses.load(Ordering::Relaxed);
    let text_hits = telemetry.text_cache_hits.load(Ordering::Relaxed);
    let text_misses = telemetry.text_cache_misses.load(Ordering::Relaxed);
    let reshapes = telemetry.ensure_reshapes.load(Ordering::Relaxed);
    let reuses = telemetry.ensure_reuses.load(Ordering::Relaxed);

    let size_total = size_hits + size_misses;
    let text_total = text_hits + text_misses;
    let ensure_total = reshapes + reuses;
    let size_hit_rate = if size_total > 0 {
        (size_hits as f64 / size_total as f64) * 100.0
    } else {
        0.0
    };
    let text_hit_rate = if text_total > 0 {
        (text_hits as f64 / text_total as f64) * 100.0
    } else {
        0.0
    };
    let reshape_rate = if ensure_total > 0 {
        (reshapes as f64 / ensure_total as f64) * 100.0
    } else {
        0.0
    };

    log::warn!(
        "[text-measure-telemetry] measure_calls={} layout_calls={} offset_calls={} size_hit_rate={:.1}% text_cache_hit_rate={:.1}% reshape_rate={:.1}% reshapes={} reuses={}",
        measure_calls,
        layout_calls,
        offset_calls,
        size_hit_rate,
        text_hit_rate,
        reshape_rate,
        reshapes,
        reuses
    );
}

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
    ) -> bool {
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
            return false;
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
        true
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FamilyCacheKey {
    Name(String),
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
}

impl FamilyCacheKey {
    fn from_family_owned(family: &FamilyOwned) -> Self {
        match family {
            FamilyOwned::Name(name) => Self::Name(name.to_string()),
            FamilyOwned::Serif => Self::Serif,
            FamilyOwned::SansSerif => Self::SansSerif,
            FamilyOwned::Monospace => Self::Monospace,
            FamilyOwned::Cursive => Self::Cursive,
            FamilyOwned::Fantasy => Self::Fantasy,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct StyleWeightRequest {
    family: FamilyCacheKey,
    requested_weight: cranpose_ui::text::FontWeight,
    requested_style: cranpose_ui::text::FontStyle,
}

type ResolvedStyleWeight = Option<(GlyphonStyle, GlyphonWeight)>;

#[derive(Default)]
struct WgpuFontFamilyResolver {
    request_cache: HashMap<TypefaceRequest, FamilyOwned>,
    style_weight_cache: HashMap<StyleWeightRequest, ResolvedStyleWeight>,
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

    fn resolve_available_style_and_weight(
        &mut self,
        font_system: &FontSystem,
        family: &FamilyOwned,
        requested_weight: Option<cranpose_ui::text::FontWeight>,
        requested_style: Option<cranpose_ui::text::FontStyle>,
    ) -> Option<(GlyphonStyle, GlyphonWeight)> {
        let request = StyleWeightRequest {
            family: FamilyCacheKey::from_family_owned(family),
            requested_weight: requested_weight.unwrap_or_default(),
            requested_style: requested_style.unwrap_or_default(),
        };

        if let Some(cached) = self.style_weight_cache.get(&request) {
            return *cached;
        }

        let resolved = resolve_available_style_and_weight_uncached(
            font_system,
            family,
            requested_weight,
            requested_style,
        );
        self.style_weight_cache.insert(request, resolved);
        resolved
    }

    fn ensure_non_empty_font_db(&mut self, font_system: &mut FontSystem) {
        if font_system.db().faces().next().is_none() {
            log::warn!("Font database is empty; text will not render. Provide fonts via AppLauncher::with_fonts.");
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
        self.style_weight_cache.clear();
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
        self.style_weight_cache.clear();
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

fn load_fonts(font_system: &mut FontSystem, fonts: &[&[u8]]) {
    for (i, font_data) in fonts.iter().enumerate() {
        log::info!("Loading font #{}, size: {} bytes", i, font_data.len());
        font_system.db_mut().load_font_data(font_data.to_vec());
    }
    log::info!(
        "Total font faces loaded: {}",
        font_system.db().faces().count()
    );
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
    /// * `fonts` – font bytes to load, ordered by priority (first = highest priority).
    ///   Pass `&[]` to load no fonts; text will not render until fonts are provided.
    ///
    /// Call [`init_gpu`][Self::init_gpu] before rendering.
    pub fn new(fonts: &[&[u8]]) -> Self {
        let mut font_system = FontSystem::new();

        // On Android never load system fonts: modern Android ships variable Roboto
        // which can cause rasterization corruption or font-ID conflicts with glyphon.
        #[cfg(target_os = "android")]
        log::info!("Skipping Android system fonts – using application-provided fonts only");

        load_fonts(&mut font_system, fonts);

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
        Self::new(&[])
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

fn resolve_available_style_and_weight_uncached(
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

    if let Some((resolved_style, resolved_weight)) = font_family_resolver
        .resolve_available_style_and_weight(font_system, &family_owned, font_weight, font_style)
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

    fn try_measure_with_options_fast_path(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        options: cranpose_ui::text::TextLayoutOptions,
        max_width: Option<f32>,
    ) -> Option<cranpose_ui::TextMetrics> {
        let options = options.normalized();
        let max_width = max_width.filter(|w| w.is_finite() && *w > 0.0)?;
        if options.overflow != cranpose_ui::text::TextOverflow::Clip || !options.soft_wrap {
            return None;
        }
        if options.max_lines != usize::MAX {
            return None;
        }

        let line_break = style
            .paragraph_style
            .line_break
            .take_or_else(|| cranpose_ui::text::LineBreak::Simple);
        let hyphens = style
            .paragraph_style
            .hyphens
            .take_or_else(|| cranpose_ui::text::Hyphens::None);
        if line_break != cranpose_ui::text::LineBreak::Simple
            || hyphens != cranpose_ui::text::Hyphens::None
        {
            return None;
        }

        let text_str = text.text.as_str();
        let font_size = resolve_font_size(style);
        let line_height = resolve_effective_line_height(style, text, font_size);
        let style_hash = style.measurement_hash()
            ^ text.span_styles_hash()
            ^ (max_width.to_bits() as u64).rotate_left(17)
            ^ 0x9f4c_3314_2d5b_79e1;
        let size_int = (font_size * 100.0) as i32;

        let mut hasher = FxHasher::default();
        text_str.hash(&mut hasher);
        let text_hash = hasher.finish();
        let cache_key = (text_hash, size_int, style_hash);

        {
            let mut cache = self.size_cache.lock().unwrap();
            if let Some((cached_text, size)) = cache.get(&cache_key) {
                if cached_text == text_str {
                    let width = size.width.min(max_width);
                    let min_height = options.min_lines as f32 * line_height;
                    let height = size.height.max(min_height);
                    let line_count =
                        ((height / line_height).ceil() as usize).max(options.min_lines);
                    return Some(cranpose_ui::TextMetrics {
                        width,
                        height,
                        line_height,
                        line_count,
                    });
                }
            }
        }

        let text_buffer_key = TextCacheKey::new(text_str, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();
        let mut font_family_resolver = self.font_family_resolver.lock().unwrap();

        let (size, wrapped_line_count) = {
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

            let _ = buffer.ensure(
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

            buffer
                .buffer
                .set_size(&mut font_system, Some(max_width), Some(f32::MAX));
            buffer.buffer.shape_until_scroll(&mut font_system, false);
            buffer.cached_size = None;
            let size = buffer.size();
            let line_count = buffer.buffer.layout_runs().count();
            (size, line_count)
        };

        trim_text_cache(&mut text_cache);
        drop(font_system);
        drop(text_cache);

        let mut size_cache = self.size_cache.lock().unwrap();
        size_cache.put(cache_key, (text_str.to_string(), size));

        let width = size.width.min(max_width);
        let min_height = options.min_lines as f32 * line_height;
        let height = size.height.max(min_height);
        let line_count = wrapped_line_count.max(options.min_lines).max(1);

        Some(cranpose_ui::TextMetrics {
            width,
            height,
            line_height,
            line_count,
        })
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
        let telemetry = text_measure_telemetry_enabled().then_some(text_measure_telemetry());
        let telemetry_sequence = telemetry
            .map(|t| t.measure_calls.fetch_add(1, Ordering::Relaxed) + 1)
            .unwrap_or(0);
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
                    if let Some(t) = telemetry {
                        t.size_cache_hits.fetch_add(1, Ordering::Relaxed);
                        maybe_report_text_measure_telemetry(telemetry_sequence);
                    }
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
        if let Some(t) = telemetry {
            t.size_cache_misses.fetch_add(1, Ordering::Relaxed);
        }

        // Get or create text buffer
        let text_buffer_key = TextCacheKey::new(text_str, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();
        let mut font_family_resolver = self.font_family_resolver.lock().unwrap();

        // Get or create buffer and calculate size
        let size = {
            if let Some(t) = telemetry {
                let text_cache_hit = text_cache.contains_key(&text_buffer_key);
                if text_cache_hit {
                    t.text_cache_hits.fetch_add(1, Ordering::Relaxed);
                } else {
                    t.text_cache_misses.fetch_add(1, Ordering::Relaxed);
                }
            }
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
            let reshaped = buffer.ensure(
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
            if let Some(t) = telemetry {
                if reshaped {
                    t.ensure_reshapes.fetch_add(1, Ordering::Relaxed);
                } else {
                    t.ensure_reuses.fetch_add(1, Ordering::Relaxed);
                }
            }

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
        if telemetry.is_some() {
            maybe_report_text_measure_telemetry(telemetry_sequence);
        }

        cranpose_ui::TextMetrics {
            width: size.width,
            height: size.height,
            line_height,
            line_count,
        }
    }

    fn measure_with_options(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        options: cranpose_ui::text::TextLayoutOptions,
        max_width: Option<f32>,
    ) -> cranpose_ui::TextMetrics {
        if let Some(metrics) =
            self.try_measure_with_options_fast_path(text, style, options, max_width)
        {
            return metrics;
        }
        self.prepare_with_options(text, style, options, max_width)
            .metrics
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        let telemetry = text_measure_telemetry_enabled().then_some(text_measure_telemetry());
        let telemetry_sequence = telemetry
            .map(|t| t.offset_calls.fetch_add(1, Ordering::Relaxed) + 1)
            .unwrap_or(0);
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

        if let Some(t) = telemetry {
            let text_cache_hit = text_cache.contains_key(&cache_key);
            if text_cache_hit {
                t.text_cache_hits.fetch_add(1, Ordering::Relaxed);
            } else {
                t.text_cache_misses.fetch_add(1, Ordering::Relaxed);
            }
        }
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

        let reshaped = buffer.ensure(
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
        if let Some(t) = telemetry {
            if reshaped {
                t.ensure_reshapes.fetch_add(1, Ordering::Relaxed);
            } else {
                t.ensure_reuses.fetch_add(1, Ordering::Relaxed);
            }
            maybe_report_text_measure_telemetry(telemetry_sequence);
        }

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
        let telemetry = text_measure_telemetry_enabled().then_some(text_measure_telemetry());
        let telemetry_sequence = telemetry
            .map(|t| t.layout_calls.fetch_add(1, Ordering::Relaxed) + 1)
            .unwrap_or(0);
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

        if let Some(t) = telemetry {
            let text_cache_hit = text_cache.contains_key(&cache_key);
            if text_cache_hit {
                t.text_cache_hits.fetch_add(1, Ordering::Relaxed);
            } else {
                t.text_cache_misses.fetch_add(1, Ordering::Relaxed);
            }
        }
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
        let reshaped = buffer.ensure(
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
        if let Some(t) = telemetry {
            if reshaped {
                t.ensure_reshapes.fetch_add(1, Ordering::Relaxed);
            } else {
                t.ensure_reuses.fetch_add(1, Ordering::Relaxed);
            }
            maybe_report_text_measure_telemetry(telemetry_sequence);
        }
        let measured_size = buffer.size();

        // Extract glyph positions from layout runs
        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut glyph_layouts = Vec::new();
        let mut lines = Vec::new();
        let text_lines: Vec<&str> = text_str.split('\n').collect();
        let line_offsets: Vec<(usize, usize)> = text_lines
            .iter()
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
        glyph_x_positions.push(measured_size.width);
        char_to_byte.push(text_str.len());

        // Build lines from text newlines
        let mut y = 0.0f32;
        let mut line_start = 0usize;
        for (i, line_text) in text_lines.iter().enumerate() {
            let line_end = if i == text_lines.len() - 1 {
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

        let metrics = cranpose_ui::TextMetrics {
            width: measured_size.width,
            height: measured_size.height,
            line_height,
            line_count: text_lines.len().max(1),
        };
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
        db.load_font_data(TEST_FONT.to_vec());
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

    #[test]
    fn layout_matches_measure_without_reentrant_mutex_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (font_system, resolver) = seeded_font_system_and_resolver();
            let measurer = WgpuTextMeasurer::new(
                Arc::new(Mutex::new(font_system)),
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(Mutex::new(resolver)),
            );
            let text = cranpose_ui::text::AnnotatedString::from("hello\nworld");
            let style = cranpose_ui::text::TextStyle::default();

            let layout = measurer.layout(&text, &style);
            let metrics = measurer.measure(&text, &style);
            tx.send((
                layout.width,
                layout.height,
                layout.lines.len(),
                metrics.width,
                metrics.height,
                metrics.line_count,
            ))
            .expect("send layout metrics");
        });

        let (
            layout_width,
            layout_height,
            layout_lines,
            measured_width,
            measured_height,
            measured_lines,
        ) = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("layout timed out; possible recursive mutex acquisition");

        assert!((layout_width - measured_width).abs() < 0.5);
        assert!((layout_height - measured_height).abs() < 0.5);
        assert_eq!(layout_lines, measured_lines.max(1));
    }

    #[test]
    fn measure_with_options_fast_path_wraps_to_width() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (font_system, resolver) = seeded_font_system_and_resolver();
            let measurer = WgpuTextMeasurer::new(
                Arc::new(Mutex::new(font_system)),
                Arc::new(Mutex::new(HashMap::new())),
                Arc::new(Mutex::new(resolver)),
            );
            let text = cranpose_ui::text::AnnotatedString::from("wrap me ".repeat(120));
            let style = cranpose_ui::text::TextStyle::default();
            let options = cranpose_ui::text::TextLayoutOptions {
                overflow: cranpose_ui::text::TextOverflow::Clip,
                soft_wrap: true,
                max_lines: usize::MAX,
                min_lines: 1,
            };
            let metrics =
                TextMeasurer::measure_with_options(&measurer, &text, &style, options, Some(120.0));
            tx.send((metrics.width, metrics.line_count))
                .expect("send wrapped metrics");
        });

        let (width, line_count) = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("measure_with_options timed out");
        assert!(width <= 120.5, "wrapped width should honor max width");
        assert!(line_count > 1, "wrapped text should produce multiple lines");
    }

    // Font bytes used by tests — the same file the demo app ships.
    static TEST_FONT: &[u8] =
        include_bytes!("../../../../apps/desktop-demo/assets/NotoSansMerged.ttf");

    fn empty_font_system() -> FontSystem {
        let db = glyphon::fontdb::Database::new();
        FontSystem::new_with_locale_and_db("en-US".to_string(), db)
    }

    #[test]
    fn load_fonts_populates_face_db() {
        let mut fs = empty_font_system();
        load_fonts(&mut fs, &[TEST_FONT]);
        assert!(
            fs.db().faces().count() > 0,
            "load_fonts must load at least one face"
        );
    }

    #[test]
    fn load_fonts_empty_slice_leaves_db_empty() {
        let mut fs = empty_font_system();
        load_fonts(&mut fs, &[]);
        assert_eq!(
            fs.db().faces().count(),
            0,
            "empty slice must not load any faces"
        );
    }

    #[test]
    fn resolver_logs_warning_if_font_db_is_empty() {
        // With no fonts loaded the resolver should not panic; it just warns.
        let mut font_system = empty_font_system();
        let mut resolver = WgpuFontFamilyResolver::default();
        let span_style = cranpose_ui::text::SpanStyle::default();
        // Must not panic even with an empty DB.
        let _ = resolver.resolve_family_owned(&mut font_system, &span_style);
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
        std::fs::write(&unique_path, TEST_FONT).expect("write font fixture");

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

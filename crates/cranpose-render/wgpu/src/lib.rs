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
mod text_raster;

pub use scene::{BackdropLayer, ClickAction, DrawShape, HitRegion, ImageDraw, Scene, TextDraw};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_ui::{set_text_measurer, LayoutTree, TextMeasurer};
use cranpose_ui_graphics::Size;
use glyphon::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style as GlyphonStyle,
    Weight as GlyphonWeight,
};
use lru::LruCache;
use render::GpuRenderer;
use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use text_raster::configure_raster_fonts;

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

impl SharedTextBuffer {
    /// Ensure the buffer has the correct text and font_size, only reshaping if needed
    pub(crate) fn ensure(
        &mut self,
        font_system: &mut FontSystem,
        text: &str,
        font_size: f32,
        line_height: f32,
        style_hash: u64,
        attrs: Attrs,
    ) {
        let text_changed = self.text != text;
        let font_changed = (self.font_size - font_size).abs() > 0.1;
        let line_height_changed = (self.line_height - line_height).abs() > 0.1;
        let style_changed = self.style_hash != style_hash;

        // Only reshape if something actually changed
        if !text_changed && !font_changed && !line_height_changed && !style_changed {
            return; // Nothing changed, skip reshape
        }

        // Set metrics and size for unlimited layout
        let metrics = Metrics::new(font_size, line_height);
        self.buffer.set_metrics(font_system, metrics);
        self.buffer
            .set_size(font_system, Some(f32::MAX), Some(f32::MAX));

        // Set text and shape
        self.buffer
            .set_text(font_system, text, &attrs, Shaping::Advanced);
        self.buffer.shape_until_scroll(font_system, false);

        // Update cached values
        self.text.clear();
        self.text.push_str(text);
        self.font_size = font_size;
        self.line_height = line_height;
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
        let layout_runs = self.buffer.layout_runs();
        for run in layout_runs {
            max_width = max_width.max(run.line_w);
        }
        let total_height = self.buffer.lines.len() as f32 * self.line_height.max(0.0);

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
    /// Shared text buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
    /// Root scale factor for text rendering (use for density scaling)
    root_scale: f32,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer with the specified font data.
    ///
    /// This is the recommended constructor for applications.
    /// Call `init_gpu` before rendering.
    ///
    /// # Example
    ///
    /// ```text
    /// let font_light = include_bytes!("path/to/font-light.ttf");
    /// let font_regular = include_bytes!("path/to/font-regular.ttf");
    /// let renderer = WgpuRenderer::new_with_fonts(&[font_light, font_regular]);
    /// ```
    pub fn new_with_fonts(fonts: &[&[u8]]) -> Self {
        let mut font_system = FontSystem::new();
        configure_raster_fonts(fonts);

        // On Android, DO NOT load system fonts
        // Modern Android uses Variable Fonts for Roboto which can cause
        // rasterization corruption or font ID conflicts with glyphon.
        // Use only our bundled static Roboto fonts for consistent rendering.
        #[cfg(target_os = "android")]
        {
            log::info!("Skipping Android system fonts - using application-provided fonts");
            // font_system.db_mut().load_fonts_dir("/system/fonts");  // DISABLED
        }

        // Load application-provided fonts
        for (i, font_data) in fonts.iter().enumerate() {
            log::info!("Loading font #{}, size: {} bytes", i, font_data.len());
            font_system.db_mut().load_font_data(font_data.to_vec());
        }

        let face_count = font_system.db().faces().count();
        log::info!("Total font faces loaded: {}", face_count);

        if face_count == 0 {
            log::error!("No fonts loaded! Text rendering will fail!");
        }

        let font_system = Arc::new(Mutex::new(font_system));

        // Create shared text cache for both measurement and rendering
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer = WgpuTextMeasurer::new(font_system.clone(), text_cache.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
            text_cache,
            root_scale: 1.0,
        }
    }

    /// Create a new WGPU renderer without any fonts.
    ///
    /// **Warning:** This is for internal use only. Applications should use `new_with_fonts()`.
    /// Text rendering will fail without fonts.
    pub fn new() -> Self {
        let font_system = FontSystem::new();
        let font_system = Arc::new(Mutex::new(font_system));
        let text_cache = Arc::new(Mutex::new(HashMap::new()));

        let text_measurer = WgpuTextMeasurer::new(font_system.clone(), text_cache.clone());
        set_text_measurer(text_measurer.clone());

        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            font_system,
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
        Self::new()
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
            Rc::from(text),
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

fn glyphon_family_from_font_family(font_family: &cranpose_ui::text::FontFamily) -> Family<'_> {
    match font_family {
        cranpose_ui::text::FontFamily::Default | cranpose_ui::text::FontFamily::SansSerif => {
            Family::SansSerif
        }
        cranpose_ui::text::FontFamily::Serif => Family::Serif,
        cranpose_ui::text::FontFamily::Monospace => Family::Monospace,
        cranpose_ui::text::FontFamily::Cursive => Family::Cursive,
        cranpose_ui::text::FontFamily::Fantasy => Family::Fantasy,
        cranpose_ui::text::FontFamily::Named(name) => Family::Name(name.as_str()),
    }
}

fn attrs_from_text_style<'a>(style: &'a cranpose_ui::text::TextStyle, font_size: f32) -> Attrs<'a> {
    let mut attrs = Attrs::new();
    let span_style = &style.span_style;
    let font_family = span_style.font_family.as_ref();
    let font_weight = span_style.font_weight;
    let font_style = span_style.font_style;
    let letter_spacing = span_style.letter_spacing;

    if let Some(font_family) = font_family {
        attrs = attrs.family(glyphon_family_from_font_family(font_family));
    }

    if let Some(font_weight) = font_weight {
        attrs = attrs.weight(GlyphonWeight(font_weight.0));
    }

    if let Some(font_style) = font_style {
        attrs = attrs.style(match font_style {
            cranpose_ui::text::FontStyle::Normal => GlyphonStyle::Normal,
            cranpose_ui::text::FontStyle::Italic => GlyphonStyle::Italic,
        });
    }

    attrs = match letter_spacing {
        cranpose_ui::text::TextUnit::Em(value) => attrs.letter_spacing(value),
        cranpose_ui::text::TextUnit::Sp(value) if font_size > 0.0 => {
            attrs.letter_spacing(value / font_size)
        }
        _ => attrs,
    };

    attrs
}

// Text measurer implementation for WGPU

// Text measurer implementation for WGPU

#[derive(Clone)]
struct WgpuTextMeasurer {
    font_system: Arc<Mutex<FontSystem>>,
    size_cache: TextSizeCache,
    /// Shared buffer cache used by both measurement and rendering
    text_cache: SharedTextCache,
}

impl WgpuTextMeasurer {
    fn new(font_system: Arc<Mutex<FontSystem>>, text_cache: SharedTextCache) -> Self {
        Self {
            font_system,
            // Larger cache size (1024) reduces misses, FxHasher for faster lookups
            size_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(1024).unwrap()))),
            text_cache,
        }
    }
}

// Base font size in logical units (dp) - shared between measurement and rendering

impl TextMeasurer for WgpuTextMeasurer {
    fn measure(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::TextMetrics {
        let font_size = resolve_font_size(style);
        let line_height = resolve_line_height(style, font_size);
        let style_hash = style.measurement_hash();
        let size_int = (font_size * 100.0) as i32;

        // Calculate hash to avoid allocating String for lookup
        // FxHasher is ~3x faster than DefaultHasher for short strings
        let mut hasher = FxHasher::default();
        text.hash(&mut hasher);
        let text_hash = hasher.finish();
        let cache_key = (text_hash, size_int, style_hash);

        // Check size cache first (fastest path)
        {
            let mut cache = self.size_cache.lock().unwrap();
            if let Some((cached_text, size)) = cache.get(&cache_key) {
                // Verify partial collision
                if cached_text == text {
                    let line_count = text.split('\n').count().max(1);
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
        let text_buffer_key = TextCacheKey::new(text, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

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
                text,
                font_size,
                line_height,
                style_hash,
                attrs_from_text_style(style, font_size),
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
        size_cache.put(cache_key, (text.to_string(), size));

        // Calculate line info for multiline support
        let line_count = text.split('\n').count().max(1);

        cranpose_ui::TextMetrics {
            width: size.width,
            height: size.height,
            line_height,
            line_count,
        }
    }

    fn get_offset_for_position(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        let font_size = resolve_font_size(style);
        let line_height = resolve_line_height(style, font_size);
        let style_hash = style.measurement_hash();
        if text.is_empty() {
            return 0;
        }

        // Calculate which line was clicked based on Y coordinate
        let line_index = (y / line_height).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        // Calculate byte offset to start of target line
        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1; // +1 for newline
        }

        // Get the text of the target line for hit testing
        let line_text = lines.get(target_line).unwrap_or(&"");

        if line_text.is_empty() {
            return line_start_byte;
        }

        // Use glyphon's hit testing for the specific line
        let cache_key = TextCacheKey::new(line_text, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

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
            line_text,
            font_size,
            line_height,
            style_hash,
            attrs_from_text_style(style, font_size),
        );

        // Find closest glyph position using layout runs
        let mut best_offset = 0;
        let mut best_distance = f32::INFINITY;

        for run in buffer.buffer.layout_runs() {
            let mut glyph_x = 0.0f32;
            for glyph in run.glyphs.iter() {
                // Check distance to left edge of glyph
                let left_dist = (x - glyph_x).abs();
                if left_dist < best_distance {
                    best_distance = left_dist;
                    // glyph.start is byte index in line_text
                    best_offset = glyph.start;
                }

                // Update x position for next glyph
                glyph_x += glyph.w;

                // Check distance to right edge (after glyph)
                let right_dist = (x - glyph_x).abs();
                if right_dist < best_distance {
                    best_distance = right_dist;
                    best_offset = glyph.end;
                }
            }
        }

        // Return absolute byte offset (line start + offset within line)
        line_start_byte + best_offset.min(line_text.len())
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        let clamped_offset = offset.min(text.len());
        if clamped_offset == 0 {
            return 0.0;
        }

        // Measure text up to offset
        let prefix = &text[..clamped_offset];
        self.measure(prefix, style).width
    }

    fn layout(
        &self,
        text: &str,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        use cranpose_ui::text_layout_result::{LineLayout, TextLayoutResult};

        let font_size = resolve_font_size(style);
        let line_height = resolve_line_height(style, font_size);
        let style_hash = style.measurement_hash();

        // Get buffer to extract glyph positions
        let cache_key = TextCacheKey::new(text, font_size, style_hash);
        let mut font_system = self.font_system.lock().unwrap();
        let mut text_cache = self.text_cache.lock().unwrap();

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
            text,
            font_size,
            line_height,
            style_hash,
            attrs_from_text_style(style, font_size),
        );

        // Extract glyph positions from layout runs
        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut lines = Vec::new();

        let mut current_line_y = 0.0f32;
        let mut line_start_offset = 0usize;

        for run in buffer.buffer.layout_runs() {
            let line_idx = run.line_i;
            let line_y = line_idx as f32 * line_height;

            // Track line boundaries
            if lines.is_empty() || line_y != current_line_y {
                if !lines.is_empty() {
                    // Close previous line
                    if let Some(_last) = lines.last_mut() {
                        // end_offset will be updated when we see a newline or end
                    }
                }
                current_line_y = line_y;
            }

            for glyph in run.glyphs.iter() {
                glyph_x_positions.push(glyph.x);
                char_to_byte.push(glyph.start);

                // Track line end
                if glyph.end > line_start_offset {
                    line_start_offset = glyph.end;
                }
            }
        }

        // Add end position
        let total_width = self.measure(text, style).width;
        glyph_x_positions.push(total_width);
        char_to_byte.push(text.len());

        // Build lines from text newlines
        let mut y = 0.0f32;
        let mut line_start = 0usize;
        for (i, line_text) in text.split('\n').enumerate() {
            let line_end = if i == text.split('\n').count() - 1 {
                text.len()
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
            metrics.width,
            metrics.height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            text,
        )
    }
}

//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile Android.

#![deny(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
mod cost_tuner;
mod effect_renderer;
mod frame_graph;
pub(crate) mod gpu_stats;
mod layer_events;
mod layer_surface_cache;
mod normalized_scene;
mod offscreen;
mod pipeline;
mod render;
mod scene;
mod shader_cache;
mod shaders;
#[cfg(not(target_arch = "wasm32"))]
mod shape_replay;
#[cfg(not(target_arch = "wasm32"))]
mod worker_pool;
mod surface_executor;
mod surface_plan;
mod surface_requirements;
#[cfg(test)]
mod test_support;

pub use gpu_stats::FrameStatsSnapshot as RenderStatsSnapshot;
pub use scene::{ClickAction, HitRegion, Scene};
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use shape_replay::live_stats as shape_replay_live_stats;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    graph::RenderGraph,
    software_text_raster::{
        software_text_font_set_from_fonts_or_default, SoftwareTextFontSet, SoftwareTextMeasurer,
    },
    RenderScene, Renderer,
};
use cranpose_ui::{LayoutTree, TextMeasurer};
use cranpose_ui_graphics::{Rect, Size};
use render::GpuRenderer;
use std::rc::{Rc, Weak};
use std::sync::Arc;

/// Convert an axis-aligned rectangle to four corner positions (TL, TR, BL, BR).
pub(crate) fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
    [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
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

#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DebugCpuAllocationStats {
    pub scene_graph_node_count: usize,
    pub scene_graph_heap_bytes: usize,
    pub scene_hits_len: usize,
    pub scene_hits_cap: usize,
    pub scene_node_index_len: usize,
    pub scene_node_index_cap: usize,
    pub text_renderer_pool_len: usize,
    pub text_renderer_pool_cap: usize,
    pub swash_image_cache_len: usize,
    pub swash_image_cache_cap: usize,
    pub swash_outline_cache_len: usize,
    pub swash_outline_cache_cap: usize,
    pub image_texture_cache_len: usize,
    pub image_texture_cache_cap: usize,
    pub scratch_shape_data_cap: usize,
    pub scratch_gradients_cap: usize,
    pub scratch_image_vertices_cap: usize,
    pub scratch_image_indices_cap: usize,
    pub scratch_image_cmds_cap: usize,
    pub scratch_segment_items_cap: usize,
    pub scratch_effect_ranges_cap: usize,
    pub scratch_layer_events_cap: usize,
    pub staged_upload_bytes_cap: usize,
    pub staged_upload_copies_cap: usize,
    pub layer_surface_cache_len: usize,
    pub layer_surface_cache_cap: usize,
    pub layer_surface_cache_identity_len: usize,
    pub layer_surface_cache_identity_cap: usize,
    pub layer_surface_rect_cache_len: usize,
    pub layer_surface_rect_cache_cap: usize,
    pub layer_surface_requirements_cache_len: usize,
    pub layer_surface_requirements_cache_cap: usize,
    pub layer_cache_seen_this_frame_len: usize,
    pub layer_cache_seen_this_frame_cap: usize,
}

pub(crate) struct TextSystemState {
    measurer: SoftwareTextMeasurer,
}

impl TextSystemState {
    fn from_font_set(fonts: SoftwareTextFontSet) -> Self {
        Self {
            measurer: SoftwareTextMeasurer::from_font_set(fonts, 8192),
        }
    }

    pub(crate) fn text_cache_len(&self) -> usize {
        0
    }
}

impl pipeline::TextLayoutResolver for TextSystemState {
    fn layout_text(
        &mut self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        if cranpose_ui::has_current_app_context() {
            cranpose_ui::text::layout_text(text, style)
        } else {
            self.measurer.layout(text, style)
        }
    }
}

#[derive(Clone)]
pub struct WgpuTextSystem {
    software_fonts: SoftwareTextFontSet,
}

impl WgpuTextSystem {
    pub fn from_fonts(fonts: &[&[u8]]) -> Self {
        Self {
            software_fonts: software_text_font_set_from_fonts_or_default(fonts),
        }
    }

    /// Adopt a font set an app already built — the path app-supplied families
    /// take, where faces were parsed once at startup rather than from static
    /// byte slices here.
    pub fn from_font_set(software_fonts: SoftwareTextFontSet) -> Self {
        Self { software_fonts }
    }

    fn render_state(&self) -> TextSystemState {
        TextSystemState::from_font_set(self.software_fonts.clone())
    }

    fn software_fonts(&self) -> SoftwareTextFontSet {
        self.software_fonts.clone()
    }
}

/// Create an accurate WGPU text measurer for headless tests without launching a window.
pub fn headless_text_measurer() -> Rc<dyn TextMeasurer> {
    headless_text_measurer_with_fonts(&[])
}

/// Create an accurate WGPU text measurer for headless tests with explicit fonts.
pub fn headless_text_measurer_with_fonts(fonts: &[&[u8]]) -> Rc<dyn TextMeasurer> {
    Rc::new(SoftwareTextMeasurer::from_fonts_or_default(fonts, 8192))
}

/// WGPU-based renderer for GPU-accelerated 2D rendering.
///
/// This renderer supports:
/// - GPU-accelerated shape rendering (rectangles, rounded rectangles)
/// - Gradients (solid, linear, radial)
/// - GPU text rendering via retained raster image batches
/// - Cross-platform support (Desktop, Web, Android)
pub struct WgpuRenderer {
    scene: Scene,
    gpu_renderer: Option<GpuRenderer>,
    text_state: TextSystemState,
    text_fonts: SoftwareTextFontSet,
    app_context: Option<Weak<cranpose_ui::AppContext>>,
    /// Root scale factor for text rendering (use for density scaling)
    root_scale: f32,
    dev_overlay_cache: Option<DevOverlayCache>,
    dev_overlay_graph: Option<RenderGraph>,
}

#[derive(Clone, Debug)]
struct DevOverlayCache {
    text: String,
    viewport_width_bits: u32,
    viewport_height_bits: u32,
}

impl WgpuRenderer {
    /// Create a new WGPU renderer.
    ///
    /// * `fonts` – font bytes to load, ordered by priority (first = highest priority).
    ///   Pass `&[]` to load no fonts; text will not render until fonts are provided.
    ///
    /// Call [`init_gpu`][Self::init_gpu] before rendering.
    pub fn new(fonts: &[&[u8]]) -> Self {
        Self::with_text_system(WgpuTextSystem::from_fonts(fonts))
    }

    /// Create a renderer over an already-parsed font set.
    ///
    /// Measurement and rasterization both take clones of this one set, so an
    /// app-supplied family resolves identically on both sides.
    pub fn with_font_set(fonts: SoftwareTextFontSet) -> Self {
        Self::with_text_system(WgpuTextSystem::from_font_set(fonts))
    }

    pub fn with_text_system(text_system: WgpuTextSystem) -> Self {
        Self {
            scene: Scene::new(),
            gpu_renderer: None,
            text_state: text_system.render_state(),
            text_fonts: text_system.software_fonts(),
            app_context: None,
            root_scale: 1.0,
            dev_overlay_cache: None,
            dev_overlay_graph: None,
        }
    }

    /// Initialize GPU resources with a WGPU device and queue.
    pub fn init_gpu(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) {
        self.gpu_renderer = Some(GpuRenderer::new(
            device,
            queue,
            surface_format,
            adapter_backend,
            self.text_fonts.clone(),
        ));
    }

    /// Set root scale factor for text rendering (e.g., density scaling on Android)
    pub fn set_root_scale(&mut self, scale: f32) {
        self.root_scale = scale;
    }

    pub fn root_scale(&self) -> f32 {
        self.root_scale
    }

    /// Render the scene to a texture view.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        if let Some(gpu_renderer) = &mut self.gpu_renderer {
            let graph = self
                .scene
                .graph
                .as_ref()
                .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
            let text_state = &mut self.text_state;
            let root_scale = self.root_scale;
            let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
            let result = if let Some(app_context) = app_context {
                app_context.enter(|| {
                    gpu_renderer.render(
                        text_state,
                        view,
                        graph,
                        self.dev_overlay_graph.as_ref(),
                        width,
                        height,
                        root_scale,
                    )
                })
            } else {
                gpu_renderer.render(
                    text_state,
                    view,
                    graph,
                    self.dev_overlay_graph.as_ref(),
                    width,
                    height,
                    root_scale,
                )
            };
            result.map_err(WgpuRendererError::Wgpu)
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
            let graph = self
                .scene
                .graph
                .as_ref()
                .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
            let text_state = &mut self.text_state;
            let app_context = self.app_context.as_ref().and_then(Weak::upgrade);
            let pixels = if let Some(app_context) = app_context {
                app_context.enter(|| {
                    gpu_renderer.render_to_rgba_pixels(
                        text_state,
                        graph,
                        self.dev_overlay_graph.as_ref(),
                        width,
                        height,
                        root_scale,
                    )
                })
            } else {
                gpu_renderer.render_to_rgba_pixels(
                    text_state,
                    graph,
                    self.dev_overlay_graph.as_ref(),
                    width,
                    height,
                    root_scale,
                )
            }
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

    pub fn last_frame_stats(&self) -> Option<RenderStatsSnapshot> {
        self.gpu_renderer
            .as_ref()
            .and_then(GpuRenderer::last_frame_stats)
    }

    pub fn debug_cpu_allocation_stats(&self) -> DebugCpuAllocationStats {
        let mut stats = self
            .gpu_renderer
            .as_ref()
            .map(GpuRenderer::debug_cpu_allocation_stats)
            .unwrap_or_default();
        stats.scene_graph_node_count = self
            .scene
            .graph
            .as_ref()
            .map(RenderGraph::node_count)
            .unwrap_or(0);
        stats.scene_graph_heap_bytes = self
            .scene
            .graph
            .as_ref()
            .map(RenderGraph::heap_bytes)
            .unwrap_or(0);
        stats.scene_hits_len = self.scene.hits.len();
        stats.scene_hits_cap = self.scene.hits.capacity();
        stats.scene_node_index_len = self.scene.node_index.len();
        stats.scene_node_index_cap = self.scene.node_index.capacity();
        stats
    }

    /// Return the WGPU device when GPU resources are initialized.
    pub fn try_device(&self) -> Option<&wgpu::Device> {
        self.gpu_renderer.as_ref().map(|r| &*r.device)
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

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        app_context.set_text_measurer(SoftwareTextMeasurer::from_font_set(
            self.text_fonts.clone(),
            8192,
        ));
        self.app_context = Some(app_context.downgrade());
    }

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
        self.dev_overlay_graph = None;
        self.dev_overlay_cache = None;
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
        self.dev_overlay_graph = None;
        self.dev_overlay_cache = None;
        // Build scene in logical dp - scaling happens in GPU vertex upload
        // Traverse layout nodes via applier instead of rebuilding LayoutTree
        pipeline::render_from_applier(applier, root, &mut self.scene, 1.0);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        viewport: Size,
        dirty_nodes: &[NodeId],
    ) -> Result<(), Self::Error> {
        if dirty_nodes.is_empty() {
            return self.rebuild_scene_from_applier(applier, root, viewport);
        }
        pipeline::update_from_applier(applier, root, &mut self.scene, 1.0, dirty_nodes, true);
        Ok(())
    }

    fn update_visual_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        viewport: Size,
        dirty_nodes: &[NodeId],
    ) -> Result<(), Self::Error> {
        if dirty_nodes.is_empty() {
            return self.rebuild_scene_from_applier(applier, root, viewport);
        }
        pipeline::update_from_applier(applier, root, &mut self.scene, 1.0, dirty_nodes, false);
        Ok(())
    }

    fn draw_dev_overlay(&mut self, text: &str, viewport: Size) {
        const DEV_OVERLAY_NODE_ID: NodeId = NodeId::MAX;
        let key = cranpose_render_common::dev_overlay::DevOverlayKey::new(text, viewport);
        if self.dev_overlay_graph.is_some()
            && self.dev_overlay_cache.as_ref().is_some_and(|cache| {
                cache.text == key.text
                    && cache.viewport_width_bits == key.viewport_width_bits
                    && cache.viewport_height_bits == key.viewport_height_bits
            })
        {
            return;
        }
        self.dev_overlay_graph = Some(
            cranpose_render_common::dev_overlay::build_dev_overlay_graph(
                text,
                viewport,
                DEV_OVERLAY_NODE_ID,
            ),
        );
        self.dev_overlay_cache = Some(DevOverlayCache {
            text: key.text,
            viewport_width_bits: key.viewport_width_bits,
            viewport_height_bits: key.viewport_height_bits,
        });
    }

    fn needs_frame_warmup(&self) -> bool {
        self.gpu_renderer
            .as_ref()
            .is_some_and(GpuRenderer::needs_frame_warmup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::TextLayoutResolver;
    use cranpose_render_common::graph::RenderNode;
    use cranpose_ui_graphics::GraphicsLayer;
    use std::cell::Cell;

    static TEST_FONT: &[u8] =
        cranpose_render_common::software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES;

    #[test]
    fn dev_overlay_is_recorded_outside_app_graph() {
        let mut renderer = WgpuRenderer::new(&[]);
        renderer.draw_dev_overlay(
            "240 FPS | avg 4.0ms | p95 4.5ms",
            Size {
                width: 800.0,
                height: 600.0,
            },
        );

        assert!(
            renderer
                .scene
                .graph
                .as_ref()
                .is_none_or(|graph| graph.root.children.iter().all(|child| {
                    !matches!(
                        child,
                        RenderNode::Layer(layer) if layer.node_id == Some(NodeId::MAX)
                    )
                })),
            "dev overlay must not be mixed into the app scene graph"
        );

        let graph = renderer.dev_overlay_graph.as_ref().expect("overlay graph");
        let Some(RenderNode::Layer(overlay)) = graph.root.children.last() else {
            panic!("dev overlay should be the final top-level layer");
        };

        assert_eq!(overlay.node_id, Some(NodeId::MAX));
        assert_eq!(
            overlay.graphics_layer.compositing_strategy,
            GraphicsLayer::default().compositing_strategy,
            "dev overlay should not allocate an offscreen surface"
        );
    }

    struct CountingTextMeasurer {
        inner: SoftwareTextMeasurer,
        layout_calls: Rc<Cell<usize>>,
    }

    impl CountingTextMeasurer {
        fn new(layout_calls: Rc<Cell<usize>>) -> Self {
            Self {
                inner: SoftwareTextMeasurer::from_fonts_or_default(&[TEST_FONT], 16),
                layout_calls,
            }
        }
    }

    impl TextMeasurer for CountingTextMeasurer {
        fn measure(
            &self,
            text: &cranpose_ui::text::AnnotatedString,
            style: &cranpose_ui::text::TextStyle,
        ) -> cranpose_ui::TextMetrics {
            self.inner.measure(text, style)
        }

        fn get_offset_for_position(
            &self,
            text: &cranpose_ui::text::AnnotatedString,
            style: &cranpose_ui::text::TextStyle,
            x: f32,
            y: f32,
        ) -> usize {
            self.inner.get_offset_for_position(text, style, x, y)
        }

        fn get_cursor_x_for_offset(
            &self,
            text: &cranpose_ui::text::AnnotatedString,
            style: &cranpose_ui::text::TextStyle,
            offset: usize,
        ) -> f32 {
            self.inner.get_cursor_x_for_offset(text, style, offset)
        }

        fn layout(
            &self,
            text: &cranpose_ui::text::AnnotatedString,
            style: &cranpose_ui::text::TextStyle,
        ) -> cranpose_ui::text_layout_result::TextLayoutResult {
            self.layout_calls.set(self.layout_calls.get() + 1);
            self.inner.layout(text, style)
        }
    }

    #[test]
    fn headless_text_measurer_uses_software_text_font() {
        let measurer = headless_text_measurer_with_fonts(&[TEST_FONT]);
        let text = cranpose_ui::text::AnnotatedString::from("software text measurement");
        let style = cranpose_ui::text::TextStyle::default();

        let metrics = measurer.measure(&text, &style);
        let layout = measurer.layout(&text, &style);

        assert!(metrics.width > 0.0);
        assert!(metrics.height > 0.0);
        assert_eq!(layout.lines.len(), metrics.line_count);
    }

    #[test]
    fn renderer_measurement_uses_software_text_service_without_render_cache_side_effect() {
        let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
        let app_context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&app_context);

        let metrics = app_context.enter(|| {
            let text = cranpose_ui::text::AnnotatedString::from("phase local text cache");
            let style = cranpose_ui::text::TextStyle {
                span_style: cranpose_ui::text::SpanStyle {
                    font_size: cranpose_ui::text::TextUnit::Sp(14.0),
                    ..Default::default()
                },
                paragraph_style: cranpose_ui::text::ParagraphStyle {
                    platform_style: Some(cranpose_ui::text::PlatformParagraphStyle {
                        include_font_padding: None,
                        shaping: Some(cranpose_ui::text::TextShaping::Basic),
                    }),
                    ..Default::default()
                },
            };
            cranpose_ui::text::measure_text(&text, &style)
        });

        assert!(
            metrics.width > 0.0,
            "software text service should measure text"
        );
        assert_eq!(
            renderer.text_state.text_cache_len(),
            0,
            "WGPU must not keep a renderer-side shaping cache for measurement"
        );
    }

    #[test]
    fn renderer_attached_text_service_measures_long_multiline_text_with_software_line_height() {
        let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
        let app_context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&app_context);

        let prepared = app_context.enter(|| {
            let text = cranpose_ui::text::AnnotatedString::from(
                (0..48)
                    .map(|line| format!("// markdown code line {line:02}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
            let style = cranpose_ui::text::TextStyle::default();
            cranpose_ui::text::prepare_text_layout(
                &text,
                &style,
                cranpose_ui::text::TextLayoutOptions::default(),
                Some(952.0),
            )
        });

        assert_eq!(prepared.metrics.line_count, 48);
        assert!(
            prepared.metrics.line_height > 18.0,
            "renderer-attached text service must not use fallback monospaced line height: {:?}",
            prepared.metrics
        );
        assert!(
            prepared.metrics.height > 900.0,
            "48 software-measured lines should not collapse to a viewport-sized block: {:?}",
            prepared.metrics
        );
    }

    #[test]
    fn render_text_layout_routes_through_attached_app_context_service() {
        let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
        let app_context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&app_context);
        let layout_calls = Rc::new(Cell::new(0));
        app_context.set_text_measurer(CountingTextMeasurer::new(Rc::clone(&layout_calls)));

        app_context.enter(|| {
            let text = cranpose_ui::text::AnnotatedString::from("render text");
            let style = cranpose_ui::text::TextStyle::default();
            let layout = renderer.text_state.layout_text(&text, &style);
            assert!(layout.width > 0.0);
        });

        assert_eq!(layout_calls.get(), 1);
    }
}

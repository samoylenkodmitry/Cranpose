//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile Android.

#![deny(unsafe_code)]

#[cfg(not(target_arch = "wasm32"))]
mod cost_tuner;
mod display_clip;
mod effect_renderer;
mod frame_graph;
mod frame_packet;
mod frontend;
pub(crate) mod gpu_stats;
mod layer_events;
mod layer_surface_cache;
mod lazy_resource;
mod normalized_scene;
mod offscreen;
mod pipeline;
mod render;
mod run_entry;
mod scene;
mod shader_cache;
mod shaders;
#[cfg(not(target_arch = "wasm32"))]
mod shape_replay;
#[cfg(not(target_arch = "wasm32"))]
mod stage_executor;
mod surface_executor;
mod surface_plan;
mod surface_requirements;
#[cfg(test)]
mod test_support;

#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use display_clip::pixel_is_visible as display_clip_pixel_is_visible;
pub use display_clip::DisplayVisibleRegion;
#[doc(hidden)]
pub use frame_packet::{CancelReason, PresentOutcome};
pub use gpu_stats::FrameStatsSnapshot as RenderStatsSnapshot;
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use pipeline::retained_feed_generation;
pub use render::frames_presented;
pub use scene::{ClickAction, HitRegion, Scene};
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use shape_replay::feed_live_stats as command_feed_live_stats;
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use shape_replay::{inject_feed_capture_for_tests, pending_feed_capture_count_for_tests};
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use shape_replay::{planner_replay_queue_stats_for_tests, recycled_ops_capacities_for_tests};

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
use frame_packet::RenderReturns;
use frontend::{DevOverlayCache, RendererFrontend};
use render::GpuRenderer;
use std::rc::Rc;
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

    pub(crate) fn render_state(&self) -> TextSystemState {
        TextSystemState::from_font_set(self.software_fonts.clone())
    }

    pub(crate) fn software_fonts(&self) -> SoftwareTextFontSet {
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
    /// Producer stage: scene graph, text layout, and the lowering of every
    /// frame into a [`frame_packet::FramePacket`].
    frontend: RendererFrontend,
    /// Present stage: consumes packets and draws; it never lowers.
    gpu_renderer: Option<GpuRenderer>,
    /// Which `GpuRenderer` instance packets are currently built against:
    /// bumped by every [`init_gpu`][Self::init_gpu] (first init → 1) and
    /// stamped into each packet, so a packet that outlives its renderer is
    /// cancelled by the present stage instead of drawn.
    renderer_epoch: u64,
    /// Which surface configuration packets are currently built against:
    /// bumped by [`note_surface_reconfigured`][Self::note_surface_reconfigured]
    /// and stamped into each packet, so a packet that straddles a surface
    /// reconfigure is cancelled by the present stage instead of drawn.
    surface_epoch: u64,
    /// The display's visible region from
    /// [`set_display_visible_region`][Self::set_display_visible_region]:
    /// the part of the surface the panel physically shows. Held here so a
    /// `GpuRenderer` replaced by `init_gpu` inherits it. Never derived
    /// from app content — only the platform layer (or a host standing in
    /// for it) may set it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    display_visible_region: DisplayVisibleRegion,
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
            frontend: RendererFrontend::new(
                text_system.render_state(),
                text_system.software_fonts(),
            ),
            gpu_renderer: None,
            renderer_epoch: 0,
            surface_epoch: 0,
            display_visible_region: DisplayVisibleRegion::Full,
        }
    }

    /// Initialize GPU resources with a WGPU device and queue.
    ///
    /// Replacing a live renderer (Android surface recreation, device loss)
    /// drops every retained replay slot with the old `GpuRenderer`, so the
    /// bypass contract fails closed BEFORE the new renderer exists: every
    /// slot confirmation is revoked and the feed generation bumped — no
    /// scene build may omit primitives against buffers that died, and
    /// already-built frames rematerialize their bypassed spans instead of
    /// referencing the dead renderer's slot ids.
    pub fn init_gpu(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.gpu_renderer.is_some() {
            crate::shape_replay::SHAPE_REPLAY.with(|state| state.borrow_mut().renderer_replaced());
            log::warn!(
                "[command-feed] renderer replaced: retained slots retired, \
                 confirmations revoked, feed generation bumped"
            );
        }
        self.renderer_epoch = self.renderer_epoch.wrapping_add(1);
        // The new store adopts the producer's CURRENT feed generation —
        // read after `renderer_replaced` bumped it, so packets planned
        // against the dead store fail the store's generation gate.
        #[cfg(not(target_arch = "wasm32"))]
        let store_feed_generation = pipeline::retained_feed_generation();
        #[cfg(target_arch = "wasm32")]
        let store_feed_generation = 0;
        self.gpu_renderer = Some(GpuRenderer::new(
            device,
            queue,
            surface_format,
            adapter_backend,
            self.frontend.text_fonts.clone(),
            self.renderer_epoch,
            store_feed_generation,
        ));
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gpu_renderer) = self.gpu_renderer.as_mut() {
            gpu_renderer.set_display_visible_region(self.display_visible_region);
        }
    }

    /// The display's visible region — the part of this renderer's
    /// full-screen surface the panel physically shows. Set by the
    /// platform layer (never by app content); the renderer then culls
    /// everything outside the region on the full-frame pass, for any app
    /// and any layout. The round display is the first provider: Android's
    /// `AConfiguration` screenRound maps to
    /// [`DisplayVisibleRegion::InscribedCircle`] for a non-multi-window
    /// activity. Future providers (display cutouts/insets, host-declared
    /// clips) plug in as new region variants without touching the cull
    /// machinery. Default [`DisplayVisibleRegion::Full`]: rendering is
    /// bitwise identical to a renderer without this capability.
    pub fn set_display_visible_region(&mut self, region: DisplayVisibleRegion) {
        self.display_visible_region = region;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gpu_renderer) = self.gpu_renderer.as_mut() {
            gpu_renderer.set_display_visible_region(region);
        }
    }

    /// Record that the surface was reconfigured (resize, format change,
    /// swapchain recreation): bumps the surface epoch stamped into every
    /// subsequent packet, so a packet built against the previous
    /// configuration is cancelled by the present stage instead of drawn.
    pub fn note_surface_reconfigured(&mut self) {
        self.surface_epoch = self.surface_epoch.wrapping_add(1);
    }

    /// Set root scale factor for text rendering (e.g., density scaling on Android)
    pub fn set_root_scale(&mut self, scale: f32) {
        self.frontend.root_scale = scale;
    }

    pub fn root_scale(&self) -> f32 {
        self.frontend.root_scale
    }

    /// Render the scene to a texture view.
    ///
    /// Producer first, present second: the frontend lowers the frame into a
    /// [`frame_packet::FramePacket`] (direct root, root surface, and dev
    /// overlay alike), the GPU renderer consumes it, and the present
    /// stage's returns — the recycled scene and the replay ack — fold back
    /// into the frontend afterwards.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        let Some(gpu_renderer) = self.gpu_renderer.as_mut() else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ));
        };
        let packet = self
            .frontend
            .build_frame_packet(
                width,
                height,
                gpu_renderer.replay_supported(),
                self.renderer_epoch,
                self.surface_epoch,
            )
            .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
        let frontend = &mut self.frontend;
        let mut returns = RenderReturns::default();
        // Packet consumption runs OUTSIDE the producer's app context on
        // purpose: the packet is the complete frame, so the present stage
        // must never need the context — running bare proves it every frame.
        let result = gpu_renderer.render(
            view,
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        if let Some(confirmations) = frontend.apply_returns(returns) {
            gpu_renderer.restore_replay_ack_confirmations(confirmations);
        }
        result.map_err(WgpuRendererError::Wgpu)
    }

    /// Render the current scene into an RGBA pixel buffer for robot tests.
    ///
    /// Uses the renderer's configured root scale.
    pub fn capture_frame(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<CapturedFrame, WgpuRendererError> {
        self.capture_frame_with_scale(width, height, self.frontend.root_scale)
    }

    /// Render the current scene into an RGBA pixel buffer with an explicit scale.
    pub fn capture_frame_with_scale(
        &mut self,
        width: u32,
        height: u32,
        root_scale: f32,
    ) -> Result<CapturedFrame, WgpuRendererError> {
        let Some(gpu_renderer) = self.gpu_renderer.as_mut() else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ));
        };
        let packet = self
            .frontend
            .build_frame_packet_with_scale(
                width,
                height,
                root_scale,
                gpu_renderer.replay_supported(),
                self.renderer_epoch,
                self.surface_epoch,
            )
            .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
        let frontend = &mut self.frontend;
        let mut returns = RenderReturns::default();
        // Bare like `render`: the capture path consumes the packet with no
        // producer app context current.
        let result = gpu_renderer.render_to_rgba_pixels(
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        if let Some(confirmations) = frontend.apply_returns(returns) {
            gpu_renderer.restore_replay_ack_confirmations(confirmations);
        }
        let pixels = result.map_err(WgpuRendererError::Wgpu)?;
        Ok(CapturedFrame {
            width,
            height,
            pixels,
        })
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
            .frontend
            .scene
            .graph
            .as_ref()
            .map(RenderGraph::node_count)
            .unwrap_or(0);
        stats.scene_graph_heap_bytes = self
            .frontend
            .scene
            .graph
            .as_ref()
            .map(RenderGraph::heap_bytes)
            .unwrap_or(0);
        stats.scene_hits_len = self.frontend.scene.hits.len();
        stats.scene_hits_cap = self.frontend.scene.hits.capacity();
        stats.scene_node_index_len = self.frontend.scene.node_index.len();
        stats.scene_node_index_cap = self.frontend.scene.node_index.capacity();
        // The producer frontend owns the renderer's only lowering-memo
        // pair (the present backend reports zeros); add it here so its
        // retained capacity stays visible to leak tooling.
        stats.layer_surface_rect_cache_len += self.frontend.layer_surface_rect_cache.len();
        stats.layer_surface_rect_cache_cap += self.frontend.layer_surface_rect_cache.capacity();
        stats.layer_surface_requirements_cache_len +=
            self.frontend.layer_surface_requirements_cache.len();
        stats.layer_surface_requirements_cache_cap +=
            self.frontend.layer_surface_requirements_cache.capacity();
        stats
    }

    /// Return the WGPU device when GPU resources are initialized.
    pub fn try_device(&self) -> Option<&wgpu::Device> {
        self.gpu_renderer.as_ref().map(|r| &*r.device)
    }

    /// Test/diagnostic view of the latched instanced-quad selection: `true`
    /// when the live GPU renderer's ordinary shape draws ride
    /// `vs_shape_instanced` (storage mode && `CRANPOSE_INSTANCED_QUADS` != 0
    /// at construction).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn instanced_quads_active(&self) -> bool {
        self.gpu_renderer
            .as_ref()
            .is_some_and(GpuRenderer::instanced_quads_active)
    }

    /// Test/diagnostic view of retained arc meshes: (slots holding a mesh,
    /// total live replay slots).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_slot_mesh_stats(&self) -> (usize, usize) {
        self.gpu_renderer
            .as_ref()
            .map(GpuRenderer::replay_slot_mesh_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the retained bundle cache: lifetime
    /// (rebuilds, cached executes).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn retained_bundle_stats(&self) -> (u64, u64) {
        self.gpu_renderer
            .as_ref()
            .map(GpuRenderer::retained_bundle_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the transient rim mesh path: lifetime count
    /// of dynamic circle rims drawn as band meshes instead of full bounding
    /// quads (`CRANPOSE_RIM_MESH` kill switch).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn rim_meshes_emitted(&self) -> u64 {
        self.gpu_renderer
            .as_ref()
            .map(GpuRenderer::rim_meshes_emitted)
            .unwrap_or(0)
    }

    /// Test/diagnostic view of the opaque static leading-span cache:
    /// lifetime (hits, recaptures) — frames drawn with the cached
    /// full-target blit standing in for the leading span, and capture
    /// passes rendered (`CRANPOSE_STATIC_SPAN` kill switch).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn static_span_stats(&self) -> (u64, u64) {
        self.gpu_renderer
            .as_ref()
            .map(GpuRenderer::static_span_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the present store's lifetime count of
    /// replay-ops batches dropped by the generation check. Surface (non
    /// direct) frames must never move it: their packets carry the default
    /// plan, which the consume gate never feeds to the store.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_generation_drops_for_tests(&self) -> u64 {
        self.gpu_renderer
            .as_ref()
            .map(GpuRenderer::replay_generation_drops)
            .unwrap_or(0)
    }

    /// Test hook for the replay message protocol: one planner→store→planner
    /// cycle outside a frame, the batch's generation skewed by
    /// `generation_skew` from the store's own; returns how many captures
    /// the store confirmed. A skew landing BELOW the store's generation
    /// manufactures the fail-closed drop; one landing above it exercises
    /// adopt-forward. Neither is producible synchronously through the
    /// public render path.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_ops_roundtrip_for_tests(&mut self, generation_skew: u64) -> usize {
        self.gpu_renderer
            .as_mut()
            .expect("GPU renderer not initialized")
            .replay_ops_roundtrip_for_tests(generation_skew)
    }

    /// Test hook for the cancellation protocol: builds a packet NOW,
    /// stamped with the current epochs, and hands it to the caller instead
    /// of rendering it — the public render path builds and consumes
    /// atomically, so a packet in flight across an epoch change is only
    /// constructible here.
    #[doc(hidden)]
    pub fn build_frame_packet_for_tests(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<HeldFramePacket> {
        let replay_supported = self
            .gpu_renderer
            .as_ref()
            .is_some_and(GpuRenderer::replay_supported);
        self.frontend
            .build_frame_packet(
                width,
                height,
                replay_supported,
                self.renderer_epoch,
                self.surface_epoch,
            )
            .map(HeldFramePacket)
    }

    /// Test hook: consumes a held packet through the exact production seam
    /// (`GpuRenderer::render` + `apply_returns` + ack-buffer restore) and
    /// reports the present outcome.
    #[doc(hidden)]
    pub fn render_held_packet_for_tests(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        packet: HeldFramePacket,
    ) -> Result<PresentOutcome, WgpuRendererError> {
        let Some(gpu_renderer) = self.gpu_renderer.as_mut() else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized. Call init_gpu() first.".to_string(),
            ));
        };
        let frontend = &mut self.frontend;
        let mut returns = RenderReturns::default();
        let result = gpu_renderer.render(
            view,
            width,
            height,
            packet.0,
            self.surface_epoch,
            &mut returns,
        );
        let outcome = returns.outcome;
        if let Some(confirmations) = frontend.apply_returns(returns) {
            gpu_renderer.restore_replay_ack_confirmations(confirmations);
        }
        result.map_err(WgpuRendererError::Wgpu)?;
        Ok(outcome)
    }

    /// Test hook: whether the producer pool holds a recycled direct scene —
    /// the cancellation contract's proof the packet's scene came back.
    #[doc(hidden)]
    pub fn has_retained_direct_scene_for_tests(&self) -> bool {
        self.frontend.retained_direct_scene.is_some()
    }
}

/// Opaque handle to a built-but-unrendered frame packet, for the
/// cancellation-protocol tests only.
#[doc(hidden)]
pub struct HeldFramePacket(frame_packet::FramePacket);

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
            self.frontend.text_fonts.clone(),
            8192,
        ));
        self.frontend.app_context = Some(app_context.downgrade());
    }

    fn scene(&self) -> &Self::Scene {
        &self.frontend.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.frontend.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.frontend.scene.clear();
        self.frontend.dev_overlay_graph = None;
        self.frontend.dev_overlay_cache = None;
        // Build scene in logical dp - scaling happens in GPU vertex upload
        pipeline::render_layout_tree(layout_tree.root(), &mut self.frontend.scene);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.frontend.scene.clear();
        self.frontend.dev_overlay_graph = None;
        self.frontend.dev_overlay_cache = None;
        // Build scene in logical dp - scaling happens in GPU vertex upload
        // Traverse layout nodes via applier instead of rebuilding LayoutTree
        pipeline::render_from_applier(applier, root, &mut self.frontend.scene, 1.0);
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
        pipeline::update_from_applier(
            applier,
            root,
            &mut self.frontend.scene,
            1.0,
            dirty_nodes,
            true,
        );
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
        pipeline::update_from_applier(
            applier,
            root,
            &mut self.frontend.scene,
            1.0,
            dirty_nodes,
            false,
        );
        Ok(())
    }

    fn draw_dev_overlay(&mut self, text: &str, viewport: Size) {
        const DEV_OVERLAY_NODE_ID: NodeId = NodeId::MAX;
        let key = cranpose_render_common::dev_overlay::DevOverlayKey::new(text, viewport);
        if self.frontend.dev_overlay_graph.is_some()
            && self
                .frontend
                .dev_overlay_cache
                .as_ref()
                .is_some_and(|cache| {
                    cache.text == key.text
                        && cache.viewport_width_bits == key.viewport_width_bits
                        && cache.viewport_height_bits == key.viewport_height_bits
                })
        {
            return;
        }
        self.frontend.dev_overlay_graph = Some(
            cranpose_render_common::dev_overlay::build_dev_overlay_graph(
                text,
                viewport,
                DEV_OVERLAY_NODE_ID,
            ),
        );
        self.frontend.dev_overlay_cache = Some(DevOverlayCache {
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
                .frontend
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

        let graph = renderer
            .frontend
            .dev_overlay_graph
            .as_ref()
            .expect("overlay graph");
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
            renderer.frontend.text_state.text_cache_len(),
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
            let layout = renderer.frontend.text_state.layout_text(&text, &style);
            assert!(layout.width > 0.0);
        });

        assert_eq!(layout_calls.get(), 1);
    }
}

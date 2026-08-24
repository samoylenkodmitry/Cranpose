//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile Android.

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
mod output_conversion;
mod pipeline;
#[cfg(not(target_arch = "wasm32"))]
mod pipeline_disk_cache;
#[cfg(not(target_arch = "wasm32"))]
mod present_runtime;
mod render;
mod run_entry;
mod scene;
#[cfg(not(target_arch = "wasm32"))]
mod segment_surface;
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

use std::{rc::Rc, sync::Arc};

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
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use display_clip::pixel_is_visible as display_clip_pixel_is_visible;
pub use display_clip::DisplayVisibleRegion;
pub use frame_packet::PresentTimings;
use frame_packet::RenderReturns;
#[cfg(not(target_arch = "wasm32"))]
use frame_packet::ReplayConfirmation;
#[doc(hidden)]
pub use frame_packet::{CancelReason, PresentOutcome};
use frontend::{DevOverlayCache, RendererFrontend};
pub use gpu_stats::FrameStatsSnapshot as RenderStatsSnapshot;
#[doc(hidden)]
#[cfg(not(target_arch = "wasm32"))]
pub use pipeline::retained_feed_generation;
#[cfg(not(target_arch = "wasm32"))]
use present_runtime::{
    PresentControl, PresentHandle, PresentMsg, PresentRuntimeInit, PresentState,
};
pub use render::frames_presented;
use render::GpuRenderer;
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

/// Which present stage a [`WgpuRenderer`] drives.
///
/// `Sync` is today's synchronous path (desktop, web, iOS, tests): the
/// producer builds a packet and consumes it in the same call. `Threaded`
/// is the Android depth-one runtime: the `GpuRenderer` lives on the
/// present thread and only a `PresentHandle` stays here. Boxed because a
/// `GpuRenderer` is kilobytes of retained state, moved only at init.
enum PresentBackend {
    /// No GPU yet (`init_gpu`/`init_gpu_threaded` not called).
    None,
    /// The synchronous present stage, consumed in `render`.
    Sync(Box<GpuRenderer>),
    /// The spawned (or inline-for-tests) present runtime.
    #[cfg(not(target_arch = "wasm32"))]
    Threaded(PresentHandle),
}

/// What [`WgpuRenderer::publish_frame`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    /// No scene graph exists; nothing to lower.
    NoGraph,
    /// The depth-one slot is occupied (or the renderer is not in threaded
    /// mode): NO packet was built — backpressure lands before lowering.
    NoCredit,
    /// A packet was built and handed to the present runtime.
    Published,
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
    backend: PresentBackend,
    /// Threaded mode: the emptied ack-confirmations buffer drained from
    /// the planner, parked here until the next packet carries it back to
    /// the present-side store (capacity round-trip, no per-frame alloc).
    #[cfg(not(target_arch = "wasm32"))]
    pending_recycled_confirmations: Option<Vec<ReplayConfirmation>>,
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
    fn update_scene_and_plan_memo(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        dirty_nodes: &[NodeId],
        refresh_hits: bool,
    ) {
        let mut changed_nodes = std::mem::take(&mut self.frontend.changed_nodes);
        let outcome = pipeline::update_from_applier(
            applier,
            root,
            &mut self.frontend.scene,
            1.0,
            dirty_nodes,
            refresh_hits,
            &mut changed_nodes,
        );
        match outcome {
            pipeline::SceneUpdateOutcome::Patched => {
                for node_id in &changed_nodes {
                    self.frontend
                        .layer_surface_requirements_cache
                        .remove(node_id);
                }
            }
            pipeline::SceneUpdateOutcome::Rebuilt => {
                self.frontend.layer_surface_requirements_cache.clear();
            }
        }
        changed_nodes.clear();
        self.frontend.changed_nodes = changed_nodes;
    }

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
            backend: PresentBackend::None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_recycled_confirmations: None,
            renderer_epoch: 0,
            surface_epoch: 0,
            display_visible_region: DisplayVisibleRegion::Full,
        }
    }

    /// The synchronous present backend, when that is what is live.
    fn sync_gpu_renderer(&self) -> Option<&GpuRenderer> {
        match &self.backend {
            PresentBackend::Sync(gpu_renderer) => Some(gpu_renderer.as_ref()),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sync_gpu_renderer_mut(&mut self) -> Option<&mut GpuRenderer> {
        match &mut self.backend {
            PresentBackend::Sync(gpu_renderer) => Some(gpu_renderer.as_mut()),
            _ => None,
        }
    }

    /// The threaded present handle, when that is what is live.
    #[cfg(not(target_arch = "wasm32"))]
    fn present_handle_mut(&mut self) -> Option<&mut PresentHandle> {
        match &mut self.backend {
            PresentBackend::Threaded(handle) => Some(handle),
            _ => None,
        }
    }

    /// Retire whatever present backend is live before a replacement init:
    /// drain and fold in any pending threaded returns (their buffers are
    /// still recyclable), shut the runtime down, and run the planner's
    /// renderer-replacement hygiene — retained slots retired, confirmations
    /// revoked, feed generation bumped — exactly as `init_gpu` always did
    /// for a live sync renderer.
    fn retire_live_backend(&mut self) {
        #[allow(unused_mut)]
        let mut backend = std::mem::replace(&mut self.backend, PresentBackend::None);
        if matches!(backend, PresentBackend::None) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let PresentBackend::Threaded(handle) = &mut backend {
                // Early acks first (they precede their frames' returns);
                // the imminent `renderer_replaced` revokes every
                // confirmation anyway, but the buffers are still worth
                // recycling.
                while let Some((ack, recycled)) = handle.try_drain_ack() {
                    let confirmations = crate::shape_replay::SHAPE_REPLAY
                        .with(|state| state.borrow_mut().apply_ack(ack, recycled));
                    self.stash_recycled_confirmations(confirmations);
                }
                while let Some(returns) = handle.try_drain() {
                    if let Some(confirmations) = self.frontend.apply_returns(returns) {
                        self.stash_recycled_confirmations(confirmations);
                    }
                }
                handle.shutdown();
            }
            crate::shape_replay::SHAPE_REPLAY.with(|state| state.borrow_mut().renderer_replaced());
            log::warn!(
                "[command-feed] renderer replaced: retained slots retired, \
                 confirmations revoked, feed generation bumped"
            );
        }
        drop(backend);
    }

    /// Park a planner-drained confirmations buffer for the next packet,
    /// keeping the larger capacity if one is somehow already parked.
    #[cfg(not(target_arch = "wasm32"))]
    fn stash_recycled_confirmations(&mut self, confirmations: Vec<ReplayConfirmation>) {
        match &self.pending_recycled_confirmations {
            Some(parked) if parked.capacity() >= confirmations.capacity() => {}
            _ => self.pending_recycled_confirmations = Some(confirmations),
        }
    }

    /// Threaded mode: fold every pending early [`ReplayAck`] into the
    /// planner (`frame_packet::ReplayAck` — confirmations register feed
    /// slots, the batch's emptied buffers recycle). The present thread
    /// sends these BEFORE surface acquire, so draining here — ahead of
    /// the next `build_frame_packet` — gives a capture the same one-frame
    /// confirmation latency the synchronous path has. Returns how many
    /// acks were applied; no-op outside threaded mode.
    ///
    /// `pub` + hidden for the contract tests, which pin the ack arriving
    /// AHEAD of its frame's returns; the renderer's own callers are
    /// `publish_frame` and `drain_present_returns_with`
    /// (`retire_live_backend` drains the channel inline while the backend
    /// is detached).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn drain_replay_acks(&mut self) -> usize {
        let mut drained = 0;
        loop {
            let (ack, recycled) = {
                let PresentBackend::Threaded(handle) = &mut self.backend else {
                    break;
                };
                match handle.try_drain_ack() {
                    Some(batch) => batch,
                    None => break,
                }
            };
            drained += 1;
            let confirmations = crate::shape_replay::SHAPE_REPLAY
                .with(|state| state.borrow_mut().apply_ack(ack, recycled));
            self.stash_recycled_confirmations(confirmations);
        }
        drained
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
        adapter_downlevel: wgpu::DownlevelFlags,
    ) {
        self.retire_live_backend();
        self.renderer_epoch = self.renderer_epoch.wrapping_add(1);
        // The new store adopts the producer's CURRENT feed generation —
        // read after `renderer_replaced` bumped it, so packets planned
        // against the dead store fail the store's generation gate.
        #[cfg(not(target_arch = "wasm32"))]
        let store_feed_generation = pipeline::retained_feed_generation();
        #[cfg(target_arch = "wasm32")]
        let store_feed_generation = 0;
        self.backend = PresentBackend::Sync(Box::new(GpuRenderer::new(
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            self.frontend.text_fonts.clone(),
            self.renderer_epoch,
            store_feed_generation,
        )));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let region = self.display_visible_region;
            if let Some(gpu_renderer) = self.sync_gpu_renderer_mut() {
                gpu_renderer.set_display_visible_region(region);
            }
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
    ///
    /// Threaded mode routes the region to the present thread as a control
    /// message; the message queue is FIFO, so it lands before any packet
    /// published after this call — the same ordering the sync path's
    /// direct call has.
    pub fn set_display_visible_region(&mut self, region: DisplayVisibleRegion) {
        self.display_visible_region = region;
        #[cfg(not(target_arch = "wasm32"))]
        match &mut self.backend {
            PresentBackend::Sync(gpu_renderer) => {
                gpu_renderer.set_display_visible_region(region);
            }
            PresentBackend::Threaded(handle) => {
                handle.send_display_visible_region(region);
            }
            PresentBackend::None => {}
        }
    }

    /// [`init_gpu`][Self::init_gpu] for the threaded present runtime
    /// (Android): the same epoch bump and planner replacement hygiene, but
    /// instead of constructing a `GpuRenderer` here, everything it needs —
    /// all owned, all `Send` — crosses to a spawned present thread that
    /// constructs its own (its `Rc` caches are thread-confined). Frames
    /// then flow through [`publish_frame`][Self::publish_frame] /
    /// [`drain_present_returns`][Self::drain_present_returns] under the
    /// depth-one credit protocol instead of [`render`][Self::render].
    ///
    /// * `waker` — wakes the producer's event loop after every returns
    ///   send (the Android frame waker).
    /// * `clock` — producer's monotonic nanosecond clock, so present-side
    ///   [`PresentTimings`] share the producer telemetry's clock domain;
    ///   `None` leaves timings at zero.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_arguments)]
    pub fn init_gpu_threaded(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
        adapter_downlevel: wgpu::DownlevelFlags,
        waker: Arc<dyn Fn() + Send + Sync>,
        clock: Option<Arc<dyn Fn() -> i64 + Send + Sync>>,
    ) -> Result<(), WgpuRendererError> {
        self.retire_live_backend();
        self.renderer_epoch = self.renderer_epoch.wrapping_add(1);
        // As in `init_gpu`: read AFTER the hygiene bump.
        let store_feed_generation = pipeline::retained_feed_generation();
        let init = PresentRuntimeInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts: self.frontend.text_fonts.clone(),
            renderer_epoch: self.renderer_epoch,
            store_feed_generation,
            clock,
            display_visible_region: self.display_visible_region,
        };
        let handle = PresentHandle::spawn(init, waker).map_err(WgpuRendererError::Wgpu)?;
        self.backend = PresentBackend::Threaded(handle);
        Ok(())
    }

    /// [`init_gpu_threaded`][Self::init_gpu_threaded] without the thread:
    /// the state machine and its message queue are handed back for the
    /// test to pump by hand — the same prepare/validate/present protocol,
    /// deterministically observable.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn init_gpu_inline_for_tests(
        &mut self,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
        adapter_downlevel: wgpu::DownlevelFlags,
    ) -> InlinePresentRuntime {
        self.retire_live_backend();
        self.renderer_epoch = self.renderer_epoch.wrapping_add(1);
        let store_feed_generation = pipeline::retained_feed_generation();
        let init = PresentRuntimeInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts: self.frontend.text_fonts.clone(),
            renderer_epoch: self.renderer_epoch,
            store_feed_generation,
            clock: None,
            display_visible_region: self.display_visible_region,
        };
        let (handle, state, msg_rx) = PresentHandle::new_inline(init, Arc::new(|| {}));
        self.backend = PresentBackend::Threaded(handle);
        InlinePresentRuntime {
            state,
            msg_rx,
            shutdown_seen: false,
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
    /// `frame_packet::FramePacket` (direct root, root surface, and dev
    /// overlay alike), the GPU renderer consumes it, and the present
    /// stage's returns — the recycled scene and the replay ack — fold back
    /// into the frontend afterwards.
    pub fn render(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        self.render_frame(view, width, height)
    }

    pub fn render_surface_texture(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        self.render_frame(view, width, height)
    }

    fn render_frame(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        let PresentBackend::Sync(gpu_renderer) = &mut self.backend else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized for synchronous rendering. Call init_gpu() first."
                    .to_string(),
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
        let PresentBackend::Sync(gpu_renderer) = &mut self.backend else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized for synchronous rendering. Call init_gpu() first."
                    .to_string(),
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

    /// Threaded mode: whether the depth-one slot has room for a packet.
    /// The Android loop checks this BEFORE `shell.update()` so
    /// backpressure lands before the expensive update/lowering work.
    /// Always `true` on the sync path, which has no slot to fill.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn has_frame_credit(&self) -> bool {
        match &self.backend {
            PresentBackend::Threaded(handle) => handle.has_credit(),
            PresentBackend::Sync(_) | PresentBackend::None => true,
        }
    }

    /// Threaded mode: lower the current scene into a packet and hand it to
    /// the present runtime. Credit is checked FIRST — a `NoCredit` return
    /// means no packet was built at all (`frame_sequence` does not
    /// advance). Returns `NoCredit` (with an error log) when the renderer
    /// is not in threaded mode.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn publish_frame(&mut self, width: u32, height: u32) -> PublishOutcome {
        // Fold in every early replay ack BEFORE lowering: the collect
        // inside `build_frame_packet` is where the bypass gate and
        // `feed_slots` are read, so an ack applied here serves the very
        // frame about to be planned — the same one-frame confirmation
        // latency the sync path has (see `PresentState::consume_packet`).
        self.drain_replay_acks();
        let PresentBackend::Threaded(handle) = &mut self.backend else {
            log::error!("publish_frame called without a threaded present runtime");
            return PublishOutcome::NoCredit;
        };
        if !handle.has_credit() {
            return PublishOutcome::NoCredit;
        }
        let replay_supported = handle
            .status()
            .replay_supported
            .load(std::sync::atomic::Ordering::Relaxed);
        let Some(mut packet) = self.frontend.build_frame_packet(
            width,
            height,
            replay_supported,
            self.renderer_epoch,
            self.surface_epoch,
        ) else {
            return PublishOutcome::NoGraph;
        };
        packet.recycled_confirmations = self.pending_recycled_confirmations.take();
        match handle.publish(packet) {
            Ok(()) => PublishOutcome::Published,
            Err(mut packet) => {
                // The runtime is gone (panic/shutdown race). Recover every
                // buffer the packet carried instead of leaking it.
                if let Some(confirmations) = packet.recycled_confirmations.take() {
                    self.pending_recycled_confirmations = Some(confirmations);
                }
                let mut returns = RenderReturns::default();
                let _ = GpuRenderer::cancel_packet(
                    *packet,
                    CancelReason::SurfaceUnavailable,
                    &mut returns,
                );
                if let Some(confirmations) = self.frontend.apply_returns(returns) {
                    self.stash_recycled_confirmations(confirmations);
                }
                log::error!("present runtime unavailable; frame recovered, not published");
                PublishOutcome::NoCredit
            }
        }
    }

    /// Threaded mode: fold every pending `RenderReturns` back into
    /// producer state (scene recycling, replay ack, planner re-queue of
    /// cancelled plans) and free the publish credit. Returns how many were
    /// drained. No-op outside threaded mode.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn drain_present_returns(&mut self) -> usize {
        self.drain_present_returns_with(&mut |_, _, _| {})
    }

    /// [`drain_present_returns`][Self::drain_present_returns], reporting
    /// each drained frame's id, outcome and present-thread timings — the
    /// Android loop feeds its frame telemetry from this.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn drain_present_returns_with(
        &mut self,
        on_return: &mut dyn FnMut(u64, PresentOutcome, PresentTimings),
    ) -> usize {
        // Acks first: a frame's early ack always precedes its returns, and
        // draining both here keeps the ack channel empty across idle
        // iterations that never reach `publish_frame`.
        self.drain_replay_acks();
        let mut drained = 0;
        loop {
            // Scoped so the handle borrow ends before `apply_returns`
            // needs the frontend through `&mut self`.
            let returns = {
                let PresentBackend::Threaded(handle) = &mut self.backend else {
                    break;
                };
                match handle.try_drain() {
                    Some(returns) => returns,
                    None => break,
                }
            };
            drained += 1;
            let frame_id = returns.frame_id;
            let outcome = returns.outcome;
            let timings = returns.timings;
            if let Some(confirmations) = self.frontend.apply_returns(returns) {
                self.stash_recycled_confirmations(confirmations);
            }
            on_return(frame_id, outcome, timings);
        }
        drained
    }

    /// Threaded mode: install a (re)created surface on the present thread
    /// and wait for the acknowledgement. The caller must have bumped the
    /// surface epoch first (`note_surface_reconfigured`
    /// [Self::note_surface_reconfigured]) when the message invalidates
    /// in-flight packets; the message carries the current epoch.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn present_replace_surface(
        &mut self,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> bool {
        let surface_epoch = self.surface_epoch;
        let Some(handle) = self.present_handle_mut() else {
            log::error!("present_replace_surface called without a threaded present runtime");
            return false;
        };
        handle.send_control_and_wait(
            move |ack| PresentControl::ReplaceSurface {
                surface,
                config,
                surface_epoch,
                ack,
            },
            "replace surface",
        )
    }

    /// Threaded mode: reconfigure the present thread's surface (resize)
    /// and wait for the acknowledgement. Same epoch contract as
    /// [`present_replace_surface`][Self::present_replace_surface].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn present_reconfigure(&mut self, config: wgpu::SurfaceConfiguration) -> bool {
        let surface_epoch = self.surface_epoch;
        let Some(handle) = self.present_handle_mut() else {
            log::error!("present_reconfigure called without a threaded present runtime");
            return false;
        };
        handle.send_control_and_wait(
            move |ack| PresentControl::Reconfigure {
                config,
                surface_epoch,
                ack,
            },
            "reconfigure surface",
        )
    }

    /// Threaded mode: drop the present thread's surface (the window died;
    /// the renderer survives for the next one) and wait for the
    /// acknowledgement. Bump the epoch first so in-flight packets cancel.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn present_drop_surface(&mut self) -> bool {
        let Some(handle) = self.present_handle_mut() else {
            log::error!("present_drop_surface called without a threaded present runtime");
            return false;
        };
        handle.send_control_and_wait(|ack| PresentControl::DropSurface { ack }, "drop surface")
    }

    /// Threaded mode: drain outstanding returns, stop the present thread
    /// and join it. The renderer returns to the uninitialized state.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn shutdown_present_runtime(&mut self) {
        if matches!(self.backend, PresentBackend::Threaded(_)) {
            self.retire_live_backend();
        }
    }

    /// Test hook: attach the present runtime's offscreen surrogate target
    /// so packets render headlessly, and wait for the acknowledgement.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn present_attach_offscreen_for_tests(&mut self, width: u32, height: u32) -> bool {
        let Some(handle) = self.present_handle_mut() else {
            return false;
        };
        handle.send_control_and_wait(
            move |ack| PresentControl::AttachOffscreenTargetForTests { width, height, ack },
            "attach offscreen target",
        )
    }

    /// Test hook: send the offscreen-target attach WITHOUT waiting for
    /// the ack — the inline mode has no thread to ack until pumped.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn send_attach_offscreen_unacked_for_tests(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<std::sync::mpsc::Receiver<()>> {
        let handle = self.present_handle_mut()?;
        handle.send_control_unacked(move |ack| PresentControl::AttachOffscreenTargetForTests {
            width,
            height,
            ack,
        })
    }

    /// Test hook: send a `Reconfigure` WITHOUT waiting for the ack,
    /// returning the ack receiver — the inline protocol tests assert the
    /// invalidation-before-ack ordering with it.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn send_reconfigure_unacked_for_tests(
        &mut self,
        config: wgpu::SurfaceConfiguration,
    ) -> Option<std::sync::mpsc::Receiver<()>> {
        let surface_epoch = self.surface_epoch;
        let handle = self.present_handle_mut()?;
        handle.send_control_unacked(move |ack| PresentControl::Reconfigure {
            config,
            surface_epoch,
            ack,
        })
    }

    /// Test hook: send a `DropSurface` WITHOUT waiting for the ack.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn send_drop_surface_unacked_for_tests(&mut self) -> Option<std::sync::mpsc::Receiver<()>> {
        let handle = self.present_handle_mut()?;
        handle.send_control_unacked(|ack| PresentControl::DropSurface { ack })
    }

    /// The producer's monotone packet sequence: the `frame_id` stamped on
    /// the most recently lowered packet. After a `Published` outcome this
    /// is the published frame's id (the Android loop keys its telemetry on
    /// it); it also proves a `NoCredit` publish never lowered a frame.
    pub fn last_published_frame_id(&self) -> u64 {
        self.frontend.frame_sequence
    }

    /// Test hook: park a recycled-confirmations buffer with `capacity` as
    /// if a previous frame's ack had been drained — the capacity
    /// round-trip test's deterministic seed (a real confirmed capture
    /// needs the multi-frame verification heuristics).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn seed_recycled_confirmations_for_tests(&mut self, capacity: usize) {
        self.pending_recycled_confirmations = Some(Vec::with_capacity(capacity));
    }

    /// Test inspector: capacity of the parked recycled-confirmations
    /// buffer (`None` when nothing is parked — e.g. right after a publish
    /// carried it back to the store).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn pending_recycled_confirmations_capacity_for_tests(&self) -> Option<usize> {
        self.pending_recycled_confirmations
            .as_ref()
            .map(Vec::capacity)
    }

    /// Test inspector: the shared present-status snapshot's
    /// (needs_frame_warmup, replay_supported, presented_frames) triple.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn present_status_snapshot_for_tests(&self) -> Option<(bool, bool, u64)> {
        match &self.backend {
            PresentBackend::Threaded(handle) => {
                let status = handle.status();
                Some((
                    status
                        .needs_frame_warmup
                        .load(std::sync::atomic::Ordering::Relaxed),
                    status
                        .replay_supported
                        .load(std::sync::atomic::Ordering::Relaxed),
                    status
                        .presented_frames
                        .load(std::sync::atomic::Ordering::Relaxed),
                ))
            }
            _ => None,
        }
    }

    pub fn last_frame_stats(&self) -> Option<RenderStatsSnapshot> {
        self.sync_gpu_renderer()
            .and_then(GpuRenderer::last_frame_stats)
    }

    pub fn debug_cpu_allocation_stats(&self) -> DebugCpuAllocationStats {
        let mut stats = self
            .sync_gpu_renderer()
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
    /// Sync backend only (desktop/web reconfigure paths); the threaded
    /// runtime owns its device on the present thread.
    pub fn try_device(&self) -> Option<&wgpu::Device> {
        self.sync_gpu_renderer().map(|r| &*r.device)
    }

    /// Test/diagnostic view of the device-error sentry: lifetime
    /// uncaptured wgpu errors recorded on the sync renderer's device
    /// (`CRANPOSE_SURVIVE_GPU_ERRORS` kill switch).
    #[doc(hidden)]
    pub fn device_error_count_for_tests(&self) -> u64 {
        self.sync_gpu_renderer()
            .map(GpuRenderer::device_error_count)
            .unwrap_or(0)
    }

    /// Test/diagnostic view of the latched instanced-quad selection: `true`
    /// when the live GPU renderer's ordinary shape draws ride
    /// `vs_shape_instanced` (storage mode && `CRANPOSE_INSTANCED_QUADS` != 0
    /// at construction).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn instanced_quads_active(&self) -> bool {
        self.sync_gpu_renderer()
            .is_some_and(GpuRenderer::instanced_quads_active)
    }

    /// Test/diagnostic view of retained arc meshes: (slots holding a mesh,
    /// total live replay slots).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_slot_mesh_stats(&self) -> (usize, usize) {
        self.sync_gpu_renderer()
            .map(GpuRenderer::replay_slot_mesh_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the retained capture size gate, summed over
    /// live slots holding a mesh: (arc bands meshed, stroked-circle rim
    /// bands meshed, passthrough quads).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_slot_mesh_engagement(&self) -> (usize, usize, usize) {
        self.sync_gpu_renderer()
            .map(GpuRenderer::replay_slot_mesh_engagement)
            .unwrap_or((0, 0, 0))
    }

    /// Test/diagnostic view of the retained bundle cache: lifetime
    /// (rebuilds, cached executes).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn retained_bundle_stats(&self) -> (u64, u64) {
        self.sync_gpu_renderer()
            .map(GpuRenderer::retained_bundle_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the transient rim mesh path: lifetime count
    /// of dynamic circle rims drawn as band meshes instead of full bounding
    /// quads (`CRANPOSE_RIM_MESH` kill switch).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn rim_meshes_emitted(&self) -> u64 {
        self.sync_gpu_renderer()
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
        self.sync_gpu_renderer()
            .map(GpuRenderer::static_span_stats)
            .unwrap_or((0, 0))
    }

    /// Test/diagnostic view of the retained-segment surface cache
    /// (`CRANPOSE_SEGMENT_SURFACE` opt-in): lifetime (captures, composite
    /// draws, dirty recaptures, churn rejections, economics rejections).
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn segment_surface_stats(&self) -> (u64, u64, u64, u64, u64) {
        self.sync_gpu_renderer()
            .map(GpuRenderer::segment_surface_stats)
            .unwrap_or((0, 0, 0, 0, 0))
    }

    /// Test/diagnostic view of the present store's lifetime count of
    /// replay-ops batches dropped by the generation check. Surface (non
    /// direct) frames must never move it: their packets carry the default
    /// plan, which the consume gate never feeds to the store.
    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn replay_generation_drops_for_tests(&self) -> u64 {
        self.sync_gpu_renderer()
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
        self.sync_gpu_renderer_mut()
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
        let replay_supported = match &self.backend {
            PresentBackend::Sync(gpu_renderer) => gpu_renderer.replay_supported(),
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => handle
                .status()
                .replay_supported
                .load(std::sync::atomic::Ordering::Relaxed),
            PresentBackend::None => false,
        };
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
        let PresentBackend::Sync(gpu_renderer) = &mut self.backend else {
            return Err(WgpuRendererError::Wgpu(
                "GPU renderer not initialized for synchronous rendering. Call init_gpu() first."
                    .to_string(),
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
        !self.frontend.retained_direct_scenes.is_empty()
    }
}

/// Opaque handle to a built-but-unrendered frame packet, for the
/// cancellation-protocol tests only.
#[doc(hidden)]
pub struct HeldFramePacket(frame_packet::FramePacket);

/// The present runtime's state machine WITHOUT its thread, handed out by
/// [`WgpuRenderer::init_gpu_inline_for_tests`]: the protocol tests pump
/// the message queue by hand and inspect present-side state directly.
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub struct InlinePresentRuntime {
    state: PresentState,
    msg_rx: std::sync::mpsc::Receiver<PresentMsg>,
    shutdown_seen: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl InlinePresentRuntime {
    /// Process every pending message exactly like one wakeful pass of the
    /// present thread's loop (controls drained against the waiting packet
    /// first, then the packet consumes). Returns `false` once `Shutdown`
    /// has been processed.
    pub fn pump(&mut self) -> bool {
        if self.shutdown_seen {
            return false;
        }
        while let Ok(msg) = self.msg_rx.try_recv() {
            if !self.state.run_once(msg) {
                self.shutdown_seen = true;
                return false;
            }
        }
        self.state.consume_waiting();
        true
    }

    /// Whether a packet sits unconsumed in the depth-one slot (only
    /// observable between a partial pump's steps; `pump` always consumes).
    pub fn has_waiting_packet(&self) -> bool {
        self.state.has_waiting_packet()
    }

    /// Receive and process exactly one queued message (no packet
    /// consumption) — for tests that need to interleave control against a
    /// waiting packet.
    pub fn step_one_message(&mut self) -> bool {
        if self.shutdown_seen {
            return false;
        }
        match self.msg_rx.try_recv() {
            Ok(msg) => {
                if !self.state.run_once(msg) {
                    self.shutdown_seen = true;
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Consume the waiting packet, if any, exactly like the thread loop's
    /// idle branch.
    pub fn consume_waiting(&mut self) {
        self.state.consume_waiting();
    }

    /// Store-side ack confirmations buffer capacity — the threaded
    /// capacity round-trip's observable end state.
    pub fn store_ack_confirmations_capacity(&self) -> usize {
        self.state.store_ack_confirmations_capacity()
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
        self.frontend.layer_surface_requirements_cache.clear();
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
        self.frontend.layer_surface_requirements_cache.clear();
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
        self.update_scene_and_plan_memo(applier, root, dirty_nodes, true);
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
        self.update_scene_and_plan_memo(applier, root, dirty_nodes, false);
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
        match &self.backend {
            PresentBackend::Sync(gpu_renderer) => gpu_renderer.needs_frame_warmup(),
            // The producer reads this every loop iteration; in threaded
            // mode it is the present thread's atomic snapshot, never the
            // renderer itself.
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => handle
                .status()
                .needs_frame_warmup
                .load(std::sync::atomic::Ordering::Relaxed),
            PresentBackend::None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use cranpose_render_common::graph::RenderNode;
    use cranpose_ui_graphics::GraphicsLayer;

    use super::*;
    use crate::pipeline::TextLayoutResolver;

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

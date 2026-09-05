//! WGPU renderer backend for GPU-accelerated 2D rendering.
//!
//! This renderer uses WGPU for cross-platform GPU support across
//! desktop (Windows/Mac/Linux), web (WebGPU), and mobile Android.

pub(crate) use cranpose_render_common::debug_toggles;
pub use debug_toggles::{
    DebugToggle, debug_toggle, debug_toggle_os, set_debug_toggle, set_debug_toggle_os,
};
pub use offscreen::composition_bytes_per_pixel;
pub use render::presentable_root_usages;
mod capture_hash;
mod collect;
mod draw_pass;
mod effect_renderer;
mod fast_cores;
mod frame;
mod geometry;
mod layer_cache;
pub use fast_cores::pin_current_thread_to_fast_cores;
mod frame_graph;
mod frame_packet;
mod frontend;
pub(crate) mod gpu_stats;
mod initial_present;
mod lazy_resource;
mod offscreen;
mod opaque_prefix;
mod output_conversion;
pub(crate) mod pass_timing;
mod pipeline;
#[cfg(not(target_arch = "wasm32"))]
mod pipeline_disk_cache;
#[cfg(not(target_arch = "wasm32"))]
mod present_runtime;
mod record_columns;
mod render;
mod run_geometry;
mod run_store;
mod scene;
mod shader_cache;
mod shaders;
#[cfg(test)]
mod test_support;

use std::{rc::Rc, sync::Arc};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    RenderScene, Renderer,
    graph::RenderGraph,
    software_text_raster::{
        SoftwareTextFontSet, SoftwareTextMeasurer, software_text_font_set_from_fonts_or_default,
    },
};
use cranpose_ui::{LayoutTree, TextMeasurer};
use cranpose_ui_graphics::{Rect, Size};
pub use frame_packet::PresentTimings;
use frame_packet::RenderReturns;
#[doc(hidden)]
pub use frame_packet::{CancelReason, PresentOutcome};
use frontend::{DevOverlayCache, RendererFrontend};
pub use gpu_stats::FrameStatsSnapshot as RenderStatsSnapshot;
pub use initial_present::clear_to_default_background;
pub use pass_timing::{GpuPassTimingEntry, GpuPassTimingReport};
#[cfg(not(target_arch = "wasm32"))]
use present_runtime::{
    PresentControl, PresentHandle, PresentMsg, PresentRuntimeInit, PresentState,
};
use render::GpuRenderer;
pub use render::frames_presented;
pub use scene::{ClickAction, HitRegion, Scene};

/// The optional device features the renderer exploits when the adapter
/// offers them: pipeline caching (see `pipeline_disk_cache`) and the
/// timestamp queries behind `CRANPOSE_GPU_PASS_TIMING`. Every platform's
/// `request_device` passes this so a profiling toggle never needs a rebuilt
/// binary; intersecting with the adapter's own features keeps the request
/// valid on adapters without them.
pub fn optional_device_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & (wgpu::Features::PIPELINE_CACHE | wgpu::Features::TIMESTAMP_QUERY)
}

#[doc(hidden)]
pub fn offscreen_render_target_for_tests(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = offscreen::create_2d_texture(
        device,
        wgpu::TextureFormat::Rgba8Unorm,
        width,
        height,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        Some(label),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

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
    pub image_texture_cache_len: usize,
    pub image_texture_cache_cap: usize,
    pub run_arena_staging_bytes: usize,
    pub run_store_bytes: usize,
    pub run_store_runs: usize,
    pub scratch_image_vertices_cap: usize,
    pub scratch_image_indices_cap: usize,
    pub scratch_image_cmds_cap: usize,
    pub layer_cache_len: usize,
    pub layer_cache_bytes: u64,
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

enum PresentBackend {
    None,
    Sync(Box<GpuRenderer>),
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
    frontend: RendererFrontend,
    backend: PresentBackend,
    renderer_epoch: u64,
    surface_epoch: u64,
}

impl WgpuRenderer {
    fn update_scene(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        dirty_nodes: &[NodeId],
        refresh_hits: bool,
    ) {
        let mut changed_nodes = std::mem::take(&mut self.frontend.changed_nodes);
        pipeline::update_from_applier(
            applier,
            root,
            &mut self.frontend.scene,
            1.0,
            dirty_nodes,
            refresh_hits,
            &mut changed_nodes,
        );
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
            renderer_epoch: 0,
            surface_epoch: 0,
        }
    }

    fn sync_gpu_renderer(&self) -> Option<&GpuRenderer> {
        match &self.backend {
            PresentBackend::Sync(gpu_renderer) => Some(gpu_renderer.as_ref()),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn present_handle_mut(&mut self) -> Option<&mut PresentHandle> {
        match &mut self.backend {
            PresentBackend::Threaded(handle) => Some(handle),
            _ => None,
        }
    }

    fn retire_live_backend(&mut self) {
        #[allow(unused_mut)]
        let mut backend = std::mem::replace(&mut self.backend, PresentBackend::None);
        #[cfg(not(target_arch = "wasm32"))]
        if let PresentBackend::Threaded(handle) = &mut backend {
            while let Some(returns) = handle.try_drain() {
                self.frontend.apply_returns(returns);
            }
            handle.shutdown();
        }
        drop(backend);
    }

    /// Initialize GPU resources with a WGPU device and queue.
    ///
    /// Replacing a live renderer (Android surface recreation, device loss)
    /// bumps the renderer epoch, so a packet built against the previous
    /// renderer is cancelled instead of drawn.
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
        self.backend = PresentBackend::Sync(Box::new(GpuRenderer::new(
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            self.frontend.text_fonts.clone(),
            self.renderer_epoch,
        )));
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
        let init = PresentRuntimeInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts: self.frontend.text_fonts.clone(),
            renderer_epoch: self.renderer_epoch,
            clock,
        };
        let handle = PresentHandle::spawn(init, waker).map_err(WgpuRendererError::Wgpu)?;
        self.backend = PresentBackend::Threaded(handle);
        Ok(())
    }

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
        let init = PresentRuntimeInit {
            device,
            queue,
            surface_format,
            adapter_backend,
            adapter_downlevel,
            text_fonts: self.frontend.text_fonts.clone(),
            renderer_epoch: self.renderer_epoch,
            clock: None,
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
    /// Producer first, present second: the frontend collects the frame into
    /// a `frame_packet::FramePacket` (root and dev overlay alike), the GPU
    /// renderer consumes it, and the present stage's returns fold back into
    /// the frontend afterwards.
    pub fn render(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        self.render_frame(texture, view, width, height)
    }

    /// Renders the frame into a presentable image. When the image carries the
    /// composition format and the capture usages, the scene renders into it
    /// directly and no output conversion pass runs.
    pub fn render_surface_texture(
        &mut self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), WgpuRendererError> {
        self.render_frame(texture, view, width, height)
    }

    fn render_frame(
        &mut self,
        texture: &wgpu::Texture,
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
            .build_frame_packet(width, height, self.renderer_epoch, self.surface_epoch)
            .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
        let mut returns = RenderReturns::default();
        let result = gpu_renderer.render(
            texture,
            view,
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        self.frontend.apply_returns(returns);
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
                self.renderer_epoch,
                self.surface_epoch,
            )
            .ok_or_else(|| WgpuRendererError::Wgpu("scene graph is missing".to_string()))?;
        let mut returns = RenderReturns::default();
        let result = gpu_renderer.render_to_rgba_pixels(
            width,
            height,
            packet,
            self.surface_epoch,
            &mut returns,
        );
        self.frontend.apply_returns(returns);
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
        let PresentBackend::Threaded(handle) = &mut self.backend else {
            log::error!("publish_frame called without a threaded present runtime");
            return PublishOutcome::NoCredit;
        };
        if !handle.has_credit() {
            return PublishOutcome::NoCredit;
        }
        let Some(packet) = self.frontend.build_frame_packet(
            width,
            height,
            self.renderer_epoch,
            self.surface_epoch,
        ) else {
            return PublishOutcome::NoGraph;
        };
        match handle.publish(packet) {
            Ok(()) => PublishOutcome::Published,
            Err(packet) => {
                let mut returns = RenderReturns::default();
                let _ = GpuRenderer::cancel_packet(
                    *packet,
                    CancelReason::SurfaceUnavailable,
                    &mut returns,
                );
                self.frontend.apply_returns(returns);
                log::error!("present runtime unavailable; frame recovered, not published");
                PublishOutcome::NoCredit
            }
        }
    }

    /// Threaded mode: fold every pending `RenderReturns` back into
    /// producer state and free the publish credit. Returns how many were
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
        let mut drained = 0;
        loop {
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
            self.frontend.apply_returns(returns);
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

    #[cfg(not(target_arch = "wasm32"))]
    #[doc(hidden)]
    pub fn present_status_snapshot_for_tests(&self) -> Option<(bool, u64, u64)> {
        match &self.backend {
            PresentBackend::Threaded(handle) => {
                let status = handle.status();
                Some((
                    status
                        .needs_frame_warmup
                        .load(std::sync::atomic::Ordering::Relaxed),
                    status
                        .presented_frames
                        .load(std::sync::atomic::Ordering::Relaxed),
                    status
                        .placeholder_frames
                        .load(std::sync::atomic::Ordering::Relaxed),
                ))
            }
            _ => None,
        }
    }

    pub fn last_frame_stats(&self) -> Option<RenderStatsSnapshot> {
        match &self.backend {
            PresentBackend::Sync(gpu_renderer) => gpu_renderer.last_frame_stats(),
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => *handle
                .status()
                .last_frame_stats
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            PresentBackend::None => None,
        }
    }

    /// GPU milliseconds by pass label, aggregated since the last `[GPU-PASS]`
    /// print. Empty unless `CRANPOSE_GPU_PASS_TIMING` armed pass timing on a
    /// device with [`wgpu::Features::TIMESTAMP_QUERY`].
    pub fn gpu_pass_timings(&self) -> GpuPassTimingReport {
        self.sync_gpu_renderer()
            .map(GpuRenderer::gpu_pass_timings)
            .unwrap_or_default()
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
        stats
    }

    /// Return the WGPU device when GPU resources are initialized.
    /// Sync backend only (desktop/web reconfigure paths); the threaded
    /// runtime owns its device on the present thread.
    pub fn try_device(&self) -> Option<&wgpu::Device> {
        self.sync_gpu_renderer().map(|r| &*r.device)
    }

    #[doc(hidden)]
    pub fn try_queue_for_tests(&self) -> Option<&wgpu::Queue> {
        self.sync_gpu_renderer().map(|r| &*r.queue)
    }

    #[doc(hidden)]
    pub fn device_error_count_for_tests(&self) -> u64 {
        self.sync_gpu_renderer()
            .map(GpuRenderer::device_error_count)
            .unwrap_or(0)
    }

    #[doc(hidden)]
    pub fn build_frame_packet_for_tests(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<HeldFramePacket> {
        self.frontend
            .build_frame_packet(width, height, self.renderer_epoch, self.surface_epoch)
            .map(HeldFramePacket)
    }

    #[doc(hidden)]
    pub fn render_held_packet_for_tests(
        &mut self,
        texture: &wgpu::Texture,
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
        let mut returns = RenderReturns::default();
        let result = gpu_renderer.render(
            texture,
            view,
            width,
            height,
            packet.0,
            self.surface_epoch,
            &mut returns,
        );
        let outcome = returns.outcome;
        self.frontend.apply_returns(returns);
        result.map_err(WgpuRendererError::Wgpu)?;
        Ok(outcome)
    }
}

#[doc(hidden)]
pub struct HeldFramePacket(frame_packet::FramePacket);

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub struct InlinePresentRuntime {
    state: PresentState,
    msg_rx: std::sync::mpsc::Receiver<PresentMsg>,
    shutdown_seen: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl InlinePresentRuntime {
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

    pub fn has_waiting_packet(&self) -> bool {
        self.state.has_waiting_packet()
    }

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

    pub fn consume_waiting(&mut self) {
        self.state.consume_waiting();
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
        self.frontend.dev_overlay_graph = None;
        self.frontend.dev_overlay_cache = None;
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
        self.update_scene(applier, root, dirty_nodes, true);
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
        self.update_scene(applier, root, dirty_nodes, false);
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

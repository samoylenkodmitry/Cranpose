//! The one deliberately trivial GPU operation every platform's surface
//! installation needs before a single content pipeline can exist: clear a
//! render target to the framework's own default frame background
//! ([`crate::render::CLEAR_COLOR`], the same base every real frame clears
//! to underneath its own content) and submit it.
//!
//! A render pass with a `Clear` load op and zero draw calls binds no
//! pipeline at all, so [`clear_to_default_background`] is safe to call the
//! instant a device and a texture view exist — before a single shape or
//! glyph `PassPipeline` has finished compiling, and independent of whether
//! this device was ever granted a `wgpu::PipelineCache`.
//!
//! Every platform's surface-installation code (desktop and web in
//! `wgpu_surface::present_initial_placeholder_frame`, Android's threaded
//! present runtime in `present_runtime::PresentState::present_placeholder_frame`)
//! calls this immediately after `wgpu::Surface::configure`, before the
//! app's first real content frame can possibly be ready. Without it, the
//! compositor shows whatever the swapchain held right after `configure` —
//! undefined memory, or nothing — for however long the first frame's
//! pipelines take to compile; on a slow driver that is the black screen
//! this exists to close out.

use crate::{
    frame_graph::{WgpuFrameGraph, WgpuFrameGraphExecutor},
    render::CLEAR_COLOR,
};

/// Clears `view` to the framework's default background and submits the
/// work, through the same [`WgpuFrameGraph`] every other command encoder
/// and submission in this crate is required to go through (enforced by
/// `render_contract.rs`'s `wgpu_command_buffers_are_owned_by_frame_graph_executor`)
/// — a fresh, one-shot [`WgpuFrameGraphExecutor`], since a placeholder
/// clear has no frame-to-frame state worth pooling. Does not present or
/// acquire anything itself — callers that mean to show the clear on
/// screen still acquire a frame and call `wgpu::SurfaceTexture::present`
/// themselves, exactly as a real content frame would.
pub fn clear_to_default_background(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
) {
    let mut graph = WgpuFrameGraph::new(Some("Cranpose Initial Present Clear"));
    let target = graph.import_surface("initial-present-clear-target");
    graph.add_fallible_command_pass(Some("Initial Present Clear"), &[], &[target], |context| {
        let _pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Cranpose Initial Present Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        Ok(())
    });
    if let Err(error) = WgpuFrameGraphExecutor::new().execute_recorded_graph(device, queue, graph) {
        log::error!("[initial-present] placeholder clear failed: {error:?}");
    }
}

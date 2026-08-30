use crate::{
    frame_graph::{WgpuFrameGraph, WgpuFrameGraphExecutor},
    render::CLEAR_COLOR,
};

/// Clears `view` to the framework's default background and submits the
/// work, through the same `WgpuFrameGraph` every other command encoder
/// and submission in this crate is required to go through (enforced by
/// `render_contract.rs`'s `wgpu_command_buffers_are_owned_by_frame_graph_executor`)
/// — a fresh, one-shot `WgpuFrameGraphExecutor`, since a placeholder
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

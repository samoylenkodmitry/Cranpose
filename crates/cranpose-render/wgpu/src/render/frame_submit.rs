use std::sync::Arc;

use super::GpuRendererInit;
#[cfg(target_arch = "wasm32")]
use crate::frame_graph::WgpuFrameEncoder;
use crate::frame_graph::{
    FrameGraphError, FrameGraphExecution, WgpuFrameGraph, WgpuFrameGraphExecutor,
};

// The executor that records and submits a renderer's frame graphs, with the
// device and the queue it uses. Its owner keeps it beside the renderer, not in
// it, because a frame's passes borrow the renderer while the executor runs.
pub(crate) struct FrameSubmit {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pub(crate) executor: WgpuFrameGraphExecutor,
}

impl FrameSubmit {
    pub(crate) fn new(init: &GpuRendererInit) -> Self {
        let mut executor = WgpuFrameGraphExecutor::new();
        executor.init_pass_timing(&init.device, &init.queue);
        #[cfg(target_arch = "wasm32")]
        executor.batch_queue_writes(init.adapter_backend == wgpu::Backend::BrowserWebGpu);
        Self {
            device: Arc::clone(&init.device),
            queue: Arc::clone(&init.queue),
            executor,
        }
    }

    pub(crate) fn execute(
        &mut self,
        graph: WgpuFrameGraph<'_>,
    ) -> Result<FrameGraphExecution, FrameGraphError> {
        self.executor
            .execute_recorded_graph(&self.device, &self.queue, graph)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn begin(&mut self, label: Option<&'static str>) -> WgpuFrameEncoder<'_> {
        self.executor.begin(&self.device, &self.queue, label)
    }

    pub(crate) fn end_pass_timing_frame(&self) {
        self.executor
            .end_pass_timing_frame(&self.device, &self.queue);
    }
}

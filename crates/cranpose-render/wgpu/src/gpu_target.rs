use std::sync::Arc;

/// The device a renderer draws with, the surface format it presents and what
/// the adapter behind the device supports.
#[derive(Clone)]
pub struct GpuTarget {
    /// The device every GPU resource is created on.
    pub device: Arc<wgpu::Device>,
    /// The queue frames are submitted to.
    pub queue: Arc<wgpu::Queue>,
    /// The format of the surface frames are presented to.
    pub surface_format: wgpu::TextureFormat,
    /// The graphics API the adapter runs on.
    pub adapter_backend: wgpu::Backend,
    /// What the adapter supports below the full WebGPU feature set.
    pub adapter_downlevel: wgpu::DownlevelFlags,
}

impl GpuTarget {
    /// A target on a device of `adapter`, presenting to a surface of
    /// `surface_format`.
    pub fn on_adapter(
        adapter: &wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        Self {
            device,
            queue,
            surface_format,
            adapter_backend: adapter.get_info().backend,
            adapter_downlevel: adapter.get_downlevel_capabilities().flags,
        }
    }
}

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

/// The optional device features the renderer exploits when the adapter
/// offers them: pipeline caching (see `pipeline_disk_cache`), the
/// timestamp queries behind `CRANPOSE_GPU_PASS_TIMING`, and, on a GPU
/// that shares the host's memory, vertex and uniform buffers the CPU
/// writes directly, so a frame's uploads need no copy. Every platform's
/// `request_device` passes this so a profiling toggle never needs a rebuilt
/// binary; intersecting with the adapter's own features keeps the request
/// valid on adapters without them.
pub fn optional_device_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    let mut wanted = wgpu::Features::PIPELINE_CACHE | wgpu::Features::TIMESTAMP_QUERY;
    if matches!(
        adapter.get_info().device_type,
        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::Cpu
    ) {
        wanted |= wgpu::Features::MAPPABLE_PRIMARY_BUFFERS;
    }
    adapter.features() & wanted
}

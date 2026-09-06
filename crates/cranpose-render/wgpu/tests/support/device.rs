use std::sync::Arc;

use cranpose_render_wgpu::WgpuRenderer;

pub struct HeadlessDevice {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    backend: wgpu::Backend,
    downlevel: wgpu::DownlevelFlags,
}

impl HeadlessDevice {
    pub fn request(
        backends: wgpu::Backends,
        limits: wgpu::Limits,
        label: &str,
    ) -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|err| format!("adapter request failed: {err:?}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(label),
            required_features: cranpose_render_wgpu::optional_device_features(&adapter),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| format!("device request failed: {err:?}"))?;
        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            backend: adapter.get_info().backend,
            downlevel: adapter.get_downlevel_capabilities().flags,
        })
    }

    pub fn attach(self, renderer: &mut WgpuRenderer, format: wgpu::TextureFormat) {
        renderer.init_gpu(
            self.device,
            self.queue,
            format,
            self.backend,
            self.downlevel,
        );
    }
}

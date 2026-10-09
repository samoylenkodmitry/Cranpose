use std::sync::{Arc, Mutex, PoisonError};

use cranpose_render_wgpu::WgpuRenderer;

/// The test process's one wgpu instance for `backends`.
///
/// Test threads that create and drop Vulkan instances at once race inside
/// the loader, which loads and unloads driver libraries per instance: with a
/// driver that fails to load, a parallel run jumped to a null function
/// pointer in `vkEnumerateInstanceExtensionProperties` (#859). One instance
/// per backend set, kept for the life of the process, scans the drivers once.
fn shared_instance(backends: wgpu::Backends) -> wgpu::Instance {
    static INSTANCES: Mutex<Vec<(wgpu::Backends, wgpu::Instance)>> = Mutex::new(Vec::new());
    let mut instances = INSTANCES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, instance)) = instances.iter().find(|(held, _)| *held == backends) {
        return instance.clone();
    }
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backends;
    let instance = wgpu::Instance::new(descriptor);
    instances.push((backends, instance.clone()));
    instance
}

pub fn headless_adapter(backends: wgpu::Backends) -> Result<wgpu::Adapter, String> {
    let instance = shared_instance(backends);
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        ..wgpu::RequestAdapterOptions::default()
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))
}

/// Where a test renderer compiles its pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pipelines {
    /// On the background compiler, as an app does.
    Background,
    /// Where each is first needed.
    Inline,
}

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
        Self::request_without_features(backends, limits, label, wgpu::Features::empty())
    }

    /// A device without `features` of those the renderer asks for: without
    /// `MAPPABLE_PRIMARY_BUFFERS` it copies its uploads as a discrete GPU
    /// does.
    pub fn request_without_features(
        backends: wgpu::Backends,
        limits: wgpu::Limits,
        label: &str,
        features: wgpu::Features,
    ) -> Result<Self, String> {
        let adapter = headless_adapter(backends)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(label),
            required_features: cranpose_render_wgpu::optional_device_features(&adapter) - features,
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

    pub fn without(mut self, flags: wgpu::DownlevelFlags) -> Self {
        self.downlevel.remove(flags);
        self
    }

    /// Gives `renderer` this device. With [`Pipelines::Inline`] it compiles
    /// every pipeline where first needed, so its frames never draw with a
    /// stand-in: what a reference renderer needs.
    pub fn attach(
        self,
        renderer: &mut WgpuRenderer,
        format: wgpu::TextureFormat,
        pipelines: Pipelines,
    ) {
        match pipelines {
            Pipelines::Background => renderer.init_gpu(
                self.device,
                self.queue,
                format,
                self.backend,
                self.downlevel,
            ),
            Pipelines::Inline => renderer.init_gpu_compiling_inline_for_tests(
                self.device,
                self.queue,
                format,
                self.backend,
                self.downlevel,
            ),
        }
    }
}

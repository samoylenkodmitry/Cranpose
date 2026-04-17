use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard};

use cranpose_render_wgpu::WgpuRenderer;

pub static TEST_FONT: &[u8] =
    include_bytes!("../../../../apps/desktop-demo/assets/NotoSansMerged.ttf");

static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

pub struct LockedRenderer {
    _lock: MutexGuard<'static, ()>,
    renderer: WgpuRenderer,
}

impl Deref for LockedRenderer {
    type Target = WgpuRenderer;

    fn deref(&self) -> &Self::Target {
        &self.renderer
    }
}

impl DerefMut for LockedRenderer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.renderer
    }
}

pub fn headless_renderer() -> Result<LockedRenderer, String> {
    let lock = GPU_TEST_LOCK.lock().expect("GPU test lock poisoned");
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Shared Render Contract Test Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| format!("device request failed: {err:?}"))?;

    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Bgra8UnormSrgb,
        adapter.get_info().backend,
    );
    Ok(LockedRenderer {
        _lock: lock,
        renderer,
    })
}

use std::sync::LazyLock;

pub(crate) static OFFSCREEN_INSTANCE: LazyLock<wgpu::Instance> = LazyLock::new(|| {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::all();
    wgpu::Instance::new(descriptor)
});

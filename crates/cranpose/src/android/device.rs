// The device Cranpose asks an Android adapter for.
pub(crate) fn android_device(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        // Display timing lets the frame pacer see when frames reach the
        // screen; see `android_display_timing`.
        required_features: cranpose_render_wgpu::optional_device_features(adapter)
            | (adapter.features() & wgpu::Features::VULKAN_GOOGLE_DISPLAY_TIMING),
        required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: crate::gpu_limits::mobile_memory_hints(),
        trace: wgpu::Trace::Off,
    }
}

// Opens the device that Cranpose asks `adapter` for. An app built with the `video` feature opens
// it with the extensions that import a decoder's hardware buffers, where the adapter has them.
pub(crate) fn request_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let descriptor = android_device(adapter);
    #[cfg(feature = "video")]
    if let Some(device) = super::hardware_buffer_device::request_device(adapter, &descriptor) {
        return Ok(device);
    }
    pollster::block_on(adapter.request_device(&descriptor))
}

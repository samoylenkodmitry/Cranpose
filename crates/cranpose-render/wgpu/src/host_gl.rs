// Whether the device draws on a GL context that its host made and keeps current on its own
// thread, as an OpenXR app does with the context its session runs on. GL takes that context's
// calls on that thread only, so its pipelines compile there too: a compiler thread would call GL
// with no context current.
#[cfg(all(
    feature = "backend-gles",
    not(target_arch = "wasm32"),
    not(target_vendor = "apple")
))]
pub(crate) fn on_host_gl_context(device: &wgpu::Device) -> bool {
    #[expect(
        unsafe_code,
        reason = "wgpu hands out its GL device only through as_hal"
    )]
    // SAFETY: the GL device is only asked whether wgpu owns its context.
    let device = unsafe { device.as_hal::<wgpu::hal::api::Gles>() };
    device.is_some_and(|device| !device.context().is_owned())
}

#[cfg(not(all(
    feature = "backend-gles",
    not(target_arch = "wasm32"),
    not(target_vendor = "apple")
)))]
pub(crate) fn on_host_gl_context(_device: &wgpu::Device) -> bool {
    false
}

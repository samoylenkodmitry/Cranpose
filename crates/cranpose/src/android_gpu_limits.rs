//! Device-limit policy for the Android GPU device request.
//!
//! Host-compilable so the policy stays unit-tested in regular CI even though
//! the Android runtime module only builds for `target_os = "android"`.

/// Device limits for the Android GPU device request.
///
/// `downlevel_defaults()` caps uniform bindings at 16 KiB and
/// `using_resolution()` only raises texture limits, never buffer limits, while
/// the renderer's desktop-sized shape batch uniform needs up to 60 KiB. Request
/// the uniform binding size the adapter actually supports, up to the regular
/// desktop default; the renderer derives its batch capacities from whatever is
/// granted, so true 16 KiB-minimum devices still work with smaller batches.
pub(crate) fn android_device_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter_limits.clone());
    limits.max_uniform_buffer_binding_size = adapter_limits
        .max_uniform_buffer_binding_size
        .min(wgpu::Limits::default().max_uniform_buffer_binding_size);
    limits
}

#[cfg(test)]
mod tests {
    use super::android_device_limits;

    #[test]
    fn uniform_binding_size_follows_adapter_up_to_desktop_default() {
        let desktop_binding = wgpu::Limits::default().max_uniform_buffer_binding_size;

        // A capable adapter grants the full desktop-sized binding so shape
        // batches keep their desktop capacity (the 0.1.13 Android crash was
        // requesting only the 16 KiB downlevel cap on such devices).
        let capable = android_device_limits(wgpu::Limits::default());
        assert_eq!(capable.max_uniform_buffer_binding_size, desktop_binding);

        // An adapter at the spec minimum is never asked for more than it has.
        let mut minimal = wgpu::Limits::downlevel_defaults();
        minimal.max_uniform_buffer_binding_size = 16384;
        let limits = android_device_limits(minimal);
        assert_eq!(limits.max_uniform_buffer_binding_size, 16384);

        // Texture resolution still follows the adapter as before.
        let big_textures = wgpu::Limits {
            max_texture_dimension_2d: 16384,
            ..wgpu::Limits::default()
        };
        let limits = android_device_limits(big_textures);
        assert_eq!(limits.max_texture_dimension_2d, 16384);
    }
}

//! Device-limit and memory policy for mobile GPU device requests (Android and
//! iOS).
//!
//! Host-compilable so the policy stays unit-tested in regular CI even though
//! the mobile runtime modules only build for `target_os = "android"` / `"ios"`.

/// Allocation strategy for a mobile GPU device request.
///
/// `MemoryHints::default()` is `Performance`, which tells wgpu's Vulkan backend
/// to sub-allocate resources out of memory blocks of 128 MiB (device-local) or
/// 64 MiB (host-visible), growing to 256 MiB / 128 MiB. Desktop GPUs have the
/// headroom for that; phones and watches do not, and on them the choice is
/// worse than it looks. Mobile GPUs have unified memory, so *every* Vulkan
/// memory type is `HOST_VISIBLE` — `gpu-allocator` therefore treats even the
/// device-local heap as host memory and reserves the 64 MiB block for the very
/// first allocation. That first allocation is wgpu's own 512 KiB zero-init
/// buffer, created inside `request_device`, so the process gives up 64 MiB of
/// GPU memory (and, on 32-bit Wear OS, 64 MiB of address space) before the
/// renderer has drawn anything. It never shrinks, and it does not scale with
/// the screen: a 454x454 watch face paid exactly what a 1280x2856 phone did.
///
/// `MemoryUsage` asks for 4 MiB host / 8 MiB device blocks growing to 32 / 64
/// MiB, which is a far better match for a renderer whose live GPU footprint is
/// a few megabytes. The cost is more `VkDeviceMemory` objects on a workload
/// that does grow past the first block — bounded by the doubling in
/// `gpu-allocator`, and cheap next to reserving 60 MiB that nothing will use.
///
/// Backends that do not sub-allocate (Metal, GLES) ignore the hint.
pub(crate) fn mobile_memory_hints() -> wgpu::MemoryHints {
    wgpu::MemoryHints::MemoryUsage
}

/// Device limits for a mobile GPU device request.
///
/// `Limits::default()` requests caps that mobile/simulator Metal cannot grant
/// (for example `max_inter_stage_shader_variables: 16`, where the iOS Simulator
/// allows 15), so start from `downlevel_defaults()`.
///
/// `downlevel_defaults()` caps uniform bindings at 16 KiB and
/// `using_resolution()` only raises texture limits, never buffer limits, while
/// the renderer's desktop-sized shape batch uniform needs up to 60 KiB. Request
/// the uniform binding size the adapter actually supports, up to the regular
/// desktop default; the renderer derives its batch capacities from whatever is
/// granted, so true 16 KiB-minimum devices still work with smaller batches.
pub(crate) fn mobile_device_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter_limits.clone());
    limits.max_uniform_buffer_binding_size = adapter_limits
        .max_uniform_buffer_binding_size
        .min(wgpu::Limits::default().max_uniform_buffer_binding_size);
    // The renderer uses neither compute pipelines nor storage buffers (the
    // latter aren't available to WebGL fragment shaders, so the renderer avoids
    // them everywhere), but `downlevel_defaults` still requests both. Some
    // adapters — notably the Android emulator's GLES driver — report zero for
    // these limits, which fails device creation. Request only what the adapter
    // grants for the unused capabilities so the device is always created.
    limits.max_compute_workgroup_storage_size = adapter_limits.max_compute_workgroup_storage_size;
    limits.max_compute_invocations_per_workgroup =
        adapter_limits.max_compute_invocations_per_workgroup;
    limits.max_compute_workgroup_size_x = adapter_limits.max_compute_workgroup_size_x;
    limits.max_compute_workgroup_size_y = adapter_limits.max_compute_workgroup_size_y;
    limits.max_compute_workgroup_size_z = adapter_limits.max_compute_workgroup_size_z;
    limits.max_compute_workgroups_per_dimension =
        adapter_limits.max_compute_workgroups_per_dimension;
    limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;
    limits.max_storage_buffers_per_shader_stage =
        adapter_limits.max_storage_buffers_per_shader_stage;
    limits.max_storage_textures_per_shader_stage =
        adapter_limits.max_storage_textures_per_shader_stage;
    limits.max_dynamic_storage_buffers_per_pipeline_layout =
        adapter_limits.max_dynamic_storage_buffers_per_pipeline_layout;
    limits
}

#[cfg(test)]
mod tests {
    use super::mobile_device_limits;

    #[test]
    fn uniform_binding_size_follows_adapter_up_to_desktop_default() {
        let desktop_binding = wgpu::Limits::default().max_uniform_buffer_binding_size;

        // A capable adapter grants the full desktop-sized binding so shape
        // batches keep their desktop capacity (the 0.1.13 Android crash was
        // requesting only the 16 KiB downlevel cap on such devices).
        let capable = mobile_device_limits(wgpu::Limits::default());
        assert_eq!(capable.max_uniform_buffer_binding_size, desktop_binding);

        // An adapter at the spec minimum is never asked for more than it has.
        let mut minimal = wgpu::Limits::downlevel_defaults();
        minimal.max_uniform_buffer_binding_size = 16384;
        let limits = mobile_device_limits(minimal);
        assert_eq!(limits.max_uniform_buffer_binding_size, 16384);

        // Texture resolution still follows the adapter as before.
        let big_textures = wgpu::Limits {
            max_texture_dimension_2d: 16384,
            ..wgpu::Limits::default()
        };
        let limits = mobile_device_limits(big_textures);
        assert_eq!(limits.max_texture_dimension_2d, 16384);
    }

    #[test]
    fn compute_limits_never_exceed_adapter() {
        // The Android emulator's GLES driver reports zero compute limits; the
        // renderer uses no compute, so request only what the adapter grants.
        let no_compute = wgpu::Limits {
            max_compute_workgroups_per_dimension: 0,
            max_compute_invocations_per_workgroup: 0,
            max_compute_workgroup_size_x: 0,
            max_compute_workgroup_size_y: 0,
            max_compute_workgroup_size_z: 0,
            max_compute_workgroup_storage_size: 0,
            ..wgpu::Limits::downlevel_defaults()
        };
        let limits = mobile_device_limits(no_compute);
        assert_eq!(limits.max_compute_workgroups_per_dimension, 0);
        assert_eq!(limits.max_compute_invocations_per_workgroup, 0);
        assert_eq!(limits.max_compute_workgroup_storage_size, 0);
    }

    #[test]
    fn storage_limits_never_exceed_adapter() {
        // The Android emulator's GLES driver reports zero storage-buffer limits;
        // the renderer uses no storage buffers, so request only what the adapter
        // grants (`downlevel_defaults` otherwise asks for 128 MiB and fails).
        let no_storage = wgpu::Limits {
            max_storage_buffer_binding_size: 0,
            max_storage_buffers_per_shader_stage: 0,
            max_storage_textures_per_shader_stage: 0,
            max_dynamic_storage_buffers_per_pipeline_layout: 0,
            ..wgpu::Limits::downlevel_defaults()
        };
        let limits = mobile_device_limits(no_storage);
        assert_eq!(limits.max_storage_buffer_binding_size, 0);
        assert_eq!(limits.max_storage_buffers_per_shader_stage, 0);
        assert_eq!(limits.max_storage_textures_per_shader_stage, 0);
        assert_eq!(limits.max_dynamic_storage_buffers_per_pipeline_layout, 0);
    }
}

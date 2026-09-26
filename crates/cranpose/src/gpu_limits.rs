/// Device memory blocks the GPU allocator carves allocations from on mobile.
/// A fixed size keeps the reserve close to what is allocated: the
/// `MemoryUsage` preset doubles each new block up to 64 MB of device and
/// 32 MB of host memory, so a frame that briefly needs one more buffer
/// reserves a block twice the last for good. Host blocks are half of this,
/// the allocator's 4 MB floor.
const MOBILE_MEMORY_BLOCK_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn mobile_memory_hints() -> wgpu::MemoryHints {
    wgpu::MemoryHints::Manual {
        suballocated_device_memory_block_size: MOBILE_MEMORY_BLOCK_BYTES..MOBILE_MEMORY_BLOCK_BYTES,
    }
}

pub(crate) fn mobile_device_limits(adapter_limits: wgpu::Limits) -> wgpu::Limits {
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter_limits.clone());
    limits.max_uniform_buffer_binding_size = adapter_limits
        .max_uniform_buffer_binding_size
        .min(wgpu::Limits::default().max_uniform_buffer_binding_size);
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
#[path = "tests/gpu_limits_tests.rs"]
mod tests;

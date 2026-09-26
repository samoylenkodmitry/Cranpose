use super::{MOBILE_MEMORY_BLOCK_BYTES, mobile_device_limits, mobile_memory_hints};

#[test]
fn uniform_binding_size_follows_adapter_up_to_desktop_default() {
    let desktop_binding = wgpu::Limits::default().max_uniform_buffer_binding_size;

    let capable = mobile_device_limits(wgpu::Limits::default());
    assert_eq!(capable.max_uniform_buffer_binding_size, desktop_binding);

    let mut minimal = wgpu::Limits::downlevel_defaults();
    minimal.max_uniform_buffer_binding_size = 16384;
    let limits = mobile_device_limits(minimal);
    assert_eq!(limits.max_uniform_buffer_binding_size, 16384);

    let big_textures = wgpu::Limits {
        max_texture_dimension_2d: 16384,
        ..wgpu::Limits::default()
    };
    let limits = mobile_device_limits(big_textures);
    assert_eq!(limits.max_texture_dimension_2d, 16384);
}

#[test]
fn compute_limits_never_exceed_adapter() {
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

#[test]
fn mobile_gpu_memory_comes_in_fixed_size_blocks() {
    let wgpu::MemoryHints::Manual {
        suballocated_device_memory_block_size,
    } = mobile_memory_hints()
    else {
        panic!("mobile hints fix the block size");
    };
    assert_eq!(
        suballocated_device_memory_block_size,
        MOBILE_MEMORY_BLOCK_BYTES..MOBILE_MEMORY_BLOCK_BYTES,
        "blocks do not grow past the first"
    );
}

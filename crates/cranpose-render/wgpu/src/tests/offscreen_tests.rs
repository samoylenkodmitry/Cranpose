use super::*;

#[test]
fn android_composites_in_eight_bits_and_the_rest_in_float() {
    assert_eq!(
        resolve_composition_format(None, true),
        wgpu::TextureFormat::Rgba8Unorm
    );
    assert_eq!(
        resolve_composition_format(None, false),
        wgpu::TextureFormat::Rgba16Float
    );
}

#[test]
fn the_override_wins_in_both_directions_on_any_platform() {
    assert_eq!(
        resolve_composition_format(Some("0"), true),
        wgpu::TextureFormat::Rgba16Float
    );
    assert_eq!(
        resolve_composition_format(Some(" yes "), false),
        wgpu::TextureFormat::Rgba8Unorm
    );
}

#[test]
fn an_unparsable_override_falls_back_to_the_platform_default() {
    assert_eq!(
        resolve_composition_format(Some("half"), true),
        wgpu::TextureFormat::Rgba8Unorm
    );
    assert_eq!(
        resolve_composition_format(Some(""), false),
        wgpu::TextureFormat::Rgba16Float
    );
}

#[test]
fn a_device_is_asked_whether_it_can_draw_into_a_format() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    assert!(renders_into(&device, wgpu::TextureFormat::Rgba8Unorm));
    assert!(
        !renders_into(&device, wgpu::TextureFormat::Rgb9e5Ufloat),
        "no device draws into a shared-exponent format"
    );
}

#[test]
fn a_device_that_draws_into_the_float_format_keeps_it() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let backend = device.adapter_info().backend;
    let preferred = composition_format();
    assert_eq!(settle_composition_format(&device, backend), preferred);
}

#[test]
fn a_frame_worth_of_small_surfaces_stays_pooled() {
    let bytes: u64 = (0..20)
        .map(|_| target_bytes(132, 132, composition_bytes_per_pixel()))
        .sum();
    assert!(
        bytes < MAX_POOLED_BYTES,
        "twenty control surfaces must fit the pool budget"
    );
    const _: () = assert!(20 < MAX_POOLED_TARGETS);
}

#[test]
fn the_byte_budget_bounds_full_screen_float_surfaces() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Rgba16Float, 4096);
    let full_screen = target_bytes(1080, 2244, pool.bytes_per_pixel());
    let held = MAX_POOLED_BYTES / full_screen;
    assert!(
        (2..=8).contains(&held),
        "the budget should hold a few full-screen surfaces, not dozens: {held}"
    );
}

#[test]
fn pool_starts_empty() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 8192);
    assert!(pool.available.is_empty());
    assert_eq!(pool.pool_size(), 0);
}

#[test]
fn max_texture_dimension_stored() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 2048);
    assert_eq!(pool.max_texture_dim, 2048);

    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 4096);
    assert_eq!(pool.max_texture_dim, 4096);
}

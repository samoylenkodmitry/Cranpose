use super::*;

#[test]
fn a_renderer_composites_in_its_display_images_own_eight_bit_format() {
    for (display, composition) in [
        (
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
        ),
        (
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Bgra8Unorm,
        ),
        (
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ),
        (
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Rgba8Unorm,
        ),
    ] {
        assert_eq!(composition_format(display), composition, "{display:?}");
    }
}

#[test]
fn any_other_display_format_composites_in_rgba8() {
    for display in [
        wgpu::TextureFormat::Rgb10a2Unorm,
        wgpu::TextureFormat::Rgba16Float,
    ] {
        assert_eq!(
            composition_format(display),
            wgpu::TextureFormat::Rgba8Unorm,
            "{display:?}"
        );
    }
}

#[test]
fn a_frame_worth_of_small_surfaces_stays_pooled() {
    let bytes: u64 = (0..20)
        .map(|_| target_bytes(132, 132, COMPOSITION_BYTES_PER_PIXEL))
        .sum();
    assert!(
        bytes < MAX_POOLED_BYTES,
        "twenty control surfaces must fit the pool budget"
    );
    const _: () = assert!(20 < MAX_POOLED_TARGETS);
}

#[test]
fn the_byte_budget_bounds_full_screen_surfaces() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Rgba8Unorm, 4096);
    let full_screen = target_bytes(1080, 2244, pool.bytes_per_pixel());
    let held = MAX_POOLED_BYTES / full_screen;
    assert!(
        (2..=16).contains(&held),
        "the budget should hold some full-screen surfaces, not dozens: {held}"
    );
}

#[test]
fn pool_starts_empty() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 8192);
    assert_eq!(pool.pool_size(), 0);
}

#[test]
fn max_texture_dimension_stored() {
    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 2048);
    assert_eq!(pool.max_texture_dim, 2048);

    let pool = OffscreenPool::new_with_limit(wgpu::TextureFormat::Bgra8Unorm, 4096);
    assert_eq!(pool.max_texture_dim, 4096);
}

#[test]
fn a_pooled_target_no_frame_reuses_is_dropped_after_the_idle_frames() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut pool = OffscreenPool::new_with_limit(format, 4096);
    pool.release(OffscreenTarget::new(&device, format, 8, 8));
    pool.release(OffscreenTarget::new(&device, format, 4, 4));
    for _ in 0..crate::idle_pool::IDLE_FRAMES {
        let reused = pool.acquire(&device, 4, 4, None);
        pool.release(reused);
        pool.end_frame();
    }
    assert_eq!(pool.pool_size(), 2);
    pool.end_frame();
    assert_eq!(pool.pool_size(), 1, "the target reused every frame stays");
    assert_eq!(pool.estimated_bytes(), 4 * 4 * 4);
}

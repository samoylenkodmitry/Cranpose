use super::BlurUniforms;

#[test]
fn blur_uniforms_use_vec4_packing_for_gl_backends() {
    assert_eq!(
        std::mem::size_of::<BlurUniforms>(),
        64 + 16 * super::BLUR_TAP_PAIRS + 16
    );
    assert_eq!(std::mem::offset_of!(BlurUniforms, direction_and_radius), 0);
    assert_eq!(
        std::mem::offset_of!(BlurUniforms, texture_size_and_tile_mode),
        16
    );
    assert_eq!(std::mem::offset_of!(BlurUniforms, pairs), 64);
    assert_eq!(
        std::mem::offset_of!(BlurUniforms, kernel),
        64 + 16 * super::BLUR_TAP_PAIRS
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_wide_blur_gets_a_smaller_scratch() {
    use super::blur_scratch_size;
    assert_eq!(blur_scratch_size(2.0, 2.0, 1080, 400), (1080, 400));
    assert_eq!(blur_scratch_size(8.0, 8.0, 1080, 400), (540, 200));
    assert_eq!(blur_scratch_size(30.0, 30.0, 1080, 400), (270, 100));
    assert_eq!(blur_scratch_size(30.0, 30.0, 1081, 401), (271, 101));
}

#[test]
fn a_small_target_keeps_its_full_scratch() {
    use super::blur_scratch_size;
    assert_eq!(blur_scratch_size(30.0, 30.0, 40, 40), (20, 20));
    assert_eq!(blur_scratch_size(30.0, 30.0, 20, 20), (20, 20));
}

#[test]
fn a_scissor_shrinks_with_the_scratch() {
    use super::scaled_scissor;
    assert_eq!(
        scaled_scissor(Some((10, 20, 100, 200)), 2.0, 2.0, 540, 200),
        Some((5, 10, 50, 100))
    );
    assert_eq!(
        scaled_scissor(Some((9, 9, 10, 10)), 4.0, 4.0, 270, 100),
        Some((2, 2, 3, 3))
    );
    assert_eq!(
        scaled_scissor(Some((0, 0, 4000, 4000)), 4.0, 4.0, 270, 100),
        Some((0, 0, 270, 100))
    );
    assert_eq!(scaled_scissor(None, 4.0, 4.0, 270, 100), None);
}

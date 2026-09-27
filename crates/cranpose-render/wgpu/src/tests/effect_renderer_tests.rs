use super::{BLUR_REGIONS_PER_DRAW, BlurKernelUniforms, BlurRegionUniforms, BlurUniforms};

#[test]
fn blur_uniforms_use_vec4_packing_for_gl_backends() {
    let kernel = 32 + 16 * super::BLUR_TAP_PAIRS + 16;
    assert_eq!(std::mem::size_of::<BlurKernelUniforms>(), kernel);
    assert_eq!(
        std::mem::offset_of!(BlurKernelUniforms, direction_and_radius),
        0
    );
    assert_eq!(
        std::mem::offset_of!(BlurKernelUniforms, texture_size_and_tile_mode),
        16
    );
    assert_eq!(std::mem::offset_of!(BlurKernelUniforms, pairs), 32);
    assert_eq!(
        std::mem::offset_of!(BlurKernelUniforms, kernel),
        32 + 16 * super::BLUR_TAP_PAIRS
    );
    assert_eq!(std::mem::size_of::<BlurRegionUniforms>(), 48);
    assert_eq!(std::mem::offset_of!(BlurUniforms, target_size), kernel);
    assert_eq!(std::mem::offset_of!(BlurUniforms, regions), kernel + 16);
    assert_eq!(
        std::mem::size_of::<BlurUniforms>(),
        kernel + 16 + 48 * BLUR_REGIONS_PER_DRAW
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

#[cfg(not(target_arch = "wasm32"))]
fn grouping_draw<'a>(
    renderer: &super::EffectRenderer,
    source: &'a crate::offscreen::OffscreenTarget,
    radius: f32,
    filter: super::BlurFilter,
    index: u32,
) -> super::BlurDraw<'a> {
    let region = (index * 8, 0, 8, 8);
    super::BlurDraw {
        source,
        uniforms: renderer.blur_uniforms(
            true,
            (source.width, source.height),
            region,
            region,
            (radius, radius),
            cranpose_ui_graphics::TileMode::Clamp,
        ),
        filter,
        scissor: Some(region),
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn blur_regions_sharing_a_pipeline_texture_and_kernel_draw_together() {
    use super::{BLUR_REGIONS_PER_DRAW, BlurFilter, EffectRenderer, blur_draw_groups};
    use crate::{offscreen::OffscreenTarget, pipeline_compiler::PipelineCompiler};
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = EffectRenderer::new(
        &device,
        PipelineCompiler::inactive(),
        None,
        format,
        device.adapter_info().backend,
    );
    let first = OffscreenTarget::new(&device, format, 256, 8);
    let second = OffscreenTarget::new(&device, format, 256, 8);
    let draws = [
        grouping_draw(&renderer, &first, 6.0, BlurFilter::Kernel, 0),
        grouping_draw(&renderer, &first, 6.0, BlurFilter::Kernel, 1),
        grouping_draw(&renderer, &first, 9.0, BlurFilter::Kernel, 2),
        grouping_draw(&renderer, &second, 9.0, BlurFilter::Kernel, 3),
        grouping_draw(&renderer, &second, 9.0, BlurFilter::Mean, 4),
        grouping_draw(&renderer, &second, 9.0, BlurFilter::Mean, 5),
    ];
    assert_eq!(
        blur_draw_groups(&draws).as_slice(),
        [0..2, 2..3, 3..4, 4..6],
        "a new kernel, texture or pipeline starts a new draw"
    );
    let many: Vec<_> = (0..BLUR_REGIONS_PER_DRAW as u32 + 3)
        .map(|index| grouping_draw(&renderer, &first, 6.0, BlurFilter::Kernel, index))
        .collect();
    assert_eq!(
        blur_draw_groups(&many).as_slice(),
        [
            0..BLUR_REGIONS_PER_DRAW,
            BLUR_REGIONS_PER_DRAW..BLUR_REGIONS_PER_DRAW + 3
        ],
        "a draw covers at most its uniform block's regions"
    );
    assert!(blur_draw_groups(&[]).is_empty());
}

use super::*;

const SRGB: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DRAWN: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[test]
fn a_device_that_takes_four_samples_in_both_formats_gets_four() {
    assert_eq!(volume_samples(|_| true, SRGB), Multisample::X4);
}

#[test]
fn a_device_with_no_four_samples_in_the_depth_format_gets_one() {
    let samples = volume_samples(|format| format != SCENE_DEPTH_FORMAT, SRGB);
    assert_eq!(samples, Multisample::Off);
}

#[test]
fn the_colour_is_judged_in_the_form_the_renderer_draws() {
    // A device that takes four samples in the drawn form and not in the image's own sRGB form
    let drawn_only = |format| format == DRAWN || format == SCENE_DEPTH_FORMAT;
    assert_eq!(volume_samples(drawn_only, SRGB), Multisample::X4);
    assert_eq!(
        volume_samples(drawn_only, wgpu::TextureFormat::Rgba16Float),
        Multisample::X4
    );
    let no_colour = |format| format == SCENE_DEPTH_FORMAT;
    assert_eq!(volume_samples(no_colour, SRGB), Multisample::Off);
}

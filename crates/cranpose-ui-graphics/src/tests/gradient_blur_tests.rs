use super::*;

#[test]
fn gradient_blur_carries_endpoint_radii_and_capture_padding() {
    let RenderEffect::Shader { shader } =
        gradient_blur_effect(0.5, 18.25, GradientBlurDirection::BottomToTop)
    else {
        panic!("gradient blur must use the spatial runtime shader");
    };
    assert_eq!(&shader.uniforms()[..3], &[0.5, 18.25, 3.0]);
    assert_eq!(shader.input_padding(), 19.0);
    assert_eq!(
        shader.substrates(),
        &[
            SubstrateSpec::Blur { radius_px: 18.25 },
            SubstrateSpec::Blur { radius_px: 9.125 },
            SubstrateSpec::Blur { radius_px: 4.5625 },
        ]
    );
}

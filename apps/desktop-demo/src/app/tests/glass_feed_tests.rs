use super::*;

#[test]
fn every_feed_glass_material_is_sharp_but_optically_shaped() {
    let glass = feed_glass();
    assert_eq!(glass.blur_radius, Some(0.0));
    assert_eq!(glass.refraction_depth, 0.34);
    assert_eq!(glass.refraction_depth_dp, Some(21.42));
    assert_eq!(glass.refraction_curve, 0.55);
    assert_eq!(glass.dispersion, 0.30);
    assert_eq!(glass.transmission_refraction, 1.0);
    assert_eq!(
        feed_button_spec()
            .glass
            .expect("feed buttons must override their glass material")
            .blur_radius,
        Some(0.0)
    );
    assert_eq!(feed_search_spec().glass.blur_radius, Some(0.0));
    assert_eq!(feed_search_spec().foreground, Some(Color::WHITE));
}

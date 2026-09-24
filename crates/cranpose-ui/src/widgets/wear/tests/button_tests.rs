use super::*;

#[test]
fn the_defaults_are_the_ones_the_tokens_declare() {
    let spec = WearButtonSpec::default();
    assert_eq!(spec.min_height, 52.0, "FilledButtonTokens.ContainerHeight");
    assert_eq!(spec.corner_radius, 26.0, "ShapeTokens.CornerLarge");
    assert_eq!(spec.padding_horizontal, 14.0);
    assert_eq!(spec.padding_vertical, 6.0);
    assert_eq!(spec.label_spacing, 1.0);
    assert_eq!(spec.secondary_alpha, 0.8);
    assert_eq!(spec.label_style, WearTextStyle::LABEL_MEDIUM);
    assert_eq!(spec.secondary_style, WearTextStyle::LABEL_SMALL);
}

#[test]
fn a_secondary_label_draws_its_colour_at_four_fifths() {
    let faded = fade(Color::rgba(1.0, 1.0, 1.0, 1.0), 0.8);
    assert_eq!(faded.3, 0.8);
}

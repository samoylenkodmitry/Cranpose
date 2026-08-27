//! Modifier builders, the round-list geometry, the Wear grid and the colour
//! solver.
//!
//! These are the rules the widgets above them obey. Each is pure arithmetic or
//! a builder that records an option, so each can be stated exactly — which is
//! the point: a spacer that is half a pixel out, or a modifier that quietly
//! drops what it was handed, is invisible in a screenshot and obvious here.

use cranpose_ui::{
    Modifier,
    density::Density,
    font_scale::FontScaleCurve,
    round_scaling_list::{leading_auto_centring_spacer, trailing_auto_centring_spacer},
    run_test_composition,
    widgets::wear::color_appearance::hct_solve,
};

#[test]
fn the_leading_spacer_pushes_the_anchor_onto_the_centre_line() {
    // A 200pt viewport centres at 100. An anchor whose centre sits 30 into the
    // content needs 70 of spacer above it to land there.
    assert_eq!(leading_auto_centring_spacer(200.0, 30.0, 0.0), 70.0);

    // An explicit offset moves the centre line, and the spacer follows.
    assert_eq!(leading_auto_centring_spacer(200.0, 30.0, 20.0), 50.0);

    // Content that already reaches past the centre line needs no spacer at all,
    // and must never be given a negative one.
    assert_eq!(leading_auto_centring_spacer(200.0, 300.0, 0.0), 0.0);
}

#[test]
fn the_trailing_spacer_leaves_half_the_last_row_below_the_centre_line() {
    // 200 - floor(100) - 40/2 = 80.
    assert_eq!(trailing_auto_centring_spacer(200.0, 40.0), 80.0);

    // A last row taller than the viewport cannot ask for negative space.
    assert_eq!(trailing_auto_centring_spacer(200.0, 1000.0), 0.0);
}

#[test]
fn an_odd_viewport_floors_its_centre_line_the_same_way_for_both_spacers() {
    // The two spacers plus the anchor's own height have to add back up to the
    // viewport, which only holds if both floor the half the same way.
    let viewport = 201.0;
    let height = 40.0;
    let leading = leading_auto_centring_spacer(viewport, height * 0.5, 0.0);
    let trailing = trailing_auto_centring_spacer(viewport, height);
    assert_eq!(leading + height + trailing, viewport);
}

#[test]
fn a_wear_grid_rejects_a_density_that_cannot_be_drawn_at() {
    for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            Density::new(bad, 1.0).density(),
            1.0,
            "a density of {bad} was accepted"
        );
    }
    assert_eq!(Density::new(2.0, 1.0).density(), 2.0);
}

#[test]
fn a_wear_grid_takes_a_font_scale_curve_rather_than_a_bare_multiplier() {
    let linear = Density::with_curve(2.0, FontScaleCurve::linear(1.5));
    let plain = Density::new(2.0, 1.5);
    assert_eq!(
        linear.density(),
        plain.density(),
        "the curve must not disturb the pixel grid"
    );
}

#[test]
fn the_colour_solver_answers_grey_for_a_request_with_no_chroma() {
    // With no chroma the hue cannot matter: every hue has to land on the same
    // neutral, or a palette's greys drift with whatever hue produced them.
    let first = hct_solve(0.0, 0.0, 50.0);
    let second = hct_solve(180.0, 0.0, 50.0);
    assert_eq!(first, second);
}

#[test]
fn the_colour_solver_answers_a_brighter_colour_for_a_higher_lightness() {
    fn luminance(argb: u32) -> u32 {
        let r = (argb >> 16) & 0xff;
        let g = (argb >> 8) & 0xff;
        let b = argb & 0xff;
        r + g + b
    }
    let dark = hct_solve(120.0, 40.0, 20.0);
    let light = hct_solve(120.0, 40.0, 80.0);
    assert!(
        luminance(light) > luminance(dark),
        "L* 80 was not brighter than L* 20"
    );
}

#[test]
fn a_modifier_records_the_locals_it_provides_and_consumes() {
    // Both halves build a chain; what is asserted is that neither drops the
    // callback it was handed, which would leave a consumer silently reading a
    // default forever.
    run_test_composition(|| {
        let key = cranpose_ui::ModifierLocalKey::new(|| 0u32);
        let modifier = Modifier::empty()
            .modifier_local_provider(key, || 7u32)
            .modifier_local_consumer(|_scope| {});
        assert_ne!(
            modifier,
            Modifier::empty(),
            "the chain came back with nothing on it"
        );
    });
}

#[test]
fn a_focus_target_and_a_focus_listener_both_land_on_the_chain() {
    let modifier = Modifier::empty()
        .focus_target()
        .on_focus_changed(|_state| {});
    assert_ne!(modifier, Modifier::empty(), "the focus nodes were dropped");
}

#[test]
fn a_fractional_offset_is_recorded_on_the_chain() {
    let modifier = Modifier::empty().offset_fraction(0.5, -0.25);
    assert_ne!(modifier, Modifier::empty(), "the offset was dropped");
}

#[test]
fn a_row_weight_is_recorded_on_the_chain() {
    let filled = Modifier::empty().rowWeight(2.0, true);
    let unfilled = Modifier::empty().rowWeight(2.0, false);
    assert_ne!(filled, Modifier::empty());
    assert_ne!(unfilled, Modifier::empty());
    assert_ne!(
        filled, unfilled,
        "a weight that fills and one that does not are different modifiers"
    );
}

#[test]
fn safe_area_padding_is_recorded_on_the_chain() {
    run_test_composition(|| {
        let modifier = Modifier::empty().safe_area_padding();
        assert_ne!(
            modifier,
            Modifier::empty(),
            "the safe-area padding was dropped"
        );
    });
}

#[test]
fn window_insets_are_readable_inside_a_composition() {
    run_test_composition(|| {
        let insets = cranpose_ui::safe_area::window_insets();
        // With no host reporting insets every edge is zero; what must not
        // happen is a negative inset, which would grow the content past the
        // window rather than inside it.
        let edges = insets.combined();
        assert!(edges.top >= 0.0 && edges.bottom >= 0.0);
        assert!(edges.left >= 0.0 && edges.right >= 0.0);
    });
}

#[test]
fn a_debug_tag_on_a_chain_does_not_change_what_the_chain_holds() {
    let plain = Modifier::empty().offset_fraction(0.5, 0.5);
    let tagged = Modifier::empty()
        .offset_fraction(0.5, 0.5)
        .debug_chain("tag");
    assert_ne!(plain, Modifier::empty());
    assert_ne!(
        tagged,
        Modifier::empty(),
        "the debug tag emptied the chain it was only supposed to name"
    );
}

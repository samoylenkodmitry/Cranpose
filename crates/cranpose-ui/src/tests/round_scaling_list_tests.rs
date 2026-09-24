use super::*;

const VIEWPORT: f32 = 227.0;

#[test]
fn a_row_in_the_middle_is_left_alone() {
    let middle = scale_and_alpha(VIEWPORT, VIEWPORT * 0.45, VIEWPORT * 0.55).unwrap();
    assert_eq!(middle, ScaleAlpha::UNCHANGED);
}

#[test]
fn a_row_at_the_edge_is_shrunk_and_faded_together() {
    let edge = scale_and_alpha(VIEWPORT, 0.0, 20.0).unwrap();
    assert!(edge.scale < 1.0 && edge.scale >= EDGE_SCALE, "{edge:?}");
    assert!(edge.alpha < 1.0 && edge.alpha >= EDGE_ALPHA, "{edge:?}");
    let top = scale_and_alpha(VIEWPORT, 0.0, 0.0).unwrap();
    assert!((top.scale - EDGE_SCALE).abs() < 1e-3, "{top:?}");
    assert!((top.alpha - EDGE_ALPHA).abs() < 1e-3, "{top:?}");
}

#[test]
fn the_two_edges_treat_a_row_the_same() {
    let height = 40.0;
    let near_top = scale_and_alpha(VIEWPORT, 8.0, 8.0 + height).unwrap();
    let near_bottom = scale_and_alpha(VIEWPORT, VIEWPORT - 8.0 - height, VIEWPORT - 8.0).unwrap();
    assert!((near_top.scale - near_bottom.scale).abs() < 1e-5);
    assert!((near_top.alpha - near_bottom.alpha).abs() < 1e-5);
}

#[test]
fn a_taller_row_starts_shrinking_further_from_the_edge() {
    let top = VIEWPORT - 10.0;
    let short = scale_and_alpha(VIEWPORT, top, top + VIEWPORT * 0.2).unwrap();
    let tall = scale_and_alpha(VIEWPORT, top, top + VIEWPORT * 0.62).unwrap();
    assert!(tall.scale < short.scale, "short {short:?} tall {tall:?}");
}

#[test]
fn a_row_is_placed_from_the_full_heights_above_it_not_the_scaled_ones() {
    let first = place_row(VIEWPORT, 0.0, 50.0, 2.0).unwrap();
    let second = place_row(VIEWPORT, 50.0, 50.0, 2.0).unwrap();
    assert!(first.scale < 1.0, "the first row is at the edge: {first:?}");
    assert!(second.top >= 49.0, "{second:?}");
}

#[test]
fn a_row_above_the_centre_line_keeps_its_bottom_edge() {
    let above = place_row(VIEWPORT, 4.0, 50.0, 2.0).unwrap();
    assert!(above.scale < 1.0, "{above:?}");
    assert!(
        above.top > 4.0,
        "shrinking should pull the top down: {above:?}"
    );

    let below = place_row(VIEWPORT, VIEWPORT - 54.0, 50.0, 2.0).unwrap();
    assert!(below.scale < 1.0, "{below:?}");
    assert!(
        (below.top - (VIEWPORT - 54.0)).abs() < 0.6,
        "the top is pinned below the line: {below:?}"
    );
}

#[test]
fn an_odd_pixel_height_carries_the_half_pixel_composes_integer_halving_leaves() {
    assert_eq!(odd_pixel(50.0), 0.0);
    assert_eq!(odd_pixel(51.0), 0.5);
    let odd = place_row(VIEWPORT, 3.0, 25.5, 2.0).unwrap();
    assert!(odd.scale < 1.0, "needs to be in the scaled band: {odd:?}");
}

#[test]
fn a_density_of_zero_falls_back_to_continuous_placement_instead_of_dividing_by_it() {
    let placed = place_row(VIEWPORT, 10.0, 50.0, 0.0).unwrap();
    assert!(
        placed.top.is_finite() && placed.height.is_finite(),
        "{placed:?}"
    );
    assert_eq!(placed.top, 10.0);
    let negative = place_row(VIEWPORT, 10.0, 50.0, -2.0).unwrap();
    assert_eq!(negative, placed, "a nonsense density is not a crash");
}

#[test]
fn an_empty_viewport_leaves_everything_alone_rather_than_dividing_by_it() {
    assert_eq!(scale_and_alpha(0.0, 0.0, 10.0), Some(ScaleAlpha::UNCHANGED));
    assert_eq!(
        scale_and_alpha(-5.0, 0.0, 10.0),
        Some(ScaleAlpha::UNCHANGED)
    );
}

#[test]
fn invalid_geometry_is_rejected_instead_of_producing_nan() {
    assert_eq!(scale_and_alpha(f32::NAN, 0.0, 10.0), None);
    assert_eq!(scale_and_alpha(VIEWPORT, 10.0, 9.0), None);
    assert_eq!(place_row(VIEWPORT, 0.0, -1.0, 2.0), None);
    assert_eq!(place_row(VIEWPORT, 0.0, 10.0, f32::INFINITY), None);
}

#[test]
fn the_easing_is_monotonic_and_spans_the_whole_range() {
    assert!((ease(0.0) - 0.0).abs() < 1e-3, "{}", ease(0.0));
    assert!((ease(1.0) - 1.0).abs() < 1e-3, "{}", ease(1.0));
    let mut previous = -1.0;
    for step in 0..=20 {
        let value = ease(step as f32 / 20.0);
        assert!(value >= previous - 1e-4, "not monotonic at {step}");
        previous = value;
    }
}

#[test]
fn the_easing_matches_the_current_wear_compose_curve() {
    assert!((ease(0.25) - 0.166_779).abs() < 1e-3, "{}", ease(0.25));
}

#[test]
fn the_default_scaling_params_are_the_constants_the_module_documents() {
    let params = ScalingParams::default();
    assert_eq!(params.edge_scale, EDGE_SCALE);
    assert_eq!(params.edge_alpha, EDGE_ALPHA);
    assert_eq!(params.min_element_height, MIN_ELEMENT_HEIGHT);
    assert_eq!(params.max_element_height, MAX_ELEMENT_HEIGHT);
    assert_eq!(params.min_transition_area, MIN_TRANSITION_AREA);
    assert_eq!(params.max_transition_area, MAX_TRANSITION_AREA);
    assert_eq!(
        scale_and_alpha_with(params, VIEWPORT, 0.0, 20.0),
        scale_and_alpha(VIEWPORT, 0.0, 20.0)
    );
    assert_eq!(
        place_row_with(params, VIEWPORT, 4.0, 50.0, 2.0),
        place_row(VIEWPORT, 4.0, 50.0, 2.0)
    );
}

#[test]
fn reduced_motion_turns_the_ramp_off_rather_than_damping_it() {
    let params = ScalingParams::default().reduced_motion();
    let edge = scale_and_alpha_with(params, VIEWPORT, 0.0, 0.0).unwrap();
    assert_eq!(edge, ScaleAlpha::UNCHANGED);
}

#[test]
fn the_supplied_params_reach_the_pixel_path_and_not_only_the_continuous_one() {
    let params = ScalingParams::default().reduced_motion();
    let still = place_row_with(params, VIEWPORT, 4.0, 50.0, 2.0).unwrap();
    assert_eq!(still.scale, 1.0, "{still:?}");
    assert_eq!(still.alpha, 1.0, "{still:?}");
    assert_eq!(still.top, 4.0, "an unscaled row is not pinned anywhere");
    assert!(place_row(VIEWPORT, 4.0, 50.0, 2.0).unwrap().scale < 1.0);
}

#[test]
fn a_row_is_placed_against_the_scaled_size_of_the_row_between_it_and_the_centre() {
    let viewport = 192.0;
    let density = 2.0;
    let (gap, height) = (4.0, 52.0);
    let anchor_top = 70.0;

    let anchor = place_row(viewport, anchor_top, height, density).unwrap();
    assert_eq!(anchor.scale, 1.0, "the anchored row is not scaled");
    assert_eq!(
        anchor.reported_height, height,
        "so it reports its full height"
    );

    let first_top = anchor_top + anchor.reported_height + gap;
    assert_eq!(first_top, anchor_top + height + gap);
    let first = place_row(viewport, first_top, height, density).unwrap();
    assert!(first.scale < 1.0, "{first:?}");
    assert!(
        first.reported_height < height,
        "and it reports less than its full height: {first:?}"
    );

    let second_top = first_top + first.reported_height + gap;
    let stacked_top = first_top + height + gap;
    assert!(
        second_top < stacked_top,
        "cursor {second_top} vs full stack {stacked_top}"
    );
    let by_cursor = place_row(viewport, second_top, height, density).unwrap();
    let by_stack = place_row(viewport, stacked_top, height, density).unwrap();
    assert_ne!(by_cursor.top, by_stack.top);
}

#[test]
fn the_reported_height_is_the_rounded_one_and_the_drawn_height_is_not() {
    let placed = place_row(VIEWPORT, 3.0, 25.5, 2.0).unwrap();
    assert!(
        placed.scale < 1.0,
        "needs to be in the scaled band: {placed:?}"
    );
    assert_eq!(
        placed.reported_height * 2.0,
        (placed.reported_height * 2.0).round(),
        "the reported height is a whole pixel: {placed:?}"
    );
    assert_ne!(placed.reported_height, placed.height);
}

#[test]
fn a_stack_puts_full_heights_a_gap_apart() {
    let mut slots = Vec::new();
    stack_into([10.0, 20.0, 30.0], 4.0, &mut slots);
    assert_eq!(
        slots,
        vec![
            Slot {
                top: 0.0,
                height: 10.0
            },
            Slot {
                top: 14.0,
                height: 20.0
            },
            Slot {
                top: 38.0,
                height: 30.0
            },
        ]
    );
    assert_eq!(slots[1].centre(), 24.0);
    assert_eq!(slots[2].bottom(), 68.0);
}

#[test]
fn the_centre_anchor_puts_the_anchored_items_centre_on_the_centre_line() {
    let mut slots = Vec::new();
    stack_into([40.0, 60.0, 40.0], 4.0, &mut slots);
    let offset = centre_offset(&slots, VIEWPORT, CentreAnchor::default(), 2.0);
    shift(&mut slots, offset);
    assert!(
        (slots[1].centre() - VIEWPORT * 0.5).abs() < 1e-4,
        "{slots:?}"
    );
}

#[test]
fn a_scroll_offset_moves_the_content_up() {
    let mut slots = Vec::new();
    stack_into([40.0, 60.0, 40.0], 4.0, &mut slots);
    let still = centre_offset(&slots, VIEWPORT, CentreAnchor::default(), 0.0);
    let scrolled = centre_offset(
        &slots,
        VIEWPORT,
        CentreAnchor {
            index: 1,
            offset: 10.0,
        },
        0.0,
    );
    assert!((still - scrolled - 10.0).abs() < 1e-4, "{still} {scrolled}");
}

#[test]
fn a_fractional_scroll_travels_between_item_centres_not_item_tops() {
    let mut slots = Vec::new();
    stack_into([20.0, 100.0], 0.0, &mut slots);
    let start = centre_offset_at(&slots, VIEWPORT, 0.0, 0.0);
    let end = centre_offset_at(&slots, VIEWPORT, 1.0, 0.0);
    let middle = centre_offset_at(&slots, VIEWPORT, 0.5, 0.0);
    assert!((middle - (start + end) * 0.5).abs() < 1e-4);
    assert_eq!(
        start,
        centre_offset(
            &slots,
            VIEWPORT,
            CentreAnchor {
                index: 0,
                offset: 0.0
            },
            0.0
        )
    );
}

#[test]
fn a_scroll_past_either_end_clamps_instead_of_running_off() {
    let mut slots = Vec::new();
    stack_into([20.0, 20.0], 4.0, &mut slots);
    assert_eq!(
        centre_offset_at(&slots, VIEWPORT, -5.0, 0.0),
        centre_offset_at(&slots, VIEWPORT, 0.0, 0.0)
    );
    assert_eq!(
        centre_offset_at(&slots, VIEWPORT, 9.0, 0.0),
        centre_offset_at(&slots, VIEWPORT, 1.0, 0.0)
    );
    assert_eq!(centre_offset_at(&[], VIEWPORT, 0.0, 2.0), 0.0);
    assert_eq!(
        centre_offset(&[], VIEWPORT, CentreAnchor::default(), 2.0),
        0.0
    );
}

#[test]
fn rounding_a_length_to_a_pixel_sends_an_exact_half_up_the_way_kotlin_does() {
    assert_eq!(round_to_px(0.25, 2.0), 0.5);
    assert_eq!(round_to_px(-0.25, 2.0), 0.0);
    assert_eq!((-0.5f32).round(), -1.0);
    assert_eq!(round_to_px(0.3, 0.0), 0.3);
    assert!(round_to_px(f32::NAN, 2.0).is_nan());
}

#[test]
fn the_shift_and_the_two_spacers_are_the_same_arithmetic_seen_from_two_sides() {
    let viewport_px = 454.0;
    let mut slots = Vec::new();
    stack_into([96.0, 104.0, 104.0], 8.0, &mut slots);
    let anchor = CentreAnchor::default();
    let (leading, _) = auto_centring_spacers(&slots, viewport_px, anchor);
    let offset = centre_offset(&slots, viewport_px, anchor, 1.0);
    assert!((leading - offset).abs() < 1e-4, "{leading} vs {offset}");
}

#[test]
fn the_leading_spacer_never_pushes_the_anchor_below_the_centre_line() {
    let mut slots = Vec::new();
    stack_into([600.0], 8.0, &mut slots);
    let anchor = CentreAnchor::default();
    let (leading, trailing) = auto_centring_spacers(&slots, 454.0, anchor);
    assert_eq!(leading, 0.0, "a tall first item needs no leading spacer");
    assert!(trailing >= 0.0, "{trailing}");
}

#[test]
fn the_content_padding_is_travel_at_both_ends_and_not_blank_beyond_them() {
    let mut slots = Vec::new();
    stack_into([96.0, 104.0, 104.0, 104.0], 8.0, &mut slots);
    let anchor = slots[1].centre();
    let last = slots[3].centre();
    let (start, end) = anchor_travel(anchor, last, 68.0, 68.0);
    assert_eq!(start, anchor - 68.0);
    assert_eq!(end, last + 68.0);
    assert_eq!((end - start) - (last - anchor), 136.0);
}

#[test]
fn a_list_with_nowhere_to_go_does_not_travel_backwards() {
    let mut slots = Vec::new();
    stack_into([104.0], 8.0, &mut slots);
    let centre = slots[0].centre();
    let (start, end) = anchor_travel(centre, centre, 68.0, 0.0);
    assert!(start <= end, "{start} {end}");
    assert_eq!(end, centre);
}

#[test]
fn an_odd_viewport_gives_its_spare_pixel_to_the_trailing_spacer() {
    let mut slots = Vec::new();
    stack_into([100.0, 100.0], 8.0, &mut slots);
    let anchor = CentreAnchor::default();
    let (_, odd) = auto_centring_spacers(&slots, 455.0, anchor);
    let (_, even) = auto_centring_spacers(&slots, 454.0, anchor);
    assert_eq!(odd - even, 1.0, "odd {odd} even {even}");
}

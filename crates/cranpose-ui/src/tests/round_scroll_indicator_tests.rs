use super::*;

const LARGE_RADIUS_DP: f32 = 113.5;
const SMALL_RADIUS_DP: f32 = 96.0;

#[test]
fn stroke_width_switches_at_the_wear_large_screen_breakpoint() {
    assert_eq!(indicator_width_dp(224.99), INDICATOR_NARROW_WIDTH_DP);
    assert_eq!(indicator_width_dp(225.0), INDICATOR_WIDTH_DP);
    assert_eq!(indicator_width_dp(f32::NAN), INDICATOR_NARROW_WIDTH_DP);
}

#[test]
fn the_track_lands_where_the_shipping_compose_build_draws_it() {
    let large = indicator_arc(LARGE_RADIUS_DP);
    assert!((large.centreline() - 108.5).abs() < 0.01, "{large:?}");
    assert!((large.width() - 6.0).abs() < 0.01, "{large:?}");

    let small = indicator_arc(SMALL_RADIUS_DP);
    assert!((small.centreline() - 91.5).abs() < 0.01, "{small:?}");
    assert!((small.width() - 5.0).abs() < 0.01, "{small:?}");
}

#[test]
fn the_sweep_is_a_height_in_dp_not_a_fixed_angle() {
    let large = indicator_arc(LARGE_RADIUS_DP).sweep().to_degrees();
    let small = indicator_arc(SMALL_RADIUS_DP).sweep().to_degrees();
    assert!((large - 30.54).abs() < 0.05, "{large}");
    assert!((small - 35.73).abs() < 0.05, "{small}");
    assert!(small > large);
}

#[test]
fn a_list_that_fits_on_screen_shows_no_indicator_at_all() {
    assert_eq!(indicator_geometry(100.0, 100.0, 0.0), None);
    assert_eq!(indicator_geometry(80.0, 100.0, 0.0), None);
    assert_eq!(indicator_geometry(f32::NAN, 100.0, 0.0), None);
    assert_eq!(indicator_geometry(200.0, 0.0, 0.0), None);
}

#[test]
fn the_thumb_is_the_viewport_share_clamped_at_both_ends() {
    let half = indicator_geometry(200.0, 100.0, 0.0).unwrap();
    assert!((half.thumb - 0.5).abs() < 1e-6, "{half:?}");
    let long = indicator_geometry(10_000.0, 100.0, 0.0).unwrap();
    assert!((long.thumb - INDICATOR_MIN_THUMB).abs() < 1e-6, "{long:?}");
    let short = indicator_geometry(105.0, 100.0, 0.0).unwrap();
    assert!(
        (short.thumb - INDICATOR_MAX_THUMB).abs() < 1e-6,
        "{short:?}"
    );
}

#[test]
fn the_thumb_reaches_the_bottom_of_the_track_and_no_further() {
    let bottom = indicator_geometry(200.0, 100.0, 100.0).unwrap();
    assert!(
        (bottom.offset + bottom.thumb - 1.0).abs() < 1e-6,
        "{bottom:?}"
    );
    let past = indicator_geometry(200.0, 100.0, 500.0).unwrap();
    assert_eq!(past, bottom);
}

#[test]
fn the_indicator_is_three_segments_with_a_gap_either_side_of_the_thumb() {
    let arc = indicator_arc(LARGE_RADIUS_DP);
    let geometry = IndicatorGeometry {
        thumb: 0.4,
        offset: 0.3,
    };
    let parts = indicator_segments(arc, geometry, 1.0);
    assert_eq!(parts[0].0, IndicatorPart::Track);
    assert_eq!(parts[1].0, IndicatorPart::Thumb);
    assert_eq!(parts[2].0, IndicatorPart::Track);

    let ink_bounds = |segment: IndicatorSegment| match segment {
        IndicatorSegment::Arc { start, sweep, .. } => {
            (start - arc.cap_sweep() * 0.5, sweep + arc.cap_sweep())
        }
        other => panic!("expected an arc, got {other:?}"),
    };
    let (above_start, above_sweep) = ink_bounds(parts[0].1);
    let (thumb_start, thumb_sweep) = ink_bounds(parts[1].1);
    let (below_start, below_sweep) = ink_bounds(parts[2].1);
    let gap = arc.segment_inset() - arc.cap_sweep();

    assert!((above_start - arc.start_angle() - gap * 0.5).abs() < 1e-4);
    assert!((thumb_start - (above_start + above_sweep) - gap).abs() < 1e-4);
    assert!((below_start - (thumb_start + thumb_sweep) - gap).abs() < 1e-4);
    assert!(
        (below_start + below_sweep + gap * 0.5 - (arc.start_angle() + arc.sweep())).abs() < 1e-4,
        "the track has to end where it should"
    );
}

#[test]
fn a_segment_shorter_than_its_stroke_becomes_a_shrinking_dot() {
    let arc = indicator_arc(LARGE_RADIUS_DP);
    let parts = indicator_segments(
        arc,
        IndicatorGeometry {
            thumb: 0.7,
            offset: 0.0,
        },
        1.0,
    );
    match parts[0].1 {
        IndicatorSegment::Dot { radius, alpha, .. } => {
            assert!(
                radius <= arc.width() * 0.5,
                "a dot never exceeds the stroke"
            );
            assert!(alpha < 1.0, "it fades on the same fraction as it shrinks");
        }
        IndicatorSegment::Arc { sweep, .. } => {
            assert!(sweep <= 0.0, "an arc this short should have been a dot");
        }
    }
}

#[test]
fn fading_the_indicator_fades_every_piece_of_it() {
    let arc = indicator_arc(LARGE_RADIUS_DP);
    let geometry = IndicatorGeometry {
        thumb: 0.4,
        offset: 0.3,
    };
    for (_, segment) in indicator_segments(arc, geometry, 0.25) {
        let alpha = match segment {
            IndicatorSegment::Arc { alpha, .. } => alpha,
            IndicatorSegment::Dot { alpha, .. } => alpha,
        };
        assert!(alpha <= 0.25 + 1e-6, "{segment:?}");
    }
}

#[test]
fn a_display_too_small_to_hold_the_track_degrades_instead_of_panicking() {
    let tiny = indicator_arc(1.0);
    assert_eq!(tiny.centreline(), 0.0);
    assert_eq!(tiny.sweep(), 0.0);
    assert_eq!(tiny.cap_sweep(), 0.0);
    let parts = indicator_segments(
        tiny,
        IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        },
        1.0,
    );
    for (_, segment) in parts {
        assert!(matches!(segment, IndicatorSegment::Arc { sweep: 0.0, .. }));
    }
}

const VIEWPORT: f32 = 400.0;

fn list(visible: &[IndicatorItem]) -> ScalingList<'_> {
    ScalingList {
        visible,
        total: 10,
        viewport: VIEWPORT,
        before_padding: 0.0,
        after_padding: 0.0,
    }
}

fn row(index: usize, start_offset: f32) -> IndicatorItem {
    IndicatorItem {
        index,
        start_offset,
        size: 100.0,
    }
}

#[test]
fn a_row_flush_with_the_top_of_the_screen_is_a_whole_index() {
    let rows = [row(3, -200.0), row(6, 100.0)];
    assert_eq!(decimal_first_item_index(list(&rows)), 3.0);
}

#[test]
fn a_row_half_off_the_top_reads_half_an_index() {
    let rows = [row(3, -250.0), row(6, 100.0)];
    assert_eq!(decimal_first_item_index(list(&rows)), 3.5);
}

#[test]
fn the_last_index_counts_how_much_of_the_row_is_on_screen() {
    let rows = [row(3, -200.0), row(6, 150.0)];
    assert_eq!(decimal_last_item_index(list(&rows)), 6.5);
}

#[test]
fn the_padding_outside_the_list_counts_only_at_the_end_it_belongs_to() {
    let rows = [row(0, -250.0), row(9, 150.0)];
    let padded = ScalingList {
        before_padding: 80.0,
        after_padding: 60.0,
        ..list(&rows)
    };
    assert!((decimal_first_item_index(padded) - 130.0 / 180.0).abs() < 1e-6);
    assert!((decimal_last_item_index(padded) - (9.0 + 0.3125)).abs() < 1e-6);

    let inner = [row(3, -250.0), row(6, 150.0)];
    let inner = ScalingList {
        before_padding: 80.0,
        after_padding: 60.0,
        ..list(&inner)
    };
    assert_eq!(decimal_first_item_index(inner), 3.5);
    assert_eq!(decimal_last_item_index(inner), 6.5);
}

#[test]
fn the_thumb_is_the_share_of_the_items_on_screen_not_of_the_pixels() {
    let rows = [row(3, -250.0), row(8, 150.0)];
    let mut thumb = ThumbLength::default();
    assert!((thumb.of(list(&rows)) - 0.5).abs() < 1e-6);
}

#[test]
fn the_thumb_is_clamped_at_both_ends_however_long_the_list_is() {
    let rows = [row(3, -250.0), row(4, 150.0)];
    let mut short = ThumbLength::default();
    assert_eq!(short.of(list(&rows)), INDICATOR_MIN_THUMB);

    let rows = [row(0, -250.0), row(9, 150.0)];
    let mut long = ThumbLength::default();
    assert_eq!(long.of(list(&rows)), INDICATOR_MAX_THUMB);
}

#[test]
fn the_thumb_is_measured_once_and_then_only_when_the_list_changes_length() {
    let mut thumb = ThumbLength::default();
    let five = [row(3, -250.0), row(8, 150.0)];
    assert!((thumb.of(list(&five)) - 0.5).abs() < 1e-6);

    let three = [row(3, -250.0), row(6, 150.0)];
    assert!(
        (thumb.of(list(&three)) - 0.5).abs() < 1e-6,
        "the window shrank but the list did not, so the thumb holds"
    );

    let longer = ScalingList {
        total: 20,
        ..list(&three)
    };
    assert_eq!(thumb.of(longer), INDICATOR_MIN_THUMB);

    thumb.forget();
    assert!((thumb.of(list(&five)) - 0.5).abs() < 1e-6);
}

#[test]
fn the_position_is_how_many_items_are_left_not_how_far_the_pixels_went() {
    let rows = [row(3, -250.0), row(6, 150.0)];
    assert!((position_fraction(list(&rows)) - 0.5).abs() < 1e-6);
}

#[test]
fn a_list_at_the_top_puts_the_thumb_at_the_top_and_one_at_the_end_at_the_end() {
    let mut thumb = ThumbLength::default();
    let top = [row(0, -200.0), row(3, 150.0)];
    let geometry = scaling_list_geometry(&mut thumb, list(&top)).unwrap();
    assert_eq!(geometry.offset, 0.0);

    let mut thumb = ThumbLength::default();
    let end = [row(6, -250.0), row(9, 100.0)];
    let geometry = scaling_list_geometry(&mut thumb, list(&end)).unwrap();
    assert_eq!(decimal_last_item_index(list(&end)), 10.0);
    assert!((geometry.offset + geometry.thumb - 1.0).abs() < 1e-6);
}

#[test]
fn a_list_with_nothing_on_screen_has_no_indicator() {
    let mut thumb = ThumbLength::default();
    assert_eq!(scaling_list_geometry(&mut thumb, list(&[])), None);
    let rows = [row(3, -250.0)];
    let empty = ScalingList {
        total: 0,
        ..list(&rows)
    };
    assert_eq!(scaling_list_geometry(&mut thumb, empty), None);
}

#[test]
fn the_two_models_disagree_the_moment_the_rows_are_not_all_the_same_height() {
    let heights: Vec<f32> = std::iter::once(600.0).chain([100.0; 9]).collect();
    let content: f32 = heights.iter().sum();
    let pixel = indicator_geometry(content, VIEWPORT, 0.0).unwrap();

    let rows = [row(0, -200.0), row(3, 100.0)];
    let mut thumb = ThumbLength::default();
    let wear = scaling_list_geometry(&mut thumb, list(&rows)).unwrap();

    assert!((pixel.thumb - INDICATOR_MIN_THUMB).abs() < 1e-6);
    assert!((wear.thumb - 0.4).abs() < 1e-6, "{wear:?}");
}

#[test]
fn a_reported_row_is_not_the_row_as_it_is_drawn() {
    let mut out = Vec::new();
    let density = 2.0;
    let viewport = 227.0;
    for (height, carried) in [(51.5, 0.5), (52.0, 0.0)] {
        scaling_list_items(viewport, density, [(20.0, height)], &mut out);
        let drawn =
            crate::round_scaling_list::place_row(viewport, 20.0, height, density).expect("placed");
        let item = out.first().expect("on screen");
        assert!(
            (item.start_offset - (drawn.top * density - carried - 227.0)).abs() < 1e-4,
            "{height}dp: reported {} against drawn {}",
            item.start_offset,
            drawn.top * density
        );
        assert_eq!(item.size, (drawn.height * density).round());
    }
}

#[test]
fn a_list_that_does_not_scale_its_rows_reports_them_at_full_height() {
    let mut wear = Vec::new();
    let mut still = Vec::new();
    let rows = [(4.0, 52.0), (60.0, 52.0), (116.0, 52.0)];
    scaling_list_items(227.0, 2.0, rows, &mut wear);
    scaling_list_items_with(
        ScalingParams::WEAR.reduced_motion(),
        227.0,
        2.0,
        rows,
        &mut still,
    );
    assert_eq!(wear.len(), still.len());
    assert!(
        wear[0].size < still[0].size,
        "the top row shrinks under the Wear ramp and not under a stilled \
         one: {} vs {}",
        wear[0].size,
        still[0].size
    );
    assert_eq!(still[0].size, 104.0, "52dp at density 2, unscaled");
    let mut default = Vec::new();
    scaling_list_items_with(ScalingParams::WEAR, 227.0, 2.0, rows, &mut default);
    assert_eq!(default, wear);
}

#[test]
fn the_window_is_the_rows_that_still_meet_the_display() {
    let mut out = Vec::new();
    let rows: Vec<(f32, f32)> = (0..10)
        .map(|index| (index as f32 * 40.0 - 100.0, 40.0))
        .collect();
    scaling_list_items(227.0, 2.0, rows.iter().copied(), &mut out);
    let indices: Vec<usize> = out.iter().map(|item| item.index).collect();
    assert_eq!(indices, vec![2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn invalid_scaling_list_input_never_produces_a_non_finite_thumb() {
    let mut out = Vec::new();
    scaling_list_items(f32::NAN, 2.0, [(0.0, 40.0)], &mut out);
    assert!(out.is_empty());
    scaling_list_items(227.0, f32::NAN, [(0.0, 40.0)], &mut out);
    assert!(out.is_empty());

    let rows = [
        IndicatorItem {
            index: 0,
            start_offset: f32::NAN,
            size: 0.0,
        },
        IndicatorItem {
            index: 3,
            start_offset: f32::INFINITY,
            size: -1.0,
        },
    ];
    let mut thumb = ThumbLength::default();
    let geometry = scaling_list_geometry(&mut thumb, list(&rows)).expect("a geometry");
    assert!(
        geometry.thumb.is_finite() && geometry.offset.is_finite(),
        "{geometry:?}"
    );
    assert!(geometry.thumb >= INDICATOR_MIN_THUMB && geometry.thumb <= INDICATOR_MAX_THUMB);
    assert!(geometry.offset >= 0.0 && geometry.offset <= 1.0);
}

#[test]
fn invalid_public_inputs_never_emit_non_finite_draw_values() {
    for radius in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
        let arc = indicator_arc(radius);
        assert_eq!(arc.sweep(), 0.0);
        assert_eq!(arc.segment_inset(), 0.0);
    }

    let parts = indicator_segments(
        indicator_arc(LARGE_RADIUS_DP),
        IndicatorGeometry {
            thumb: f32::NAN,
            offset: f32::INFINITY,
        },
        f32::NAN,
    );
    for (_, part) in parts {
        match part {
            IndicatorSegment::Arc {
                start,
                sweep,
                alpha,
            } => assert!(start.is_finite() && sweep.is_finite() && alpha == 0.0),
            IndicatorSegment::Dot {
                angle,
                radius,
                alpha,
            } => assert!(angle.is_finite() && radius.is_finite() && alpha == 0.0),
        }
    }
}

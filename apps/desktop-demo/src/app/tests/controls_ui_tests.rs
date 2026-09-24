use super::*;

const STAGE: Size = Size {
    width: 360.0,
    height: STAGE_HEIGHT,
};

#[test]
fn projection_maps_scene_origin_near_stage_centre() {
    let centre = project([0.0, CAMERA_TARGET_Y, 0.0], STAGE);
    assert!((centre.x - STAGE.width * 0.5).abs() < 0.01);
    assert!((centre.y - STAGE.height * 0.5).abs() < 0.01);
}

#[test]
fn projection_keeps_rail_ends_inside_the_stage() {
    let left = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
    let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
    assert!(left.x > 0.0 && left.x < STAGE.width);
    assert!(right.x > left.x && right.x < STAGE.width);
}

#[test]
fn slider_value_follows_pointer_across_the_rail() {
    let left = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
    let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
    assert_eq!(slider_value_at(left, STAGE), 0.0);
    assert_eq!(slider_value_at(right, STAGE), 1.0);
    let middle = Point {
        x: (left.x + right.x) * 0.5,
        y: left.y,
    };
    assert!((slider_value_at(middle, STAGE) - 0.5).abs() < 1e-4);
}

#[test]
fn slider_value_clamps_outside_the_rail() {
    assert_eq!(slider_value_at(Point { x: -400.0, y: 0.0 }, STAGE), 0.0);
    assert_eq!(slider_value_at(Point { x: 900.0, y: 0.0 }, STAGE), 1.0);
}

#[test]
fn slider_value_is_zero_for_a_stage_that_was_never_laid_out() {
    assert_eq!(
        slider_value_at(Point { x: 0.0, y: 0.0 }, Size::default()),
        0.0
    );
}

#[test]
fn a_press_snaps_down_so_a_click_inside_one_batch_still_shows() {
    assert_eq!(press_response(PointerEventKind::Down), PressResponse::Snap);
    assert_eq!(press_response(PointerEventKind::Up), PressResponse::Release);
    assert_eq!(
        press_response(PointerEventKind::Cancel),
        PressResponse::Release
    );
    assert_eq!(
        press_response(PointerEventKind::Exit),
        PressResponse::Release
    );
    assert_eq!(press_response(PointerEventKind::Move), PressResponse::Hold);
    assert_eq!(press_response(PointerEventKind::Enter), PressResponse::Hold);
    assert_eq!(
        press_response(PointerEventKind::Scroll),
        PressResponse::Hold
    );
}

#[test]
fn grid_columns_narrow_to_one_and_assume_wide_before_the_first_layout() {
    assert_eq!(grid_columns(0.0), 3);
    assert_eq!(grid_columns(1200.0), 3);
    assert_eq!(grid_columns(WIDE_GRID_MIN_WIDTH), 3);
    assert_eq!(grid_columns(WIDE_GRID_MIN_WIDTH - 1.0), 2);
    assert_eq!(grid_columns(MEDIUM_GRID_MIN_WIDTH), 2);
    assert_eq!(grid_columns(MEDIUM_GRID_MIN_WIDTH - 1.0), 1);
    assert_eq!(grid_columns(320.0), 1);
}

#[test]
fn every_grid_column_count_covers_all_six_controls() {
    for width in [320.0, 700.0, 1200.0] {
        let rows: Vec<&[ControlKind]> = CONTROL_KINDS.chunks(grid_columns(width)).collect();
        let placed: Vec<ControlKind> = rows.concat();
        assert_eq!(
            placed,
            CONTROL_KINDS.to_vec(),
            "width {width} drops a control"
        );
    }
}

#[test]
fn a_stage_narrower_than_the_design_aspect_scales_the_scene_down() {
    let wide = Size {
        width: STAGE_HEIGHT * STAGE_DESIGN_ASPECT * 2.0,
        height: STAGE_HEIGHT,
    };
    assert_eq!(stage_unit(wide), STAGE_HEIGHT);
    let narrow = Size {
        width: STAGE_HEIGHT,
        height: STAGE_HEIGHT,
    };
    assert!(stage_unit(narrow) < STAGE_HEIGHT);
    let rail_end = project([-SLIDER_TRAVEL, SLIDER_LIFT, 0.0], narrow);
    assert!(
        rail_end.x > 0.0,
        "the rail must stay inside a square stage, got {}",
        rail_end.x
    );
}

#[test]
fn wrap_angle_folds_full_turns() {
    assert!((wrap_angle(0.4) - 0.4).abs() < 1e-5);
    assert!((wrap_angle(0.4 + TAU) - 0.4).abs() < 1e-4);
    assert!((wrap_angle(PI + 0.2) - (-PI + 0.2)).abs() < 1e-4);
}

#[test]
fn dial_value_turns_with_the_pointer_and_clamps() {
    let quarter = DIAL_SWEEP * 0.25;
    assert!((dial_value_after(0.4, 0.0, quarter) - 0.65).abs() < 1e-4);
    assert!((dial_value_after(0.4, quarter, 0.0) - 0.15).abs() < 1e-4);
    assert_eq!(dial_value_after(0.95, 0.0, quarter), 1.0);
    assert_eq!(dial_value_after(0.05, quarter, 0.0), 0.0);
}

#[test]
fn pointer_angle_ignores_the_pivot_dead_zone() {
    let pivot = project([0.0, DIAL_PIVOT_Y, 0.0], STAGE);
    assert!(pointer_angle(pivot, STAGE).is_none());
    let away = Point {
        x: pivot.x + 60.0,
        y: pivot.y,
    };
    assert!(pointer_angle(away, STAGE).expect("angle").abs() < 1e-5);
}

#[test]
fn pointer_tilt_is_centred_and_bounded() {
    let centre = Point {
        x: STAGE.width * 0.5,
        y: STAGE.height * 0.5,
    };
    assert_eq!(pointer_tilt(centre, STAGE), (0.0, 0.0));
    let corner = Point { x: 0.0, y: 0.0 };
    assert!((pointer_tilt(corner, STAGE).0 - TILT_YAW).abs() < 1e-5);
    assert!((pointer_tilt(corner, STAGE).1 - TILT_PITCH).abs() < 1e-5);
    assert_eq!(pointer_tilt(centre, Size::default()), (0.0, 0.0));
}

#[test]
fn a_switch_takes_the_value_its_toggleable_hands_over() {
    let off = ControlState::initial(ControlKind::Toggle);
    assert!(!off.on);
    assert!(off.toggled(true).on);
    assert!(!off.toggled(true).toggled(false).on);
}

#[test]
fn a_click_counts_one_press() {
    let button = ControlState::initial(ControlKind::PushButton);
    assert_eq!(button.presses, 0);
    assert_eq!(button.pressed_once().presses, 1);
    assert_eq!(button.pressed_once().pressed_once().presses, 2);
    assert_eq!(
        ControlState {
            presses: u32::MAX,
            ..button
        }
        .pressed_once()
        .presses,
        u32::MAX
    );
}

#[test]
fn a_drag_starts_the_slider_at_the_pressed_position_and_leaves_the_dial() {
    let right = project([SLIDER_TRAVEL, SLIDER_LIFT, 0.0], STAGE);
    let slider = ControlState::initial(ControlKind::Slider);
    assert_eq!(
        drag_start_value(ControlKind::Slider, slider, right, STAGE).value,
        1.0
    );
    let dial = ControlState::initial(ControlKind::VolumeDial);
    assert_eq!(
        drag_start_value(ControlKind::VolumeDial, dial, right, STAGE),
        dial
    );
}

#[test]
fn only_the_dragged_controls_are_continuous() {
    assert!(ControlKind::Slider.is_continuous());
    assert!(ControlKind::VolumeDial.is_continuous());
    for kind in [
        ControlKind::Checkmark,
        ControlKind::Toggle,
        ControlKind::LeverSwitch,
        ControlKind::PushButton,
    ] {
        assert!(!kind.is_continuous(), "{kind:?} is clicked, not dragged");
        assert!(kind.role().is_some(), "{kind:?} needs a semantics role");
    }
}

#[test]
fn a_value_is_clamped_to_its_track() {
    let slider = ControlState::initial(ControlKind::Slider);
    assert_eq!(slider.with_value(1.4).value, 1.0);
    assert_eq!(slider.with_value(-0.3).value, 0.0);
    assert_eq!(slider.with_value(0.42).value, 0.42);
}

#[test]
fn drag_turns_the_dial_and_records_the_new_angle() {
    let pivot = project([0.0, DIAL_PIVOT_Y, 0.0], STAGE);
    let below = Point {
        x: pivot.x,
        y: pivot.y + 60.0,
    };
    let state = ControlState::initial(ControlKind::VolumeDial);
    let tracking = DragTracking {
        active: true,
        angle: 0.0,
    };
    let (next, tracked) = drag_state(ControlKind::VolumeDial, state, tracking, below, STAGE);
    assert!(next.value > state.value);
    assert!((tracked.angle - PI * 0.5).abs() < 1e-4);
    assert!(tracked.active);
}

#[test]
fn drag_leaves_switches_untouched() {
    let state = ControlState::initial(ControlKind::Checkmark);
    let tracking = DragTracking {
        active: true,
        angle: 0.2,
    };
    let (next, tracked) = drag_state(
        ControlKind::Checkmark,
        state,
        tracking,
        Point { x: 10.0, y: 10.0 },
        STAGE,
    );
    assert_eq!(next, state);
    assert_eq!(tracked.angle, 0.2);
}

#[test]
fn labels_report_the_state_the_footer_shows() {
    assert_eq!(
        ControlState::initial(ControlKind::Checkmark).label(ControlKind::Checkmark),
        "Checked"
    );
    assert_eq!(
        ControlState::initial(ControlKind::Slider).label(ControlKind::Slider),
        "35 %"
    );
    assert_eq!(
        ControlState::initial(ControlKind::VolumeDial).label(ControlKind::VolumeDial),
        "40 %"
    );
    assert_eq!(
        ControlState::initial(ControlKind::Toggle).label(ControlKind::Toggle),
        "Off"
    );
    assert_eq!(
        ControlState::initial(ControlKind::PushButton).label(ControlKind::PushButton),
        "0 presses"
    );
    let once = ControlState {
        presses: 1,
        ..ControlState::initial(ControlKind::PushButton)
    };
    assert_eq!(once.label(ControlKind::PushButton), "1 press");
}

#[test]
fn switch_scene_values_are_binary_and_dials_are_continuous() {
    let on = ControlState {
        on: true,
        value: 0.3,
        presses: 0,
    };
    assert_eq!(on.scene_value(ControlKind::Toggle), 1.0);
    assert_eq!(
        ControlState { on: false, ..on }.scene_value(ControlKind::Toggle),
        0.0
    );
    assert_eq!(on.scene_value(ControlKind::Slider), 0.3);
}

#[test]
fn every_kind_has_a_distinct_scene_index() {
    let mut seen = Vec::new();
    for kind in CONTROL_KINDS {
        let index = kind.scene_index();
        assert!(!seen.contains(&index.to_bits()));
        seen.push(index.to_bits());
    }
    assert_eq!(seen.len(), 6);
}

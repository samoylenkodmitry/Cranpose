use super::*;

#[test]
fn loupe_rises_for_every_handle_interaction() {
    let line_bottom = 100.0;
    let line_height = 20.0;
    let on_line = loupe_target_for_drag(Point { x: 40.0, y: 95.0 }, line_bottom, line_height)
        .expect("a finger on the line raises the loupe");
    assert_eq!(on_line.focus_x, 40.0);
    assert_eq!(on_line.line_mid_y, 90.0);
    let on_dot = loupe_target_for_drag(Point { x: 40.0, y: 106.0 }, line_bottom, line_height)
        .expect("a dot grab raises the loupe too");
    assert_eq!(on_dot.line_mid_y, 90.0);
    assert!(loupe_target_for_drag(Point { x: 40.0, y: 70.0 }, line_bottom, line_height).is_some());
}

#[test]
fn growth_starts_at_the_handle_as_a_vertical_capsule() {
    assert_eq!(
        loupe_pose(0.0, LoupePhase::Birth),
        LoupePose {
            width_frac: 0.0,
            height_frac: 0.0,
            rise_frac: 0.0,
        }
    );
    let emerging = loupe_pose(0.20, LoupePhase::Birth);
    let width = LOUPE_WIDTH * emerging.width_frac;
    let height = LOUPE_HEIGHT * emerging.height_frac;
    assert!(
        height > width,
        "birth must be vertically elongated: {emerging:?}"
    );
    assert!(emerging.rise_frac < 0.5);

    let settled = loupe_pose(1.0, LoupePhase::Birth);
    assert_eq!(settled.width_frac, 1.0);
    assert_eq!(settled.height_frac, 1.0);
    assert_eq!(settled.rise_frac, 1.0);
}

#[test]
fn width_can_overshoot_without_inflating_height_or_rise() {
    let pose = loupe_pose(1.04, LoupePhase::Birth);
    assert!((pose.width_frac - 1.04).abs() < 1.0e-6);
    assert!((pose.height_frac - 1.0).abs() < 1e-6);
    assert!((pose.rise_frac - 1.0).abs() < 1e-6);
}

#[test]
fn grow_carries_energy_and_release_uses_the_measured_clock() {
    let AnimationType::Spring(grow) = loupe_grow_spring() else {
        panic!("loupe grow must use a spring");
    };
    assert!(grow.damping_ratio < 1.0, "birth must carry visible energy");
    let AnimationType::Tween(collapse) = loupe_collapse_tween() else {
        panic!("loupe collapse must use the measured linear clock");
    };
    assert_eq!(collapse.duration_millis, LOUPE_COLLAPSE_MS);
}

#[test]
fn shell_and_optics_share_one_continuous_progress() {
    let early = loupe_pose(0.10, LoupePhase::Birth);
    let middle = loupe_pose(0.50, LoupePhase::Birth);
    let late = loupe_pose(0.90, LoupePhase::Birth);
    assert!(early.width_frac < middle.width_frac && middle.width_frac < late.width_frac);
    assert!(early.height_frac < middle.height_frac && middle.height_frac < late.height_frac);
    assert!(early.rise_frac < middle.rise_frac && middle.rise_frac < late.rise_frac);
    assert!(loupe_optical_activity(0.10) < loupe_optical_activity(0.50));
    assert!(loupe_optical_activity(0.50) < loupe_optical_activity(0.90));
    assert_eq!(loupe_optical_activity(1.0), 1.0);
}

#[test]
fn loupe_effect_relaxes_optics_without_enabling_backdrop_blur() {
    let relaxed = LiquidLoupeSpec {
        activity: 0.65,
        ..LiquidLoupeSpec::default()
    };
    let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &relaxed);
    let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
        panic!("loupe must be a bare shader effect");
    };
    let u = shader.uniforms();
    assert!((u[9] - 0.34 * relaxed.activity).abs() < 1e-6);
    assert!((u[83] - (1.0 + (LOUPE_MAGNIFICATION - 1.0) * relaxed.activity)).abs() < 1e-6);
    assert!(
        (u[cranpose_ui_graphics::GLASS_DISPERSION_UNIFORM] - relaxed.dispersion * relaxed.activity)
            .abs()
            < 1e-6
    );
    assert!((u[11] - relaxed.highlight * relaxed.activity).abs() < 1e-6);
    assert_eq!(u[28], relaxed.activity);
    assert_eq!(u[90], relaxed.activity);
    assert_eq!(u[cranpose_ui_graphics::GLASS_BLUR_RADIUS_UNIFORM], 0.0);

    let grown = LiquidLoupeSpec::default();
    let effect = liquid_loupe_effect((LOUPE_WIDTH, LOUPE_HEIGHT), &grown);
    let cranpose_ui_graphics::RenderEffect::Shader { shader } = effect else {
        panic!("loupe must be a bare shader effect");
    };
    let u = shader.uniforms();
    assert_eq!(u[80], 1.0, "loupe mode on");
    assert!(
        (u[83] - LOUPE_MAGNIFICATION).abs() < 1e-6,
        "full magnification"
    );
    assert_eq!(u[81], 0.0, "focus x on the bubble center");
    assert!((u[82] - 75.0).abs() < 1e-6, "focus 75dp below the center");
    assert_eq!(
        &u[0..2],
        &[LOUPE_WIDTH, LOUPE_HEIGHT],
        "container = node dp"
    );
    assert_eq!(u[6], -1.0, "capsule sentinel");
    assert!(
        shader.input_padding() >= 75.0,
        "capture must cover the offset focus, got {}",
        shader.input_padding()
    );
}

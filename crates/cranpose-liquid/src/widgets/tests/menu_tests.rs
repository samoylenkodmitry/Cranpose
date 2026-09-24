use super::*;

#[test]
fn a_scope_keeps_actions_with_their_items() {
    let selected = Rc::new(Cell::new(false));
    let selected_by_action = Rc::clone(&selected);
    let entries = collect_entries(|scope| {
        scope.header("Section");
        scope.separator();
        scope.item(LiquidMenuItem::new("Open"), move || {
            selected_by_action.set(true);
        });
    });

    assert_eq!(entries.len(), 2);
    assert!(entries[0].item.header);
    assert!(entries[1].item.section_start);
    entries[1].action.as_ref().unwrap()();
    assert!(selected.get());
}

#[test]
fn dropdown_spec_builders_preserve_menu_configuration() {
    let menu = LiquidMenuSpec { width: 180.0 };
    let absorbed = LiquidMenuAbsorbedSource::new(
        Rect::from_origin_size(Point::ZERO, Size::new(44.0, 44.0)),
        crate::widgets::GlassButtonSpec::default(),
        44.0,
        crate::icons::SEARCH,
    );
    let spec = LiquidDropdownMenuSpec::default()
        .menu(menu)
        .absorbed(vec![absorbed]);
    assert_eq!(spec.menu, menu);
    assert_eq!(spec.absorbed.len(), 1);
}

#[test]
fn menu_rows_use_the_reference_leading_grid_and_vertical_rhythm() {
    assert_eq!(ROW_PADDING_X, 20.0);
    assert_eq!(CHECK_COLUMN, 24.0);
    assert_eq!(ICON_SIZE, 24.0);
    assert_eq!(ICON_GAP, 12.0);

    let check_center = ROW_PADDING_X + 8.0;
    let icon_center = ROW_PADDING_X + CHECK_COLUMN + ICON_SIZE * 0.5;
    let label_start = ROW_PADDING_X + CHECK_COLUMN + ICON_SIZE + ICON_GAP;
    assert_eq!((check_center, icon_center, label_start), (28.0, 56.0, 80.0));

    let row_height = ICON_SIZE + ROW_PADDING_Y * 2.0;
    assert!((42.0..=43.0).contains(&row_height));
    assert_eq!(MENU_CONTENT_INSET_Y, 9.5);
    let two_row_panel_height = row_height * 2.0 + MENU_CONTENT_INSET_Y * 2.0;
    assert!((103.5..=104.5).contains(&two_row_panel_height));
}

#[test]
fn menu_geometry_keeps_the_source_cluster_horizontal_before_card_growth() {
    let anchor = MenuShape::capsule(228.0, 22.0, 44.0, 44.0);
    let absorbed = [MenuShape::capsule(176.0, 22.0, 44.0, 44.0)];
    let target = MenuShape {
        center_x: 125.0,
        center_y: 52.0,
        width: 250.0,
        height: 104.0,
        radius: 32.0,
    };
    let pose = |appear| menu_morph_geometry(true, appear, anchor, &absorbed, target).primary;

    let initial = pose(0.0);
    assert_eq!((initial.width, initial.height), (44.0, 44.0));

    let merged = pose(0.028_576);
    assert_eq!(merged, initial);
    let source = menu_source_shape(anchor, &absorbed, target);
    assert_eq!(source.width, 96.0);
    assert!((82.5..=82.6).contains(&source.height));
    assert_eq!(source.center_y, anchor.center_y);
    assert_eq!(menu_absorbed_shape_presence(0.0), 1.0);
    assert_eq!(menu_absorbed_shape_presence(0.30), 0.0);

    let early = pose(0.070_208);
    let middle = pose(0.199_019);
    let broad = pose(0.539_174);
    assert_eq!(early, initial);
    assert_eq!(middle.width, source.width);
    assert!(middle.height >= source.height);
    assert!(broad.width > middle.width && broad.height <= target.height * 1.1);
    assert!(middle.width > middle.height);
    assert!(broad.width > broad.height * 2.0);

    let swell = pose(0.701_903);
    assert!(
        (252.0..=259.0).contains(&swell.width) && (102.0..=106.0).contains(&swell.height),
        "the broad body must overshoot horizontally without inflating vertically: {swell:?}"
    );
    assert!(menu_ellipse_blend(0.25) > 0.3);
    assert_eq!(menu_ellipse_blend(0.0), 0.0);
    assert_eq!(menu_ellipse_blend(1.0), 0.0);

    let overshoot = pose(1.08);
    assert!((250.0..=254.0).contains(&overshoot.width));
    assert!((104.0..=106.0).contains(&overshoot.height));
    assert_eq!(MENU_RADIUS, 32.0);
    assert!((0.045..=0.055).contains(&MENU_GROW_DELAY));
    assert!((55.0..=70.0).contains(&MENU_GROW_STIFFNESS));
}

#[test]
fn menu_open_spring_departs_early_then_settles_without_a_dead_interval() {
    let (source_phase, _) =
        cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.054);
    assert!(
        source_phase > MENU_GROW_DELAY,
        "the departing oval must be visible by the target's early frame: {source_phase}"
    );
    let (broad_phase, _) =
        cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.180);
    assert!(
        (0.35..=0.60).contains(&broad_phase),
        "the broad menu body must be established by 180ms: {broad_phase}"
    );
    let (settled_phase, _) =
        cranpose_animation::advance_spring(0.0, 0.0, 1.0, 0.78, MENU_GROW_STIFFNESS, 0.600);
    assert!(settled_phase > 0.95);
}

#[test]
fn menu_body_uses_the_shared_vertical_rebound_path() {
    let anchor = MenuShape::capsule(228.0, 22.0, 44.0, 44.0);
    let absorbed = [MenuShape::capsule(176.0, 22.0, 44.0, 44.0)];
    let target = MenuShape {
        center_x: 125.0,
        center_y: 52.0,
        width: 250.0,
        height: 104.0,
        radius: 32.0,
    };

    let source = menu_source_shape(anchor, &absorbed, target);
    for appear in [0.199_019, 0.296_780, 0.412_956, 0.539_174] {
        let geometry = menu_morph_geometry(true, appear, anchor, &absorbed, target);
        let phase = menu_geometry_phase(true, appear);
        let descent = smoothstep(0.10, 0.90, phase.path);
        let interpolated_y = source.center_y + (target.center_y - source.center_y) * descent;
        let expected_y = interpolated_y + menu_vertical_rebound(phase.path);
        assert!(
            (geometry.primary.center_y - expected_y).abs() < 0.001,
            "body and content must resolve the same rebound path: {geometry:?}"
        );
    }
    assert_eq!(menu_vertical_rebound(0.0), 0.0);
    assert!(menu_vertical_rebound(0.25) > 0.0);
    assert_eq!(menu_vertical_rebound(MENU_VERTICAL_REBOUND_END), 0.0);
}

#[test]
fn menu_close_reverses_through_a_smooth_oval() {
    let phase = menu_geometry_phase(false, 0.6);
    assert!(
        phase.width > 0.80,
        "the close must retain its broad body at mid-flight: {phase:?}"
    );
    assert!(
        44.0 + (250.0 - 44.0) * phase.width > 1.8 * (44.0 + (104.0 - 44.0) * phase.height),
        "the close must pass back through the wide oval in physical dimensions: {phase:?}"
    );
    assert!(
        menu_content_progress(false, 0.6, 1.0) > 0.75,
        "content must remain coherent through the initial deflation"
    );
    assert_eq!(menu_content_progress(false, 0.2, 1.0), 0.0);
    let rounded_volume = menu_geometry_phase(false, 0.21);
    assert!(
        rounded_volume.width > 0.35,
        "the terminal body must contract continuously into the anchor: {rounded_volume:?}"
    );
    assert!(
        44.0 + (250.0 - 44.0) * rounded_volume.width
            > 44.0 + (104.0 - 44.0) * rounded_volume.height,
        "the terminal body must stay smooth rather than forming a vertical leaf in physical dimensions: {rounded_volume:?}"
    );
}

#[test]
fn menu_content_materializes_early_and_is_sharp_by_settle() {
    let birth = menu_content_progress(true, 0.35, 0.25);
    assert!(
        birth > 0.02 && birth < 0.08,
        "rows must begin as a faint smudge after the blank birth phase: {birth}"
    );
    let mid = menu_content_progress(true, 0.55, 0.55);
    assert!(
        (0.55..0.70).contains(&mid),
        "rows must remain visibly soft at mid-flight: {mid}"
    );
    let settle = menu_content_progress(true, 1.0, 0.92);
    assert!(
        settle > 0.99,
        "rows must be effectively sharp when the shape settles: {settle}"
    );
    assert!(menu_content_blur(birth) > 13.0);
    assert!((7.0..8.0).contains(&menu_content_blur(mid)));
    assert!(menu_content_blur(settle) < 0.5);
    assert!((20.0..=32.0).contains(&MENU_REVEAL_STIFFNESS));
    assert!((0.34..=0.37).contains(&menu_content_alpha(0.10)));
    assert!((0.79..=0.82).contains(&menu_content_alpha(0.62)));
    assert_eq!(menu_content_alpha(1.0), 1.0);
    assert!((0.85..=0.87).contains(&menu_content_scale(0.30)));
    assert!((0.92..=0.93).contains(&menu_content_scale(0.62)));
    assert_eq!(menu_content_scale(1.0), 1.0);
}

#[test]
fn menu_surface_motion_is_smooth_and_capture_cadence_independent() {
    let merged = menu_surface_phase(true, 0.14, 0.0);
    assert_eq!(merged.anchor_presence, 0.0);
    assert_eq!(merged.glue, 0.0);
    let recoil = menu_surface_phase(true, 0.275, 0.0);
    assert_eq!(recoil.glue, 0.0);

    let early = menu_surface_phase(true, 0.40, 0.25);
    assert!(
        early.anchor_presence == 0.0,
        "the primary alone owns the anchor recoil: {early:?}"
    );
    assert_eq!(early.glue, 0.0);
    assert!(early.wobble <= 0.10);
    assert!(early.bulge <= 0.40);
    assert_eq!(early, menu_surface_phase(true, 0.40, 0.25));

    let closing = menu_surface_phase(false, 0.6, 0.68);
    assert_eq!(closing.anchor_presence, 0.0);
    assert_eq!(closing.glue, 0.0);
    assert!(closing.wobble <= 0.05);
    assert!(
        closing.bulge <= 0.30,
        "close must remain smooth: {closing:?}"
    );
}

#[test]
fn menu_trigger_backdrop_unmounts_during_the_first_absorption_frame() {
    assert!((30..=40).contains(&MENU_TRIGGER_ABSORPTION_MS));
    assert_eq!(MENU_TRIGGER_RESTORE_DELAY_MS, 205);
}

#[test]
fn absorbed_source_foreground_stays_readable_then_stretches_into_the_surface() {
    let source = LiquidMenuAbsorbedSource::new(
        Rect {
            x: 10.0,
            y: 20.0,
            width: 44.0,
            height: 44.0,
        },
        crate::widgets::GlassButtonSpec::glass()
            .with_icon_backplate(Color::from_rgb_u8(0, 122, 255))
            .with_content_color(Color::WHITE),
        44.0,
        "M0 0",
    );
    assert_eq!(source.rect.width, 44.0);
    assert_eq!(source.diameter, 44.0);
    assert_eq!(source.icon_path, "M0 0");

    let source = menu_absorbed_visual_phase(0.0, 0.0);
    assert_eq!(source.foreground_alpha, 1.0);
    assert_eq!(source.backdrop_alpha, 0.0);

    let crisp = menu_absorbed_visual_phase(0.20, 0.20);
    assert_eq!(crisp.foreground_alpha, 1.0);
    assert_eq!(crisp.backdrop_alpha, 0.0);
    assert_eq!(crisp.foreground_blur, 0.0);
    assert!((0.76..=0.78).contains(&crisp.scale_x));
    assert!((0.76..=0.78).contains(&crisp.scale_y));
    let dimmed = menu_absorbed_visual_phase(0.60, 0.30);
    assert!((0.38..=0.42).contains(&dimmed.foreground_alpha));

    let melt = menu_absorbed_visual_phase(0.95, 0.90);
    assert_eq!(melt.foreground_alpha, 0.0);
    assert_eq!(melt.backdrop_alpha, 0.62);
    assert!((0.92..=0.97).contains(&melt.scale_y));
    assert!((0.87..=0.905).contains(&melt.scale_x));

    let smear = menu_absorbed_visual_phase(0.72, 0.69);
    assert!((0.94..=0.98).contains(&smear.scale_y));
    assert!((0.89..=0.91).contains(&smear.scale_x));
    assert!((0.10..=0.18).contains(&smear.foreground_alpha));
    assert!((0.36..=0.44).contains(&smear.backdrop_alpha));
    let transition = menu_absorbed_visual_phase(0.42, 0.382);
    assert!((0.60..=0.75).contains(&transition.foreground_alpha));
    assert_eq!(transition.backdrop_alpha, 0.0);
    let settled = menu_absorbed_visual_phase(1.0, 1.0);
    assert_eq!(settled.foreground_alpha, 0.0);
    assert_eq!(settled.backdrop_alpha, 0.62);
    assert_eq!(MENU_SOURCE_FOREGROUND_HIDE_MS, 200);
    assert_eq!(MENU_SOURCE_FOREGROUND_RESTORE_DELAY_MS, 205);
}

#[test]
fn liquid_menu_item_builders_preserve_the_row_contract() {
    let item = LiquidMenuItem::new("Delete")
        .icon("M0 0")
        .checked(true)
        .destructive()
        .section_start();
    assert_eq!(item.label, "Delete");
    assert_eq!(item.icon, Some("M0 0"));
    assert!(item.checked);
    assert!(item.destructive);
    assert!(item.section_start);
    assert!(!item.header);

    let header = LiquidMenuItem::header("Show");
    assert_eq!(header.label, "Show");
    assert!(header.header);
}

#[test]
fn claimed_menu_gesture_streams_one_release_to_an_interactive_row() {
    let _runtime =
        cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
    let gesture = LiquidMenuGesture::new();
    let items = vec![LiquidMenuItem::header("Show"), LiquidMenuItem::new("Grid")];
    gesture.item_rect(0).set(Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 30.0,
    });
    gesture.item_rect(1).set(Rect {
        x: 10.0,
        y: 50.0,
        width: 100.0,
        height: 40.0,
    });

    gesture.begin(Point::new(80.0, 10.0));
    gesture.claim();
    gesture.move_to(Point::new(40.0, 65.0));
    let held = gesture.snapshot();
    assert!(held.active && held.claimed);
    assert_eq!(gesture.item_at(held.position, &items), Some(1));
    assert_eq!(gesture.item_at(Point::new(40.0, 35.0), &items), None);

    gesture.release(Point::new(40.0, 65.0));
    let released = gesture.snapshot();
    assert!(!released.active);
    assert_eq!(released.release, Some((1, Point::new(40.0, 65.0))));
    gesture.release(Point::new(40.0, 65.0));
    assert_eq!(gesture.snapshot().release, released.release);
}

#[test]
fn committing_a_row_dismisses_unless_the_row_keeps_the_menu_open() {
    let taps = Rc::new(Cell::new(0usize));
    let dismissals = Rc::new(Cell::new(0usize));
    let on_item: Rc<dyn Fn(usize)> = {
        let taps = Rc::clone(&taps);
        Rc::new(move |_| taps.set(taps.get() + 1))
    };
    let on_dismiss: Rc<dyn Fn()> = {
        let dismissals = Rc::clone(&dismissals);
        Rc::new(move || dismissals.set(dismissals.get() + 1))
    };

    commit_menu_row(0, false, &on_item, &on_dismiss);
    assert_eq!(
        (taps.get(), dismissals.get()),
        (1, 1),
        "an ordinary row dismisses"
    );

    commit_menu_row(1, true, &on_item, &on_dismiss);
    assert_eq!(
        (taps.get(), dismissals.get()),
        (2, 1),
        "a row that keeps the menu open runs its action and dismisses nothing"
    );
}

#[test]
fn a_row_is_committed_exactly_once_per_tap() {
    let dismissals = Rc::new(Cell::new(0usize));
    let on_item: Rc<dyn Fn(usize)> = Rc::new(|_| {});
    let on_dismiss: Rc<dyn Fn()> = {
        let dismissals = Rc::clone(&dismissals);
        Rc::new(move || dismissals.set(dismissals.get() + 1))
    };
    commit_menu_row(0, false, &on_item, &on_dismiss);
    assert_eq!(dismissals.get(), 1);
}

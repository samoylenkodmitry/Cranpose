use cranpose_ui::{
    TextOverflow,
    round_scaling_list::CentreAnchor,
    run_test_composition,
    widgets::{image::Painter, wear::scaling_list::rememberWearScalingListState},
};
use cranpose_ui_graphics::ImageBitmap;

#[test]
fn a_fresh_scaling_list_has_no_rows_and_so_contains_none() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        assert!(
            !state.contains_item(0),
            "a list that has measured nothing claimed to hold a row"
        );
        assert_eq!(state.visible_item_count(), 0);
    });
}

#[test]
fn a_scaling_list_reports_the_anchor_it_is_given() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        assert_eq!(state.anchor(), CentreAnchor::default());

        let moved = CentreAnchor {
            index: 4,
            offset: 12.0,
        };
        state.set_anchor(moved);
        assert_eq!(state.anchor(), moved, "the anchor did not take");
    });
}

#[test]
fn an_animation_to_a_row_the_list_does_not_have_is_not_started() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        state.animate_scroll_to_item(900, 0.0);
        assert_eq!(
            state.anchor(),
            CentreAnchor::default(),
            "the list moved towards a row it does not have"
        );
    });
}

#[test]
fn cancelling_an_animation_that_is_not_running_is_harmless() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        state.cancel_scroll_animation();
        state.cancel_scroll_animation();
        assert_eq!(state.anchor(), CentreAnchor::default());
    });
}

#[test]
fn a_distance_of_zero_is_reported_for_the_anchored_row_itself() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        assert_eq!(state.distance_to_item(0, 0.0), 0.0);
    });
}

#[test]
fn a_distance_ignores_an_offset_that_is_not_a_number() {
    run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        assert_eq!(
            state.distance_to_item(0, f32::NAN),
            state.distance_to_item(0, 0.0)
        );
    });
}

fn one_pixel() -> ImageBitmap {
    ImageBitmap::from_rgba8(1, 1, vec![255, 0, 0, 255]).expect("a one-pixel bitmap")
}

#[test]
fn a_bitmap_painter_hands_back_the_bitmap_it_was_built_from() {
    let painter = Painter::from_bitmap(one_pixel());
    let bitmap = painter.as_bitmap().expect("the painter lost its bitmap");
    assert_eq!((bitmap.width(), bitmap.height()), (1, 1));
}

#[test]
fn a_region_painter_still_reports_the_atlas_it_samples() {
    let painter = Painter::from_bitmap_region(
        one_pixel(),
        cranpose_ui_graphics::Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        Default::default(),
    );
    assert!(
        painter.as_bitmap().is_some(),
        "a region painter reported no source bitmap"
    );
}

#[test]
fn scale_down_reports_its_floor_and_the_other_policies_report_none() {
    assert_eq!(
        TextOverflow::ScaleDown {
            min_font_size_sp: 8.0
        }
        .scale_down_min_font_size_sp(),
        Some(8.0)
    );
    assert_eq!(TextOverflow::Clip.scale_down_min_font_size_sp(), None);
    assert_eq!(TextOverflow::Ellipsis.scale_down_min_font_size_sp(), None);
}

#[test]
fn a_scale_down_floor_that_cannot_be_drawn_at_is_normalised_away() {
    for impossible in [0.0, -4.0, f32::NAN] {
        let floor = TextOverflow::ScaleDown {
            min_font_size_sp: impossible,
        }
        .scale_down_min_font_size_sp()
        .expect("scale-down always reports a floor");
        assert!(floor > 0.0, "a floor of {impossible} normalised to {floor}");
    }
}

#[test]
fn the_wear_widgets_compose_against_a_scaling_list_state() {
    use cranpose_ui::{
        Modifier, measure_layout,
        widgets::wear::{
            scroll_indicator::{ScrollIndicator, ScrollIndicatorSpec},
            switch_button::{SwitchButtonNode, SwitchButtonSpec, SwitchColors, SwitchGraphic},
        },
    };

    let mut composition = run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        ScrollIndicator(Modifier::empty(), state, ScrollIndicatorSpec::default());
        SwitchButtonNode(
            Modifier::empty(),
            SwitchButtonSpec::default(),
            true,
            "Ring".to_string(),
            Some("Vibrate too".to_string()),
            || {},
        );
        SwitchGraphic(
            Modifier::empty(),
            SwitchButtonSpec::default(),
            SwitchColors::of(SwitchButtonSpec::default().colors, false),
        );
    });

    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measured = measure_layout(
        &mut applier,
        root,
        cranpose_ui_graphics::Size::new(400.0, 400.0),
    );
    applier.clear_runtime_handle();
    measured.expect("the wear screen measures");
}

#[test]
fn a_wear_button_composes_its_label() {
    use cranpose_ui::{
        Modifier,
        widgets::wear::button::{WearButton, WearButtonSpec},
    };

    run_test_composition(|| {
        WearButton(
            Modifier::empty(),
            WearButtonSpec::default(),
            "Ring".to_string(),
            None,
            || {},
        );
    });
}

#[test]
fn a_list_header_composes_its_label() {
    use cranpose_ui::{
        Modifier,
        widgets::wear::list_header::{ListHeader, ListHeaderSpec},
    };

    run_test_composition(|| {
        ListHeader(Modifier::empty(), ListHeaderSpec::default(), "Settings");
    });
}

#[test]
fn a_screen_scaffold_composes_its_content() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_ui::{
        Modifier,
        widgets::wear::scaffold::{ScreenScaffold, ScreenScaffoldSpec},
    };

    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    run_test_composition(move || {
        let counter = Rc::clone(&counter);
        let state = rememberWearScalingListState(CentreAnchor::default());
        ScreenScaffold(
            Modifier::empty(),
            state,
            ScreenScaffoldSpec::default(),
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(
        drawn.get(),
        1,
        "the screen scaffold did not compose its content"
    );
}

#[test]
fn a_switch_button_composes_without_a_secondary_label() {
    use cranpose_ui::{
        Modifier, measure_layout,
        widgets::wear::switch_button::{SwitchButton, SwitchButtonSpec},
    };

    let mut composition = run_test_composition(|| {
        SwitchButton(
            Modifier::empty(),
            SwitchButtonSpec::default(),
            false,
            "Ring".to_string(),
            None,
            |_next| {},
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measured = measure_layout(
        &mut applier,
        root,
        cranpose_ui_graphics::Size::new(400.0, 400.0),
    );
    applier.clear_runtime_handle();
    assert!(
        measured
            .expect("the switch row measures")
            .root_size()
            .height
            > 0.0,
        "a switch row with no secondary label must still measure a real height"
    );
}

#[test]
fn a_wear_scaling_lazy_column_subcomposes_its_rows_during_measurement() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_ui::{
        Modifier, TextStyle, measure_layout,
        widgets::{
            text::Text,
            wear::scaling_list::{WearScalingLazyColumn, WearScalingLazyColumnSpec},
        },
    };

    let composed_rows = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&composed_rows);
    let mut composition = run_test_composition(move || {
        let counter = Rc::clone(&counter);
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty(),
            state,
            WearScalingLazyColumnSpec::default(),
            move |scope| {
                let counter = Rc::clone(&counter);
                scope.items(12, move |index| {
                    counter.set(counter.get() + 1);
                    Text(
                        format!("Row {index}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
    let root = composition.root().expect("a composed root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout(
        &mut applier,
        root,
        cranpose_ui_graphics::Size::new(400.0, 400.0),
    )
    .expect("the wear scaling list measures");
    applier.clear_runtime_handle();
    assert!(
        composed_rows.get() > 0,
        "a wear scaling lazy column must subcompose at least its first visible rows"
    );
}

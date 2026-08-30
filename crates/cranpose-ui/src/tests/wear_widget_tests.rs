use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::{PointerButton, PointerButtons, lazy::LazyItems};
use cranpose_ui_graphics::Size as ViewportSize;

use super::*;
use crate::{
    modifier::{ModifierNodeSlices, PointerEvent, PointerEventKind},
    round_scaling_list::{CentreAnchor, scale_and_alpha},
    round_scroll_indicator::{
        IndicatorGeometry, ThumbLength, decimal_first_item_index, decimal_last_item_index,
        indicator_geometry,
    },
    widgets::{
        Spacer,
        wear::{
            ListHeader, ListHeaderSpec, ScreenScaffold, ScreenScaffoldSpec, ScrollIndicatorSpec,
            SwitchButton, SwitchButtonSpec, SwitchColors, WearButton, WearButtonSpec, WearColors,
            WearScalingLazyColumn, WearScalingLazyColumnSpec, WearScalingListState, WearTextStyle,
            indicator_for_scaling_list,
        },
    },
};

const PX: f32 = 2.0;
const WATCH: f32 = 227.0;
const SETTINGS_SIDE: f32 = 18.0;
const SCREEN_VERTICAL: f32 = 34.0;
const HEADER_HEIGHT: f32 = 48.0;
const ROW_HEIGHT: f32 = 52.0;

fn measured_colors() -> WearColors {
    WearColors {
        primary: Color::from_rgb_u8(0xB9, 0xF2, 0xFF),
        primary_container: Color::from_rgb_u8(0x0F, 0x36, 0x4E),
        on_primary: Color::from_rgb_u8(0x00, 0x00, 0x00),
        on_primary_container: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        surface_container: Color::from_rgb_u8(0x0A, 0x16, 0x22),
        on_surface: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        on_surface_variant: Color::from_rgb_u8(0x5E, 0x7E, 0x93),
        outline: Color::from_rgb_u8(0x1D, 0x4D, 0x69),
        background: Color::from_rgb_u8(0x00, 0x00, 0x00),
        on_background: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
        content: Color::WHITE,
        indicator_thumb: Color::from_rgb_u8(0xB4, 0xCA, 0xD3),
        indicator_track: Color::from_rgb_u8(0x1E, 0x33, 0x3A),
    }
}

fn settings_spec() -> WearScalingLazyColumnSpec {
    WearScalingLazyColumnSpec::default().content_padding(SETTINGS_SIDE, SCREEN_VERTICAL)
}

type CapturedListState = Rc<RefCell<Option<WearScalingListState>>>;

fn compose_fixed_rows(
    heights: Vec<f32>,
    spec: WearScalingLazyColumnSpec,
) -> (TestComposition, CapturedListState) {
    let captured_state: CapturedListState = Rc::new(RefCell::new(None));
    let composition = run_test_composition({
        let captured_state = Rc::clone(&captured_state);
        move || {
            crate::set_density(PX);
            let state = rememberWearScalingListState(CentreAnchor::default());
            *captured_state.borrow_mut() = Some(state);
            let heights = heights.clone();
            WearScalingLazyColumn(
                Modifier::empty().fill_max_size(),
                state,
                spec,
                move |scope| {
                    let heights = heights.clone();
                    scope.items(
                        LazyItems::new(heights.len()).key(|index: usize| index as u64),
                        move |index| {
                            Spacer(Size {
                                width: 0.0,
                                height: heights[index],
                            });
                        },
                    );
                },
            );
        }
    });
    (composition, captured_state)
}

use crate::widgets::wear::rememberWearScalingListState;

fn state(captured_state: &CapturedListState) -> WearScalingListState {
    (*captured_state.borrow()).expect("state captured")
}

fn tree(composition: &mut TestComposition, root: NodeId) -> crate::LayoutTree {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = measure_layout(
        &mut applier,
        root,
        ViewportSize {
            width: WATCH,
            height: WATCH,
        },
    )
    .expect("layout measurement")
    .into_layout_tree()
    .expect("layout tree");
    applier.clear_runtime_handle();
    tree
}

fn item_layers(tree: &crate::LayoutTree) -> Vec<(f32, f32, Rc<ModifierNodeSlices>)> {
    fn walk(node: &crate::LayoutBox, out: &mut Vec<(f32, f32, Rc<ModifierNodeSlices>)>) {
        if node.node_data.modifier_slices().graphics_layer().is_some() {
            out.push((
                node.rect.y,
                node.rect.height,
                Rc::clone(&node.node_data.modifier_slices),
            ));
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree.root(), &mut out);
    out
}

#[test]
fn the_settings_screen_puts_its_first_two_rows_where_the_framebuffer_has_them() {
    let (mut composition, _captured_state) =
        compose_fixed_rows(vec![HEADER_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(layers.len(), 2, "both rows are on screen");
    assert_eq!(layers[0].0 * PX, 71.0, "the header row's top, in pixels");
    assert_eq!(layers[1].0 * PX, 175.0, "the switch row's top, in pixels");
    assert_eq!((layers[0].0 + layers[0].1) * PX, 167.0);
    assert_eq!((layers[1].0 + layers[1].1) * PX, 279.0);
}

#[test]
fn content_padding_is_absorbed_by_auto_centring_rather_than_stacking_on_it() {
    let (mut composition, _captured_state) =
        compose_fixed_rows(vec![HEADER_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    let anchored_centre = layers[1].0 + layers[1].1 * 0.5;
    assert_eq!(anchored_centre, WATCH * 0.5);
    assert_eq!(anchored_centre * PX, 227.0, "the measured row centre");
}

#[test]
fn a_list_with_no_content_padding_centres_its_anchor_in_the_same_place() {
    let (mut composition, _captured_state) = compose_fixed_rows(
        vec![HEADER_HEIGHT, ROW_HEIGHT],
        WearScalingLazyColumnSpec::default(),
    );
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(layers[1].0 + layers[1].1 * 0.5, WATCH * 0.5);
}

#[test]
fn a_top_aligned_list_still_starts_one_content_padding_down() {
    let (mut composition, _captured_state) = compose_fixed_rows(
        vec![HEADER_HEIGHT, ROW_HEIGHT],
        settings_spec().auto_centering(None),
    );
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(layers[0].0 * PX, SCREEN_VERTICAL * PX);
}

#[test]
fn a_row_is_placed_from_the_full_heights_above_it_not_the_scaled_ones() {
    let (mut composition, captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 6], settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    state(&captured_state).scroll_by(ROW_HEIGHT * 2.0);
    composition
        .process_invalid_scopes()
        .expect("scroll recomposition");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    let scales: Vec<f32> = layers
        .iter()
        .map(|(_, _, slices)| slices.graphics_layer().expect("item layer").scale)
        .collect();
    assert!(
        scales.iter().any(|scale| *scale < 1.0),
        "the ramp has hold of something: {scales:?}"
    );
    let unscaled: Vec<f32> = layers
        .iter()
        .zip(scales.iter())
        .filter(|(_, scale)| **scale == 1.0)
        .map(|((top, _, _), _)| *top)
        .collect();
    assert!(unscaled.len() >= 2, "{scales:?}");
    for pair in unscaled.windows(2) {
        assert_eq!(pair[1] - pair[0], ROW_HEIGHT + 4.0);
    }
}

#[test]
fn a_row_shrinks_and_fades_by_the_amounts_the_pixels_show() {
    let untouched = scale_and_alpha(454.0, 287.0, 287.0 + 104.0).expect("in range");
    assert_eq!(untouched.scale, 1.0);
    let biting = scale_and_alpha(454.0, 399.0, 399.0 + 104.0).expect("in range");
    assert_eq!((382.0 * biting.scale).round(), 298.0);

    let top_px = 355.0;
    let height_px = 104.0;
    let transform = scale_and_alpha(454.0, top_px, top_px + height_px).expect("in range");
    assert!(
        (transform.scale - 0.89166).abs() < 1e-4,
        "scale {}",
        transform.scale
    );
    assert!(
        (transform.alpha - 0.81943).abs() < 1e-4,
        "alpha {}",
        transform.alpha
    );
    assert_eq!((382.0 * transform.scale).floor(), 340.0);
    assert_eq!((104.0 * transform.scale).floor(), 92.0);
    let container = measured_colors().primary_container;
    let faded = |channel: f32| ((channel * 255.0).round() * transform.alpha).round() as u8;
    assert_eq!(
        (faded(container.0), faded(container.1), faded(container.2)),
        (0x0C, 0x2C, 0x40)
    );
}

#[test]
fn the_scale_a_frame_draws_with_is_the_scale_that_frame_measured() {
    let (mut composition, captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 8], settings_spec());
    let root = composition.root().expect("list root");
    let state = state(&captured_state);

    let mut seen = Vec::new();
    for step in 0..5 {
        if step > 0 {
            state.scroll_by(ROW_HEIGHT * 0.5);
            composition
                .process_invalid_scopes()
                .expect("scroll recomposition");
        }
        let tree = tree(&mut composition, root);
        let layers = item_layers(&tree);
        let expected: Vec<_> = expected_rows(8, state.anchor())
            .into_iter()
            .filter(|row| row.top < WATCH && row.top + row.height > 0.0)
            .collect();
        assert_eq!(layers.len(), expected.len());
        for (index, ((top, _, slices), row)) in layers.iter().zip(expected.iter()).enumerate() {
            let layer = slices.graphics_layer().expect("item layer");
            assert!(
                (top - row.top).abs() < 1e-3,
                "step {step} row {index}: placed at {top}, expected {}",
                row.top
            );
            assert!(
                (layer.scale - row.scale).abs() < 1e-4,
                "step {step} row {index}: layer {} vs this frame's geometry {}",
                layer.scale,
                row.scale
            );
            assert!(
                (layer.alpha - row.alpha).abs() < 1e-4,
                "step {step} row {index}"
            );
        }
        seen.push(layers[0].2.graphics_layer().expect("item layer").scale);
    }
    assert!(
        seen.windows(2).any(|pair| pair[0] != pair[1]),
        "the ramp has to actually move for this to prove anything: {seen:?}"
    );
}

fn expected_rows(count: usize, anchor: CentreAnchor) -> Vec<crate::round_scaling_list::PlacedRow> {
    use crate::round_scaling_list::{RowRun, Slot, centre_offset, place_rows, stack_into};
    let mut slots: Vec<Slot> = Vec::new();
    stack_into(std::iter::repeat_n(ROW_HEIGHT, count), 4.0, &mut slots);
    let offset = centre_offset(&slots, WATCH, anchor, PX);
    let index = anchor.index.min(count.saturating_sub(1));
    let mut rows = Vec::new();
    place_rows(
        RowRun {
            viewport: WATCH,
            anchor: index,
            anchor_top: slots[index].top + offset,
            gap: 4.0,
            density: PX,
        },
        &vec![ROW_HEIGHT; count],
        &mut rows,
    );
    rows
}

#[test]
fn a_shrunk_item_reaches_the_renderer_through_a_layer_pinned_to_its_top_edge() {
    let (mut composition, _captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 6], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    for (_, _, slices) in item_layers(&tree) {
        let layer = slices.graphics_layer().expect("item layer");
        assert_eq!(layer.transform_origin.pivot_fraction_x, 0.5);
        assert_eq!(layer.transform_origin.pivot_fraction_y, 0.0);
        assert_eq!(layer.scale_x, 1.0, "the uniform scale carries it");
        assert_eq!(layer.scale_y, 1.0);
    }
}

#[test]
fn an_item_off_the_bottom_is_neither_composed_nor_placed() {
    let (mut composition, captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 30], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    let info = state(&captured_state).layout_info();
    assert_eq!(info.item_count, 30, "the list still knows how long it is");
    assert!(info.visible > 0 && info.visible < 30, "{info:?}");
    assert_eq!(
        layers.len(),
        info.visible,
        "only the visible rows are placed"
    );
    let on_screen = layers
        .iter()
        .filter(|(top, height, _)| *top < WATCH && top + height > 0.0)
        .count();
    assert_eq!(on_screen, info.visible);
    assert!(
        info.composed >= info.visible && info.composed <= info.visible + 4,
        "{info:?}"
    );
    assert!(info.composed < 30, "{info:?}");
}

#[test]
fn the_composed_window_does_not_grow_with_the_list() {
    let (mut short, short_state) = compose_fixed_rows(vec![ROW_HEIGHT; 9], settings_spec());
    let root = short.root().expect("list root");
    let _ = tree(&mut short, root);
    let nine = state(&short_state).layout_info();
    drop(short);

    let (mut long, long_state) = compose_fixed_rows(vec![ROW_HEIGHT; 60], settings_spec());
    let root = long.root().expect("list root");
    let _ = tree(&mut long, root);
    let sixty = state(&long_state).layout_info();

    assert_eq!(nine.item_count, 9);
    assert_eq!(sixty.item_count, 60);
    assert_eq!(
        nine.composed, sixty.composed,
        "same rows on screen, same rows composed: {nine:?} vs {sixty:?}"
    );
    assert_eq!(nine.visible, sixty.visible);
}

#[test]
fn the_anchored_items_top_does_not_depend_on_the_heights_above_it() {
    let (mut thin, _thin_state) =
        compose_fixed_rows(vec![7.5, ROW_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = thin.root().expect("list root");
    let tree_thin = tree(&mut thin, root);
    let thin_anchor = item_layers(&tree_thin)[1].0;
    drop(thin);

    let (mut fat, _fat_state) =
        compose_fixed_rows(vec![101.5, ROW_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = fat.root().expect("list root");
    let tree_fat = tree(&mut fat, root);
    let fat_layers = item_layers(&tree_fat);
    let fat_anchor = fat_layers
        .iter()
        .map(|(top, _, _)| *top)
        .find(|top| (top - thin_anchor).abs() < 1e-3);
    assert_eq!(
        fat_anchor,
        Some(thin_anchor),
        "the anchored row moved when a row above it changed height; \
         placed tops were {:?} against {thin_anchor}",
        fat_layers
            .iter()
            .map(|(top, _, _)| *top)
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_indicator_travel_runs_between_the_first_and_last_row_centres() {
    let (mut composition, captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 10], settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    let info = state(&captured_state).layout_info();
    assert_eq!(info.item_count, 10);
    assert_eq!(info.travel(), 9.0 * (ROW_HEIGHT + 4.0));
}

fn compose_widget(mut build: impl FnMut() + 'static) -> TestComposition {
    run_test_composition(move || {
        crate::set_density(PX);
        build();
    })
}

fn root_size(composition: &mut TestComposition) -> Size {
    let root = composition.root().expect("root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measurements = measure_layout(
        &mut applier,
        root,
        ViewportSize {
            width: WATCH - SETTINGS_SIDE * 2.0,
            height: WATCH,
        },
    )
    .expect("layout measurement");
    applier.clear_runtime_handle();
    measurements.root_size()
}

#[test]
fn a_list_header_is_at_least_forty_eight_dp_tall_and_wraps_its_words() {
    let mut composition = compose_widget(|| {
        ListHeader(
            Modifier::empty(),
            ListHeaderSpec::default().colors(measured_colors()),
            "SETTINGS".to_string(),
        );
    });
    let size = root_size(&mut composition);
    assert_eq!(size.height, HEADER_HEIGHT, "ListHeaderTokens.Height wins");
    assert!(
        size.width < WATCH - SETTINGS_SIDE * 2.0,
        "a header wraps its content rather than spanning the list: {}",
        size.width
    );
}

#[test]
fn a_wear_button_is_at_least_fifty_two_dp_tall_and_fills_the_width_it_is_given() {
    let mut composition = compose_widget(|| {
        WearButton(
            Modifier::empty(),
            WearButtonSpec::default().colors(measured_colors()),
            "Credits".to_string(),
            None,
            || {},
        );
    });
    let size = root_size(&mut composition);
    assert_eq!(size.height, ROW_HEIGHT);
    assert_eq!(size.width, WATCH - SETTINGS_SIDE * 2.0);
    assert_eq!(size.width * PX, 382.0);
}

#[test]
fn a_two_label_button_grows_past_the_floor_when_its_labels_need_the_room() {
    let one = {
        let mut composition = compose_widget(|| {
            WearButton(
                Modifier::empty(),
                WearButtonSpec::default(),
                "Sensitivity".to_string(),
                None,
                || {},
            );
        });
        root_size(&mut composition).height
    };
    let two = {
        let mut composition = compose_widget(|| {
            WearButton(
                Modifier::empty(),
                WearButtonSpec::default(),
                "Sensitivity".to_string(),
                Some("NORMAL".to_string()),
                || {},
            );
        });
        root_size(&mut composition).height
    };
    assert!(two >= one, "a second label never shrinks the capsule");
    assert!(two >= ROW_HEIGHT);
}

#[test]
fn a_switch_row_is_fifty_two_dp_tall_and_its_switch_sits_two_dp_above_centre() {
    let mut composition = compose_widget(|| {
        SwitchButton(
            Modifier::empty(),
            SwitchButtonSpec::default()
                .colors(measured_colors())
                .progress(1.0),
            true,
            "Haptics".to_string(),
            None,
            |_| {},
        );
    });
    let root = composition.root().expect("switch root");
    let handle = composition.runtime_handle();
    let tree = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let tree = measure_layout(
            &mut applier,
            root,
            ViewportSize {
                width: WATCH - SETTINGS_SIDE * 2.0,
                height: WATCH,
            },
        )
        .expect("layout measurement")
        .into_layout_tree()
        .expect("layout tree");
        applier.clear_runtime_handle();
        tree
    };
    let row = tree.root().rect;
    assert_eq!(row.height, ROW_HEIGHT);

    fn find_switch(node: &crate::LayoutBox) -> Option<crate::modifier::Rect> {
        if (node.rect.width - 32.0).abs() < 1e-3 && (node.rect.height - 22.0).abs() < 1e-3 {
            return Some(node.rect);
        }
        node.children.iter().find_map(find_switch)
    }
    let switch = find_switch(tree.root()).expect("the 32x22dp switch graphic");
    let row_centre = row.y + row.height * 0.5;
    let switch_centre = switch.y + switch.height * 0.5;
    assert_eq!(
        row_centre - switch_centre,
        1.0,
        "the 22dp graphic is top-aligned in a 24dp slot, so its centre is 1dp high"
    );
    assert_eq!((row_centre - switch_centre) * PX, 2.0, "2px, as measured");
    assert_eq!(row.width - (switch.x + switch.width), 14.0);
}

#[test]
fn a_switch_row_is_toggleable_and_reports_the_value_it_is_moving_to() {
    let seen: Rc<RefCell<Option<bool>>> = Rc::new(RefCell::new(None));
    let sink = seen.clone();
    let mut composition = compose_widget(move || {
        let sink = sink.clone();
        SwitchButton(
            Modifier::empty(),
            SwitchButtonSpec::default(),
            true,
            "Haptics".to_string(),
            None,
            move |next| *sink.borrow_mut() = Some(next),
        );
    });
    let root = composition.root().expect("switch root");
    let tree = tree(&mut composition, root);
    fn find_pointer(node: &crate::LayoutBox) -> Option<Rc<dyn Fn(PointerEvent)>> {
        if let Some(handler) = node.node_data.modifier_slices().pointer_inputs().first() {
            return Some(Rc::clone(handler));
        }
        node.children.iter().find_map(find_pointer)
    }
    let handler = find_pointer(tree.root()).expect("a toggleable row takes pointer input");
    let at = crate::modifier::Point { x: 40.0, y: 26.0 };
    for kind in [PointerEventKind::Down, PointerEventKind::Up] {
        let mut event = PointerEvent::new(kind, at, at);
        event.buttons = PointerButtons::new().with(PointerButton::Primary);
        handler(event);
    }
    assert_eq!(
        *seen.borrow(),
        Some(false),
        "checked, so a tap turns it off"
    );
}

#[test]
fn a_scaffold_draws_its_indicator_over_the_content_and_not_beside_it() {
    let mut composition = run_test_composition(|| {
        crate::set_density(PX);
        let state = rememberWearScalingListState(CentreAnchor::default());
        let inner = state;
        ScreenScaffold(
            Modifier::empty(),
            state,
            ScreenScaffoldSpec::default()
                .indicator(ScrollIndicatorSpec::default().colors(measured_colors())),
            move || {
                WearScalingLazyColumn(
                    Modifier::empty().fill_max_size(),
                    inner,
                    settings_spec(),
                    |scope| {
                        scope.items(LazyItems::new(10).key(|index: usize| index as u64), |_| {
                            Spacer(Size {
                                width: 0.0,
                                height: ROW_HEIGHT,
                            });
                        });
                    },
                );
            },
        );
    });
    let root = composition.root().expect("scaffold root");
    let tree = tree(&mut composition, root);
    assert_eq!(
        tree.root().rect.width,
        WATCH,
        "the scaffold fills the watch"
    );
    assert_eq!(tree.root().rect.height, WATCH);
    assert_eq!(tree.root().children.len(), 2);
    for child in &tree.root().children {
        assert_eq!(child.rect.x, 0.0);
        assert_eq!(child.rect.width, WATCH);
    }
}

fn header_and_rows() -> Vec<f32> {
    let mut heights = vec![HEADER_HEIGHT];
    heights.extend([ROW_HEIGHT; 9]);
    heights
}

fn indicator_at_vertical_padding(vertical: f32) -> IndicatorGeometry {
    let spec = WearScalingLazyColumnSpec::default().content_padding(SETTINGS_SIDE, vertical);
    let (mut composition, captured_state) = compose_fixed_rows(header_and_rows(), spec);
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    indicator_for_scaling_list(&state(&captured_state)).expect("a ten-row list is scrollable")
}

#[test]
fn the_composed_indicator_reads_item_indices_where_the_flat_model_reads_pixels() {
    let (mut composition, captured_state) = compose_fixed_rows(header_and_rows(), settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    let list = state(&captured_state);
    let wear = indicator_for_scaling_list(&list).expect("a ten-row list is scrollable");

    let span = list.with_indicator_list(|_, items| {
        (decimal_last_item_index(items) - decimal_first_item_index(items)) / items.total as f32
    });
    assert!(
        (wear.thumb - span).abs() < 1e-6,
        "{wear:?} against an item span of {span}"
    );

    let info = list.layout_info();
    let flat = indicator_geometry(info.content, info.viewport, info.scrolled())
        .expect("the flat model has an answer too");
    assert!(
        (wear.thumb - flat.thumb).abs() > 0.05,
        "wear {wear:?} against flat {flat:?}"
    );
    assert!(
        (wear.offset - flat.offset).abs() > 0.05,
        "wear {wear:?} against flat {flat:?}"
    );
}

#[test]
fn the_window_the_indicator_reads_carries_the_lists_own_item_indices() {
    let (mut composition, captured_state) = compose_fixed_rows(header_and_rows(), settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    let list = state(&captured_state);
    let at_rest = indicator_for_scaling_list(&list).expect("indicator");
    assert_eq!(
        list.with_indicator_list(|_, items| items.visible.first().map(|item| item.index)),
        Some(0),
        "at rest the list really is showing its first row"
    );
    assert!(at_rest.offset < 0.1, "{at_rest:?}");

    list.scroll_by(list.layout_info().travel());
    composition
        .process_invalid_scopes()
        .expect("scroll recomposition");
    let _ = tree(&mut composition, root);
    let list = state(&captured_state);
    let indices = list
        .with_indicator_list(|_, items| items.visible.iter().map(|i| i.index).collect::<Vec<_>>());
    assert_eq!(
        indices.last().copied(),
        Some(9),
        "the end of the list is the last row, not a window-local index: {indices:?}"
    );
    assert!(
        indices.first().copied().unwrap_or(0) > 0,
        "the window no longer starts at the list's first row: {indices:?}"
    );
    let at_end = indicator_for_scaling_list(&list).expect("indicator");
    assert!(
        at_end.offset > 0.6,
        "a list scrolled to its end puts the thumb at the end of the track: {at_end:?}"
    );
}

#[test]
fn the_thumb_keeps_the_length_it_was_first_measured_at() {
    let (mut composition, captured_state) = compose_fixed_rows(header_and_rows(), settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    let list = state(&captured_state);
    let at_rest = indicator_for_scaling_list(&list).expect("indicator");

    list.scroll_by(list.layout_info().travel() * 0.5);
    composition
        .process_invalid_scopes()
        .expect("scroll recomposition");
    let _ = tree(&mut composition, root);
    let list = state(&captured_state);
    let scrolled = indicator_for_scaling_list(&list).expect("indicator");
    assert_eq!(scrolled.thumb, at_rest.thumb, "the thumb changed length");
    assert!(scrolled.offset > at_rest.offset, "and it did move");

    let remeasured = list.with_indicator_list(|_, items| ThumbLength::default().of(items));
    assert!(
        (remeasured - at_rest.thumb).abs() > 0.05,
        "a re-measured thumb is {remeasured} against the kept {}",
        at_rest.thumb
    );
}

#[test]
fn the_indicator_counts_the_vertical_padding_this_list_was_given() {
    let bare = indicator_at_vertical_padding(0.0);
    let padded = indicator_at_vertical_padding(SCREEN_VERTICAL);
    let deeper = indicator_at_vertical_padding(60.0);
    assert_eq!(
        bare.offset, 0.0,
        "with no blank above it, a list at rest is at its own top"
    );
    assert!(padded.offset > bare.offset, "{padded:?} against {bare:?}");
    assert!(
        deeper.offset > padded.offset,
        "{deeper:?} against {padded:?}"
    );
    assert!(
        deeper.thumb < padded.thumb && padded.thumb < bare.thumb,
        "deeper padding is more travel, so a shorter thumb: {bare:?} {padded:?} {deeper:?}"
    );
}

#[test]
fn a_list_that_fits_on_its_screen_has_no_indicator_at_all() {
    let (mut composition, captured_state) =
        compose_fixed_rows(vec![ROW_HEIGHT; 2], settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    assert_eq!(indicator_for_scaling_list(&state(&captured_state)), None);
}

#[test]
fn the_indicator_sweep_is_the_one_measured_on_a_454_pixel_watch() {
    let arc = crate::round_scroll_indicator::indicator_arc(113.5);
    let degrees = arc.sweep().to_degrees();
    assert!((degrees - 30.536).abs() < 0.01, "{degrees}");
    assert!((arc.centreline() - 108.5).abs() < 1e-3);
    assert_eq!(arc.width(), 6.0, "6dp on a large screen");
}

#[test]
fn a_wear_text_style_asks_for_the_line_box_rule_that_gives_a_38_pixel_header() {
    use crate::text::line_box::{FontExtent, line_box};
    let style = WearTextStyle::TITLE_MEDIUM.resolve(measured_colors().on_background);
    let extent = FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0);
    let resolved = line_box(&style, extent, 36.0, 1.0);
    assert_eq!(
        resolved.height, 38.0,
        "titleMedium overflows its own 18sp line height"
    );

    let label = WearTextStyle::LABEL_MEDIUM.resolve(measured_colors().on_surface);
    let label_extent = FontExtent::new(30.0 * 1900.0 / 2048.0, 30.0 * 500.0 / 2048.0, 0.0);
    assert_eq!(line_box(&label, label_extent, 36.0, 1.0).height, 36.0);

    let bare = WearTextStyle::BODY_LARGE
        .at_size(12.0)
        .with_line_height(16.0)
        .resolve(measured_colors().content);
    let bare_extent = FontExtent::new(24.0 * 1900.0 / 2048.0, 24.0 * 500.0 / 2048.0, 0.0);
    assert_eq!(line_box(&bare, bare_extent, 32.0, 1.0).height, 32.0);
}

#[test]
fn wear_tracking_widens_a_string_by_one_letter_space_per_character() {
    const HEADER: &str = "SETTINGS";
    let width = |tracking_sp: f32| {
        let mut style = WearTextStyle::TITLE_MEDIUM;
        style.tracking_sp = tracking_sp;
        let mut composition = compose_widget(move || {
            Text(
                HEADER.to_string(),
                Modifier::empty(),
                style.resolve(measured_colors().on_background),
            );
        });
        root_size(&mut composition).width
    };

    let chars = HEADER.chars().count() as f32;
    let grew = width(0.4) - width(0.0);
    assert!(
        (grew - chars * 0.4).abs() < 0.01,
        "{HEADER} grew by {grew} points of tracking where {chars} characters at \
         0.4sp should have widened it by {}",
        chars * 0.4
    );
}

#[test]
fn the_switch_slots_resolve_to_the_colours_the_framebuffer_shows() {
    let colors = measured_colors();
    let checked = SwitchColors::of(colors, true);
    assert_eq!(checked.container, Color::from_rgb_u8(0x0F, 0x36, 0x4E));
    assert_eq!(checked.track, Color::from_rgb_u8(0xB9, 0xF2, 0xFF));
    assert_eq!(checked.thumb, Color::from_rgb_u8(0x0F, 0x36, 0x4E));
    assert_eq!(checked.tick, Color::from_rgb_u8(0xB9, 0xF2, 0xFF));
    assert_eq!(checked.label, Color::from_rgb_u8(0xDF, 0xF6, 0xFF));
    assert_eq!(
        checked.track_border.3, 0.0,
        "suppressed when it equals the track"
    );

    let unchecked = SwitchColors::of(colors, false);
    assert_eq!(unchecked.container, Color::from_rgb_u8(0x0A, 0x16, 0x22));
    assert_eq!(unchecked.track_border, Color::from_rgb_u8(0x1D, 0x4D, 0x69));
    assert_eq!(
        unchecked.secondary_label,
        Color::from_rgb_u8(0x5E, 0x7E, 0x93)
    );
}

fn compose_credits_screen() -> (TestComposition, CapturedListState) {
    let captured_state: CapturedListState = Rc::new(RefCell::new(None));
    let composition = run_test_composition({
        let captured_state = Rc::clone(&captured_state);
        move || {
            crate::set_density(PX);
            let state = rememberWearScalingListState(CentreAnchor::default());
            *captured_state.borrow_mut() = Some(state);
            let inner = state;
            ScreenScaffold(
                Modifier::empty().fill_max_size(),
                state,
                ScreenScaffoldSpec::default()
                    .indicator(ScrollIndicatorSpec::default().colors(measured_colors())),
                move || {
                    WearScalingLazyColumn(
                        Modifier::empty().fill_max_size(),
                        inner,
                        WearScalingLazyColumnSpec::default().content_padding(30.0, SCREEN_VERTICAL),
                        move |scope| {
                            // Six lines and then the button, rather than four: a
                            // scaling list stacks its DRAWN boxes a gap apart, so a
                            // shrunken row does not push the one after it down and
                            // a short list keeps more of itself on the first
                            // screen. `a_row_below_the_fold_paints_once_it_is_scrolled_to`
                            // needs the button genuinely off screen at rest, and
                            // with four lines above it no longer is.
                            let lines = [
                                "ORBIT BREAKER",
                                "Version 1.0.0-debug",
                                "Designed and built for Wear OS.",
                                "Every graphic and sound in this game is generated inside the project.",
                                "No third-party assets, no downloads, nothing loaded at runtime.",
                                "Built on Cranpose, a Compose-shaped UI framework written in Rust.",
                                "Back",
                            ];
                            let button = lines.len() - 1;
                            scope.items(
                                LazyItems::new(lines.len()).key(|index: usize| index as u64),
                                move |index| match index {
                                    0 => {
                                        ListHeader(
                                            Modifier::empty(),
                                            ListHeaderSpec::default().colors(measured_colors()),
                                            lines[0].to_string(),
                                        );
                                    }
                                    other if other == button => {
                                        WearButton(
                                            Modifier::empty().fill_max_width(),
                                            WearButtonSpec::default().colors(measured_colors()),
                                            lines[button].to_string(),
                                            None,
                                            || {},
                                        );
                                    }
                                    other => {
                                        Text(
                                            lines[other].to_string(),
                                            Modifier::empty().fill_max_width(),
                                            WearTextStyle::BODY_LARGE
                                                .at_size(12.0)
                                                .with_line_height(16.0)
                                                .resolve(measured_colors().content),
                                        );
                                    }
                                },
                            );
                        },
                    );
                },
            );
        }
    });
    (composition, captured_state)
}

#[test]
fn a_credits_screen_of_text_measured_rows_places_rows_that_are_not_empty() {
    let (mut composition, captured_state) = compose_credits_screen();
    let root = composition.root().expect("credits root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    let info = state(&captured_state).layout_info();
    assert_eq!(info.item_count, 7);
    assert_eq!(
        layers.len(),
        info.visible,
        "one graphics layer per VISIBLE item, however tall the item measured"
    );
    assert!(
        layers.len() >= 4,
        "a 227pt screen holds at least four of these rows; got {}",
        layers.len()
    );
    for (index, (y, height, _)) in layers.iter().enumerate() {
        assert!(
            *height > 0.0,
            "item {index} measured {height} tall at y={y}; a row whose height \
             is zero draws nothing, which is what an empty screen looks like"
        );
    }
    let tops: Vec<f32> = layers.iter().map(|(y, _, _)| *y).collect();
    assert!(
        tops.windows(2).all(|pair| pair[1] > pair[0]),
        "items should descend the screen, got tops {tops:?}"
    );
}

#[test]
fn a_credits_screen_emits_a_text_primitive_for_every_row_it_placed() {
    let (mut composition, _captured_state) = compose_credits_screen();
    let root = composition.root().expect("credits root");
    let tree = tree(&mut composition, root);
    let texts = scene_texts(&tree);
    assert!(
        texts.iter().any(|t| t.contains("ORBIT BREAKER")),
        "the ListHeader's own string should reach the scene; got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Wear OS")),
        "a credit line's string should reach the scene; got {texts:?}"
    );
}

#[test]
fn a_row_below_the_fold_paints_once_it_is_scrolled_to() {
    let (mut composition, captured_state) = compose_credits_screen();
    let root = composition.root().expect("credits root");
    let list = state(&captured_state);
    assert!(
        !scene_texts(&tree(&mut composition, root)).contains(&"Back".to_string()),
        "the button starts below the fold, or this test proves nothing"
    );

    let travel = list.layout_info().travel();
    assert!(travel > 0.0, "the credits list has to be scrollable");
    list.scroll_by(travel);
    composition
        .process_invalid_scopes()
        .expect("scroll recomposition");
    let texts = scene_texts(&tree(&mut composition, root));
    assert!(
        texts.iter().any(|t| t.contains("Back")),
        "the button's label should reach the scene once scrolled to; got {texts:?}"
    );
}

fn scene_texts(tree: &crate::LayoutTree) -> Vec<String> {
    crate::renderer::HeadlessRenderer::new()
        .render(tree)
        .operations()
        .iter()
        .filter_map(|op| match op {
            crate::renderer::RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

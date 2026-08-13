//! Wear widgets, measured against a real Wear OS screen.
//!
//! The numbers asserted here are not "what the code does today". They are the
//! accessibility-node bounds and framebuffer colours of the Settings and
//! Credits screens of a shipping Wear app, captured on a 454x454 emulator at
//! density 2. Where a test names a pixel, that pixel was measured.
//!
//! Everything is stated in **layout points** and converted with `PX`, because a
//! Cranpose point is a dp: the 454px display is 227 points across, and 139px is
//! 69.5 points.

use super::*;
use crate::modifier::{ModifierNodeSlices, PointerEvent, PointerEventKind};
use crate::round_scaling_list::{scale_and_alpha, CentreAnchor};
use crate::widgets::wear::{
    ListHeader, ListHeaderSpec, ScreenScaffold, ScreenScaffoldSpec, ScrollIndicatorSpec,
    SwitchButton, SwitchButtonSpec, SwitchColors, WearButton, WearButtonSpec, WearColors,
    WearScalingLazyColumn, WearScalingLazyColumnSpec, WearScalingListState, WearTextStyle,
};
use crate::widgets::Spacer;
use cranpose_core::NodeId;
use cranpose_foundation::{PointerButton, PointerButtons};
use cranpose_ui_graphics::Size as ViewportSize;
use std::cell::RefCell;
use std::rc::Rc;

/// Device pixels per layout point on the watch these numbers came from.
const PX: f32 = 2.0;
/// The display, in layout points: 454px at density 2.
const WATCH: f32 = 227.0;
/// Settings' horizontal content padding.
const SETTINGS_SIDE: f32 = 18.0;
/// Both screens' vertical content padding.
const SCREEN_VERTICAL: f32 = 34.0;
/// `ListHeaderTokens.Height`.
const HEADER_HEIGHT: f32 = 48.0;
/// `FilledButtonTokens.ContainerHeight`, and `SwitchButton`'s `MIN_HEIGHT`.
const ROW_HEIGHT: f32 = 52.0;

thread_local! {
    static LAST_STATE: RefCell<Option<WearScalingListState>> = const { RefCell::new(None) };
}

/// The palette the screens were measured in, straight out of the spec.
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

/// A list of fixed-height rows, so the geometry under test is the list's and
/// not the text measurer's.
fn compose_fixed_rows(heights: Vec<f32>, spec: WearScalingLazyColumnSpec) -> TestComposition {
    let composition = run_test_composition(move || {
        crate::set_density(PX);
        let state = rememberWearScalingListState(CentreAnchor::default());
        LAST_STATE.with(|cell| *cell.borrow_mut() = Some(state.clone()));
        let heights = heights.clone();
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            spec,
            move |scope| {
                let heights = heights.clone();
                scope.items(heights.len(), move |index| {
                    Spacer(Size {
                        width: 0.0,
                        height: heights[index],
                    });
                });
            },
        );
    });
    composition
}

use crate::widgets::wear::rememberWearScalingListState;

fn state() -> WearScalingListState {
    LAST_STATE.with(|cell| cell.borrow().clone().expect("state captured"))
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

/// Every placed node that carries a graphics layer, in tree order — one per
/// visible list item.
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

// ── The list ────────────────────────────────────────────────────────────────

#[test]
fn the_settings_screen_puts_its_first_two_rows_where_the_framebuffer_has_them() {
    // Measured on the device: the "SETTINGS" header row spans y = 139..235 and
    // the "Haptics" switch row y = 243..347. Everything that produces those two
    // numbers is under test here at once — the 48dp and 52dp floors, the 4dp
    // item spacing, `AutoCenteringParams(itemIndex = 1)`, and the fact that the
    // 34dp vertical content padding stacks ON TOP of auto-centring rather than
    // being absorbed by it.
    let mut composition = compose_fixed_rows(vec![HEADER_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(layers.len(), 2, "both rows are on screen");
    assert_eq!(layers[0].0 * PX, 139.0, "the header row's top, in pixels");
    assert_eq!(layers[1].0 * PX, 243.0, "the switch row's top, in pixels");
    assert_eq!((layers[0].0 + layers[0].1) * PX, 235.0);
    assert_eq!((layers[1].0 + layers[1].1) * PX, 347.0);
}

#[test]
fn content_padding_and_auto_centring_stack_rather_than_cancel() {
    // The anchored item's centre lands at H/2 PLUS the vertical padding, not at
    // H/2. Reading `ScalingLazyColumn` as though `contentPadding` were absorbed
    // by the auto-centring spacer puts every Settings row 68px too high.
    let mut composition = compose_fixed_rows(vec![HEADER_HEIGHT, ROW_HEIGHT], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    let anchored_centre = layers[1].0 + layers[1].1 * 0.5;
    assert_eq!(anchored_centre, WATCH * 0.5 + SCREEN_VERTICAL);
    assert_eq!(anchored_centre * PX, 295.0, "the measured row centre");
}

#[test]
fn a_row_is_placed_from_the_full_heights_above_it_not_the_scaled_ones() {
    // Six rows, scrolled far enough that the ramp has hold of the top ones.
    // However much a row shrank, the row below it sits a full row plus a gap
    // lower — stacking scaled heights instead makes the list drift further out
    // of place with every row.
    let mut composition = compose_fixed_rows(vec![ROW_HEIGHT; 6], settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    state().scroll_by(ROW_HEIGHT * 2.0);
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
    // Two rows the ramp left alone are exactly one row and one gap apart. A
    // scaled row's own top moves — above the centre line it keeps its bottom
    // edge — but that movement never reaches the row below it.
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
fn the_second_switch_row_shrinks_and_fades_by_the_amounts_the_pixels_show() {
    // Measured: the "Sound effects" row, unscaled 382x104px with its top at
    // y = 355, draws 340x92px, and its #0F364E container comes out #0C2C40.
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
    // 382 x 0.89166 = 340.6, and the device reports 340.
    assert_eq!((382.0 * transform.scale).floor(), 340.0);
    assert_eq!((104.0 * transform.scale).floor(), 92.0);
    // And the faded container matches on all three channels. This is the
    // prediction the spec validated itself with.
    let container = measured_colors().primary_container;
    let faded = |channel: f32| ((channel * 255.0).round() * transform.alpha).round() as u8;
    assert_eq!(
        (faded(container.0), faded(container.1), faded(container.2)),
        (0x0C, 0x2C, 0x40)
    );
}

#[test]
fn the_scale_a_frame_draws_with_is_the_scale_that_frame_measured() {
    // The experiment the design called for. A layer resolver runs at scene
    // build, after measure, so driving a scroll ramp and re-reading the layer
    // must give the CURRENT frame's scale — not the previous frame's, which is
    // what a channel filled during composition would give.
    let mut composition = compose_fixed_rows(vec![ROW_HEIGHT; 8], settings_spec());
    let root = composition.root().expect("list root");
    let state = state();

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
        // What this frame's anchor says every row should be, worked out from
        // the library geometry rather than from the widget.
        let expected = expected_rows(8, state.anchor());
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

/// What the library geometry says a list of equal rows should look like, given
/// an anchor. Deliberately written from `round_scaling_list` directly, so the
/// widget is checked against the geometry rather than against itself.
fn expected_rows(count: usize, anchor: CentreAnchor) -> Vec<crate::round_scaling_list::PlacedRow> {
    use crate::round_scaling_list::{centre_offset, place_row, stack_into, Slot};
    let mut slots: Vec<Slot> = Vec::new();
    stack_into(std::iter::repeat_n(ROW_HEIGHT, count), 4.0, &mut slots);
    let offset = SCREEN_VERTICAL + centre_offset(&slots, WATCH, anchor, PX);
    slots
        .iter()
        .map(|slot| place_row(WATCH, slot.top + offset, slot.height, PX).expect("in range"))
        .collect()
}

#[test]
fn a_shrunk_item_reaches_the_renderer_through_a_layer_pinned_to_its_top_edge() {
    // `transformOrigin` is (0.5, 0.0) on every item. The pinning of whichever
    // edge faces the centre line is already in the placed top; doing it with
    // the origin as well would pin the row twice.
    let mut composition = compose_fixed_rows(vec![ROW_HEIGHT; 6], settings_spec());
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
fn an_item_off_the_bottom_is_still_placed_and_reports_as_not_visible() {
    // Every item gets a real position; the list clips. Leaving one unplaced
    // would leave the node holding the rectangle it had last frame, which the
    // next frame's ramp would then read back as though it were current.
    let mut composition = compose_fixed_rows(vec![ROW_HEIGHT; 30], settings_spec());
    let root = composition.root().expect("list root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(layers.len(), 30, "all thirty are placed");
    let visible = state().layout_info().visible;
    assert!(visible > 0 && visible < 30, "{visible} of 30 on screen");
    let on_screen = layers
        .iter()
        .filter(|(top, height, _)| *top < WATCH && top + height > 0.0)
        .count();
    assert_eq!(on_screen, visible);
}

#[test]
fn the_indicator_travel_runs_between_the_first_and_last_row_centres() {
    let mut composition = compose_fixed_rows(vec![ROW_HEIGHT; 10], settings_spec());
    let root = composition.root().expect("list root");
    let _ = tree(&mut composition, root);
    let info = state().layout_info();
    assert_eq!(info.item_count, 10);
    // Nine gaps of one row plus one spacing between ten centres.
    assert_eq!(info.travel(), 9.0 * (ROW_HEIGHT + 4.0));
}

// ── The row widgets ─────────────────────────────────────────────────────────

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
    // `fillMaxWidth` is outermost and pins min == max, so the intrinsic width
    // never gets a say. 191dp is 382px, the measured row width.
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

    // The switch is the child sized exactly 32x22.
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
    // And it is flush with the row's end padding.
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
    // A composed row's click arrives as a pointer input on its node chain,
    // not as a raw click slice: the slice is what a bare `Modifier` exposes,
    // and the chain is what a node actually runs.
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

// ── The scaffold and the indicator ──────────────────────────────────────────

#[test]
fn a_scaffold_draws_its_indicator_over_the_content_and_not_beside_it() {
    let mut composition = run_test_composition(|| {
        crate::set_density(PX);
        let state = rememberWearScalingListState(CentreAnchor::default());
        LAST_STATE.with(|cell| *cell.borrow_mut() = Some(state.clone()));
        let inner = state.clone();
        ScreenScaffold(
            Modifier::empty(),
            state,
            ScreenScaffoldSpec::default()
                .indicator(ScrollIndicatorSpec::default().colors(measured_colors())),
            move || {
                WearScalingLazyColumn(
                    Modifier::empty().fill_max_size(),
                    inner.clone(),
                    settings_spec(),
                    |scope| {
                        scope.items(10, |_| {
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
    // Two children in one box: the list and the indicator, stacked rather than
    // laid out side by side.
    assert_eq!(tree.root().children.len(), 2);
    for child in &tree.root().children {
        assert_eq!(child.rect.x, 0.0);
        assert_eq!(child.rect.width, WATCH);
    }
}

#[test]
fn the_indicator_sweep_is_the_one_measured_on_a_454_pixel_watch() {
    // 30.536 degrees at r = 113.5dp. The two other implementations of this in
    // the same family compute 26.64 and are wrong by 3.90 degrees of sweep.
    let arc = crate::round_scroll_indicator::indicator_arc(113.5);
    let degrees = arc.sweep().to_degrees();
    assert!((degrees - 30.536).abs() < 0.01, "{degrees}");
    assert!((arc.centreline() - 108.5).abs() < 1e-3);
    assert_eq!(arc.width(), 6.0, "6dp on a large screen");
}

// ── Typography ──────────────────────────────────────────────────────────────

#[test]
fn a_wear_text_style_asks_for_the_line_box_rule_that_gives_a_38_pixel_header() {
    use crate::text::line_box::{line_box, FontExtent};
    let style = WearTextStyle::TITLE_MEDIUM.resolve(measured_colors().on_background);
    // Roboto's hhea metrics at 16sp on a density-2 watch: 32px of glyph,
    // ascender 1900/2048 and descender 500/2048.
    let extent = FontExtent::new(32.0 * 1900.0 / 2048.0, 32.0 * 500.0 / 2048.0, 0.0);
    let resolved = line_box(&style, extent, 36.0, 1.0);
    assert_eq!(
        resolved.height, 38.0,
        "titleMedium overflows its own 18sp line height"
    );

    // labelMedium does not, which is why only the header exposes it.
    let label = WearTextStyle::LABEL_MEDIUM.resolve(measured_colors().on_surface);
    let label_extent = FontExtent::new(30.0 * 1900.0 / 2048.0, 30.0 * 500.0 / 2048.0, 0.0);
    assert_eq!(line_box(&label, label_extent, 36.0, 1.0).height, 36.0);

    // And a 12sp bare Text in an inherited 16sp line box measures 32px.
    let bare = WearTextStyle::BODY_LARGE
        .at_size(12.0)
        .with_line_height(16.0)
        .resolve(measured_colors().content);
    let bare_extent = FontExtent::new(24.0 * 1900.0 / 2048.0, 24.0 * 500.0 / 2048.0, 0.0);
    assert_eq!(line_box(&bare, bare_extent, 32.0, 1.0).height, 32.0);
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

/// A whole Credits screen: a scaffold over a list of real, text-measured rows.
///
/// Every other test here feeds the list `Spacer`s of a fixed height, so that
/// what is under test is the list's geometry and not the text measurer's. This
/// one deliberately does the opposite, because an app does: a `ListHeader`, a
/// pile of wrapping `Text`, and a `WearButton`, all sized by measuring their
/// own strings.
fn compose_credits_screen() -> TestComposition {
    run_test_composition(|| {
        crate::set_density(PX);
        let state = rememberWearScalingListState(CentreAnchor::default());
        LAST_STATE.with(|cell| *cell.borrow_mut() = Some(state.clone()));
        let inner = state.clone();
        ScreenScaffold(
            Modifier::empty().fill_max_size(),
            state,
            ScreenScaffoldSpec::default()
                .indicator(ScrollIndicatorSpec::default().colors(measured_colors())),
            move || {
                WearScalingLazyColumn(
                    Modifier::empty().fill_max_size(),
                    inner.clone(),
                    WearScalingLazyColumnSpec::default().content_padding(30.0, SCREEN_VERTICAL),
                    move |scope| {
                        let lines = [
                            "ORBIT BREAKER",
                            "Version 1.0.0-debug",
                            "Designed and built for Wear OS.",
                            "Every graphic and sound in this game is generated inside the project.",
                            "Back",
                        ];
                        scope.items(lines.len(), move |index| match index {
                            0 => {
                                ListHeader(
                                    Modifier::empty(),
                                    ListHeaderSpec::default().colors(measured_colors()),
                                    lines[0].to_string(),
                                );
                            }
                            4 => {
                                WearButton(
                                    Modifier::empty().fill_max_width(),
                                    WearButtonSpec::default().colors(measured_colors()),
                                    lines[4].to_string(),
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
                        });
                    },
                );
            },
        );
    })
}

#[test]
fn a_credits_screen_of_text_measured_rows_places_rows_that_are_not_empty() {
    let mut composition = compose_credits_screen();
    let root = composition.root().expect("credits root");
    let tree = tree(&mut composition, root);
    let layers = item_layers(&tree);
    assert_eq!(
        layers.len(),
        5,
        "one graphics layer per item, however tall the item measured"
    );
    for (index, (y, height, _)) in layers.iter().enumerate() {
        assert!(
            *height > 0.0,
            "item {index} measured {height} tall at y={y}; a row whose height \
             is zero draws nothing, which is what an empty screen looks like"
        );
    }
    // And they are stacked, not piled on one coordinate.
    let tops: Vec<f32> = layers.iter().map(|(y, _, _)| *y).collect();
    assert!(
        tops.windows(2).all(|pair| pair[1] > pair[0]),
        "items should descend the screen, got tops {tops:?}"
    );
}

#[test]
fn a_credits_screen_emits_a_text_primitive_for_every_row_it_composed() {
    // Layout is not paint. Every other test here stops at the layout tree,
    // which is why a screen whose rows measure perfectly and rasterise to
    // nothing passes all of them and shows a blank watch face.
    let mut composition = compose_credits_screen();
    let root = composition.root().expect("credits root");
    let tree = tree(&mut composition, root);
    let scene = crate::renderer::HeadlessRenderer::new().render(&tree);
    let texts: Vec<&str> = scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            crate::renderer::RenderOp::Text { value, .. } => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("ORBIT BREAKER")),
        "the ListHeader's own string should reach the scene; got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Wear OS")),
        "a credit line's string should reach the scene; got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Back")),
        "the button's label should reach the scene; got {texts:?}"
    );
}

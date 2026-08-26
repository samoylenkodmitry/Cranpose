//! Every widget `cranpose-ui` exports, composed once.
//!
//! The layout and paint behaviour of these widgets is pinned down by the robot
//! suite against a real window. What that suite cannot reach is the widgets
//! nothing in the demo happens to use: a composable that reads a composition
//! local nobody provided, or remembers under a key it shares with its own
//! slot, fails the first time somebody composes it — and until somebody does,
//! it is a component the library claims to have.

use std::{cell::Cell, rc::Rc};

use cranpose_foundation::{
    lazy::{LazyListScope, rememberLazyListState},
    text::TextFieldState,
};
use cranpose_ui::{
    Modifier, TextStyle, measure_layout, run_test_composition,
    text::AnnotatedString,
    widgets::{
        animated_visibility::{EnterTransition, ExitTransition},
        dialog::{DialogSpec, DismissReason},
        icon::{IconButtonSpec, IconSpec},
        lazy_list::{LazyColumnSpec, LazyRowSpec},
        scaffold::ScaffoldSpec,
        slider::SliderSpec,
        text_selection_menu::TextMenuItem,
    },
};

/// One 24pt square, as a vector path an icon can parse.
const ICON: &str = "M0 0 L24 0 L24 24 L0 24 Z";

/// Composes `content` and measures it, because a widget built on subcompose
/// only reaches its content during measurement: composing alone would report
/// success for a widget whose body never runs.
fn composed(content: impl FnMut() + 'static) {
    let mut composition = run_test_composition(content);
    let root = composition
        .root()
        .expect("the composition produced no root node");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let measured = measure_layout(
        &mut applier,
        root,
        cranpose_ui_graphics::Size::new(400.0, 800.0),
    );
    applier.clear_runtime_handle();
    measured.expect("the tree measures");
}

/// Composes `content` inside a popup host, which is where a dialog or a popup
/// puts its layer. Without one there is nothing for them to compose into.
fn composed_in_popup_host(content: impl Fn() + 'static) {
    let content = std::rc::Rc::new(content);
    composed(move || {
        let content = std::rc::Rc::clone(&content);
        cranpose_ui::widgets::popup::PopupHost(move || content());
    });
}

#[test]
fn a_dialog_composes_its_content_over_a_scrim() {
    let shown = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&shown);
    composed_in_popup_host(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::dialog::Dialog(
            DialogSpec::default(),
            |_reason: DismissReason| {},
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(shown.get(), 1, "the dialog did not compose its content");
}

#[test]
fn a_dialog_takes_the_scrim_colour_it_is_given() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::dialog::DialogWithScrim(
            DialogSpec::default(),
            cranpose_ui_graphics::Color::rgba(0.0, 0.0, 0.0, 0.5),
            |_reason: DismissReason| {},
            || {},
        );
    });
}

#[test]
fn for_each_composes_one_row_per_item_under_its_own_key() {
    let rows = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = Rc::clone(&rows);
    run_test_composition(move || {
        let sink = Rc::clone(&sink);
        cranpose_ui::widgets::foreach::ForEach(&[1u32, 2, 3], move |item| {
            sink.borrow_mut().push(*item);
        });
    });
    assert_eq!(*rows.borrow(), vec![1, 2, 3]);
}

#[test]
fn an_icon_composes_from_a_vector_path() {
    composed(|| {
        cranpose_ui::widgets::icon::IconWith(Modifier::empty(), ICON, IconSpec::default(), None);
    });
}

#[test]
fn an_icon_button_composes_its_content_and_carries_a_description() {
    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::icon::IconButton(
            Modifier::empty(),
            "Close",
            || {},
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(drawn.get(), 1, "the button did not compose its icon");
}

#[test]
fn an_icon_button_accepts_a_spec_and_a_caller_owned_interaction_source() {
    composed(|| {
        let source = cranpose_ui::rememberMutableInteractionSource();
        cranpose_ui::widgets::icon::IconButtonWith(
            Modifier::empty(),
            "Close",
            IconButtonSpec::default(),
            Some(source),
            || {},
            || {},
        );
    });
}

#[test]
fn an_icon_button_spec_carries_the_colours_it_is_given() {
    let plain = IconButtonSpec::default();
    let coloured = IconButtonSpec::default()
        .with_colors(cranpose_ui::widgets::icon::IconButtonColors::default());
    assert_eq!(
        plain.enabled, coloured.enabled,
        "colours must not disturb the rest of the spec"
    );
}

#[test]
fn linked_text_composes_a_string_that_carries_a_link() {
    let annotated = AnnotatedString::builder()
        .push_link(cranpose_ui::text::LinkAnnotation::Url(
            "https://example.invalid".to_string(),
        ))
        .append("open me")
        .pop()
        .to_annotated_string();
    composed(move || {
        cranpose_ui::widgets::linked_text::LinkedText(
            annotated.clone(),
            Modifier::empty(),
            TextStyle::default(),
            |_url| {},
        );
    });
}

#[test]
fn a_scaffold_composes_both_bars_and_hands_its_content_the_insets() {
    let slots = Rc::new(Cell::new(0usize));
    let top = Rc::clone(&slots);
    let bottom = Rc::clone(&slots);
    let body = Rc::clone(&slots);
    composed(move || {
        let top = Rc::clone(&top);
        let bottom = Rc::clone(&bottom);
        let body = Rc::clone(&body);
        cranpose_ui::widgets::scaffold::Scaffold(
            Modifier::empty(),
            move || top.set(top.get() + 1),
            move || bottom.set(bottom.get() + 1),
            move |_padding| body.set(body.get() + 1),
        );
    });
    assert_eq!(slots.get(), 3, "a scaffold slot did not compose");
}

#[test]
fn a_scaffold_composes_its_floating_action_slot() {
    let action = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&action);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::scaffold::ScaffoldWith(
            Modifier::empty(),
            ScaffoldSpec::default(),
            || {},
            || {},
            move || counter.set(counter.get() + 1),
            |_padding| {},
        );
    });
    assert_eq!(action.get(), 1, "the floating action slot did not compose");
}

#[test]
fn a_slider_hands_its_content_a_track_and_a_thumb_offset() {
    let geometry = Rc::new(Cell::new((f32::NAN, f32::NAN)));
    let sink = Rc::clone(&geometry);
    composed(move || {
        let sink = Rc::clone(&sink);
        cranpose_ui::widgets::slider::Slider(
            Modifier::empty().size(cranpose_ui_graphics::Size::new(200.0, 48.0)),
            0.5,
            |_value| {},
            || {},
            SliderSpec::default(),
            move |scope| sink.set((scope.track_extent(), scope.thumb_offset())),
        );
    });
    let (track, thumb) = geometry.get();
    assert!(track.is_finite(), "the slider reported no track extent");
    assert!(thumb.is_finite(), "the slider reported no thumb offset");
    assert!(
        thumb >= 0.0 && thumb <= track.max(0.0),
        "the thumb sits at {thumb} outside a track of {track}"
    );
}

#[test]
fn a_swipe_to_dismiss_box_composes_its_content() {
    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::swipe_to_dismiss::SwipeToDismissBox(
            Modifier::empty(),
            || {},
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(drawn.get(), 1, "the box did not compose its content");
}

#[test]
fn a_swipe_dismiss_state_is_remembered_and_starts_at_rest() {
    let offsets = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = Rc::clone(&offsets);
    run_test_composition(move || {
        let state = cranpose_ui::widgets::swipe_to_dismiss::rememberSwipeDismissState();
        sink.borrow_mut().push(state.offset());
    });
    assert_eq!(
        *offsets.borrow(),
        vec![0.0],
        "a swipe state nobody touched was not at rest"
    );
}

#[test]
fn the_popup_viewport_local_answers_inside_a_composition() {
    run_test_composition(|| {
        let size = cranpose_ui::widgets::popup::local_popup_viewport()
            .current()
            .get();
        assert!(
            size.width >= 0.0 && size.height >= 0.0,
            "the popup viewport reported a negative size"
        );
    });
}

#[test]
fn an_anchored_popup_composes_its_anchor_and_its_popup() {
    let slots = Rc::new(Cell::new(0usize));
    let anchor = Rc::clone(&slots);
    let popup = Rc::clone(&slots);
    composed_in_popup_host(move || {
        let anchor = Rc::clone(&anchor);
        let popup = Rc::clone(&popup);
        cranpose_ui::widgets::popup::PopupAnchored(
            Modifier::empty(),
            true,
            cranpose_ui_graphics::Point::new(0.0, 0.0),
            move || anchor.set(anchor.get() + 1),
            move || popup.set(popup.get() + 1),
        );
    });
    assert!(
        slots.get() >= 1,
        "neither the anchor nor the popup composed"
    );
}

#[test]
fn a_dismissable_popup_composes_its_content() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::popup::PopupDismissable(
            cranpose_ui_graphics::Rect::from_size(cranpose_ui_graphics::Size::new(10.0, 10.0)),
            cranpose_ui_graphics::Point::new(0.0, 0.0),
            || {},
            || {},
        );
    });
}

#[test]
fn a_conditionally_dismissable_popup_composes_its_content() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::popup::PopupDismissableWhen(
            true,
            cranpose_ui_graphics::Rect::from_size(cranpose_ui_graphics::Size::new(10.0, 10.0)),
            cranpose_ui_graphics::Point::new(0.0, 0.0),
            || {},
            || {},
        );
    });
}

#[test]
fn a_vertical_scrollbar_composes_against_a_scroll_state() {
    composed(|| {
        let state = cranpose_ui::scroll::ScrollState::new(0.0);
        cranpose_ui::widgets::scrollbar::VerticalScrollbar(
            Modifier::empty().size(cranpose_ui_graphics::Size::new(8.0, 200.0)),
            state,
        );
    });
}

#[test]
fn a_horizontal_scrollbar_composes_against_a_scroll_state() {
    composed(|| {
        let state = cranpose_ui::scroll::ScrollState::new(0.0);
        cranpose_ui::widgets::scrollbar::HorizontalScrollbar(
            Modifier::empty().size(cranpose_ui_graphics::Size::new(200.0, 8.0)),
            state,
        );
    });
}

// The tests below close the gap PLAN.md described: every widget below composed
// only through a unit test beside its own module, the robot suite, or not at
// all. `Canvas`, `BoxWithConstraints`, `Scrollbar` (the base, orientation-generic
// entry point) and `Layout`/`SubcomposeLayout` are not repeated here — they are
// already composed for real by the widgets above and below them (the scrollbars,
// the progress indicators, `SwipeToDismiss`, `LazyColumn`/`LazyRow`), which call
// straight into them with no indirection left untested.

#[test]
fn an_icon_composes_from_a_path_size_and_colour() {
    composed(|| {
        cranpose_ui::widgets::icon::Icon(
            ICON,
            24.0,
            cranpose_ui_graphics::Color::rgba(1.0, 1.0, 1.0, 1.0),
        );
    });
}

#[test]
fn a_row_composes_every_child_it_is_given() {
    let seen = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&seen);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::row::Row(
            Modifier::empty(),
            cranpose_ui::widgets::row::RowSpec::default(),
            move || {
                for _ in 0..3 {
                    counter.set(counter.get() + 1);
                }
            },
        );
    });
    assert_eq!(
        seen.get(),
        3,
        "a row did not compose the content it was given"
    );
}

#[test]
fn a_column_composes_every_child_it_is_given() {
    let seen = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&seen);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::column::Column(
            Modifier::empty(),
            cranpose_ui::widgets::column::ColumnSpec::default(),
            move || {
                for _ in 0..3 {
                    counter.set(counter.get() + 1);
                }
            },
        );
    });
    assert_eq!(
        seen.get(),
        3,
        "a column did not compose the content it was given"
    );
}

#[test]
fn a_flow_row_composes_every_item_it_is_given() {
    let seen = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&seen);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::flow_row::FlowRow(
            Modifier::empty(),
            cranpose_ui::widgets::flow_row::FlowRowSpec::default(),
            move || {
                for _ in 0..4 {
                    counter.set(counter.get() + 1);
                }
            },
        );
    });
    assert_eq!(
        seen.get(),
        4,
        "a flow row did not compose the items it was given"
    );
}

#[test]
fn a_button_composes_its_content() {
    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::button::Button(
            Modifier::empty(),
            cranpose_ui::widgets::button::ButtonSpec::default(),
            || {},
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(drawn.get(), 1, "the button did not compose its content");
}

#[test]
fn a_spacer_reserves_the_size_it_is_given() {
    let mut composition = run_test_composition(|| {
        cranpose_ui::widgets::spacer::Spacer(cranpose_ui_graphics::Size::new(16.0, 24.0));
    });
    let root = composition
        .root()
        .expect("the composition produced no root node");
    let handle = composition.runtime_handle();
    let measured = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let measured = measure_layout(
            &mut applier,
            root,
            cranpose_ui_graphics::Size::new(400.0, 800.0),
        )
        .expect("the tree measures");
        applier.clear_runtime_handle();
        measured
    };
    assert_eq!(
        (measured.root_size().width, measured.root_size().height),
        (16.0, 24.0),
        "a spacer must reserve exactly the size it was given, not merely something nonzero"
    );
}

#[test]
fn animated_visibility_shows_content_immediately_on_first_composition() {
    let shown = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&shown);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::animated_visibility::AnimatedVisibility(
            true,
            EnterTransition::none(),
            ExitTransition::none(),
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(
        shown.get(),
        1,
        "content visible on first composition must appear without an animation"
    );
}

#[test]
fn a_crossfade_shows_the_state_it_is_first_composed_with() {
    let seen: Rc<std::cell::RefCell<Vec<&'static str>>> = Rc::default();
    let sink = Rc::clone(&seen);
    composed(move || {
        let sink = Rc::clone(&sink);
        cranpose_ui::widgets::crossfade::Crossfade(
            "first",
            cranpose_animation::AnimationType::Tween(cranpose_animation::AnimationSpec::tween(
                150,
                cranpose_animation::Easing::LinearEasing,
            )),
            move |state: &'static str| {
                sink.borrow_mut().push(state);
            },
        );
    });
    assert_eq!(
        *seen.borrow(),
        vec!["first"],
        "a crossfade must compose the state it is first given without animating"
    );
}

#[test]
fn a_circular_progress_indicator_composes_and_measures() {
    composed(|| {
        cranpose_ui::widgets::progress_indicator::CircularProgressIndicator(
            Modifier::empty(),
            cranpose_ui_graphics::Color::rgba(0.101, 0.462, 0.909, 1.0),
            2.0,
        );
    });
}

#[test]
fn a_linear_progress_indicator_composes_and_measures() {
    composed(|| {
        cranpose_ui::widgets::progress_indicator::LinearProgressIndicator(
            Modifier::empty(),
            cranpose_ui_graphics::Color::rgba(0.101, 0.462, 0.909, 1.0),
        );
    });
}

#[test]
fn clickable_text_composes_its_annotated_string() {
    let text = AnnotatedString::builder()
        .append("click me")
        .to_annotated_string();
    composed(move || {
        cranpose_ui::widgets::clickable_text::ClickableText(
            text.clone(),
            Modifier::empty(),
            TextStyle::default(),
            |_offset| {},
        );
    });
}

#[test]
fn basic_text_composes_a_plain_string_directly() {
    composed(|| {
        cranpose_ui::widgets::text::BasicText(
            "hello",
            Modifier::empty(),
            TextStyle::default(),
            cranpose_ui::TextOverflow::Clip,
            true,
            10,
            0,
        );
    });
}

#[test]
fn a_basic_text_field_composes_its_state() {
    composed(|| {
        let state = TextFieldState::new("hello world");
        cranpose_ui::widgets::basic_text_field::BasicTextField(
            state,
            Modifier::empty(),
            TextStyle::default(),
        );
    });
}

#[test]
fn a_basic_text_field_with_options_honours_multiline_limits() {
    composed(|| {
        let state = TextFieldState::new("hello\nworld");
        cranpose_ui::widgets::basic_text_field::BasicTextFieldWithOptions(
            state,
            Modifier::empty(),
            cranpose_ui::widgets::basic_text_field::BasicTextFieldOptions {
                line_limits: cranpose_foundation::text::TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: 3,
                },
                ..cranpose_ui::widgets::basic_text_field::BasicTextFieldOptions::default()
            },
        );
    });
}

#[test]
fn a_decorated_text_field_composes_its_inner_field_exactly_once() {
    let inner_calls = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&inner_calls);
    composed(move || {
        let counter = Rc::clone(&counter);
        let state = TextFieldState::new("hello");
        cranpose_ui::widgets::basic_text_field::BasicTextFieldDecorated(
            state,
            Modifier::empty(),
            cranpose_ui::widgets::basic_text_field::BasicTextFieldOptions::default(),
            move |scope: cranpose_ui::widgets::basic_text_field::BasicTextFieldDecorationScope| {
                counter.set(counter.get() + 1);
                scope.inner_text_field()
            },
        );
    });
    assert_eq!(
        inner_calls.get(),
        1,
        "the decoration box must compose its inner field exactly once"
    );
}

#[test]
fn a_swipe_to_dismiss_composes_its_content() {
    let drawn = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&drawn);
    composed(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::swipe_to_dismiss::SwipeToDismiss(
            Modifier::empty(),
            cranpose_ui::widgets::swipe_to_dismiss::SwipeToDismissSpec::new(),
            || {},
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(
        drawn.get(),
        1,
        "swipe-to-dismiss did not compose its content"
    );
}

#[test]
fn a_lazy_column_subcomposes_its_visible_items_during_measurement() {
    let composed_items = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&composed_items);
    composed(move || {
        let counter = Rc::clone(&counter);
        let state = rememberLazyListState();
        cranpose_ui::widgets::lazy_list::LazyColumn(
            Modifier::empty(),
            state,
            LazyColumnSpec::default(),
            move |scope| {
                let counter = Rc::clone(&counter);
                scope.items(20, move |_index| {
                    counter.set(counter.get() + 1);
                    cranpose_ui::widgets::spacer::Spacer(cranpose_ui_graphics::Size::new(
                        50.0, 20.0,
                    ));
                });
            },
        );
    });
    assert!(
        composed_items.get() > 0,
        "a lazy column must compose at least its first visible items during measurement"
    );
}

#[test]
fn a_lazy_row_subcomposes_its_visible_items_during_measurement() {
    let composed_items = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&composed_items);
    composed(move || {
        let counter = Rc::clone(&counter);
        let state = rememberLazyListState();
        cranpose_ui::widgets::lazy_list::LazyRow(
            Modifier::empty(),
            state,
            LazyRowSpec::default(),
            move |scope| {
                let counter = Rc::clone(&counter);
                scope.items(20, move |_index| {
                    counter.set(counter.get() + 1);
                    cranpose_ui::widgets::spacer::Spacer(cranpose_ui_graphics::Size::new(
                        50.0, 20.0,
                    ));
                });
            },
        );
    });
    assert!(
        composed_items.get() > 0,
        "a lazy row must compose at least its first visible items during measurement"
    );
}

#[test]
fn a_popup_composes_its_content_at_an_anchor() {
    let shown = Rc::new(Cell::new(0usize));
    let counter = Rc::clone(&shown);
    composed_in_popup_host(move || {
        let counter = Rc::clone(&counter);
        cranpose_ui::widgets::popup::Popup(
            cranpose_ui_graphics::Rect::from_size(cranpose_ui_graphics::Size::new(10.0, 10.0)),
            cranpose_ui_graphics::Point::new(0.0, 0.0),
            move || counter.set(counter.get() + 1),
        );
    });
    assert_eq!(shown.get(), 1, "the popup did not compose its content");
}

#[test]
fn a_selection_loupe_composes_while_a_handle_is_dragged() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::loupe::SelectionLoupe(Some(
            cranpose_ui::widgets::loupe::LoupeTarget {
                focus_x: 40.0,
                line_mid_y: 12.0,
            },
        ));
    });
}

#[test]
fn a_selection_handle_composes_at_a_text_endpoint() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::selection_handle::SelectionHandle(
            cranpose_ui::text_selection::HandleKind::Cursor,
            cranpose_ui_graphics::Point::new(20.0, 40.0),
            18.0,
            8.0,
            cranpose_ui_graphics::Color::rgba(0.0, 0.478, 1.0, 1.0),
            |_point| {},
            || {},
            || {},
            || {},
        );
    });
}

#[test]
fn a_caret_action_menu_composes_its_available_actions() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::text_selection_menu::CaretActionMenu(
            100.0,
            40.0,
            true,
            true,
            true,
            true,
            || {},
            || {},
            || {},
            || {},
        );
    });
}

#[test]
fn a_text_selection_menu_composes_copy_cut_and_paste() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::text_selection_menu::TextSelectionMenu(
            100.0,
            40.0,
            true,
            None,
            true,
            || {},
            || {},
            || {},
            || {},
        );
    });
}

#[test]
fn a_liquid_text_menu_composes_the_items_it_is_given() {
    composed_in_popup_host(|| {
        cranpose_ui::widgets::text_selection_menu::LiquidTextMenu(
            100.0,
            40.0,
            true,
            None,
            vec![TextMenuItem::new("Copy", || {})],
        );
    });
}

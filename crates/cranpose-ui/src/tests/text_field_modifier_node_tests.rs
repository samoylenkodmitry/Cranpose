use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;
use crate::text::TextStyle;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

#[test]
fn text_field_node_creation() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        let node = TextFieldModifierNode::new(state, TextStyle::default());
        assert_eq!(node.text(), "Hello");
        assert!(!node.is_focused());
    });
}

#[test]
fn selection_rects_follow_wrapped_visual_lines() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = "aaaaa\nbb";
    let style = TextStyle::default();
    let line_height = 10.0_f32;

    let rects = range_visual_line_rects(
        text,
        &style,
        None,
        Some(30.0),
        0.0,
        0.0,
        0.0,
        line_height,
        6,
        8,
    );
    assert_eq!(rects.len(), 1, "one visual line touched, got {rects:?}");
    assert_eq!(
        rects[0].y,
        2.0 * line_height,
        "highlight must land on visual line 2, not logical line 1"
    );
    assert!(rects[0].width > 0.0);

    let spanning = range_visual_line_rects(
        text,
        &style,
        None,
        Some(30.0),
        0.0,
        0.0,
        0.0,
        line_height,
        0,
        5,
    );
    assert_eq!(spanning.len(), 2, "wrapped line spans two visual rows");
    assert_eq!(spanning[0].y, 0.0);
    assert_eq!(spanning[1].y, line_height);
}

#[test]
fn tap_resolves_offset_on_wrapped_visual_line() {
    let _app_context = crate::render_state::app_context_test_scope();
    let text = "aaaaa\nbb";
    let style = TextStyle::default();
    let line_height = 10.0_f32;

    let off = crate::text::offset_for_position_wrapped(
        text,
        &style,
        None,
        Some(30.0),
        line_height,
        8.0,
        22.0,
    );
    assert!(
        (6..=8).contains(&off),
        "tap on visual line 'bb' resolved to {off}, expected 6..=8"
    );

    let off1 = crate::text::offset_for_position_wrapped(
        text,
        &style,
        None,
        Some(30.0),
        line_height,
        4.0,
        12.0,
    );
    assert!(
        (3..=5).contains(&off1),
        "tap on wrapped 'aa' resolved to {off1}, expected 3..=5"
    );

    let off2 = crate::text::offset_for_position_wrapped(
        "hello",
        &style,
        None,
        None,
        line_height,
        0.0,
        0.0,
    );
    assert_eq!(off2, 0);
}

#[test]
fn text_field_node_focus() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Test");
        let mut node = TextFieldModifierNode::new(state, TextStyle::default());
        assert!(!node.is_focused());

        node.set_focused(true);
        assert!(node.is_focused());

        node.set_focused(false);
        assert!(!node.is_focused());
    });
}

#[test]
fn text_field_element_creates_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello World");
        let element = TextFieldElement::new(state, TextStyle::default());

        let node = element.create();
        assert_eq!(node.text(), "Hello World");
    });
}

#[test]
fn every_primary_pointer_source_publishes_direct_manipulation_metrics() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("hello world");
        let controller = TextFieldHandleController::new();
        let mut node = TextFieldModifierNode::new(state, TextStyle::default())
            .with_handle_controller(controller.clone());
        node.measured_size.set(Size {
            width: 120.0,
            height: 20.0,
        });

        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");
        let draw = node
            .create_draw_closure()
            .expect("field exposes a draw closure");
        let at = Point { x: 12.0, y: 8.0 };
        let size = Size {
            width: 120.0,
            height: 20.0,
        };
        let run_draw = || {
            let mut scope = crate::draw::command_draw_scope(size);
            draw(&mut scope);
        };

        node.set_focused(true);
        run_draw();
        let keyboard_metrics = controller
            .metrics()
            .expect("focused field publishes handle metrics");
        assert!(!keyboard_metrics.direct_manipulation);

        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
        );
        run_draw();
        let metrics = controller
            .metrics()
            .expect("focused field publishes handle metrics");
        assert!(metrics.focused, "a tap focuses the field");
        assert!(
            metrics.direct_manipulation,
            "a touch tap must expose direct-manipulation handles"
        );
        assert!(
            controller.press().is_some(),
            "touch must publish the live press"
        );

        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Mouse),
        );
        run_draw();
        let metrics = controller
            .metrics()
            .expect("focused field publishes handle metrics");
        assert!(
            metrics.direct_manipulation,
            "a mouse tap must expose the same direct-manipulation handles"
        );
        assert!(
            controller.press().is_some(),
            "mouse must publish the live press"
        );

        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Stylus),
        );
        run_draw();
        let metrics = controller
            .metrics()
            .expect("focused field publishes handle metrics");
        assert!(
            metrics.direct_manipulation,
            "a stylus contact must expose the same direct-manipulation handles"
        );
        assert!(
            controller.press().is_some(),
            "stylus must publish the live press"
        );

        crate::text_field_focus::clear_focus();
    });
}

#[test]
fn double_tap_selects_the_word_under_the_finger() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("hello world");
        let node = TextFieldModifierNode::new(state, TextStyle::default());
        node.measured_size.set(Size {
            width: 200.0,
            height: 20.0,
        });
        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");

        let at = Point { x: 2.0, y: 8.0 };
        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
        );
        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
        );

        let selection = state.selection();
        assert!(
            !selection.collapsed(),
            "a double tap must produce a (word) selection, got {selection:?}"
        );
        let selected = &state.text()[selection.min()..selection.max()];
        assert_eq!(
            selected, "hello",
            "double tap should select the whole word under the finger"
        );

        crate::text_field_focus::clear_focus();
    });
}

#[test]
fn repeated_taps_escalate_word_line_paragraph_then_cycle() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let text = "alpha beta\ngamma delta\n\nsecond para";
        let state = TextFieldState::new(text);
        let node = TextFieldModifierNode::new(state, TextStyle::default()).with_line_limits(
            TextFieldLineLimits::MultiLine {
                min_lines: 1,
                max_lines: usize::MAX,
            },
        );
        node.measured_size.set(Size {
            width: 400.0,
            height: 80.0,
        });
        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");

        let at = Point { x: 2.0, y: 4.0 };
        let tap = || {
            handler(
                PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
            );
        };
        let selected = |state: &TextFieldState| {
            let s = state.selection();
            state.text()[s.min()..s.max()].to_string()
        };

        tap();
        assert!(state.selection().collapsed(), "first tap places the caret");
        tap();
        assert_eq!(selected(&state), "alpha", "double tap selects the word");
        tap();
        assert_eq!(
            selected(&state),
            "alpha beta",
            "triple tap selects the line"
        );
        tap();
        assert_eq!(
            selected(&state),
            "alpha beta\ngamma delta",
            "fourth tap grows to the paragraph"
        );
        tap();
        assert_eq!(
            selected(&state),
            "alpha",
            "fifth tap cycles back to the word"
        );

        crate::text_field_focus::clear_focus();
    });
}

#[test]
fn single_tap_inside_selection_selects_the_word() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("hello world");
        let node = TextFieldModifierNode::new(state, TextStyle::default());
        node.measured_size.set(Size {
            width: 200.0,
            height: 20.0,
        });
        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");

        state.edit(|buffer| buffer.select(TextRange::new(0, 11)));
        assert!(!state.selection().collapsed());

        let at = Point { x: 2.0, y: 8.0 };
        handler(
            PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
        );

        let selection = state.selection();
        assert!(
            !selection.collapsed(),
            "a tap inside a selection must not collapse it, got {selection:?}"
        );
        assert_eq!(
            &state.text()[selection.min()..selection.max()],
            "hello",
            "a tap inside a selection re-selects the word under the finger"
        );

        crate::text_field_focus::clear_focus();
    });
}

#[test]
fn slow_taps_inside_selection_cycle_word_line_paragraph_by_location() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let text = "alpha beta\ngamma delta\n\nsecond para";
        let state = TextFieldState::new(text);
        let node = TextFieldModifierNode::new(state, TextStyle::default()).with_line_limits(
            TextFieldLineLimits::MultiLine {
                min_lines: 1,
                max_lines: usize::MAX,
            },
        );
        node.measured_size.set(Size {
            width: 400.0,
            height: 80.0,
        });
        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");

        state.edit(|buffer| buffer.select(TextRange::new(0, text.len())));

        let at = Point { x: 2.0, y: 4.0 };
        let selected = |state: &TextFieldState| {
            let s = state.selection();
            state.text()[s.min()..s.max()].to_string()
        };
        let slow_tap = || {
            node.refs.last_click_time.set(None);
            handler(
                PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
            );
        };

        slow_tap();
        assert_eq!(
            selected(&state),
            "alpha",
            "tap inside selection grabs the word"
        );
        slow_tap();
        assert_eq!(
            selected(&state),
            "alpha beta",
            "same-spot tap grows to the line even after the timeout"
        );
        slow_tap();
        assert_eq!(
            selected(&state),
            "alpha beta\ngamma delta",
            "same-spot tap grows to the paragraph"
        );
        slow_tap();
        assert_eq!(
            selected(&state),
            "alpha",
            "same-spot tap cycles back to the word"
        );

        crate::text_field_focus::clear_focus();
    });
}

#[test]
fn text_field_element_equality() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state1 = TextFieldState::new("Hello");
        let state2 = TextFieldState::new("Hello");

        let elem1 = TextFieldElement::new(state1, TextStyle::default());
        let elem2 = TextFieldElement::new(state1, TextStyle::default());
        let elem3 = TextFieldElement::new(state2, TextStyle::default());

        assert_eq!(elem1, elem2, "Same state should be equal");
        assert_ne!(elem1, elem3, "Different states should not be equal");
    });
}

#[test]
fn text_field_element_update_refreshes_existing_node_style() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("themed text");
        let dark_style = TextStyle::from_span_style(crate::text::SpanStyle {
            color: Some(Color::from_rgba_u8(228, 240, 252, 255)),
            ..crate::text::SpanStyle::default()
        });
        let light_style = TextStyle::from_span_style(crate::text::SpanStyle {
            color: Some(Color::from_rgba_u8(14, 58, 96, 255)),
            ..crate::text::SpanStyle::default()
        });
        let initial = TextFieldElement::new(state, dark_style);
        let updated = TextFieldElement::new(state, light_style.clone());
        let mut node = initial.create();

        updated.update(&mut node);

        assert_eq!(node.text(), "themed text");
        assert_eq!(node.style(), &light_style);
    });
}

#[test]
fn multiline_field_measures_wrapped_height() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let long = "abcd ".repeat(40);
        let state = TextFieldState::new(&long);
        let node = TextFieldModifierNode::new(state, TextStyle::default());
        assert!(
            !node.line_limits().is_single_line(),
            "default fields are multi-line"
        );

        let natural = node.measure_text_content(None);
        let wrapped = node.measure_text_content(node.wrap_width(20.0));

        assert!(
            wrapped.height > natural.height,
            "wrapped multi-line height {} must exceed the single-line height {}",
            wrapped.height,
            natural.height
        );
    });
}

#[test]
fn single_line_field_never_wraps() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("abcd ".repeat(40));
        let node = TextFieldModifierNode::new(state, TextStyle::default())
            .with_line_limits(TextFieldLineLimits::SingleLine);
        assert_eq!(
            node.wrap_width(20.0),
            None,
            "single-line fields must not wrap"
        );
    });
}

#[test]
fn test_cursor_x_position_calculation() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = crate::text::TextStyle::default();

        let empty_width =
            crate::text::measure_text(&crate::text::AnnotatedString::from(""), &style).width;
        assert!(
            empty_width.abs() < 0.1,
            "Empty text should have 0 width, got {empty_width}"
        );

        let hi_width =
            crate::text::measure_text(&crate::text::AnnotatedString::from("Hi"), &style).width;
        assert!(
            hi_width > 0.0,
            "Text 'Hi' should have positive width: {hi_width}"
        );

        let h_width =
            crate::text::measure_text(&crate::text::AnnotatedString::from("H"), &style).width;
        assert!(h_width > 0.0, "Text 'H' should have positive width");
        assert!(
            h_width < hi_width,
            "'H' width {h_width} should be less than 'Hi' width {hi_width}"
        );

        let state = TextFieldState::new("Hi");
        assert_eq!(
            state.selection().start,
            2,
            "Cursor should be at position 2 (end of 'Hi')"
        );

        let text = state.text();
        let cursor_pos = state.selection().start;
        let text_before_cursor = &text[..cursor_pos.min(text.len())];
        assert_eq!(text_before_cursor, "Hi");

        let cursor_x = crate::text::measure_text(
            &crate::text::AnnotatedString::from(text_before_cursor),
            &style,
        )
        .width;
        assert!(
            (cursor_x - hi_width).abs() < 0.1,
            "Cursor x {cursor_x} should equal 'Hi' width {hi_width}"
        );
    });
}

#[test]
fn test_focused_node_creates_cursor() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Test");
        let element = TextFieldElement::new(state, TextStyle::default());
        let node = element.create();

        assert!(!node.is_focused());

        *node.refs.is_focused.borrow_mut() = true;
        assert!(node.is_focused());

        assert_eq!(node.text(), "Test");

        assert_eq!(node.selection().start, 4);
    });
}

#[test]
fn a_focus_requester_makes_the_text_field_receive_keyboard_input() {
    use cranpose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

    use crate::{
        key_event::{KeyCode, KeyEvent, KeyEventType, Modifiers},
        modifier::{FocusRequester, FocusRequesterElement},
    };

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("");
        let requester = FocusRequester::new();

        let mut context = BasicModifierNodeContext::new();
        context.set_node_id(Some(1));
        let mut chain = ModifierNodeChain::new();
        chain.update(
            vec![
                cranpose_foundation::modifier_element(FocusRequesterElement::new(
                    requester.clone(),
                )),
                cranpose_foundation::modifier_element(TextFieldElement::new(
                    state,
                    TextStyle::default(),
                )),
            ],
            &mut context,
        );

        assert!(!crate::text_field_focus::has_focused_field());

        requester
            .request_focus()
            .expect("the text field must accept a programmatic focus request");

        assert!(crate::text_field_focus::has_focused_field());

        let key_down = KeyEvent::new(KeyCode::H, "h", Modifiers::NONE, KeyEventType::KeyDown);
        assert!(
            crate::text_field_focus::dispatch_key_event(&key_down),
            "the field must consume a key event once focused programmatically"
        );
        assert_eq!(state.text(), "h");
    });
}

#[test]
fn two_text_fields_hand_off_keyboard_focus_via_their_requesters() {
    use cranpose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

    use crate::modifier::{FocusRequester, FocusRequesterElement};

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state_a = TextFieldState::new("a-text");
        let state_b = TextFieldState::new("b-text");
        let requester_a = FocusRequester::new();
        let requester_b = FocusRequester::new();

        let mut context = BasicModifierNodeContext::new();
        context.set_node_id(Some(1));
        let mut chain_a = ModifierNodeChain::new();
        chain_a.update(
            vec![
                cranpose_foundation::modifier_element(FocusRequesterElement::new(
                    requester_a.clone(),
                )),
                cranpose_foundation::modifier_element(TextFieldElement::new(
                    state_a,
                    TextStyle::default(),
                )),
            ],
            &mut context,
        );

        context.set_node_id(Some(2));
        let mut chain_b = ModifierNodeChain::new();
        chain_b.update(
            vec![
                cranpose_foundation::modifier_element(FocusRequesterElement::new(
                    requester_b.clone(),
                )),
                cranpose_foundation::modifier_element(TextFieldElement::new(
                    state_b,
                    TextStyle::default(),
                )),
            ],
            &mut context,
        );

        requester_a.request_focus().expect("field a accepts focus");
        assert_eq!(
            crate::text_field_focus::focused_field_node(),
            Some(1),
            "field a should own text-field keyboard focus"
        );

        requester_b.request_focus().expect("field b accepts focus");
        assert_eq!(
            crate::text_field_focus::focused_field_node(),
            Some(2),
            "field b must take over text-field keyboard focus from field a"
        );
    });
}

#[test]
fn a_mouse_double_click_keeps_the_word_through_its_release_and_a_jitter() {
    use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("hello world");
        let node = TextFieldModifierNode::new(state, TextStyle::default());
        node.measured_size.set(Size {
            width: 200.0,
            height: 20.0,
        });
        let handler = node
            .pointer_input_handler()
            .expect("field exposes a pointer handler");
        let event = |kind, x: f32| {
            let at = Point { x, y: 8.0 };
            PointerEvent::new(kind, at, at).with_source(PointerSource::Mouse)
        };

        handler(event(PointerEventKind::Down, 60.0));
        handler(event(PointerEventKind::Up, 60.0));
        handler(event(PointerEventKind::Down, 60.0));
        handler(event(PointerEventKind::Move, 60.0));
        handler(event(PointerEventKind::Move, 61.0));
        handler(event(PointerEventKind::Up, 61.0));

        let selection = state.selection();
        assert_eq!(
            &state.text()[selection.min()..selection.max()],
            "world",
            "a double click selects the word under the pointer, got {selection:?}"
        );

        crate::text_field_focus::clear_focus();
    });
}

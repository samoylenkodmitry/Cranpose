//! Tests for cursor draw command position in text fields.

use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, modifier_element,
    text::{TextFieldLineLimits, TextFieldState, TextRange},
};

use crate::{
    modifier::{collect_modifier_slices, collect_semantics_from_chain},
    text::{SpanStyle, TextStyle, TextUnit},
    text_field_modifier_node::{TextFieldElement, TextFieldModifierNode},
};

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

/// Records a draw command into a fresh consumer-owned scope, the way the
/// renderers do, and returns what it recorded.
fn record(
    func: &crate::draw::DrawCommandFn,
    size: crate::modifier::Size,
) -> Vec<cranpose_ui_graphics::DrawPrimitive> {
    use cranpose_ui_graphics::DrawScope as _;
    let mut scope = crate::draw::command_draw_scope(size);
    func(&mut scope);
    scope.into_primitives()
}

fn focused_text_field_chain(state: TextFieldState, style: TextStyle) -> ModifierNodeChain {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![modifier_element(TextFieldElement::new(state, style))];
    chain.update_from_slice(&elements, &mut context);

    let mut node = chain
        .node_mut::<TextFieldModifierNode>(0)
        .expect("text field node exists");
    node.set_focused(true);
    drop(node);

    chain
}

fn element_hash(element: &TextFieldElement) -> u64 {
    let mut hasher = cranpose_core::hash::default::new();
    element.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn text_field_element_hash_tracks_style_and_line_limits() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("same");
        let base = TextFieldElement::new(state, TextStyle::default());
        let styled = TextFieldElement::new(
            state,
            TextStyle::from_span_style(SpanStyle {
                font_size: TextUnit::Sp(18.0),
                ..SpanStyle::default()
            }),
        );
        let single_line = TextFieldElement::new(state, TextStyle::default())
            .with_line_limits(TextFieldLineLimits::SingleLine);

        assert_ne!(element_hash(&base), element_hash(&styled));
        assert_ne!(element_hash(&base), element_hash(&single_line));
    });
}

#[test]
fn text_field_semantics_expose_editable_selection_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::with_selection("Hello World", TextRange::new(1, 5));
        let chain = focused_text_field_chain(state, TextStyle::default());
        let semantics = collect_semantics_from_chain(&chain).expect("text field semantics");

        assert_eq!(
            semantics.content_description.as_deref(),
            Some("Hello World")
        );
        assert!(semantics.is_editable_text);
        assert_eq!(semantics.text_selection, Some(TextRange::new(1, 5)));
    });
}

/// Test that cursor draw command is created when text field is focused.
#[test]
fn cursor_draw_command_created_when_focused() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        let style = TextStyle::default();
        let chain = focused_text_field_chain(state, style);

        // Collect slices - this should create cursor draw command
        let slices = collect_modifier_slices(&chain);

        // There should be at least one draw command (the cursor)
        assert!(
            !slices.draw_commands().is_empty(),
            "Expected cursor draw command when text field is focused"
        );
    });
}

/// Test that cursor x position matches the width of text before cursor.
#[test]
fn cursor_x_position_matches_text_width() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        // Cursor should be at end (position 5)
        assert_eq!(state.selection().start, 5);

        let style = TextStyle::default();
        let chain = focused_text_field_chain(state, style.clone());

        let slices = collect_modifier_slices(&chain);

        // Execute the draw command to get the primitives
        let size = crate::modifier::Size {
            width: 200.0,
            height: 40.0,
        };
        let draw_commands = slices.draw_commands();
        assert!(!draw_commands.is_empty());

        // Execute the field's OVERLAY command (the caret layer): the field
        // now also registers a Behind command for the selection highlight,
        // so pick the overlay explicitly instead of assuming index 0.
        let primitives = draw_commands
            .iter()
            .find_map(|command| match command {
                crate::DrawCommand::Overlay(func) => Some(record(func, size)),
                _ => None,
            })
            .expect("Expected Overlay draw command for cursor");

        assert!(!primitives.is_empty(), "Expected cursor primitive");

        // Get the cursor rect
        let cursor_rect = match &primitives[0] {
            cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
            _ => panic!("Expected Rect primitive for cursor"),
        };

        // Expected cursor x = width of "Hello"
        let expected_x =
            crate::text::measure_text(&crate::text::AnnotatedString::from("Hello"), &style).width;

        assert!(
            (cursor_rect.x - expected_x).abs() < 0.1,
            "Cursor x position {} should match text width {}",
            cursor_rect.x,
            expected_x
        );
    });
}

/// Test that cursor position is 0 for empty text.
#[test]
fn cursor_at_start_for_empty_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("");
        let style = TextStyle::default();
        let chain = focused_text_field_chain(state, style);

        let slices = collect_modifier_slices(&chain);
        let size = crate::modifier::Size {
            width: 200.0,
            height: 40.0,
        };

        let primitives = slices
            .draw_commands()
            .iter()
            .find_map(|command| match command {
                crate::DrawCommand::Overlay(func) => Some(record(func, size)),
                _ => None,
            })
            .expect("Expected Overlay");

        let cursor_rect = match &primitives[0] {
            cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
            _ => panic!("Expected Rect"),
        };

        assert!(
            cursor_rect.x.abs() < 0.1,
            "Cursor x should be 0 for empty text, got {}",
            cursor_rect.x
        );
    });
}

/// Test that selection draw command is created when text is selected.
#[test]
fn selection_draw_command_created_when_selected() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::with_selection("Hello World", TextRange::new(0, 5));
        let style = TextStyle::default();
        let chain = focused_text_field_chain(state, style.clone());

        let slices = collect_modifier_slices(&chain);

        let draw_commands = slices.draw_commands();
        assert!(
            !draw_commands.is_empty(),
            "Expected selection/cursor draw command"
        );

        let size = crate::modifier::Size {
            width: 200.0,
            height: 40.0,
        };

        // Get the Behind command (selection)
        let primitives = match &draw_commands[0] {
            crate::DrawCommand::Behind(func) => record(func, size),
            crate::DrawCommand::Overlay(func) => record(func, size),
            crate::DrawCommand::WithContent(func) => record(func, size),
        };

        assert!(!primitives.is_empty(), "Expected selection primitive");

        // Selection width should match width of "Hello"
        let expected_width =
            crate::text::measure_text(&crate::text::AnnotatedString::from("Hello"), &style).width;
        let selection_rect = primitives.iter().find_map(|primitive| {
            if let cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. } = primitive {
                if (rect.width - expected_width).abs() < 1.0 {
                    return Some(rect);
                }
            }
            None
        });

        let selection_rect = selection_rect.expect("Expected selection rect for highlighted text");
        assert!(
            selection_rect.y.abs() < 0.1,
            "Selection y should be 0 without padding, got {}",
            selection_rect.y
        );
    });
}

/// Test that cursor Y position is at 0 without any padding.
#[test]
fn cursor_y_position_at_zero_without_padding() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = TextFieldState::new("Test");
        let style = TextStyle::default();
        let chain = focused_text_field_chain(state, style);

        let slices = collect_modifier_slices(&chain);
        let size = crate::modifier::Size {
            width: 200.0,
            height: 40.0,
        };

        // Get last command which should be cursor
        let cursor_cmd = slices.draw_commands().last().unwrap();
        let primitives = match cursor_cmd {
            crate::DrawCommand::Overlay(func) => record(func, size),
            crate::DrawCommand::WithContent(func) => record(func, size),
            _ => panic!("Expected Overlay for cursor"),
        };

        if primitives.is_empty() {
            // Cursor might be in blink-off phase, that's ok
            return;
        }

        let cursor_rect = match &primitives[0] {
            cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
            _ => panic!("Expected Rect"),
        };

        // Without padding, Y should be at 0
        assert!(
            cursor_rect.y.abs() < 0.1,
            "Cursor y should be 0 without padding, got {}",
            cursor_rect.y
        );
    });
}

// ============================================================================
// Horizontal pan (single-line cursor-follow scrolling) and clipping tests
// ============================================================================

use crate::text_field_modifier_node::{compute_horizontal_scroll_offset, intersect_rect};

const CURSOR_WIDTH: f32 = 2.0;
const LONG_TEXT: &str = "The quick brown fox jumps over the lazy dog and keeps on running";

fn focused_single_line_chain(state: TextFieldState, style: TextStyle) -> ModifierNodeChain {
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    let elements = vec![modifier_element(
        TextFieldElement::new(state, style).with_line_limits(TextFieldLineLimits::SingleLine),
    )];
    chain.update_from_slice(&elements, &mut context);

    let mut node = chain
        .node_mut::<TextFieldModifierNode>(0)
        .expect("text field node exists");
    node.set_focused(true);
    drop(node);

    chain
}

/// Tap-count classification drives the selection through the real pointer
/// handler: one tap places the cursor, two select the word under the tap, and
/// three select its line/paragraph.
#[test]
fn tap_count_selects_cursor_word_and_line() {
    use cranpose_foundation::{
        PointerInputNode,
        nodes::input::{PointerEvent, PointerEventKind},
    };
    use cranpose_ui_graphics::Point;

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = TextStyle::default();
        let state = TextFieldState::new("hello world\nsecond line");
        let chain = focused_text_field_chain(state, style.clone());
        let handler = chain
            .node::<TextFieldModifierNode>(0)
            .expect("text field node")
            .pointer_input_handler()
            .expect("pointer handler");

        // Tap at the x of the middle of "world" so the offset lands inside it.
        let tap_x =
            crate::text::measure_text(&crate::text::AnnotatedString::from("hello wor"), &style)
                .width;
        let tap = || {
            let position = Point { x: tap_x, y: 0.0 };
            handler(PointerEvent::new(
                PointerEventKind::Down,
                position,
                position,
            ));
        };

        // Single tap: collapsed cursor inside "world".
        tap();
        let single = state.selection();
        assert!(single.collapsed(), "single tap must collapse the selection");
        assert!(
            (6..=11).contains(&single.start),
            "single tap must land inside 'world', got {single:?}"
        );

        // Second tap (same spot, immediately): selects the word "world".
        tap();
        assert_eq!(state.selection(), TextRange::new(6, 11));

        // Third tap: selects the whole first line (up to the newline).
        tap();
        assert_eq!(state.selection(), TextRange::new(0, 11));

        crate::text_field_focus::clear_focus();
    });
}

fn run_field_draw(
    chain: &ModifierNodeChain,
    size: crate::modifier::Size,
) -> Vec<cranpose_ui_graphics::DrawPrimitive> {
    let slices = collect_modifier_slices(chain);
    let draw_commands = slices.draw_commands();
    assert!(!draw_commands.is_empty(), "expected draw commands");
    draw_commands
        .iter()
        .flat_map(|command| match command {
            crate::DrawCommand::Behind(func) => record(func, size),
            crate::DrawCommand::Overlay(func) => record(func, size),
            crate::DrawCommand::WithContent(func) => record(func, size),
        })
        .collect()
}

/// Pan-offset math keeps the cursor in view at both ends of the text.
#[test]
fn horizontal_scroll_offset_math_keeps_cursor_visible() {
    let viewport = 100.0;
    let text_width = 400.0;

    // Cursor past the right edge: pan so the cursor sits at the right edge.
    let offset = compute_horizontal_scroll_offset(0.0, 400.0, text_width, viewport);
    assert!(
        (offset - (400.0 - viewport + CURSOR_WIDTH)).abs() < 0.01,
        "cursor at text end must pan to right edge, got {offset}"
    );
    let cursor_in_viewport = 400.0 - offset;
    assert!(cursor_in_viewport >= 0.0 && cursor_in_viewport + CURSOR_WIDTH <= viewport);

    // Cursor within the visible window: offset must not move.
    let stable = compute_horizontal_scroll_offset(offset, 350.0, text_width, viewport);
    assert_eq!(stable, offset, "offset must be stable while cursor visible");

    // Cursor moved left of the visible window: pan back so it sits at the left edge.
    let left = compute_horizontal_scroll_offset(offset, 50.0, text_width, viewport);
    assert!(
        (left - 50.0).abs() < 0.01,
        "cursor left of window must pan back, got {left}"
    );
    assert!((50.0 - left) >= 0.0 && (50.0 - left) + CURSOR_WIDTH <= viewport);

    // Cursor at the very start: offset returns to zero.
    let start = compute_horizontal_scroll_offset(left, 0.0, text_width, viewport);
    assert_eq!(start, 0.0, "cursor at text start must fully pan back");

    // Text narrower than the viewport: never pans.
    assert_eq!(
        compute_horizontal_scroll_offset(37.0, 40.0, 60.0, viewport),
        0.0
    );

    // Offset clamps to the scrollable range even from a stale value.
    let clamped = compute_horizontal_scroll_offset(1000.0, 200.0, text_width, viewport);
    assert!(clamped <= text_width + CURSOR_WIDTH - viewport + 0.01);

    // Degenerate viewport is a no-op.
    assert_eq!(
        compute_horizontal_scroll_offset(10.0, 50.0, 400.0, 0.0),
        0.0
    );
}

/// Dragging a selection handle past the visible window auto-pans the field so
/// the dragged edge stays in view, reusing the same cursor-follow pan resolver.
#[test]
fn handle_drag_auto_pans_to_keep_dragged_edge_visible() {
    use crate::text_selection::{HandleKind, selection_after_handle_drag};

    let viewport = 100.0;
    let text_width = 400.0;

    // Drag the end handle to a text offset whose x sits well past the right
    // edge: the pan advances so the dragged edge lands at the right edge.
    let dragged_edge_x = 360.0;
    let panned = compute_horizontal_scroll_offset(0.0, dragged_edge_x, text_width, viewport);
    let edge_in_viewport = dragged_edge_x - panned;
    assert!(
        edge_in_viewport >= 0.0 && edge_in_viewport + CURSOR_WIDTH <= viewport + 0.01,
        "dragged edge must be visible after auto-pan, got {edge_in_viewport}"
    );

    // The selection math keeps the fixed (start) edge and extends to the drag.
    let (min, max) = selection_after_handle_drag(HandleKind::SelectionEnd, 2, 40, 64);
    assert_eq!((min, max), (2, 40));

    // Dragging the same handle back before the visible window pans back to it.
    let back_edge_x = 20.0;
    let panned_back = compute_horizontal_scroll_offset(panned, back_edge_x, text_width, viewport);
    assert!(
        (back_edge_x - panned_back) >= 0.0,
        "dragging the edge back must pan back to keep it visible"
    );
}

#[test]
fn intersect_rect_clips_to_bounds() {
    let bounds = cranpose_ui_graphics::Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 40.0,
    };
    // Fully inside: unchanged.
    let inner = cranpose_ui_graphics::Rect {
        x: 10.0,
        y: 5.0,
        width: 20.0,
        height: 10.0,
    };
    assert_eq!(intersect_rect(inner, bounds), Some(inner));
    // Overhanging the right edge: clamped.
    let overhang = cranpose_ui_graphics::Rect {
        x: 90.0,
        y: 0.0,
        width: 50.0,
        height: 40.0,
    };
    let clipped = intersect_rect(overhang, bounds).expect("clipped rect");
    assert_eq!(clipped.x, 90.0);
    assert_eq!(clipped.width, 10.0);
    // Fully outside: dropped.
    let outside = cranpose_ui_graphics::Rect {
        x: 150.0,
        y: 0.0,
        width: 20.0,
        height: 40.0,
    };
    assert_eq!(intersect_rect(outside, bounds), None);
}

/// A single-line field with text wider than the viewport pans so the cursor
/// (at the end of the text) stays visible at the right edge.
#[test]
fn single_line_field_pans_to_keep_cursor_visible() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = TextStyle::default();
        let text_width =
            crate::text::measure_text(&crate::text::AnnotatedString::from(LONG_TEXT), &style).width;
        let size = crate::modifier::Size {
            width: 80.0,
            height: 40.0,
        };
        assert!(
            text_width > size.width,
            "test precondition: text must be wider than the field"
        );

        // Cursor is placed at the end of the text by default.
        let state = TextFieldState::new(LONG_TEXT);
        let chain = focused_single_line_chain(state, style);
        let primitives = run_field_draw(&chain, size);

        let cursor_rect = primitives
            .iter()
            .find_map(|primitive| match primitive {
                cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. }
                    if rect.width <= CURSOR_WIDTH + 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("cursor rect must be drawn");

        // The cursor must sit at the right edge of the viewport, not at
        // text_width (which lies far outside the field).
        assert!(
            cursor_rect.x + cursor_rect.width <= size.width + 0.01,
            "cursor must stay inside the field, got x={}",
            cursor_rect.x
        );
        assert!(
            cursor_rect.x >= size.width - CURSOR_WIDTH - 0.5,
            "cursor must be panned to the right edge, got x={} (field width {})",
            cursor_rect.x,
            size.width
        );

        // The node reports the pan offset that renderers use to shift glyphs.
        let node = chain
            .node::<TextFieldModifierNode>(0)
            .expect("text field node");
        let offset = node.scroll_offset();
        assert!(
            (offset - (text_width + CURSOR_WIDTH - size.width)).abs() < 0.5,
            "pan offset {offset} must scroll the text end into view"
        );
    });
}

/// Moving the cursor back to the start pans the text fully back.
#[test]
fn single_line_field_pans_back_when_cursor_moves_to_start() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = TextStyle::default();
        let size = crate::modifier::Size {
            width: 80.0,
            height: 40.0,
        };
        let state = TextFieldState::new(LONG_TEXT);
        let chain = focused_single_line_chain(state, style);

        // First draw: cursor at end, field pans right.
        let _ = run_field_draw(&chain, size);
        {
            let node = chain
                .node::<TextFieldModifierNode>(0)
                .expect("text field node");
            assert!(node.scroll_offset() > 0.0, "expected initial pan");
        }

        // Move the cursor to the start and draw again.
        state.edit(|buffer| buffer.place_cursor_at_start());
        let primitives = run_field_draw(&chain, size);

        let cursor_rect = primitives
            .iter()
            .find_map(|primitive| match primitive {
                cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. }
                    if rect.width <= CURSOR_WIDTH + 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("cursor rect must be drawn");
        assert!(
            cursor_rect.x.abs() < 0.01,
            "cursor at text start must draw at the left edge, got {}",
            cursor_rect.x
        );

        let node = chain
            .node::<TextFieldModifierNode>(0)
            .expect("text field node");
        assert_eq!(node.scroll_offset(), 0.0, "pan must return to zero");
    });
}

/// Selecting text that extends beyond the viewport must not draw selection
/// rectangles outside the field bounds.
#[test]
fn selection_primitives_clipped_to_field_bounds() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = TextStyle::default();
        let size = crate::modifier::Size {
            width: 80.0,
            height: 40.0,
        };
        // Select the entire long text; unclipped, the highlight would extend
        // far past the field's right edge.
        let state = TextFieldState::with_selection(LONG_TEXT, TextRange::new(0, LONG_TEXT.len()));
        let chain = focused_single_line_chain(state, style);
        let primitives = run_field_draw(&chain, size);
        assert!(!primitives.is_empty(), "expected selection primitives");

        for primitive in &primitives {
            if let cranpose_ui_graphics::DrawPrimitive::Rect { rect, .. } = primitive {
                assert!(
                    rect.x >= -0.01 && rect.x + rect.width <= size.width + 0.01,
                    "primitive escapes field horizontally: {rect:?} (field width {})",
                    size.width
                );
                assert!(
                    rect.y >= -0.01 && rect.y + rect.height <= size.height + 0.01,
                    "primitive escapes field vertically: {rect:?} (field height {})",
                    size.height
                );
            }
        }
    });
}

/// Multi-line fields do not pan; only single-line fields expose a pan
/// resolver for the renderer to shift text glyphs.
#[test]
fn only_single_line_fields_expose_pan_resolver() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let style = TextStyle::default();

        let single = focused_single_line_chain(TextFieldState::new(LONG_TEXT), style.clone());
        let single_slices = collect_modifier_slices(&single);
        let resolver = single_slices
            .text_pan_resolver()
            .expect("single-line field exposes a pan resolver");
        // The resolver reports the offset for a given viewport width and
        // keeps the node's stored offset in sync.
        let offset = resolver(80.0);
        assert!(offset > 0.0, "long text must pan, got {offset}");

        let multi = focused_text_field_chain(TextFieldState::new(LONG_TEXT), style);
        let multi_slices = collect_modifier_slices(&multi);
        assert!(
            multi_slices.text_pan_resolver().is_none(),
            "multi-line fields must not pan horizontally"
        );
    });
}

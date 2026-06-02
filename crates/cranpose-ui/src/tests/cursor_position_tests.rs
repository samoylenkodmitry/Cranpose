//! Tests for cursor draw command position in text fields.

use crate::modifier::{collect_modifier_slices, collect_semantics_from_chain};
use crate::text::{SpanStyle, TextStyle, TextUnit};
use crate::text_field_modifier_node::{TextFieldElement, TextFieldModifierNode};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};
use cranpose_foundation::{modifier_element, BasicModifierNodeContext, ModifierNodeChain};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
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
        let base = TextFieldElement::new(state.clone(), TextStyle::default());
        let styled = TextFieldElement::new(
            state.clone(),
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

        // Get the first Overlay command and execute it
        let primitives = match &draw_commands[0] {
            crate::DrawCommand::Overlay(func) => func(size),
            crate::DrawCommand::WithContent(func) => func(size),
            _ => panic!("Expected Overlay draw command for cursor"),
        };

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

        let primitives = match &slices.draw_commands()[0] {
            crate::DrawCommand::Overlay(func) => func(size),
            crate::DrawCommand::WithContent(func) => func(size),
            _ => panic!("Expected Overlay"),
        };

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
            crate::DrawCommand::Behind(func) => func(size),
            crate::DrawCommand::Overlay(func) => func(size),
            crate::DrawCommand::WithContent(func) => func(size),
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
            crate::DrawCommand::Overlay(func) => func(size),
            crate::DrawCommand::WithContent(func) => func(size),
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

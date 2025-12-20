//! Tests for cursor draw command position in text fields.

use compose_foundation::text::TextFieldState;
use compose_foundation::{ModifierNodeChain, ModifierNodeElement};
use crate::modifier::collect_modifier_slices;
use crate::text_field_modifier_node::{TextFieldElement, TextFieldModifierNode};
use crate::text_field_focus::request_focus;
use std::rc::Rc;
use std::cell::RefCell;

/// Test that cursor draw command is created when text field is focused.
#[test]
fn cursor_draw_command_created_when_focused() {
    let state = TextFieldState::new("Hello");
    let element = TextFieldElement::new(state.clone());
    
    // Create the node
    let mut node = element.create();
    
    // Request focus (this sets the is_focused flag)
    let focus_flag = Rc::new(RefCell::new(false));
    *node.is_focused.borrow_mut() = true;
    
    // Create a modifier chain with just this node
    let mut chain = ModifierNodeChain::default();
    chain.push(Box::new(node));
    
    // Collect slices - this should create cursor draw command
    let slices = collect_modifier_slices(&chain);
    
    // There should be at least one draw command (the cursor)
    assert!(!slices.draw_commands().is_empty(), 
        "Expected cursor draw command when text field is focused");
}

/// Test that cursor x position matches the width of text before cursor.
#[test]
fn cursor_x_position_matches_text_width() {
    let state = TextFieldState::new("Hello");
    // Cursor should be at end (position 5)
    assert_eq!(state.selection().start, 5);
    
    let element = TextFieldElement::new(state.clone());
    let mut node = element.create();
    *node.is_focused.borrow_mut() = true;
    
    let mut chain = ModifierNodeChain::default();
    chain.push(Box::new(node));
    
    let slices = collect_modifier_slices(&chain);
    
    // Execute the draw command to get the primitives
    let size = crate::modifier::Size { width: 200.0, height: 40.0 };
    let draw_commands = slices.draw_commands();
    assert!(!draw_commands.is_empty());
    
    // Get the first Overlay command and execute it
    let primitives = match &draw_commands[0] {
        crate::DrawCommand::Overlay(func) => func(size),
        _ => panic!("Expected Overlay draw command for cursor"),
    };
    
    assert!(!primitives.is_empty(), "Expected cursor primitive");
    
    // Get the cursor rect
    let cursor_rect = match &primitives[0] {
        compose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
        _ => panic!("Expected Rect primitive for cursor"),
    };
    
    // Expected cursor x = width of "Hello"
    let expected_x = crate::text::measure_text("Hello").width;
    
    assert!(
        (cursor_rect.x - expected_x).abs() < 0.1,
        "Cursor x position {} should match text width {}", 
        cursor_rect.x, expected_x
    );
}

/// Test that cursor position is 0 for empty text.
#[test]
fn cursor_at_start_for_empty_text() {
    let state = TextFieldState::new("");
    let element = TextFieldElement::new(state.clone());
    let mut node = element.create();
    *node.is_focused.borrow_mut() = true;
    
    let mut chain = ModifierNodeChain::default();
    chain.push(Box::new(node));
    
    let slices = collect_modifier_slices(&chain);
    let size = crate::modifier::Size { width: 200.0, height: 40.0 };
    
    let primitives = match &slices.draw_commands()[0] {
        crate::DrawCommand::Overlay(func) => func(size),
        _ => panic!("Expected Overlay"),
    };
    
    let cursor_rect = match &primitives[0] {
        compose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
        _ => panic!("Expected Rect"),
    };
    
    assert!(
        cursor_rect.x.abs() < 0.1,
        "Cursor x should be 0 for empty text, got {}", cursor_rect.x
    );
}

/// Test that selection draw command is created when text is selected.
#[test]
fn selection_draw_command_created_when_selected() {
    let state = TextFieldState::with_selection("Hello World", 0..5);
    let element = TextFieldElement::new(state.clone());
    let mut node = element.create();
    *node.is_focused.borrow_mut() = true;
    
    let mut chain = ModifierNodeChain::default();
    chain.push(Box::new(node));
    
    let slices = collect_modifier_slices(&chain);
    
    // First draw command should be Behind (selection), second Overlay (cursor)
    assert!(slices.draw_commands().len() >= 2, 
        "Expected at least 2 draw commands (selection + cursor)");
    
    let size = crate::modifier::Size { width: 200.0, height: 40.0 };
    
    // Get the Behind command (selection)
    let primitives = match &slices.draw_commands()[0] {
        crate::DrawCommand::Behind(func) => func(size),
        other => panic!("Expected Behind draw command for selection, got {:?}", other),
    };
    
    assert!(!primitives.is_empty(), "Expected selection primitive");
    
    // Get the selection rect
    let selection_rect = match &primitives[0] {
        compose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
        _ => panic!("Expected Rect primitive for selection"),
    };
    
    // Without any padding node in chain, Y should start at 0
    assert!(
        selection_rect.y.abs() < 0.1,
        "Selection y should be 0 without padding, got {}", selection_rect.y
    );
    
    // Selection width should match width of "Hello"
    let expected_width = crate::text::measure_text("Hello").width;
    assert!(
        (selection_rect.width - expected_width).abs() < 1.0,
        "Selection width {} should match text width {}", 
        selection_rect.width, expected_width
    );
}

/// Test that cursor Y position is at 0 without any padding.
#[test]
fn cursor_y_position_at_zero_without_padding() {
    let state = TextFieldState::new("Test");
    let element = TextFieldElement::new(state.clone());
    let mut node = element.create();
    *node.is_focused.borrow_mut() = true;
    
    let mut chain = ModifierNodeChain::default();
    chain.push(Box::new(node));
    
    let slices = collect_modifier_slices(&chain);
    let size = crate::modifier::Size { width: 200.0, height: 40.0 };
    
    // Get last command which should be cursor
    let cursor_cmd = slices.draw_commands().last().unwrap();
    let primitives = match cursor_cmd {
        crate::DrawCommand::Overlay(func) => func(size),
        _ => panic!("Expected Overlay for cursor"),
    };
    
    if primitives.is_empty() {
        // Cursor might be in blink-off phase, that's ok
        return;
    }
    
    let cursor_rect = match &primitives[0] {
        compose_ui_graphics::DrawPrimitive::Rect { rect, .. } => rect,
        _ => panic!("Expected Rect"),
    };
    
    // Without padding, Y should be at 0
    assert!(
        cursor_rect.y.abs() < 0.1,
        "Cursor y should be 0 without padding, got {}", cursor_rect.y
    );
}

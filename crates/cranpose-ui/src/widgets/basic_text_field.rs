//! BasicTextField widget for editable text input.
//!
//! This module provides the `BasicTextField` composable following Jetpack Compose's
//! `BasicTextField` pattern from `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicTextField.kt`.

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::EmptyMeasurePolicy;
use crate::modifier::Modifier;
use crate::text::{measure_text, AnnotatedString, TextStyle};
use crate::text_field_modifier_node::{
    TextFieldElement, TextFieldHandleController, TextFieldHandleMetrics,
};
use crate::text_selection::{selection_after_handle_drag, HandleKind, HANDLE_RADIUS};
use crate::widgets::{Layout, SelectionHandle};
use cranpose_core::{remember, NodeId};
use cranpose_foundation::modifier_element;
use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};
use cranpose_ui_graphics::{Color, Point};
use std::rc::Rc;

/// Fill color of the finger selection/cursor handles (Android accent blue).
const HANDLE_COLOR: Color = Color(0.26, 0.52, 0.96, 1.0);

/// Window-space position where a handle's tip should sit for the caret/selection
/// endpoint at byte `offset`: the bottom of that offset's visual line.
fn handle_tip_window_pos(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
) -> Point {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line_index = before.matches('\n').count();
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let caret_x = measure_text(&AnnotatedString::from(&before[line_start..]), style).width;
    Point {
        x: metrics.node_origin.x + metrics.padding_left + caret_x - metrics.scroll_offset,
        y: metrics.node_origin.y
            + metrics.padding_top
            + (line_index as f32 + 1.0) * metrics.line_height,
    }
}

/// Maps a window-space drag position (finger on the handle bulb) back to the
/// nearest text byte offset in the field.
fn window_pos_to_offset(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    window_pos: Point,
) -> usize {
    let local_x = (window_pos.x - metrics.node_origin.x - metrics.padding_left
        + metrics.scroll_offset)
        .max(0.0);
    // The bulb hangs one line below its tip, so bias the sampled y up by a line
    // to select the line the tip points at rather than the one below it.
    let local_y =
        (window_pos.y - metrics.node_origin.y - metrics.padding_top - metrics.line_height).max(0.0);
    crate::text::get_offset_for_position(&AnnotatedString::from(text), style, local_x, local_y)
}
///
/// # When to use
/// Use this when you need an editable text input but want full control over the
/// styling (no built-in borders or labels).
///
/// # Arguments
///
/// * `state` - The observable text field state that holds text content and cursor position.
/// * `modifier` - Modifiers for styling and layout.
/// * `style` - Text styling (color, font size).
///
/// # Example
///
/// ```rust,ignore
/// let text = remember_text_field_state("Initial text");
/// BasicTextField(text, Modifier::padding(8.0), TextStyle::default());
/// ```
#[composable]
pub fn BasicTextField(state: TextFieldState, modifier: Modifier, style: TextStyle) -> NodeId {
    BasicTextFieldWithOptions(
        state,
        modifier,
        BasicTextFieldOptions {
            text_style: style,
            ..BasicTextFieldOptions::default()
        },
    )
}

/// Options for customizing BasicTextField appearance and behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicTextFieldOptions {
    /// Text style
    pub text_style: TextStyle,
    /// Cursor color
    pub cursor_color: Color,
    /// Line limits: SingleLine or MultiLine with optional min/max
    pub line_limits: TextFieldLineLimits,
}

impl Default for BasicTextFieldOptions {
    fn default() -> Self {
        Self {
            text_style: TextStyle::default(),
            cursor_color: Color(0.0, 0.0, 0.0, 1.0), // Black
            line_limits: TextFieldLineLimits::default(),
        }
    }
}

/// Creates an editable text field with custom options.
///
/// This is the full version of `BasicTextField` with all configuration options.
#[composable]
pub fn BasicTextFieldWithOptions(
    state: TextFieldState,
    modifier: Modifier,
    options: BasicTextFieldOptions,
) -> NodeId {
    // Read text + selection to create composition dependencies: the field (and
    // its finger handles) recompose when either changes.
    let _text = state.text();
    let _selection = state.selection();

    // Shared channel through which the field node publishes live handle geometry
    // (focus, touch, on-screen origin, metrics). Remembered so it is stable
    // across recompositions.
    let controller =
        remember(TextFieldHandleController::new).with(TextFieldHandleController::clone);

    // Build the text field element with line limits + the handle controller.
    let text_field_element = TextFieldElement::new(state.clone(), options.text_style.clone())
        .with_cursor_color(options.cursor_color)
        .with_line_limits(options.line_limits)
        .with_handle_controller(controller.clone());

    // Wrap it in a modifier
    let text_field_modifier = modifier_element(text_field_element);
    let final_modifier = Modifier::from_parts(vec![text_field_modifier]);
    let combined_modifier = modifier.then(final_modifier);

    // Use EmptyMeasurePolicy - TextFieldModifierNode handles all measurement
    // This matches Jetpack Compose's BasicTextField architecture
    let node = Layout(
        combined_modifier,
        EmptyMeasurePolicy,
        || {}, // No children
    );

    // Finger selection handles (touch only): a caret handle for a collapsed
    // selection, start/end teardrops for a range. Rendered in the top-level
    // overlay via `Popup` so they escape the field's clip and hang below the
    // last line. A `PopupHost` at the app root (installed by the shell) is
    // required for them to appear.
    SelectionHandles(state, options.text_style, controller);

    node
}

/// Emits the finger selection/cursor handles for the field when it is focused
/// and was last touched (never for mouse input, which keeps a clean caret).
#[composable]
fn SelectionHandles(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
) {
    let Some(metrics) = controller.metrics() else {
        return;
    };
    if !metrics.focused || !metrics.touch {
        return;
    }

    let text = state.text();
    let selection = state.selection();

    if selection.collapsed() {
        // Collapsed caret: a single symmetric cursor handle.
        let tip = handle_tip_window_pos(&text, &style, &metrics, selection.start);
        let on_drag = drag_caret_closure(state.clone(), style.clone(), controller.clone());
        SelectionHandle(
            HandleKind::Cursor,
            tip,
            HANDLE_RADIUS,
            HANDLE_COLOR,
            move |pos| on_drag(pos),
            || {},
        );
    } else {
        // Range selection: start (leftmost) and end (rightmost) teardrops.
        let start = selection.min();
        let end = selection.max();
        let start_tip = handle_tip_window_pos(&text, &style, &metrics, start);
        let end_tip = handle_tip_window_pos(&text, &style, &metrics, end);

        let on_drag_start = drag_edge_closure(
            HandleKind::SelectionStart,
            state.clone(),
            style.clone(),
            controller.clone(),
        );
        SelectionHandle(
            HandleKind::SelectionStart,
            start_tip,
            HANDLE_RADIUS,
            HANDLE_COLOR,
            move |pos| on_drag_start(pos),
            || {},
        );

        let on_drag_end = drag_edge_closure(
            HandleKind::SelectionEnd,
            state.clone(),
            style.clone(),
            controller.clone(),
        );
        SelectionHandle(
            HandleKind::SelectionEnd,
            end_tip,
            HANDLE_RADIUS,
            HANDLE_COLOR,
            move |pos| on_drag_end(pos),
            || {},
        );
    }
}

/// Builds the drag handler for the collapsed cursor handle: moves the caret to
/// the dragged position.
fn drag_caret_closure(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let offset = window_pos_to_offset(&text, &style, &metrics, window_pos);
        state.set_selection(TextRange::new(offset, offset));
        crate::request_render_invalidation();
    })
}

/// Builds the drag handler for a selection start/end teardrop: extends the
/// selection to the dragged position while keeping the opposite edge fixed and
/// never letting the edges cross.
fn drag_edge_closure(
    dragged: HandleKind,
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let dragged_offset = window_pos_to_offset(&text, &style, &metrics, window_pos);
        let selection = state.selection();
        let fixed_edge = match dragged {
            HandleKind::SelectionStart => selection.max(),
            _ => selection.min(),
        };
        let (min, max) =
            selection_after_handle_drag(dragged, fixed_edge, dragged_offset, text.len());
        state.set_selection(TextRange::new(min, max));
        crate::request_render_invalidation();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_core::{location_key, Composition, DefaultScheduler, MemoryApplier, Runtime};
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    /// Composes just the finger handles for a collapsed caret with the given
    /// published metrics, and returns the rendered scene. The teardrop
    /// rasterizes to an image primitive, so counting images counts handles.
    fn render_collapsed_handles(touch: bool) -> crate::renderer::RecordedRenderScene {
        use crate::layout::LayoutEngine;
        use crate::renderer::HeadlessRenderer;
        use crate::widgets::PopupHost;
        use cranpose_ui_graphics::Size;

        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let state = TextFieldState::new("hello world");

        let mut content = {
            let state = state.clone();
            move || {
                let state = state.clone();
                PopupHost(move || {
                    let controller = TextFieldHandleController::new();
                    controller.publish(TextFieldHandleMetrics {
                        focused: true,
                        touch,
                        node_origin: Point { x: 0.0, y: 10.0 },
                        padding_left: 0.0,
                        padding_top: 0.0,
                        scroll_offset: 0.0,
                        line_height: 18.0,
                    });
                    SelectionHandles(state.clone(), TextStyle::default(), controller);
                });
            }
        };

        composition.render(key, &mut content).expect("render");
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition.reconcile(key, &mut content).expect("reconcile");
        }
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 400.0,
                    height: 400.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        HeadlessRenderer::new().render(&layout)
    }

    fn image_count(scene: &crate::renderer::RecordedRenderScene) -> usize {
        use crate::renderer::RenderOp;
        use cranpose_ui_graphics::DrawPrimitive;
        scene
            .operations()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    RenderOp::Primitive {
                        primitive: DrawPrimitive::Image { .. },
                        ..
                    }
                )
            })
            .count()
    }

    #[test]
    fn cursor_handle_shows_for_touch_only() {
        let _app_context = crate::render_state::app_context_test_scope();
        assert_eq!(
            image_count(&render_collapsed_handles(true)),
            1,
            "a touch caret should show one finger cursor handle in the overlay"
        );
        assert_eq!(
            image_count(&render_collapsed_handles(false)),
            0,
            "a mouse caret should keep a clean caret with no finger handle"
        );
    }

    #[test]
    fn basic_text_field_creates_node() {
        let _app_context = crate::render_state::app_context_test_scope();
        let mut composition = Composition::new(MemoryApplier::new());
        let state = TextFieldState::new("Test content");

        let result = composition.render(location_key(file!(), line!(), column!()), {
            let state = state.clone();
            move || {
                BasicTextField(state.clone(), Modifier::empty(), TextStyle::default());
            }
        });

        assert!(result.is_ok());
        assert!(composition.root().is_some());
    }

    #[test]
    fn basic_text_field_state_updates() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            assert_eq!(state.text(), "Hello");

            state.edit(|buffer| {
                buffer.place_cursor_at_end();
                buffer.insert("!");
            });

            assert_eq!(state.text(), "Hello!");
        });
    }
}

//! BasicTextField widget for editable text input.
//!
//! This module provides the `BasicTextField` composable following Jetpack Compose's
//! `BasicTextField` pattern from `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicTextField.kt`.

#![allow(non_snake_case)]

use crate::bring_into_view::local_bring_into_view_responder;
use crate::clipboard_session::{clipboard_read_text, clipboard_write_text};
use crate::composable;
use crate::layout::policies::EmptyMeasurePolicy;
use crate::modifier::Modifier;
use crate::safe_area::local_ime_insets;
use crate::text::{measure_text, AnnotatedString, TextStyle};
use crate::text_field_focus::{dispatch_copy, dispatch_cut, dispatch_paste, dispatch_select_all};
use crate::text_field_modifier_node::{
    TextFieldElement, TextFieldHandleController, TextFieldHandleMetrics,
};
use crate::text_selection::{selection_after_handle_drag, HandleKind, LineAffinity, HANDLE_RADIUS};
use crate::widgets::{
    loupe_target_for_drag, CaretActionMenu, Layout, SelectionHandle, SelectionLoupe,
    TextSelectionMenu,
};
use cranpose_core::{mutableStateOf, remember, MutableState, NodeId, SideEffect};
use cranpose_foundation::modifier_element;
use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};
use cranpose_ui_graphics::{Color, Point, Rect};
use std::cell::Cell;
use std::rc::Rc;

/// Alpha of the selection highlight relative to the field's accent
/// ([`TextFieldOptions::cursor_color`]): the reference highlight is the tint
/// at ~0.32 opacity, while the caret and both selection handles carry it
/// solid — one accent drives all three.
pub const SELECTION_HIGHLIGHT_ALPHA: f32 = 0.32;

/// Window-space position where a handle's tip should sit for the caret/selection
/// endpoint at byte `offset`: the bottom of that offset's visual line.
/// `affinity` decides the line at a shared soft-wrap boundary: selection ENDS,
/// the cursor handle and the loupe anchor upstream (the line the finger rides),
/// the selection START anchors downstream (the first highlighted glyph).
fn handle_tip_window_pos(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
    affinity: LineAffinity,
) -> Point {
    let offset = offset.min(text.len());
    // Resolve the caret's VISUAL (wrapped) line so the handle tip anchors on the
    // same glyph as the drawn caret (the field wraps long lines; counting only
    // logical `\n` lines would place the handle on the wrong line, far right).
    let (line_index, line_start) = crate::text_field_modifier_node::caret_visual_line_for_offset(
        text,
        style,
        None,
        metrics.wrap_width,
        offset,
        affinity,
    );
    let caret_x = measure_text(&AnnotatedString::from(&text[line_start..offset]), style).width;
    Point {
        x: metrics.node_origin.x + metrics.padding_left + caret_x - metrics.scroll_offset,
        // The tip rides the TIGHT glyph box bottom, not the slot bottom —
        // handles (and the caret) anchor on the glyphs like the reference.
        y: metrics.node_origin.y
            + metrics.padding_top
            + line_index as f32 * metrics.line_height
            + metrics.glyph_box.0
            + metrics.glyph_box.1,
    }
}

/// Maps a window-space drag position back to the nearest text byte offset in
/// the field. `y_bias` is the finger-to-line offset captured when the handle
/// was grabbed (`grab line bottom − finger y`): adding it back keeps the drag
/// targeting the line the finger means, whether the grab was on the line
/// itself (stem/edge) or on the dot hanging outside it — the reference drags
/// preserve the initial finger-to-line relationship.
fn window_pos_to_offset(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    window_pos: Point,
    y_bias: f32,
) -> usize {
    let local_x = (window_pos.x - metrics.node_origin.x - metrics.padding_left
        + metrics.scroll_offset)
        .max(0.0);
    // The biased y lands on the grabbed line's bottom; sample half a line up
    // to hit the line's middle.
    let local_y = (window_pos.y + y_bias
        - 0.5 * metrics.line_height
        - metrics.node_origin.y
        - metrics.padding_top)
        .max(0.0);
    // Resolve the VISUAL (wrapped) line the same way the drawn caret and
    // `handle_tip_window_pos` do. The plain measurer maps `y` through logical
    // `\n` lines only, so on wrapped text a handle drag lands on the wrong line
    // (an offset that grows with each wrapped line above the finger).
    crate::text::offset_for_position_wrapped(
        text,
        style,
        None,
        metrics.wrap_width,
        metrics.line_height,
        local_x,
        local_y,
    )
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
            // The field accent: caret + selection handles solid, selection
            // highlight at [`SELECTION_HIGHLIGHT_ALPHA`] (the reference blue).
            cursor_color: Color(0.0, 0.478, 1.0, 1.0),
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

    // Scroll the focused field's caret above the soft keyboard (bug 2): asks the
    // nearest scroll container's `BringIntoViewResponder` to reveal the caret on
    // focus / caret move / keyboard animation. No-op without a scrollable
    // ancestor or when the caret already fits.
    BringCaretIntoView(
        state.clone(),
        options.text_style.clone(),
        controller.clone(),
    );

    // Finger selection handles (touch only): a caret handle for a collapsed
    // selection, start/end lollipops for a range. Rendered in the top-level
    // overlay via `Popup` so they escape the field's clip and hang outside the
    // line. A `PopupHost` at the app root (installed by the shell) is
    // required for them to appear.
    SelectionHandles(state, options.text_style, controller, options.cursor_color);

    node
}

/// Window-space rect of the field's caret (the cursor line at byte `offset`),
/// derived from the field's published handle [`TextFieldHandleMetrics`]. Its top
/// is the top of the caret's visual line; its height is one line.
fn caret_window_rect(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
) -> Rect {
    // `handle_tip_window_pos` returns the tight glyph-box BOTTOM (the handle
    // tip); the caret rect spans that box.
    let tip = handle_tip_window_pos(text, style, metrics, offset, LineAffinity::Upstream);
    Rect {
        x: tip.x,
        y: tip.y - metrics.glyph_box.1,
        width: 2.0,
        height: metrics.glyph_box.1,
    }
}

/// Consumer half of bug 2: while the field is focused, asks the nearest scroll
/// container (via [`local_bring_into_view_responder`]) to scroll the caret clear
/// of the on-screen keyboard ([`local_ime_insets`]).
///
/// The request is triggered only by focus, caret movement, or a change in the
/// keyboard inset — never by scrolling — so the user is never yanked back while
/// deliberately scrolling the field out of view. The caret rect handed to the
/// responder is always recomputed from the live metrics, so the scroll delta is
/// correct even as the keyboard animates in.
#[composable]
fn BringCaretIntoView(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
) {
    let Some(metrics) = controller.metrics() else {
        return;
    };
    // Read the keyboard inset and responder unconditionally so the composable
    // re-runs when either changes even before the field is focused.
    let ime_bottom = local_ime_insets().current().bottom;
    let responder = local_bring_into_view_responder().current();

    let previous: Rc<Cell<Option<(usize, usize, i64)>>> =
        remember(|| Rc::new(Cell::new(None))).with(Rc::clone);

    if !metrics.focused {
        // Reset so the next focus re-requests even if the caret/keyboard match a
        // previous request.
        previous.set(None);
        return;
    }
    let Some(responder) = responder else {
        return;
    };

    let text = state.text();
    let selection = state.selection();
    let caret = caret_window_rect(&text, &style, &metrics, selection.start);

    // Trigger key: caret position + keyboard inset (quantised). Deliberately
    // excludes the field's scroll-driven window origin so scrolling does not
    // re-fire the request.
    let key = (
        selection.start,
        selection.end,
        (ime_bottom * 4.0).round() as i64,
    );
    SideEffect(move || {
        if previous.get() != Some(key) {
            previous.set(Some(key));
            responder.bring_into_view(caret, ime_bottom);
        }
    });
}

/// Emits the finger selection/cursor handles for the field when it is focused
/// and was last touched (never for mouse input, which keeps a clean caret).
/// `accent` is the field's tint (its cursor color): handles are drawn solid in
/// it, matching the caret and the highlight derived from it.
#[composable]
fn SelectionHandles(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
    accent: Color,
) {
    let selection = state.selection();
    let current_range = (selection.min(), selection.max());

    // Whether the contextual menu is open. Reopens whenever the selection range
    // changes (a fresh selection), so tapping an action dismisses it until the
    // next selection.
    let menu_open = remember(|| mutableStateOf(true)).with(|state| *state);
    // Whether the collapsed-caret action popup (Paste / Select all / Undo /
    // Redo) is open. Opened by tapping the cursor handle; closed by an action or
    // when the caret leaves the offset it was opened at (typing / tapping
    // elsewhere). Kept out of the range branch so hook order stays stable.
    let caret_menu_open = remember(|| mutableStateOf(false)).with(|state| *state);
    // The caret offset the popup was opened at, so it auto-dismisses once the
    // caret moves away.
    let caret_menu_offset: Rc<Cell<usize>> =
        remember(|| Rc::new(Cell::new(0usize))).with(Rc::clone);
    let previous_range: Rc<Cell<(usize, usize)>> =
        remember(|| Rc::new(Cell::new(current_range))).with(Rc::clone);
    {
        let previous_range = Rc::clone(&previous_range);
        SideEffect(move || {
            if previous_range.get() != current_range {
                previous_range.set(current_range);
                menu_open.set(true);
            }
        });
    }
    // Auto-dismiss the caret popup once the caret moves off the offset it was
    // opened at (the user typed or tapped elsewhere), matching Android.
    {
        let caret_menu_offset = Rc::clone(&caret_menu_offset);
        let caret_start = selection.start;
        SideEffect(move || {
            if caret_menu_open.value()
                && (!selection.collapsed() || caret_start != caret_menu_offset.get())
            {
                caret_menu_open.set(false);
            }
        });
    }

    let Some(metrics) = controller.metrics() else {
        return;
    };
    if !metrics.focused || !metrics.touch {
        return;
    }

    let text = state.text();

    // Window position of an in-progress handle drag (touch), or `None` when no
    // handle is being dragged. Drives the glass loupe (below) so it floats
    // above the finger while the caret/selection edge is being placed.
    let drag_pos: MutableState<Option<Point>> =
        remember(|| mutableStateOf(None::<Point>)).with(|state| *state);
    // The finger-to-line offset captured at the grab (grab line bottom −
    // finger y): drags keep targeting the line the finger means whether the
    // handle was grabbed on the line or by its dot outside it. One cell
    // serves all handles — only one drags at a time.
    let drag_bias: Rc<Cell<Option<f32>>> = remember(|| Rc::new(Cell::new(None))).with(Rc::clone);

    if selection.collapsed() {
        // Collapsed caret: a single cursor handle (a dot below the caret).
        let tip = handle_tip_window_pos(
            &text,
            &style,
            &metrics,
            selection.start,
            LineAffinity::Upstream,
        );
        let on_drag = drag_caret_closure(
            state.clone(),
            style.clone(),
            controller.clone(),
            Rc::clone(&drag_bias),
        );
        // Tapping the cursor handle opens the caret action popup, anchored at
        // the caret's current offset so it auto-dismisses when the caret moves.
        let open_caret_menu = {
            let caret_menu_offset = Rc::clone(&caret_menu_offset);
            let state = state.clone();
            move || {
                caret_menu_offset.set(state.selection().start);
                caret_menu_open.set(true);
            }
        };
        let on_tap = open_caret_menu.clone();
        let on_long_press = open_caret_menu;
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        let tip_y = tip.y;
        SelectionHandle(
            HandleKind::Cursor,
            tip,
            metrics.glyph_box.1,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                if grab_bias.get().is_none() {
                    grab_bias.set(Some(tip_y - pos.y));
                }
                drag_pos.set(Some(pos));
                on_drag(pos);
            },
            move || {
                drag_pos.set(None);
                end_bias.set(None);
            },
            on_long_press,
            on_tap,
        );

        // The caret action popup (Paste / Select all / Undo / Redo), floating
        // just above the caret. It dissolves the moment a handle drag starts
        // and rematerializes after release (the widget runs the measured
        // timings; it stays composed while fading).
        if caret_menu_open.value() {
            let can_paste = clipboard_read_text().is_some();
            let can_undo = state.can_undo();
            let can_redo = state.can_redo();
            let undo_state = state.clone();
            let redo_state = state.clone();
            CaretActionMenu(
                tip.x,
                tip.y - metrics.glyph_box.1,
                drag_pos.value().is_none(),
                can_paste,
                can_undo,
                can_redo,
                move || {
                    if let Some(text) = clipboard_read_text() {
                        dispatch_paste(&text);
                    }
                    caret_menu_open.set(false);
                },
                move || {
                    dispatch_select_all();
                    caret_menu_open.set(false);
                },
                move || {
                    undo_state.undo();
                    crate::request_render_invalidation();
                    caret_menu_open.set(false);
                },
                move || {
                    redo_state.redo();
                    crate::request_render_invalidation();
                    caret_menu_open.set(false);
                },
            );
        }
    } else {
        // Range selection: start (leftmost) and end (rightmost) lollipops.
        let start = selection.min();
        let end = selection.max();
        let start_tip =
            handle_tip_window_pos(&text, &style, &metrics, start, LineAffinity::Downstream);
        let end_tip = handle_tip_window_pos(&text, &style, &metrics, end, LineAffinity::Upstream);

        let on_drag_start = drag_edge_closure(
            HandleKind::SelectionStart,
            state.clone(),
            style.clone(),
            controller.clone(),
            Rc::clone(&drag_bias),
        );
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        let start_tip_y = start_tip.y;
        SelectionHandle(
            HandleKind::SelectionStart,
            start_tip,
            metrics.glyph_box.1,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                if grab_bias.get().is_none() {
                    grab_bias.set(Some(start_tip_y - pos.y));
                }
                drag_pos.set(Some(pos));
                on_drag_start(pos);
            },
            move || {
                drag_pos.set(None);
                end_bias.set(None);
            },
            // Long-pressing a handle re-opens the contextual menu even when the
            // selection range has not changed (e.g. after it was dismissed by a
            // previous action), so the text actions stay reachable.
            move || menu_open.set(true),
            // A tap on a selection-edge handle also re-opens the menu.
            move || menu_open.set(true),
        );

        let on_drag_end = drag_edge_closure(
            HandleKind::SelectionEnd,
            state.clone(),
            style.clone(),
            controller.clone(),
            Rc::clone(&drag_bias),
        );
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        let end_tip_y = end_tip.y;
        SelectionHandle(
            HandleKind::SelectionEnd,
            end_tip,
            metrics.glyph_box.1,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                if grab_bias.get().is_none() {
                    grab_bias.set(Some(end_tip_y - pos.y));
                }
                drag_pos.set(Some(pos));
                on_drag_end(pos);
            },
            move || {
                drag_pos.set(None);
                end_bias.set(None);
            },
            move || menu_open.set(true),
            move || menu_open.set(true),
        );

        // Contextual menu (Copy / Cut / Paste / Select all) floating above the
        // selection. Actions run against the focused field and dismiss the
        // menu. It dissolves the moment a handle drag starts and
        // rematerializes after release (the widget runs the measured timings;
        // it stays composed while fading).
        if menu_open.value() {
            let can_paste = clipboard_read_text().is_some();
            TextSelectionMenu(
                // Centered over the selection, above its first line.
                (start_tip.x + end_tip.x) * 0.5,
                start_tip.y - metrics.glyph_box.1,
                drag_pos.value().is_none(),
                can_paste,
                move || {
                    if let Some(text) = dispatch_copy() {
                        clipboard_write_text(&text);
                    }
                    menu_open.set(false);
                },
                move || {
                    if let Some(text) = dispatch_cut() {
                        clipboard_write_text(&text);
                    }
                    menu_open.set(false);
                },
                move || {
                    if let Some(text) = clipboard_read_text() {
                        dispatch_paste(&text);
                    }
                    menu_open.set(false);
                },
                move || {
                    dispatch_select_all();
                    menu_open.set(false);
                },
            );
        }
    }

    // The glass loupe: while a handle drag covers the text line, a liquid
    // glass bubble floats over the dragged line magnifying the live scene
    // under the finger (text, highlight, the handle itself — it is a backdrop
    // lens). Dragging by the dot below the line magnifies nothing. Emitted
    // unconditionally so the bubble stays mounted through its release
    // deflation.
    let loupe_target = drag_pos.value().and_then(|finger| {
        let bias = drag_bias.get().unwrap_or(0.0);
        let offset = window_pos_to_offset(&text, &style, &metrics, finger, bias);
        // Upstream: the loupe magnifies the line the FINGER rides — at a
        // shared wrap boundary that is the upper line the mapping sampled.
        let line_bottom =
            handle_tip_window_pos(&text, &style, &metrics, offset, LineAffinity::Upstream).y;
        loupe_target_for_drag(finger, line_bottom, metrics.glyph_box.1)
    });
    SelectionLoupe(loupe_target);
}

/// Builds the drag handler for the collapsed cursor handle: moves the caret to
/// the dragged position. `drag_bias` is the finger-to-line offset captured at
/// the grab (see [`window_pos_to_offset`]).
fn drag_caret_closure(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
    drag_bias: Rc<Cell<Option<f32>>>,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let bias = drag_bias.get().unwrap_or(0.0);
        let offset = window_pos_to_offset(&text, &style, &metrics, window_pos, bias);
        state.set_selection(TextRange::new(offset, offset));
        crate::request_render_invalidation();
    })
}

/// Builds the drag handler for a selection start/end handle: extends the
/// selection to the dragged position while keeping the opposite edge fixed and
/// never letting the edges cross. `drag_bias` as in [`drag_caret_closure`].
fn drag_edge_closure(
    dragged: HandleKind,
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
    drag_bias: Rc<Cell<Option<f32>>>,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let bias = drag_bias.get().unwrap_or(0.0);
        let dragged_offset = window_pos_to_offset(&text, &style, &metrics, window_pos, bias);
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
                        glyph_box: (0.0, 18.0),
                        wrap_width: None,
                    });
                    SelectionHandles(
                        state.clone(),
                        TextStyle::default(),
                        controller,
                        Color(0.0, 0.478, 1.0, 1.0),
                    );
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

    /// Composes the handles + contextual menu for a range selection with the
    /// given metrics, returning the rendered scene.
    fn render_range_menu(touch: bool) -> crate::renderer::RecordedRenderScene {
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
                    if state.selection() != TextRange::new(0, 5) {
                        state.set_selection(TextRange::new(0, 5));
                    }
                    controller.publish(TextFieldHandleMetrics {
                        focused: true,
                        touch,
                        node_origin: Point { x: 0.0, y: 40.0 },
                        padding_left: 0.0,
                        padding_top: 0.0,
                        scroll_offset: 0.0,
                        line_height: 18.0,
                        glyph_box: (0.0, 18.0),
                        wrap_width: None,
                    });
                    SelectionHandles(
                        state.clone(),
                        TextStyle::default(),
                        controller,
                        Color(0.0, 0.478, 1.0, 1.0),
                    );
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

    /// Like [`render_range_menu`], but the metrics + `SelectionHandles` are
    /// composed inside a `BoxWithConstraints` (which subcomposes its content off
    /// the measure pass), mirroring a real app where text fields live inside
    /// `BoxWithConstraints`/`LazyColumn`. The overlay `Popup`s must still reach
    /// the enclosing `PopupHost` across the subcomposition boundary.
    fn render_range_menu_subcomposed(touch: bool) -> crate::renderer::RecordedRenderScene {
        use crate::layout::LayoutEngine;
        use crate::renderer::HeadlessRenderer;
        use crate::widgets::{BoxWithConstraints, PopupHost};
        use cranpose_ui_graphics::Size;

        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let state = TextFieldState::new("hello world");

        let mut content = {
            let state = state.clone();
            move || {
                let state = state.clone();
                PopupHost(move || {
                    let state = state.clone();
                    BoxWithConstraints(
                        Modifier::empty().size(Size {
                            width: 300.0,
                            height: 300.0,
                        }),
                        move |_scope| {
                            let controller = TextFieldHandleController::new();
                            if state.selection() != TextRange::new(0, 5) {
                                state.set_selection(TextRange::new(0, 5));
                            }
                            controller.publish(TextFieldHandleMetrics {
                                focused: true,
                                touch,
                                node_origin: Point { x: 0.0, y: 40.0 },
                                padding_left: 0.0,
                                padding_top: 0.0,
                                scroll_offset: 0.0,
                                line_height: 18.0,
                                glyph_box: (0.0, 18.0),
                                wrap_width: None,
                            });
                            SelectionHandles(
                                state.clone(),
                                TextStyle::default(),
                                controller,
                                Color(0.0, 0.478, 1.0, 1.0),
                            );
                        },
                    );
                });
            }
        };

        composition.render(key, &mut content).expect("render");
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut scene = None;
        // The Popups register during the measure-pass subcomposition, so a
        // follow-up frame (reconcile + layout) is needed for the host to render
        // them. Alternate the two a few times, as real frames do.
        for _ in 0..8 {
            for _ in 0..16 {
                if !composition.should_render() {
                    break;
                }
                composition.reconcile(key, &mut content).expect("reconcile");
            }
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle.clone());
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
            scene = Some(HeadlessRenderer::new().render(&layout));
        }
        scene.expect("scene")
    }

    /// Like [`render_range_menu_subcomposed`], but the field lives inside a
    /// `LazyColumn` *item* — the exact shape of the reported device bug (a
    /// multi-line field in a lazy list). The item is subcomposed off the
    /// measure pass through `LazyColumn`'s own `SubcomposeLayoutNode`, so the
    /// overlay `Popup`s (handles + menu) only reach the enclosing `PopupHost`
    /// once the item subcomposition inherits the call-site composition locals.
    fn render_range_menu_lazy_column(touch: bool) -> crate::renderer::RecordedRenderScene {
        use crate::layout::LayoutEngine;
        use crate::renderer::HeadlessRenderer;
        use crate::widgets::PopupHost;
        use crate::{LazyColumn, LazyColumnSpec};
        use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
        use cranpose_ui_graphics::Size;

        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let state = TextFieldState::new("hello world");

        let mut content = {
            let state = state.clone();
            move || {
                let state = state.clone();
                PopupHost(move || {
                    let state = state.clone();
                    let list_state = remember_lazy_list_state();
                    LazyColumn(
                        Modifier::empty().size(Size {
                            width: 300.0,
                            height: 300.0,
                        }),
                        list_state,
                        LazyColumnSpec::default(),
                        move |scope| {
                            let state = state.clone();
                            scope.items(
                                1,
                                None::<fn(usize) -> u64>,
                                None::<fn(usize) -> u64>,
                                move |_index| {
                                    let controller = TextFieldHandleController::new();
                                    if state.selection() != TextRange::new(0, 5) {
                                        state.set_selection(TextRange::new(0, 5));
                                    }
                                    controller.publish(TextFieldHandleMetrics {
                                        focused: true,
                                        touch,
                                        node_origin: Point { x: 0.0, y: 40.0 },
                                        padding_left: 0.0,
                                        padding_top: 0.0,
                                        scroll_offset: 0.0,
                                        line_height: 18.0,
                                        glyph_box: (0.0, 18.0),
                                        wrap_width: None,
                                    });
                                    SelectionHandles(
                                        state.clone(),
                                        TextStyle::default(),
                                        controller,
                                        Color(0.0, 0.478, 1.0, 1.0),
                                    );
                                },
                            );
                        },
                    );
                });
            }
        };

        composition.render(key, &mut content).expect("render");
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut scene = None;
        for _ in 0..8 {
            for _ in 0..16 {
                if !composition.should_render() {
                    break;
                }
                composition.reconcile(key, &mut content).expect("reconcile");
            }
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle.clone());
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
            scene = Some(HeadlessRenderer::new().render(&layout));
        }
        scene.expect("scene")
    }

    /// Regression for the core device bug: a `BasicTextField` inside a
    /// vertically-scrolled container must publish its TRUE composited window
    /// origin so the finger selection/cursor handles anchor on the glyphs and
    /// follow the list as it scrolls. Before the fix the field origin was only
    /// ever sampled from the last pointer event, so it went stale on scroll —
    /// the handles rendered offset from the text, did not move while scrolling,
    /// and the window→offset inverse mapping (drag → text offset, and the
    /// handle-grab hit region) pointed at the wrong character.
    #[test]
    fn field_window_origin_follows_vertical_scroll() {
        use crate::layout::policies::EmptyMeasurePolicy;
        use crate::layout::{LayoutBox, LayoutEngine};
        use crate::renderer::HeadlessRenderer;
        use crate::scroll::ScrollState;
        use crate::widgets::{Column, ColumnSpec, Layout, PopupHost, Spacer};
        use cranpose_core::{remember, Key};
        use cranpose_foundation::modifier_element;
        use cranpose_ui_graphics::Size;
        use std::cell::RefCell;

        let _app_context = crate::render_state::app_context_test_scope();

        // `Composition::new` installs the runtime `TextFieldState`/`ScrollState`
        // need, so build it before allocating any state.
        let mut composition = Composition::new(MemoryApplier::new());
        let state = TextFieldState::new("hello world");
        let controller_slot: Rc<RefCell<Option<TextFieldHandleController>>> =
            Rc::new(RefCell::new(None));
        // `ScrollState` allocates a `MutableState`, so it must be created inside
        // the composition's runtime; publish it out through a slot to drive it.
        let scroll_slot: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));

        let spacer_before = 200.0_f32;
        let mut content = {
            let state = state.clone();
            let controller_slot = Rc::clone(&controller_slot);
            let scroll_slot = Rc::clone(&scroll_slot);
            move || {
                let state = state.clone();
                let controller_slot = Rc::clone(&controller_slot);
                let scroll_slot = Rc::clone(&scroll_slot);
                PopupHost(move || {
                    let controller = remember(TextFieldHandleController::new)
                        .with(TextFieldHandleController::clone);
                    *controller_slot.borrow_mut() = Some(controller.clone());
                    let scroll = remember(|| ScrollState::new(0.0)).with(ScrollState::clone);
                    *scroll_slot.borrow_mut() = Some(scroll.clone());
                    let state = state.clone();
                    let controller = controller.clone();
                    Column(
                        Modifier::empty()
                            .size(Size {
                                width: 300.0,
                                height: 150.0,
                            })
                            .vertical_scroll(scroll.clone(), false),
                        ColumnSpec::default(),
                        move || {
                            Spacer(Size {
                                width: 300.0,
                                height: spacer_before,
                            });
                            let element =
                                TextFieldElement::new(state.clone(), TextStyle::default())
                                    .with_handle_controller(controller.clone());
                            let field_modifier =
                                Modifier::from_parts(vec![modifier_element(element)]);
                            Layout(field_modifier, EmptyMeasurePolicy, || {});
                            Spacer(Size {
                                width: 300.0,
                                height: 400.0,
                            });
                        },
                    );
                });
            }
        };

        // The field node is the only one carrying a window-origin sink.
        fn find_field_rect(node: &LayoutBox) -> Option<cranpose_ui_graphics::Rect> {
            if node
                .node_data
                .modifier_slices()
                .text_field_window_origin()
                .is_some()
            {
                return Some(node.rect);
            }
            node.children.iter().find_map(find_field_rect)
        }

        fn layout_and_read(
            composition: &mut Composition<MemoryApplier>,
            key: Key,
            content: &mut dyn FnMut(),
            controller_slot: &Rc<RefCell<Option<TextFieldHandleController>>>,
        ) -> (Point, f32) {
            for _ in 0..16 {
                if !composition.should_render() {
                    break;
                }
                composition
                    .reconcile(key, &mut *content)
                    .expect("reconcile");
            }
            let root = composition.root().expect("root");
            let handle = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(handle);
            let layout = applier
                .compute_layout(
                    root,
                    cranpose_ui_graphics::Size {
                        width: 400.0,
                        height: 600.0,
                    },
                )
                .expect("layout");
            applier.clear_runtime_handle();
            drop(applier);
            let _ = HeadlessRenderer::new().render(&layout);
            let field_y = find_field_rect(layout.root()).expect("field placed").y;
            let node_origin = controller_slot
                .borrow()
                .as_ref()
                .expect("controller")
                .metrics()
                .expect("metrics published")
                .node_origin;
            (node_origin, field_y)
        }

        let key = location_key(file!(), line!(), column!());
        composition.render(key, &mut content).expect("render");

        let (origin0, field_y0) =
            layout_and_read(&mut composition, key, &mut content, &controller_slot);
        // The published origin must equal the field's real placed window rect
        // (not a stale pointer sample), and start below the leading spacer.
        assert!(
            (origin0.y - field_y0).abs() < 0.5,
            "published node_origin.y {} must equal the field's placed window-y {}",
            origin0.y,
            field_y0
        );
        assert!(
            origin0.y >= spacer_before - 0.5,
            "field should start at/after the {spacer_before}px leading spacer, got {}",
            origin0.y
        );

        // Scroll the list down by 50px; the field must move up by exactly 50px
        // and the published origin must track it live.
        let scroll = scroll_slot.borrow().as_ref().expect("scroll state").clone();
        scroll.scroll_to(50.0);
        assert!(
            scroll.value() >= 49.5,
            "test setup: content must be tall enough to scroll 50px (got {})",
            scroll.value()
        );
        let (origin1, field_y1) =
            layout_and_read(&mut composition, key, &mut content, &controller_slot);
        assert!(
            (origin1.y - field_y1).abs() < 0.5,
            "after scroll, node_origin.y {} must still equal the field's placed window-y {}",
            origin1.y,
            field_y1
        );
        assert!(
            (origin1.y - (origin0.y - 50.0)).abs() < 0.5,
            "scrolling 50px must shift the published field origin up by 50px: \
             before {}, after {} (expected {})",
            origin0.y,
            origin1.y,
            origin0.y - 50.0
        );
    }

    /// After a scroll, the window→offset inverse mapping must still resolve the
    /// character the finger is over, because it reads the same live composited
    /// origin the handles are placed at. Uses the published post-scroll metrics
    /// to round-trip every caret offset through
    /// `handle_tip_window_pos` → `window_pos_to_offset`.
    #[test]
    fn window_offset_roundtrip_holds_under_scroll_offset() {
        let _app_context = crate::render_state::app_context_test_scope();
        let text = "hello world";
        let style = TextStyle::default();
        // Two different composited origins (as if the list scrolled between them).
        for node_origin in [Point { x: 12.0, y: 240.0 }, Point { x: 12.0, y: 190.0 }] {
            let metrics = TextFieldHandleMetrics {
                focused: true,
                touch: true,
                node_origin,
                padding_left: 4.0,
                padding_top: 3.0,
                scroll_offset: 0.0,
                line_height: 18.0,
                glyph_box: (0.0, 18.0),
                wrap_width: None,
            };
            for offset in 0..=text.len() {
                if !text.is_char_boundary(offset) {
                    continue;
                }
                // A grab exactly at the tip (the line bottom) captures a zero
                // bias; the mapping still resolves the grabbed line's offset.
                let tip =
                    handle_tip_window_pos(text, &style, &metrics, offset, LineAffinity::Downstream);
                let resolved = window_pos_to_offset(text, &style, &metrics, tip, 0.0);
                assert_eq!(
                    resolved, offset,
                    "finger at the tip of offset {offset} must map back to it \
                     under origin {node_origin:?}, got {resolved}"
                );
            }
        }
    }

    /// A long unbroken word wraps MID-WORD, so consecutive visual lines share
    /// their boundary byte. A selection END dragged along the upper line's
    /// right edge produces exactly that byte — its handle (and the loupe line)
    /// must anchor to the UPPER line's end, never one line down at the left
    /// edge (the wrapped-multiline handle Y-offset bug reported on device).
    /// The START handle at the same byte anchors downstream where the first
    /// highlighted glyph renders.
    #[test]
    fn shared_wrap_boundary_anchors_by_handle_affinity() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let text = "aaaaaaaaaaaaaaaaaaaaaaaa";
            let style = TextStyle::default();
            let annotated = crate::text::AnnotatedString::from(text);
            let full = crate::text::measure_text(&annotated, &style);
            // Tight enough to split the word across ≥2 visual lines.
            let wrap_width = full.width / 3.0;
            let ranges = crate::text::wrapped_line_ranges(
                None,
                &annotated,
                &style,
                crate::text::TextLayoutOptions::default(),
                Some(wrap_width),
            );
            assert!(
                ranges.len() >= 2,
                "test setup: text must wrap, got {ranges:?}"
            );
            let boundary = ranges[1].start;
            assert_eq!(
                ranges[0].end, boundary,
                "test setup: a mid-word wrap must share its boundary byte, got {ranges:?}"
            );

            let line_height = 20.0;
            let metrics = TextFieldHandleMetrics {
                focused: true,
                touch: true,
                node_origin: Point { x: 0.0, y: 0.0 },
                padding_left: 0.0,
                padding_top: 0.0,
                scroll_offset: 0.0,
                line_height,
                glyph_box: (0.0, line_height),
                wrap_width: Some(wrap_width),
            };

            let end_tip =
                handle_tip_window_pos(text, &style, &metrics, boundary, LineAffinity::Upstream);
            assert!(
                (end_tip.y - line_height).abs() < 0.5,
                "end handle must sit on the UPPER line's bottom ({line_height}), got y={}",
                end_tip.y
            );
            assert!(
                end_tip.x > 1.0,
                "end handle must sit at the upper line's right edge, got x={}",
                end_tip.x
            );

            let start_tip =
                handle_tip_window_pos(text, &style, &metrics, boundary, LineAffinity::Downstream);
            assert!(
                (start_tip.y - 2.0 * line_height).abs() < 0.5,
                "start handle must sit on the LOWER line's bottom ({}), got y={}",
                2.0 * line_height,
                start_tip.y
            );
            assert!(
                start_tip.x.abs() < 0.5,
                "start handle must sit at the lower line's left edge, got x={}",
                start_tip.x
            );
        })
    }

    fn text_values(scene: &crate::renderer::RecordedRenderScene) -> Vec<String> {
        use crate::renderer::RenderOp;
        scene
            .operations()
            .iter()
            .filter_map(|op| match op {
                RenderOp::Text { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect()
    }

    /// Composes the caret action popup directly inside a `PopupHost` (as the
    /// cursor-handle tap does once `caret_menu_open` is set) and returns the
    /// rendered scene, so the item labels can be asserted.
    fn render_caret_action_menu(
        can_paste: bool,
        can_undo: bool,
        can_redo: bool,
    ) -> crate::renderer::RecordedRenderScene {
        use crate::layout::LayoutEngine;
        use crate::renderer::HeadlessRenderer;
        use crate::widgets::PopupHost;
        use cranpose_ui_graphics::Size;

        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        let mut content = move || {
            PopupHost(move || {
                CaretActionMenu(
                    40.0,
                    60.0,
                    true,
                    can_paste,
                    can_undo,
                    can_redo,
                    || {},
                    || {},
                    || {},
                    || {},
                );
            });
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

    /// Bug (b): the caret action popup offers Paste / Select all / Undo / Redo.
    /// Paste is hidden when the clipboard is empty, and Undo/Redo when the
    /// field's history has nothing to undo/redo.
    #[test]
    fn caret_action_menu_shows_paste_select_all_undo_redo() {
        let _app_context = crate::render_state::app_context_test_scope();

        let all = text_values(&render_caret_action_menu(true, true, true));
        for label in ["Paste", "Select all", "Undo", "Redo"] {
            assert!(
                all.iter().any(|t| t == label),
                "caret menu should show {label:?}, got {all:?}"
            );
        }

        // Nothing on the clipboard and an empty history: only Select all.
        let bare = text_values(&render_caret_action_menu(false, false, false));
        assert!(
            bare.iter().any(|t| t == "Select all"),
            "Select all is always available, got {bare:?}"
        );
        assert!(
            !bare
                .iter()
                .any(|t| t == "Paste" || t == "Undo" || t == "Redo"),
            "Paste/Undo/Redo must be hidden when unavailable, got {bare:?}"
        );
    }

    #[test]
    fn context_menu_shows_for_touch_selection_only() {
        let _app_context = crate::render_state::app_context_test_scope();

        let touch = text_values(&render_range_menu(true));
        assert!(
            touch.iter().any(|t| t == "Copy"),
            "touch selection should show the Copy menu item, got {touch:?}"
        );
        assert!(
            touch.iter().any(|t| t == "Cut"),
            "expected Cut, got {touch:?}"
        );
        assert!(
            touch.iter().any(|t| t == "Select all"),
            "expected Select all, got {touch:?}"
        );

        let mouse = text_values(&render_range_menu(false));
        assert!(
            !mouse.iter().any(|t| t == "Copy"),
            "mouse selection must not show the finger contextual menu, got {mouse:?}"
        );
    }

    #[test]
    fn selection_handles_and_menu_survive_subcomposition() {
        // Regression: the selection handles and the contextual menu (all drawn
        // through `Popup`) must reach the enclosing `PopupHost` even when the
        // text field lives inside a `BoxWithConstraints`/`LazyColumn`, which
        // subcomposes its content off the measure pass. Both the two teardrop
        // handles and the menu items are expected.
        let _app_context = crate::render_state::app_context_test_scope();
        let scene = render_range_menu_subcomposed(true);

        let texts = text_values(&scene);
        assert!(
            texts.iter().any(|t| t == "Copy"),
            "a touch selection inside a subcomposition should show the Copy menu \
             item through the host, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Select all"),
            "expected Select all inside a subcomposition, got {texts:?}"
        );
        assert_eq!(
            image_count(&scene),
            2,
            "a touch range selection should show two finger teardrop handles in \
             the overlay across the subcomposition boundary"
        );
    }

    #[test]
    fn selection_handles_and_menu_survive_lazy_column_item() {
        // Regression for the reported device bug: a text field inside a
        // `LazyColumn` item shows neither its selection handles nor its context
        // menu, because the item is subcomposed off the list's measure pass and
        // the overlay `Popup`s lose the enclosing `PopupHost` registry across
        // that boundary. After capturing the call-site locals in `LazyColumn`
        // the handles + menu reach the host, just like a top-level field.
        let _app_context = crate::render_state::app_context_test_scope();
        let scene = render_range_menu_lazy_column(true);

        let texts = text_values(&scene);
        assert!(
            texts.iter().any(|t| t == "Copy"),
            "a touch selection inside a LazyColumn item should show the Copy menu \
             item through the host, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Select all"),
            "expected Select all inside a LazyColumn item, got {texts:?}"
        );
        assert_eq!(
            image_count(&scene),
            2,
            "a touch range selection should show two finger teardrop handles in \
             the overlay across the LazyColumn item subcomposition boundary"
        );
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

    /// Bug 2 end-to-end (headless): a `LazyColumn` provides a
    /// `BringIntoViewResponder`; its viewport rect is filled by the layout pass;
    /// asking it to reveal a caret hidden behind the keyboard scrolls the list
    /// FORWARD (revealing lower content), while asking it to reveal an
    /// already-visible caret does nothing. This pins the whole responder path:
    /// provision through the item subcomposition, the `report_window_rect`
    /// viewport sink, `scroll_delta_to_reveal`, and the `dispatch_scroll_delta`
    /// sign.
    #[test]
    fn lazy_column_responder_scrolls_a_hidden_caret_into_view() {
        use crate::bring_into_view::local_bring_into_view_responder;
        use crate::layout::LayoutEngine;
        use crate::renderer::HeadlessRenderer;
        use crate::widgets::{Box, BoxSpec, PopupHost};
        use crate::{LazyColumn, LazyColumnSpec};
        use cranpose_core::Key;
        use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope, LazyListState};
        use cranpose_ui_graphics::Size;
        use std::cell::RefCell;

        let _app_context = crate::render_state::app_context_test_scope();
        let mut composition = Composition::new(MemoryApplier::new());
        let responder_slot: Rc<RefCell<Option<crate::bring_into_view::BringIntoViewResponder>>> =
            Rc::new(RefCell::new(None));
        let state_slot: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));

        // Viewport 300x400 at window origin (0,0); 30 items of 80px each = 2400px
        // of content, so the list can scroll far forward.
        let mut content = {
            let responder_slot = Rc::clone(&responder_slot);
            let state_slot = Rc::clone(&state_slot);
            move || {
                let responder_slot = Rc::clone(&responder_slot);
                let state_slot = Rc::clone(&state_slot);
                PopupHost(move || {
                    let list_state = remember_lazy_list_state();
                    *state_slot.borrow_mut() = Some(list_state);
                    let responder_slot = Rc::clone(&responder_slot);
                    LazyColumn(
                        Modifier::empty().size(Size {
                            width: 300.0,
                            height: 400.0,
                        }),
                        list_state,
                        LazyColumnSpec::default(),
                        move |scope| {
                            let responder_slot = Rc::clone(&responder_slot);
                            scope.items(
                                30,
                                None::<fn(usize) -> u64>,
                                None::<fn(usize) -> u64>,
                                move |_index| {
                                    if responder_slot.borrow().is_none() {
                                        if let Some(r) = local_bring_into_view_responder().current()
                                        {
                                            *responder_slot.borrow_mut() = Some(r);
                                        }
                                    }
                                    Box(
                                        Modifier::empty().size(Size {
                                            width: 300.0,
                                            height: 80.0,
                                        }),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                },
                            );
                        },
                    );
                });
            }
        };

        fn run_layout(
            composition: &mut Composition<MemoryApplier>,
            key: Key,
            content: &mut dyn FnMut(),
        ) {
            for _ in 0..16 {
                if !composition.should_render() {
                    break;
                }
                composition
                    .reconcile(key, &mut *content)
                    .expect("reconcile");
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
                        height: 600.0,
                    },
                )
                .expect("layout");
            applier.clear_runtime_handle();
            drop(applier);
            let _ = HeadlessRenderer::new().render(&layout);
        }

        let key = location_key(file!(), line!(), column!());
        composition.render(key, &mut content).expect("render");
        run_layout(&mut composition, key, &mut content);

        let responder = responder_slot
            .borrow()
            .clone()
            .expect("LazyColumn provides a bring-into-view responder to its items");
        let list_state = state_slot.borrow().expect("list state captured");
        let offset0 = list_state.first_visible_item_scroll_offset();
        let index0 = list_state.first_visible_item_index();

        // A caret already inside the viewport (y=100, above the fold) must not
        // scroll the list.
        responder.bring_into_view(
            Rect {
                x: 10.0,
                y: 100.0,
                width: 2.0,
                height: 20.0,
            },
            0.0,
        );
        run_layout(&mut composition, key, &mut content);
        assert_eq!(
            list_state.first_visible_item_index(),
            index0,
            "an already-visible caret must not scroll the list"
        );
        assert!(
            (list_state.first_visible_item_scroll_offset() - offset0).abs() < 0.5,
            "an already-visible caret must not scroll the list"
        );

        // A caret hidden behind a keyboard covering the bottom 250px (usable
        // region 0..150) sitting at y=360 must scroll the list forward.
        responder.bring_into_view(
            Rect {
                x: 10.0,
                y: 360.0,
                width: 2.0,
                height: 20.0,
            },
            250.0,
        );
        run_layout(&mut composition, key, &mut content);
        let scrolled_forward = list_state.first_visible_item_index() > index0
            || list_state.first_visible_item_scroll_offset() > offset0 + 0.5;
        assert!(
            scrolled_forward,
            "a caret behind the keyboard must scroll the list forward \
             (index {} -> {}, offset {:.1} -> {:.1})",
            index0,
            list_state.first_visible_item_index(),
            offset0,
            list_state.first_visible_item_scroll_offset(),
        );
    }
}

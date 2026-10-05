//! BasicTextField widget for editable text input.
//!
//! This module provides the `BasicTextField` composable following Jetpack Compose's
//! `BasicTextField` pattern from `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicTextField.kt`.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use cranpose_core::{MutableState, NodeId, SideEffect, mutableStateOf, remember};
use cranpose_foundation::{
    modifier_element,
    text::{TextFieldLineLimits, TextFieldState, TextRange},
};
use cranpose_ui_graphics::{Color, Point, Rect};

use crate::{
    bring_into_view::local_bring_into_view_responder,
    clipboard_session::{clipboard_can_paste, clipboard_paste_into_focus, clipboard_write_text},
    composable,
    layout::{
        core::Alignment,
        policies::{BoxMeasurePolicy, EmptyMeasurePolicy},
    },
    modifier::Modifier,
    safe_area::local_ime_insets,
    text::{AnnotatedString, TextStyle, measure_text},
    text_field_decorator_node::TextFieldDecoratorElement,
    text_field_focus::{dispatch_copy, dispatch_cut, dispatch_select_all},
    text_field_modifier_node::{
        TextFieldElement, TextFieldHandleController, TextFieldHandleMetrics, TextFieldRefs,
    },
    text_selection::{
        HANDLE_RADIUS, HandleGrabOffset, HandleKind, LineAffinity, selection_after_handle_drag,
    },
    widgets::{
        CaretActionMenu, Layout, MenuAnchor, SelectionHandle, SelectionLoupe, TextSelectionMenu,
        loupe_target_for_drag,
    },
};

/// Alpha of the selection highlight relative to the field's accent
/// (`TextFieldOptions::cursor_color`): the reference highlight is the tint
/// at ~0.32 opacity, while the caret and both selection handles carry it
/// solid — one accent drives all three.
pub const SELECTION_HIGHLIGHT_ALPHA: f32 = 0.32;

/// Hold duration before a stationary touch press on the text claims the
/// gesture (word-select + menu while the finger is still down).
const TEXT_LONG_PRESS_MS: u64 = 500;
/// Travel beyond this (dp) before the hold elapses is a drag, not a
/// long-press.
const TEXT_LONG_PRESS_SLOP: f32 = 12.0;

/// Frame-clock watcher for the long-press → slide-to-menu gesture: armed by
/// the composition when the field node publishes a fresh touch press, it
/// claims the gesture after the hold threshold (selecting the word under
/// the press; the range-change side effect opens the menu). The composition
/// slot holds the only strong reference — dropping it (press ended, field
/// recomposed away) cancels the pending frame callback.
struct LongPressWatcher {
    controller: TextFieldHandleController,
    state: TextFieldState,
    style: TextStyle,
    start: Point,
    start_nanos: Cell<Option<u64>>,
    registration: RefCell<Option<cranpose_core::internal::FrameCallbackRegistration>>,
    frame_clock: cranpose_core::internal::FrameClock,
}

impl LongPressWatcher {
    fn arm(self: &Rc<Self>) {
        let weak: Weak<LongPressWatcher> = Rc::downgrade(self);
        let registration = self.frame_clock.with_frame_nanos(move |now| {
            let Some(watcher) = weak.upgrade() else {
                return;
            };
            watcher.tick(now);
        });
        *self.registration.borrow_mut() = Some(registration);
        crate::request_render_invalidation();
    }

    fn tick(self: Rc<Self>, now: u64) {
        self.registration.borrow_mut().take();
        let Some(press) = self.controller.press() else {
            return;
        };
        let moved = (press.position.x - self.start.x)
            .abs()
            .max((press.position.y - self.start.y).abs());
        if (press.start.x - self.start.x).abs() > 0.5
            || (press.start.y - self.start.y).abs() > 0.5
            || moved > TEXT_LONG_PRESS_SLOP
        {
            return;
        }
        let start = match self.start_nanos.get() {
            Some(value) => value,
            None => {
                self.start_nanos.set(Some(now));
                now
            }
        };
        if now.saturating_sub(start) < TEXT_LONG_PRESS_MS * 1_000_000 {
            self.arm();
            return;
        }
        self.controller.claim_gesture();
        let Some(metrics) = self.controller.metrics() else {
            return;
        };
        let text = self.state.text();
        let offset = window_pos_to_offset(&text, &self.style, &metrics, self.start, 0.0);
        let (word_start, word_end) = crate::word_boundaries::find_word_boundaries(&text, offset);
        self.state.edit(|buffer| {
            buffer.select(TextRange::new(word_start, word_end));
        });
        crate::request_render_invalidation();
    }
}

/// Local position where a handle's tip should sit for the caret/selection
/// endpoint at byte `offset`: the bottom of that offset's visual line.
/// `affinity` decides the line at a shared soft-wrap boundary: selection ENDS,
/// the cursor handle and the loupe anchor upstream (the line the finger rides),
/// the selection START anchors downstream (the first highlighted glyph).
fn handle_tip_local_pos(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
    affinity: LineAffinity,
) -> Point {
    let offset = offset.min(text.len());
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
        x: metrics.padding_left + caret_x - metrics.scroll_offset,
        y: metrics.padding_top
            + line_index as f32 * metrics.line_height
            + metrics.glyph_box.0
            + metrics.glyph_box.1,
    }
}

fn caret_window_geometry(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
    affinity: LineAffinity,
) -> (Point, Rect) {
    let tip = handle_tip_local_pos(text, style, metrics, offset, affinity);
    let bounds = metrics.local_to_window.bounds_for_rect(Rect {
        x: tip.x,
        y: tip.y - metrics.glyph_box.1,
        width: 2.0,
        height: metrics.glyph_box.1,
    });
    (metrics.local_to_window.map_point(tip), bounds)
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
    let Some(inverse) = metrics.local_to_window.inverse() else {
        return 0;
    };
    let local = inverse.map_point(Point {
        y: window_pos.y + y_bias,
        ..window_pos
    });
    let local_x = (local.x - metrics.padding_left + metrics.scroll_offset).max(0.0);
    let local_y = (local.y - 0.5 * metrics.line_height - metrics.padding_top).max(0.0);
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
/// Edits text through a retained [`TextFieldState`]. `modifier` controls the
/// field bounds and input, and `style` controls the text. Use
/// [`BasicTextFieldWithOptions`] for line limits, cursor color and edit callbacks.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn NameField() {
///     let text =
///         cranpose_core::remember(|| TextFieldState::new("Initial text")).with(|state| *state);
///     BasicTextField(text, Modifier::empty().padding(8.0), TextStyle::default());
/// }
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
    /// Whether gaining focus opens the software keyboard. Defaults to `true`.
    ///
    /// When `false`, programmatic and keyboard-navigation focus leave the
    /// software keyboard hidden. A pointer tap still explicitly opens it.
    /// Changing this to `true` while focused also requests the keyboard.
    /// Changing it to `false` preserves an already visible keyboard.
    pub show_keyboard_on_focus: bool,
}

impl Default for BasicTextFieldOptions {
    fn default() -> Self {
        Self {
            text_style: TextStyle::default(),
            cursor_color: Color(0.0, 0.478, 1.0, 1.0),
            line_limits: TextFieldLineLimits::default(),
            show_keyboard_on_focus: true,
        }
    }
}

/// Scope passed to a basic text field decoration box.
///
/// The decoration may place arbitrary composables around the editable content,
/// but must call [`inner_text_field`](Self::inner_text_field) exactly once.
#[derive(Clone)]
pub struct BasicTextFieldDecorationScope {
    inner: Rc<dyn Fn() -> NodeId>,
}

impl BasicTextFieldDecorationScope {
    pub fn inner_text_field(&self) -> NodeId {
        (self.inner)()
    }
}

impl PartialEq for BasicTextFieldDecorationScope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Creates an editable field and lets `decoration_box` place composable labels,
/// placeholders, icons, buttons, prefixes, or suffixes around it.
///
/// As in Compose, the decoration box is the field: `modifier` applies to it,
/// and it takes the field's pointer input, focus and semantics, so a press
/// anywhere in it edits the text. The inner field only lays out and draws
/// the text, as wide as the text unless the decoration stretches it (a
/// `Box` that propagates its minimum constraints, for one).
#[composable(no_skip)]
pub fn BasicTextFieldDecorated<D>(
    state: TextFieldState,
    modifier: Modifier,
    options: BasicTextFieldOptions,
    decoration_box: D,
) -> NodeId
where
    D: Fn(BasicTextFieldDecorationScope) + 'static,
{
    let refs = remember(TextFieldRefs::new).with(TextFieldRefs::clone);
    let decorator = TextFieldDecoratorElement::new(
        state,
        refs.clone(),
        options.text_style.clone(),
        options.line_limits,
        crate::modal::local_modal_depth().current(),
    );
    let scope = BasicTextFieldDecorationScope {
        inner: Rc::new(move || {
            TextFieldNode(
                state,
                Modifier::empty(),
                options.clone(),
                Some(refs.clone()),
            )
        }),
    };
    Layout(
        modifier.then(Modifier::from_parts(&[modifier_element(decorator)])),
        BoxMeasurePolicy::new(Alignment::TOP_START, true),
        move || decoration_box(scope.clone()),
    )
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
    TextFieldNode(state, modifier, options, None)
}

/// The field's node, and the handles and caret tracking around it. With
/// `decorator`, the decoration box sharing those refs takes the field's
/// input, focus and semantics.
#[composable]
fn TextFieldNode(
    state: TextFieldState,
    modifier: Modifier,
    options: BasicTextFieldOptions,
    decorator: Option<TextFieldRefs>,
) -> NodeId {
    #[cfg(feature = "localization")]
    let options = BasicTextFieldOptions {
        text_style: crate::localization::apply_text_locale(options.text_style),
        ..options
    };
    let _text = state.text();
    let _selection = state.selection();

    let controller =
        remember(TextFieldHandleController::new).with(TextFieldHandleController::clone);

    let modal_depth = crate::modal::local_modal_depth().current();
    let mut text_field_element = TextFieldElement::new(state, options.text_style.clone())
        .with_cursor_color(options.cursor_color)
        .with_line_limits(options.line_limits)
        .with_show_keyboard_on_focus(options.show_keyboard_on_focus)
        .with_handle_controller(controller.clone())
        .with_modal_depth(modal_depth);
    if let Some(refs) = decorator {
        text_field_element = text_field_element.decorated_by(refs);
    }

    let text_field_modifier = modifier_element(text_field_element);
    let final_modifier = Modifier::from_parts(&[text_field_modifier]);
    let combined_modifier = modifier.then(final_modifier);

    let node = Layout(combined_modifier, EmptyMeasurePolicy, || {});

    BringCaretIntoView(state, options.text_style.clone(), controller.clone());

    SelectionHandles(state, options.text_style, controller, options.cursor_color);

    node
}

#[cfg(test)]
#[path = "tests/basic_text_field_options_tests.rs"]
mod options_tests;

/// Window-space rect of the field's caret (the cursor line at byte `offset`),
/// derived from the field's published handle [`TextFieldHandleMetrics`]. Its top
/// is the top of the caret's visual line; its height is one line.
fn caret_window_rect(
    text: &str,
    style: &TextStyle,
    metrics: &TextFieldHandleMetrics,
    offset: usize,
) -> Rect {
    caret_window_geometry(text, style, metrics, offset, LineAffinity::Upstream).1
}

#[composable]
fn BringCaretIntoView(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
) {
    let Some(metrics) = controller.metrics() else {
        return;
    };
    let ime_bottom = local_ime_insets().current().bottom;
    let responder = local_bring_into_view_responder().current();

    if !metrics.focused {
        return;
    }
    let Some(responder) = responder else {
        return;
    };
    let window_height = super::popup::local_popup_viewport().current().get().height;

    let text = state.text();
    let selection = state.selection();

    let key = (
        selection.start,
        selection.end,
        (ime_bottom * 4.0).round() as i64,
        (window_height * 4.0).round() as i64,
    );
    let clock =
        cranpose_core::with_current_composer(cranpose_core::Composer::runtime_handle).frame_clock();
    cranpose_core::DisposableEffect(key, move |scope| {
        let registration = clock.with_frame_nanos(move |_| {
            let Some(metrics) = controller.metrics_now().filter(|metrics| metrics.focused) else {
                return;
            };
            let caret = caret_window_rect(&text, &style, &metrics, selection.start);
            responder.reveal_in_window(caret, ime_bottom, window_height);
        });
        crate::request_render_invalidation();
        scope.on_dispose(move || drop(registration))
    });
}

/// Emits selection/cursor handles for a focused field entered through any
/// primary pointer. Keyboard-only focus keeps a clean caret.
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
    let active_press = controller.press();

    let menu_open = remember(|| mutableStateOf(true)).with(|state| *state);
    let caret_menu_open = remember(|| mutableStateOf(false)).with(|state| *state);
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
    if !metrics.focused || !metrics.direct_manipulation {
        return;
    }
    // Past the gate the handles are on screen and have to travel with the
    // field, so from here the scope follows its position too.
    let Some(metrics) = controller.live_metrics() else {
        return;
    };

    let text = state.text();

    let press_watcher: Rc<Cell<Option<(u32, u32)>>> =
        remember(|| Rc::new(Cell::new(None))).with(Rc::clone);
    let press_watcher_ref: Rc<RefCell<Option<Rc<LongPressWatcher>>>> =
        remember(|| Rc::new(RefCell::new(None))).with(Rc::clone);
    match active_press {
        Some(press) => {
            let key = (press.start.x.to_bits(), press.start.y.to_bits());
            if press_watcher.get() != Some(key) {
                press_watcher.set(Some(key));
                let watcher = Rc::new(LongPressWatcher {
                    controller: controller.clone(),
                    state,
                    style: style.clone(),
                    start: press.start,
                    start_nanos: Cell::new(None),
                    registration: RefCell::new(None),
                    frame_clock: cranpose_core::with_current_composer(|composer| {
                        composer.runtime_handle()
                    })
                    .frame_clock(),
                });
                watcher.arm();
                *press_watcher_ref.borrow_mut() = Some(watcher);
            }
        }
        None => {
            press_watcher.set(None);
            press_watcher_ref.borrow_mut().take();
        }
    }

    let drag_pos: MutableState<Option<Point>> =
        remember(|| mutableStateOf(None::<Point>)).with(|state| *state);
    let drag_bias: Rc<Cell<Option<HandleGrabOffset>>> =
        remember(|| Rc::new(Cell::new(None))).with(Rc::clone);
    let last_dragged: Rc<Cell<Option<HandleKind>>> =
        remember(|| Rc::new(Cell::new(None))).with(Rc::clone);
    let menu_anchor_range: Rc<Cell<(usize, usize)>> =
        remember(|| Rc::new(Cell::new(current_range))).with(Rc::clone);
    {
        let last_dragged = Rc::clone(&last_dragged);
        let menu_anchor_range = Rc::clone(&menu_anchor_range);
        SideEffect(move || {
            if menu_anchor_range.get() != current_range {
                menu_anchor_range.set(current_range);
                if drag_pos.value().is_none() {
                    last_dragged.set(None);
                }
            }
        });
    }
    let cursor_tip_y: Rc<Cell<f32>> = remember(|| Rc::new(Cell::new(0.0f32))).with(Rc::clone);
    let start_tip_y: Rc<Cell<f32>> = remember(|| Rc::new(Cell::new(0.0f32))).with(Rc::clone);
    let end_tip_y: Rc<Cell<f32>> = remember(|| Rc::new(Cell::new(0.0f32))).with(Rc::clone);

    if selection.collapsed() {
        let (tip, caret_rect) = caret_window_geometry(
            &text,
            &style,
            &metrics,
            selection.start,
            LineAffinity::Upstream,
        );
        let on_drag = drag_caret_closure(state, style.clone(), controller, Rc::clone(&drag_bias));
        let open_caret_menu = {
            let caret_menu_offset = Rc::clone(&caret_menu_offset);
            move || {
                caret_menu_offset.set(state.selection().start);
                caret_menu_open.set(true);
            }
        };
        let on_tap = open_caret_menu.clone();
        let on_long_press = open_caret_menu;
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        cursor_tip_y.set(tip.y);
        let tip_y = Rc::clone(&cursor_tip_y);
        SelectionHandle(
            HandleKind::Cursor,
            tip,
            caret_rect.height,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                track_handle_grab(&grab_bias, HandleKind::Cursor, tip_y.get(), pos.y);
                drag_pos.set(Some(pos));
                on_drag(pos);
            },
            move || {
                drag_pos.set(None);
                end_bias.set(None);
                crate::cursor_animation::reset_cursor_blink();
            },
            on_long_press,
            on_tap,
        );

        if caret_menu_open.value() {
            let can_paste = clipboard_can_paste();
            let can_undo = state.can_undo();
            let can_redo = state.can_redo();
            let undo_state = state;
            let redo_state = state;
            CaretActionMenu(
                MenuAnchor {
                    center_x: tip.x,
                    line_top: caret_rect.y,
                    line_bottom: caret_rect.y + caret_rect.height,
                },
                drag_pos.value().is_none(),
                can_paste,
                can_undo,
                can_redo,
                move || {
                    clipboard_paste_into_focus();
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
        let start = selection.min();
        let end = selection.max();
        let (start_tip, start_rect) =
            caret_window_geometry(&text, &style, &metrics, start, LineAffinity::Downstream);
        let (end_tip, end_rect) =
            caret_window_geometry(&text, &style, &metrics, end, LineAffinity::Upstream);

        let last_dragged_start = Rc::clone(&last_dragged);
        let last_dragged_end = Rc::clone(&last_dragged);
        let on_drag_start = drag_edge_closure(
            HandleKind::SelectionStart,
            state,
            style.clone(),
            controller.clone(),
            Rc::clone(&drag_bias),
        );
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        start_tip_y.set(start_tip.y);
        let start_tip_live = Rc::clone(&start_tip_y);
        SelectionHandle(
            HandleKind::SelectionStart,
            start_tip,
            start_rect.height,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                track_handle_grab(
                    &grab_bias,
                    HandleKind::SelectionStart,
                    start_tip_live.get(),
                    pos.y,
                );
                last_dragged_start.set(Some(HandleKind::SelectionStart));
                drag_pos.set(Some(pos));
                on_drag_start(pos);
            },
            move || {
                drag_pos.set(None);
                end_bias.set(None);
            },
            move || menu_open.set(true),
            move || menu_open.set(true),
        );

        let on_drag_end = drag_edge_closure(
            HandleKind::SelectionEnd,
            state,
            style.clone(),
            controller.clone(),
            Rc::clone(&drag_bias),
        );
        let grab_bias = Rc::clone(&drag_bias);
        let end_bias = Rc::clone(&drag_bias);
        end_tip_y.set(end_tip.y);
        let end_tip_live = Rc::clone(&end_tip_y);
        SelectionHandle(
            HandleKind::SelectionEnd,
            end_tip,
            end_rect.height,
            HANDLE_RADIUS,
            accent,
            move |pos| {
                track_handle_grab(
                    &grab_bias,
                    HandleKind::SelectionEnd,
                    end_tip_live.get(),
                    pos.y,
                );
                last_dragged_end.set(Some(HandleKind::SelectionEnd));
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

        if menu_open.value() {
            let can_paste = clipboard_can_paste();
            let slide_point = if controller.gesture_claimed() {
                active_press.map(|press| press.position)
            } else {
                None
            };
            let (center_x, caret_rect) = match last_dragged.get() {
                Some(HandleKind::SelectionStart) => (start_tip.x, start_rect),
                Some(HandleKind::SelectionEnd | HandleKind::Cursor) => (end_tip.x, end_rect),
                None => ((start_tip.x + end_tip.x) * 0.5, start_rect),
            };
            TextSelectionMenu(
                MenuAnchor {
                    center_x,
                    line_top: caret_rect.y,
                    line_bottom: caret_rect.y + caret_rect.height,
                },
                drag_pos.value().is_none(),
                slide_point,
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
                    clipboard_paste_into_focus();
                    menu_open.set(false);
                },
                move || {
                    dispatch_select_all();
                    menu_open.set(false);
                },
            );
        }
    }

    let loupe_target = drag_pos.value().and_then(|finger| {
        let bias = drag_bias.get().map_or(0.0, |grab| grab.bias());
        let offset = window_pos_to_offset(&text, &style, &metrics, finger, bias);
        let (_, caret_rect) =
            caret_window_geometry(&text, &style, &metrics, offset, LineAffinity::Upstream);
        loupe_target_for_drag(finger, caret_rect.y + caret_rect.height, caret_rect.height)
    });
    SelectionLoupe(loupe_target);
}

fn track_handle_grab(
    drag_bias: &Cell<Option<HandleGrabOffset>>,
    kind: HandleKind,
    handle_tip_y: f32,
    finger_y: f32,
) -> f32 {
    let drifts = kind != HandleKind::SelectionStart;
    let mut grab = drag_bias
        .get()
        .unwrap_or_else(|| HandleGrabOffset::begin_for(handle_tip_y, finger_y, drifts));
    let bias = grab.track(finger_y);
    drag_bias.set(Some(grab));
    bias
}

/// Builds the drag handler for the collapsed cursor handle: moves the caret to
/// the dragged position. `drag_bias` is the finger-to-line offset captured at
/// the grab (see [`window_pos_to_offset`]).
fn drag_caret_closure(
    state: TextFieldState,
    style: TextStyle,
    controller: TextFieldHandleController,
    drag_bias: Rc<Cell<Option<HandleGrabOffset>>>,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let bias = drag_bias.get().map_or(0.0, |grab| grab.bias());
        let offset = window_pos_to_offset(&text, &style, &metrics, window_pos, bias);
        state.set_selection(TextRange::new(offset, offset));
        crate::cursor_animation::suspend_cursor_blink();
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
    drag_bias: Rc<Cell<Option<HandleGrabOffset>>>,
) -> Rc<dyn Fn(Point)> {
    Rc::new(move |window_pos: Point| {
        let Some(metrics) = controller.metrics() else {
            return;
        };
        let text = state.text();
        let bias = drag_bias.get().map_or(0.0, |grab| grab.bias());
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
#[path = "tests/basic_text_field_tests.rs"]
mod tests;

use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{MutableState, mutableStateOf};
use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, FocusState, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    SemanticsConfiguration, SemanticsNode, Size,
    text::{TextFieldLineLimits, TextFieldState, TextRange},
};
use cranpose_ui_graphics::{Brush, Color, Point};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextFieldHandleMetrics {
    pub focused: bool,
    pub direct_manipulation: bool,
    pub node_origin: Point,
    pub padding_left: f32,
    pub padding_top: f32,
    pub scroll_offset: f32,
    pub line_height: f32,
    pub glyph_box: (f32, f32),
    pub wrap_width: Option<f32>,
}

#[derive(Clone)]
pub struct TextFieldHandleController {
    inner: Rc<TextFieldHandleControllerInner>,
}

impl PartialEq for TextFieldHandleController {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

struct TextFieldHandleControllerInner {
    metrics: Cell<Option<TextFieldHandleMetrics>>,
    revision: MutableState<u64>,
    gesture_claim: RefCell<Option<Rc<Cell<bool>>>>,
    press_track: Cell<Option<MutableState<Option<PointerPressTrack>>>>,
}

impl TextFieldHandleController {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(TextFieldHandleControllerInner {
                metrics: Cell::new(None),
                revision: mutableStateOf(0u64),
                gesture_claim: RefCell::new(None),
                press_track: Cell::new(None),
            }),
        }
    }

    pub(crate) fn publish(&self, metrics: TextFieldHandleMetrics) {
        if self.inner.metrics.get() != Some(metrics) {
            self.inner.metrics.set(Some(metrics));
            self.inner
                .revision
                .update(|value| *value = value.wrapping_add(1));
        }
    }

    pub fn metrics(&self) -> Option<TextFieldHandleMetrics> {
        let _ = self.inner.revision.value();
        self.inner.metrics.get()
    }

    pub(crate) fn adopt_gesture_claim(&self, claim: &Rc<Cell<bool>>) {
        let mut slot = self.inner.gesture_claim.borrow_mut();
        let adopted = slot.as_ref().is_some_and(|held| Rc::ptr_eq(held, claim));
        if !adopted {
            *slot = Some(Rc::clone(claim));
        }
    }

    pub(crate) fn adopt_press_track(&self, press_track: MutableState<Option<PointerPressTrack>>) {
        if self.inner.press_track.get() != Some(press_track) {
            self.inner.press_track.set(Some(press_track));
            self.inner
                .revision
                .update(|value| *value = value.wrapping_add(1));
        }
    }

    pub fn press(&self) -> Option<PointerPressTrack> {
        self.inner.press_track.get().and_then(|state| state.get())
    }

    pub fn claim_gesture(&self) {
        if let Some(claim) = self.inner.gesture_claim.borrow().as_ref() {
            claim.set(true);
        }
    }

    pub fn gesture_claimed(&self) -> bool {
        self.inner
            .gesture_claim
            .borrow()
            .as_ref()
            .is_some_and(|claim| claim.get())
    }
}

impl Default for TextFieldHandleController {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_CURSOR_COLOR: Color = Color(1.0, 1.0, 1.0, 1.0);

const DEFAULT_SELECTION_COLOR: Color = Color(0.0, 0.5, 1.0, 0.3);

const DEFAULT_LINE_HEIGHT: f32 = 20.0;

const CURSOR_WIDTH: f32 = 2.0;

pub(crate) fn compute_horizontal_scroll_offset(
    current_offset: f32,
    cursor_x: f32,
    text_width: f32,
    viewport_width: f32,
) -> f32 {
    if viewport_width <= 0.0 {
        return 0.0;
    }
    let max_offset = (text_width + CURSOR_WIDTH - viewport_width).max(0.0);
    let mut offset = current_offset.clamp(0.0, max_offset);
    let visible_end = offset + viewport_width - CURSOR_WIDTH;
    if cursor_x > visible_end {
        offset = cursor_x - viewport_width + CURSOR_WIDTH;
    } else if cursor_x < offset {
        offset = cursor_x;
    }
    offset.clamp(0.0, max_offset)
}

pub(crate) fn intersect_rect(
    rect: cranpose_ui_graphics::Rect,
    bounds: cranpose_ui_graphics::Rect,
) -> Option<cranpose_ui_graphics::Rect> {
    let x0 = rect.x.max(bounds.x);
    let y0 = rect.y.max(bounds.y);
    let x1 = (rect.x + rect.width).min(bounds.x + bounds.width);
    let y1 = (rect.y + rect.height).min(bounds.y + bounds.height);
    (x1 > x0 && y1 > y0).then_some(cranpose_ui_graphics::Rect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// Resolver that recomputes (and stores) the horizontal pan offset for a
/// text field given the current content viewport width in px.
pub type TextPanResolver = Rc<dyn Fn(f32) -> f32>;

pub(crate) fn caret_visual_line_for_offset(
    text: &str,
    style: &TextStyle,
    node_id: Option<cranpose_core::NodeId>,
    wrap_width: Option<f32>,
    offset: usize,
    affinity: crate::text_selection::LineAffinity,
) -> (usize, usize) {
    let offset = offset.min(text.len());
    match wrap_width {
        Some(width) if width.is_finite() && width > 0.0 => {
            let annotated = crate::text::AnnotatedString::from(text);
            let ranges = crate::text::wrapped_line_ranges(
                node_id,
                &annotated,
                style,
                crate::text::TextLayoutOptions::default(),
                Some(width),
            );
            crate::text_selection::caret_visual_line(&ranges, offset, affinity)
        }
        _ => {
            let before = &text[..offset];
            let line_index = before.matches('\n').count();
            let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
            (line_index, line_start)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn range_visual_line_rects(
    text: &str,
    style: &TextStyle,
    node_id: Option<cranpose_core::NodeId>,
    wrap_width: Option<f32>,
    padding_left: f32,
    padding_top: f32,
    pan: f32,
    line_height: f32,
    start: usize,
    end: usize,
) -> Vec<cranpose_ui_graphics::Rect> {
    if start >= end {
        return Vec::new();
    }
    let annotated = crate::text::AnnotatedString::from(text);
    let line_ranges = crate::text::wrapped_line_ranges(
        node_id,
        &annotated,
        style,
        crate::text::TextLayoutOptions::default(),
        wrap_width,
    );
    let mut rects = Vec::new();
    for (line_idx, line_range) in line_ranges.iter().enumerate() {
        let line_start = line_range.start;
        let line_end = line_range.end;
        if end <= line_start || start >= line_end {
            continue;
        }
        let seg_start = start.max(line_start);
        let seg_end = end.min(line_end);
        let x0 = crate::text::measure_text(
            &crate::text::AnnotatedString::from(&text[line_start..seg_start]),
            style,
        )
        .width
            + padding_left
            - pan;
        let x1 = crate::text::measure_text(
            &crate::text::AnnotatedString::from(&text[line_start..seg_end]),
            style,
        )
        .width
            + padding_left
            - pan;
        let width = x1 - x0;
        if width > 0.0 {
            rects.push(cranpose_ui_graphics::Rect {
                x: x0,
                y: padding_top + line_idx as f32 * line_height,
                width,
                height: line_height,
            });
        }
    }
    rects
}

fn build_focus_handler(
    state: TextFieldState,
    refs: &TextFieldRefs,
    line_limits: TextFieldLineLimits,
    style: &TextStyle,
) -> Rc<dyn crate::text_field_focus::FocusedTextFieldHandler> {
    crate::text_field_handler::TextFieldHandler::new(
        state,
        refs.node_id.get(),
        line_limits,
        crate::text_field_handler::CaretGeometryRefs {
            node_origin: refs.node_origin.clone(),
            content_offset: refs.content_offset.clone(),
            content_y_offset: refs.content_y_offset.clone(),
            scroll_offset: refs.scroll_offset.clone(),
            style: style.clone(),
        },
    )
}

struct TextFieldFocusBridge {
    state: TextFieldState,
    refs: TextFieldRefs,
    style: TextStyle,
    line_limits: TextFieldLineLimits,
}

impl crate::focus_dispatch::FocusTargetHandle for TextFieldFocusBridge {
    fn set_focus_state(&self, state: FocusState) {
        if state.is_focused() {
            crate::text_field_focus::request_focus(
                self.refs.is_focused.clone(),
                build_focus_handler(self.state, &self.refs, self.line_limits, &self.style),
                self.refs.modal_depth.get(),
            );
        } else if crate::text_field_focus::focused_field_node() == self.refs.node_id.get() {
            crate::text_field_focus::clear_focus();
        }
    }
}

#[derive(Clone)]
pub(crate) struct TextFieldRefs {
    pub is_focused: Rc<RefCell<bool>>,
    pub content_offset: Rc<Cell<f32>>,
    pub content_y_offset: Rc<Cell<f32>>,
    pub drag_anchor: Rc<Cell<Option<usize>>>,
    pub last_click_time: Rc<Cell<Option<web_time::Instant>>>,
    pub last_click_pos: Rc<Cell<Option<(f32, f32)>>>,
    pub click_count: Rc<Cell<u8>>,
    pub node_id: Rc<Cell<Option<cranpose_core::NodeId>>>,
    pub scroll_offset: Rc<Cell<f32>>,
    pub direct_manipulation: Rc<Cell<bool>>,
    pub node_origin: Rc<Cell<Point>>,
    pub line_height: Rc<Cell<f32>>,
    pub wrap_width: Rc<Cell<Option<f32>>>,
    pub press_track: MutableState<Option<PointerPressTrack>>,
    pub gesture_claimed: Rc<Cell<bool>>,
    pub modal_depth: Rc<Cell<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerPressTrack {
    pub start: Point,
    pub position: Point,
}

impl TextFieldRefs {
    pub fn new() -> Self {
        Self {
            is_focused: Rc::new(RefCell::new(false)),
            content_offset: Rc::new(Cell::new(0.0_f32)),
            content_y_offset: Rc::new(Cell::new(0.0_f32)),
            drag_anchor: Rc::new(Cell::new(None::<usize>)),
            last_click_time: Rc::new(Cell::new(None::<web_time::Instant>)),
            last_click_pos: Rc::new(Cell::new(None::<(f32, f32)>)),
            click_count: Rc::new(Cell::new(0_u8)),
            node_id: Rc::new(Cell::new(None::<cranpose_core::NodeId>)),
            scroll_offset: Rc::new(Cell::new(0.0_f32)),
            direct_manipulation: Rc::new(Cell::new(false)),
            node_origin: Rc::new(Cell::new(Point { x: 0.0, y: 0.0 })),
            line_height: Rc::new(Cell::new(DEFAULT_LINE_HEIGHT)),
            wrap_width: Rc::new(Cell::new(None::<f32>)),
            press_track: mutableStateOf(None::<PointerPressTrack>),
            gesture_claimed: Rc::new(Cell::new(false)),
            modal_depth: Rc::new(Cell::new(0)),
        }
    }
}

use crate::text::TextStyle;

pub struct TextFieldModifierNode {
    state: TextFieldState,
    refs: TextFieldRefs,
    style: TextStyle,
    cursor_brush: Brush,
    selection_brush: Brush,
    line_limits: TextFieldLineLimits,
    cached_text: String,
    cached_selection: TextRange,
    node_state: NodeState,
    measured_size: Rc<Cell<Size>>,
    measured_line_height: Rc<Cell<f32>>,
    measured_wrap_width: Rc<Cell<Option<f32>>>,
    cached_handler: Rc<dyn Fn(PointerEvent)>,
    cached_pan_resolver: TextPanResolver,
    handle_controller: Option<TextFieldHandleController>,
    modal_depth: usize,
    focus_bridge: Option<Rc<dyn crate::focus_dispatch::FocusTargetHandle>>,
}

impl std::fmt::Debug for TextFieldModifierNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldModifierNode")
            .field("text", &self.state.text())
            .field("style", &self.style)
            .field("is_focused", &*self.refs.is_focused.borrow())
            .finish()
    }
}

impl TextFieldModifierNode {
    /// Creates a new text field modifier node.
    pub fn new(state: TextFieldState, style: TextStyle) -> Self {
        let value = state.value();
        let refs = TextFieldRefs::new();
        let refs_line_height = refs.line_height.clone();
        let refs_wrap_width = refs.wrap_width.clone();
        let line_limits = TextFieldLineLimits::default();
        let cached_handler =
            Self::create_handler(state, refs.clone(), line_limits, style.clone(), 0);
        let cached_pan_resolver =
            Self::create_pan_resolver(state, refs.clone(), line_limits, style.clone());

        Self {
            state,
            refs,
            style,
            cursor_brush: Brush::solid(DEFAULT_CURSOR_COLOR),
            selection_brush: Brush::solid(DEFAULT_SELECTION_COLOR),
            line_limits,
            cached_text: value.text,
            cached_selection: value.selection,
            node_state: NodeState::new(),
            measured_size: Rc::new(Cell::new(Size {
                width: 0.0,
                height: 0.0,
            })),
            measured_line_height: refs_line_height,
            measured_wrap_width: refs_wrap_width,
            cached_handler,
            cached_pan_resolver,
            handle_controller: None,
            modal_depth: 0,
            focus_bridge: None,
        }
    }

    /// Creates a node with custom line limits.
    pub fn with_line_limits(mut self, line_limits: TextFieldLineLimits) -> Self {
        self.line_limits = line_limits;
        self.rebuild_cached_closures();
        self
    }

    fn rebuild_cached_closures(&mut self) {
        self.cached_handler = Self::create_handler(
            self.state,
            self.refs.clone(),
            self.line_limits,
            self.style.clone(),
            self.modal_depth,
        );
        self.cached_pan_resolver = Self::create_pan_resolver(
            self.state,
            self.refs.clone(),
            self.line_limits,
            self.style.clone(),
        );
    }

    /// Installs the controller the field publishes live handle metrics to.
    pub fn with_handle_controller(mut self, controller: TextFieldHandleController) -> Self {
        self.handle_controller = Some(controller);
        self
    }

    fn create_pan_resolver(
        state: TextFieldState,
        refs: TextFieldRefs,
        line_limits: TextFieldLineLimits,
        style: TextStyle,
    ) -> TextPanResolver {
        Rc::new(move |viewport_width: f32| {
            if !line_limits.is_single_line() {
                refs.scroll_offset.set(0.0);
                return 0.0;
            }
            let text = state.text();
            let pos = state.selection().start.min(text.len());
            let text_width = crate::text::measure_text(
                &crate::text::AnnotatedString::from(text.as_str()),
                &style,
            )
            .width;
            let cursor_x = crate::text::measure_text(
                &crate::text::AnnotatedString::from(&text[..pos]),
                &style,
            )
            .width;
            let offset = compute_horizontal_scroll_offset(
                refs.scroll_offset.get(),
                cursor_x,
                text_width,
                viewport_width,
            );
            refs.scroll_offset.set(offset);
            offset
        })
    }

    /// Returns the pan resolver for single-line fields, `None` for multi-line.
    ///
    /// Exposed to the modifier slices so the render scene builder can pan the
    /// text glyphs by the same offset used for the cursor and selection.
    pub fn text_pan_resolver(&self) -> Option<TextPanResolver> {
        self.line_limits
            .is_single_line()
            .then(|| self.cached_pan_resolver.clone())
    }

    /// Returns the current horizontal scroll (pan) offset in px.
    pub fn scroll_offset(&self) -> f32 {
        self.refs.scroll_offset.get()
    }

    /// Returns the current line limits configuration.
    pub fn line_limits(&self) -> TextFieldLineLimits {
        self.line_limits
    }

    fn create_handler(
        state: TextFieldState,
        refs: TextFieldRefs,
        line_limits: TextFieldLineLimits,
        style: TextStyle,
        modal_depth: usize,
    ) -> Rc<dyn Fn(PointerEvent)> {
        use crate::{
            text_selection::{
                MULTI_TAP_SLOP_PX, MULTI_TAP_TIMEOUT_MS, SelectionGranularity, classify_tap_count,
                find_line_boundaries, find_paragraph_boundaries, resolve_selection_tap_count,
                tap_selection_granularity,
            },
            word_boundaries::find_word_boundaries,
        };

        Rc::new(move |event: PointerEvent| {
            refs.node_origin.set(Point {
                x: event.global_position.x - event.position.x,
                y: event.global_position.y - event.position.y,
            });

            let click_x =
                (event.position.x - refs.content_offset.get() + refs.scroll_offset.get()).max(0.0);
            let click_y = (event.position.y - refs.content_y_offset.get()).max(0.0);

            match event.kind {
                PointerEventKind::Down => {
                    refs.direct_manipulation.set(true);
                    refs.press_track.set(Some(PointerPressTrack {
                        start: event.global_position,
                        position: event.global_position,
                    }));
                    refs.gesture_claimed.set(false);

                    crate::text_field_focus::request_focus(
                        refs.is_focused.clone(),
                        build_focus_handler(state, &refs, line_limits, &style),
                        modal_depth,
                    );

                    let now = web_time::Instant::now();
                    let text = state.text();
                    let pos = crate::text::offset_for_position_wrapped(
                        &text,
                        &style,
                        refs.node_id.get(),
                        refs.wrap_width.get(),
                        refs.line_height.get(),
                        click_x,
                        click_y,
                    );

                    let previous = refs.last_click_pos.get().and_then(|(px, py)| {
                        let count = refs.click_count.get();
                        (count > 0).then_some((count, px, py))
                    });
                    let elapsed_ms = refs
                        .last_click_time
                        .get()
                        .map(|last| now.duration_since(last).as_millis())
                        .unwrap_or(u128::MAX);
                    let tap_count = classify_tap_count(
                        previous,
                        elapsed_ms,
                        event.position.x,
                        event.position.y,
                        MULTI_TAP_TIMEOUT_MS,
                        MULTI_TAP_SLOP_PX,
                    );

                    let selection = state.selection();
                    let tap_in_selection =
                        !selection.collapsed() && pos >= selection.min() && pos <= selection.max();
                    let repeat_in_place = refs
                        .last_click_pos
                        .get()
                        .map(|(px, py)| {
                            let dx = event.position.x - px;
                            let dy = event.position.y - py;
                            dx * dx + dy * dy <= MULTI_TAP_SLOP_PX * MULTI_TAP_SLOP_PX
                        })
                        .unwrap_or(false);
                    let effective_count = resolve_selection_tap_count(
                        tap_count,
                        refs.click_count.get(),
                        tap_in_selection,
                        repeat_in_place,
                    );

                    match tap_selection_granularity(effective_count) {
                        SelectionGranularity::Paragraph => {
                            let (start, end) = find_paragraph_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(start, end));
                            });
                            refs.drag_anchor.set(Some(start));
                        }
                        SelectionGranularity::Line => {
                            let (line_start, line_end) = find_line_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(line_start, line_end));
                            });
                            refs.drag_anchor.set(Some(line_start));
                        }
                        SelectionGranularity::Word => {
                            let (word_start, word_end) = find_word_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(word_start, word_end));
                            });
                            refs.drag_anchor.set(Some(word_start));
                        }
                        SelectionGranularity::Caret => {
                            refs.drag_anchor.set(Some(pos));
                            state.edit(|buffer| {
                                buffer.place_cursor_before_char(pos);
                            });
                        }
                    }

                    refs.click_count.set(effective_count);
                    refs.last_click_time.set(Some(now));
                    refs.last_click_pos
                        .set(Some((event.position.x, event.position.y)));
                    event.consume();
                }
                PointerEventKind::Move => {
                    if let Some(mut track) = refs.press_track.get() {
                        track.position = event.global_position;
                        refs.press_track.set(Some(track));
                        if let Some(node_id) = refs.node_id.get() {
                            crate::schedule_draw_repass(node_id);
                        }
                        crate::request_render_invalidation();
                    }
                    if refs.gesture_claimed.get() {
                        event.consume();
                        return;
                    }
                    if let Some(anchor) = refs.drag_anchor.get()
                        && *refs.is_focused.borrow()
                    {
                        let text = state.text();
                        let current_pos = crate::text::offset_for_position_wrapped(
                            &text,
                            &style,
                            refs.node_id.get(),
                            refs.wrap_width.get(),
                            refs.line_height.get(),
                            click_x,
                            click_y,
                        );

                        state.set_selection(TextRange::new(anchor, current_pos));

                        crate::request_render_invalidation();

                        event.consume();
                    }
                }
                PointerEventKind::Up => {
                    refs.drag_anchor.set(None);
                    refs.press_track.set(None);
                    refs.gesture_claimed.set(false);
                    if let Some(node_id) = refs.node_id.get() {
                        crate::schedule_draw_repass(node_id);
                    }
                    crate::request_render_invalidation();
                }
                PointerEventKind::Cancel => {
                    refs.press_track.set(None);
                    refs.gesture_claimed.set(false);
                    if let Some(node_id) = refs.node_id.get() {
                        crate::schedule_draw_repass(node_id);
                    }
                    crate::request_render_invalidation();
                }
                _ => {}
            }
        })
    }

    /// Creates a node with a custom accent: the caret is drawn solid in
    /// `color` and the selection highlight is derived from it at
    /// [`crate::widgets::SELECTION_HIGHLIGHT_ALPHA`] — the reference field
    /// tints caret, handles and highlight from the one accent.
    pub fn with_cursor_color(mut self, color: Color) -> Self {
        self.cursor_brush = Brush::solid(color);
        self.selection_brush = Brush::solid(
            color.with_alpha(crate::widgets::basic_text_field::SELECTION_HIGHLIGHT_ALPHA),
        );
        self
    }

    /// Sets the focus state.
    pub fn set_focused(&mut self, focused: bool) {
        let current = *self.refs.is_focused.borrow();
        if current != focused {
            *self.refs.is_focused.borrow_mut() = focused;
            if !focused {
                self.refs.direct_manipulation.set(false);
                self.refs.press_track.set(None);
                self.refs.gesture_claimed.set(false);
            }
        }
    }

    /// Returns whether the field is focused.
    pub fn is_focused(&self) -> bool {
        *self.refs.is_focused.borrow()
    }

    pub(crate) fn window_origin_sink(&self) -> Rc<Cell<Point>> {
        self.refs.node_origin.clone()
    }

    /// Returns the current text.
    pub fn text(&self) -> String {
        self.state.text()
    }

    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    /// Returns the current selection.
    pub fn selection(&self) -> TextRange {
        self.state.selection()
    }

    /// Returns the cursor brush for rendering.
    pub fn cursor_brush(&self) -> Brush {
        self.cursor_brush.clone()
    }

    /// Returns the selection brush for rendering selection highlight.
    pub fn selection_brush(&self) -> Brush {
        self.selection_brush.clone()
    }

    /// Inserts text at the current cursor position (for paste operations).
    pub fn insert_text(&mut self, text: &str) {
        self.state.edit(|buffer| {
            buffer.insert(text);
        });
    }

    /// Copies the selected text and returns it (for web copy operation).
    /// Returns None if no selection.
    pub fn copy_selection(&self) -> Option<String> {
        self.state.copy_selection()
    }

    /// Cuts the selected text: copies and deletes it.
    /// Returns the cut text, or None if no selection.
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.copy_selection();
        if text.is_some() {
            self.state.edit(|buffer| {
                buffer.delete(buffer.selection());
            });
        }
        text
    }

    /// Updates the content offset (padding.left) for accurate click-to-position cursor placement.
    /// Called from slices collection where padding is known.
    pub fn set_content_offset(&self, offset: f32) {
        self.refs.content_offset.set(offset);
    }

    /// Updates the content Y offset (padding.top) for cursor Y positioning.
    /// Called from slices collection where padding is known.
    pub fn set_content_y_offset(&self, offset: f32) {
        self.refs.content_y_offset.set(offset);
    }

    fn wrap_width(&self, available_width: f32) -> Option<f32> {
        (!self.line_limits.is_single_line() && available_width.is_finite() && available_width > 0.0)
            .then_some(available_width)
    }

    fn measure_text_content(&self, wrap_width: Option<f32>) -> Size {
        let text = self.state.text();
        let node_id = self.refs.node_id.get();
        let annotated = crate::text::AnnotatedString::from(text.as_str());
        let metrics = match wrap_width {
            Some(max_width) => crate::text::measure_text_with_options_for_node(
                node_id,
                &annotated,
                &self.style,
                crate::text::TextLayoutOptions::default(),
                Some(max_width),
            ),
            None => crate::text::measure_text_for_node(node_id, &annotated, &self.style),
        };
        self.measured_line_height.set(metrics.line_height);
        Size {
            width: metrics.width,
            height: metrics.height,
        }
    }

    fn update_cached_state(&mut self) -> bool {
        let value = self.state.value();
        let text_changed = value.text != self.cached_text;
        let selection_changed = value.selection != self.cached_selection;

        if text_changed {
            self.cached_text = value.text;
        }
        if selection_changed {
            self.cached_selection = value.selection;
        }

        text_changed || selection_changed
    }
}

impl DelegatableNode for TextFieldModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for TextFieldModifierNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.refs.node_id.set(context.node_id());

        context.invalidate(InvalidationKind::Layout);
        context.invalidate(InvalidationKind::Draw);
        context.invalidate(InvalidationKind::Semantics);

        if let Some(node_id) = context.node_id() {
            let bridge: Rc<dyn crate::focus_dispatch::FocusTargetHandle> =
                Rc::new(TextFieldFocusBridge {
                    state: self.state,
                    refs: self.refs.clone(),
                    style: self.style.clone(),
                    line_limits: self.line_limits,
                });
            self.focus_bridge = Some(Rc::clone(&bridge));
            crate::focus_dispatch::register_focus_target(node_id, bridge);
        }
    }

    fn on_detach(&mut self) {
        if let (Some(node_id), Some(bridge)) = (self.refs.node_id.get(), self.focus_bridge.take()) {
            crate::focus_dispatch::unregister_focus_target(node_id, &bridge);
        }
    }

    fn as_draw_node(&self) -> Option<&dyn DrawModifierNode> {
        Some(self)
    }

    fn as_draw_node_mut(&mut self) -> Option<&mut dyn DrawModifierNode> {
        Some(self)
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> {
        Some(self)
    }

    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNode> {
        Some(self)
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl LayoutModifierNode for TextFieldModifierNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let wrap_width = self.wrap_width(constraints.max_width);
        self.measured_wrap_width.set(wrap_width);
        let text_size = self.measure_text_content(wrap_width);

        let min_height = if text_size.height < 1.0 {
            DEFAULT_LINE_HEIGHT
        } else {
            text_size.height
        };

        let width = text_size
            .width
            .max(constraints.min_width)
            .min(constraints.max_width);
        let height = min_height
            .max(constraints.min_height)
            .min(constraints.max_height);

        let size = Size { width, height };
        self.measured_size.set(size);

        let _ = (self.cached_pan_resolver)(size.width);

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(size)
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32) -> f32 {
        self.measure_text_content(self.wrap_width(width))
            .height
            .max(DEFAULT_LINE_HEIGHT)
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32) -> f32 {
        self.measure_text_content(self.wrap_width(width))
            .height
            .max(DEFAULT_LINE_HEIGHT)
    }
}

fn content_viewport(
    measured: cranpose_ui_graphics::Size,
    size: cranpose_foundation::Size,
    padding_left: f32,
    padding_top: f32,
) -> (f32, f32) {
    let width = if measured.width > 0.0 {
        measured.width
    } else {
        (size.width - padding_left).max(0.0)
    };
    let height = if measured.height > 0.0 {
        measured.height
    } else {
        (size.height - padding_top).max(0.0)
    };
    (width, height)
}

impl DrawModifierNode for TextFieldModifierNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}

    fn create_draw_closure(
        &self,
    ) -> Option<Rc<dyn Fn(&mut cranpose_ui_graphics::DrawScopeDefault)>> {
        use cranpose_ui_graphics::{DrawPrimitive, DrawScope as _};

        let is_focused = self.refs.is_focused.clone();
        let state = self.state;
        let content_offset = self.refs.content_offset.clone();
        let content_y_offset = self.refs.content_y_offset.clone();
        let cursor_brush = self.cursor_brush.clone();
        let style = self.style.clone();
        let cached_line_height = self.measured_line_height.clone();
        let measured_size = self.measured_size.clone();
        let measured_wrap_width = self.measured_wrap_width.clone();
        let node_id = self.refs.node_id.clone();
        let pan_resolver = self.cached_pan_resolver.clone();
        let handle_controller = self.handle_controller.clone();
        let node_origin = self.refs.node_origin.clone();
        let direct_manipulation = self.refs.direct_manipulation.clone();
        let press_track = self.refs.press_track;
        let gesture_claimed = self.refs.gesture_claimed.clone();

        Some(Rc::new(move |scope| {
            let size = scope.size();
            if !*is_focused.borrow() {
                if let Some(controller) = &handle_controller {
                    controller.publish(TextFieldHandleMetrics {
                        focused: false,
                        direct_manipulation: false,
                        node_origin: node_origin.get(),
                        padding_left: 0.0,
                        padding_top: 0.0,
                        scroll_offset: 0.0,
                        line_height: cached_line_height.get(),
                        glyph_box: crate::text::glyph_line_box(&style, cached_line_height.get()),
                        wrap_width: measured_wrap_width.get(),
                    });
                }
                return;
            }

            let mut primitives = Vec::new();

            let text = state.text();
            let selection = state.selection();
            let padding_left = content_offset.get();
            let padding_top = content_y_offset.get();
            let line_height = cached_line_height.get();

            let (viewport_width, viewport_height) =
                content_viewport(measured_size.get(), size, padding_left, padding_top);
            let pan = pan_resolver(viewport_width);

            if let Some(controller) = &handle_controller {
                controller.adopt_gesture_claim(&gesture_claimed);
                controller.adopt_press_track(press_track);
                controller.publish(TextFieldHandleMetrics {
                    focused: true,
                    direct_manipulation: direct_manipulation.get(),
                    node_origin: node_origin.get(),
                    padding_left,
                    padding_top,
                    scroll_offset: pan,
                    line_height,
                    glyph_box: crate::text::glyph_line_box(&style, line_height),
                    wrap_width: measured_wrap_width.get(),
                });
            }
            let clip_bounds = cranpose_ui_graphics::Rect {
                x: padding_left,
                y: padding_top,
                width: viewport_width,
                height: viewport_height,
            };

            if let Some(comp_range) = state.composition() {
                let comp_start = comp_range.min();
                let comp_end = comp_range.max();

                if comp_start < comp_end && comp_end <= text.len() {
                    let underline_brush = cranpose_ui_graphics::Brush::solid(
                        cranpose_ui_graphics::Color(0.8, 0.8, 0.8, 0.8),
                    );
                    let underline_height: f32 = 2.0;

                    for line_rect in range_visual_line_rects(
                        &text,
                        &style,
                        node_id.get(),
                        measured_wrap_width.get(),
                        padding_left,
                        padding_top,
                        pan,
                        line_height,
                        comp_start,
                        comp_end,
                    ) {
                        let underline_rect = cranpose_ui_graphics::Rect {
                            x: line_rect.x,
                            y: line_rect.y + line_height - underline_height,
                            width: line_rect.width,
                            height: underline_height,
                        };
                        if let Some(clipped) = intersect_rect(underline_rect, clip_bounds) {
                            primitives.push(DrawPrimitive::Rect {
                                rect: clipped,
                                brush: underline_brush.clone(),
                                stroke: None,
                            });
                        }
                    }
                }
            }

            if selection.collapsed() && crate::cursor_animation::is_cursor_visible() {
                let pos = selection.start.min(text.len());
                let (line_index, line_start) = caret_visual_line_for_offset(
                    &text,
                    &style,
                    node_id.get(),
                    measured_wrap_width.get(),
                    pos,
                    crate::text_selection::LineAffinity::Upstream,
                );
                let cursor_x = crate::text::measure_text(
                    &crate::text::AnnotatedString::from(&text[line_start..pos]),
                    &style,
                )
                .width
                    + padding_left
                    - pan;
                let (box_off, box_h) = crate::text::glyph_line_box(&style, line_height);
                let cursor_y = padding_top + line_index as f32 * line_height + box_off;

                let cursor_rect = cranpose_ui_graphics::Rect {
                    x: cursor_x,
                    y: cursor_y,
                    width: CURSOR_WIDTH,
                    height: box_h,
                };

                if let Some(clipped) = intersect_rect(cursor_rect, clip_bounds) {
                    primitives.push(DrawPrimitive::Rect {
                        rect: clipped,
                        brush: cursor_brush.clone(),
                        stroke: None,
                    });
                }
            }

            scope.push_recorded(primitives);
        }))
    }

    fn create_behind_draw_closure(
        &self,
    ) -> Option<Rc<dyn Fn(&mut cranpose_ui_graphics::DrawScopeDefault)>> {
        use cranpose_ui_graphics::{DrawPrimitive, DrawScope as _};

        let is_focused = self.refs.is_focused.clone();
        let state = self.state;
        let content_offset = self.refs.content_offset.clone();
        let content_y_offset = self.refs.content_y_offset.clone();
        let selection_brush = self.selection_brush.clone();
        let style = self.style.clone();
        let cached_line_height = self.measured_line_height.clone();
        let measured_size = self.measured_size.clone();
        let measured_wrap_width = self.measured_wrap_width.clone();
        let node_id = self.refs.node_id.clone();
        let pan_resolver = self.cached_pan_resolver.clone();

        Some(Rc::new(move |scope| {
            let size = scope.size();
            if !*is_focused.borrow() {
                return;
            }
            let selection = state.selection();
            if selection.collapsed() {
                return;
            }
            let text = state.text();
            let padding_left = content_offset.get();
            let padding_top = content_y_offset.get();
            let line_height = cached_line_height.get();
            let (viewport_width, viewport_height) =
                content_viewport(measured_size.get(), size, padding_left, padding_top);
            let pan = pan_resolver(viewport_width);
            let clip_bounds = cranpose_ui_graphics::Rect {
                x: padding_left,
                y: padding_top,
                width: viewport_width,
                height: viewport_height,
            };

            let mut primitives = Vec::new();
            let (box_off, box_h) = crate::text::glyph_line_box(&style, line_height);
            for sel_rect in range_visual_line_rects(
                &text,
                &style,
                node_id.get(),
                measured_wrap_width.get(),
                padding_left,
                padding_top,
                pan,
                line_height,
                selection.min(),
                selection.max(),
            ) {
                let sel_rect = cranpose_ui_graphics::Rect {
                    y: sel_rect.y + box_off,
                    height: box_h,
                    ..sel_rect
                };
                if let Some(clipped) = intersect_rect(sel_rect, clip_bounds) {
                    primitives.push(DrawPrimitive::Rect {
                        rect: clipped,
                        brush: selection_brush.clone(),
                        stroke: None,
                    });
                }
            }
            scope.push_recorded(primitives);
        }))
    }
}

impl SemanticsNode for TextFieldModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        let text = self.state.text();
        config.content_description = Some(text);
        config.is_editable_text = true;
        config.text_selection = Some(self.state.selection());
    }
}

impl PointerInputNode for TextFieldModifierNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _event: &PointerEvent,
    ) -> bool {
        false
    }

    fn hit_test(&self, x: f32, y: f32) -> bool {
        let size = self.measured_size.get();
        x >= 0.0 && x <= size.width && y >= 0.0 && y <= size.height
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(self.cached_handler.clone())
    }
}

/// Element that creates and updates `TextFieldModifierNode` instances.
///
/// This follows the modifier element pattern where the element is responsible for:
/// - Creating new nodes (via `create`)
/// - Updating existing nodes when properties change (via `update`)
/// - Declaring capabilities (LAYOUT | DRAW | SEMANTICS)
#[derive(Clone)]
pub struct TextFieldElement {
    state: TextFieldState,
    style: TextStyle,
    cursor_color: Color,
    line_limits: TextFieldLineLimits,
    handle_controller: Option<TextFieldHandleController>,
    modal_depth: usize,
}

impl TextFieldElement {
    /// Creates a new text field element.
    pub fn new(state: TextFieldState, style: TextStyle) -> Self {
        Self {
            state,
            style,
            cursor_color: DEFAULT_CURSOR_COLOR,
            line_limits: TextFieldLineLimits::default(),
            handle_controller: None,
            modal_depth: 0,
        }
    }

    /// Creates an element with custom cursor color.
    pub fn with_cursor_color(mut self, color: Color) -> Self {
        self.cursor_color = color;
        self
    }

    /// Creates an element with custom line limits.
    pub fn with_line_limits(mut self, line_limits: TextFieldLineLimits) -> Self {
        self.line_limits = line_limits;
        self
    }

    /// Installs the finger-handle metrics channel shared with the composable.
    pub fn with_handle_controller(mut self, controller: TextFieldHandleController) -> Self {
        self.handle_controller = Some(controller);
        self
    }

    /// Sets the modal depth this field was composed at (see
    /// [`crate::modal::local_modal_depth`]).
    pub fn with_modal_depth(mut self, depth: usize) -> Self {
        self.modal_depth = depth;
        self
    }
}

impl std::fmt::Debug for TextFieldElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldElement")
            .field("text", &self.state.text())
            .field("style", &self.style)
            .field("cursor_color", &self.cursor_color)
            .finish()
    }
}

impl Hash for TextFieldElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.state.id().hash(state);
        self.cursor_color.0.to_bits().hash(state);
        self.cursor_color.1.to_bits().hash(state);
        self.cursor_color.2.to_bits().hash(state);
        self.cursor_color.3.to_bits().hash(state);
        self.style.render_hash().hash(state);
        self.line_limits.hash(state);
        self.modal_depth.hash(state);
    }
}

impl PartialEq for TextFieldElement {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.style == other.style
            && self.cursor_color == other.cursor_color
            && self.line_limits == other.line_limits
            && self.modal_depth == other.modal_depth
    }
}

impl Eq for TextFieldElement {}

impl ModifierNodeElement for TextFieldElement {
    type Node = TextFieldModifierNode;

    fn create(&self) -> Self::Node {
        let mut node = TextFieldModifierNode::new(self.state, self.style.clone())
            .with_cursor_color(self.cursor_color)
            .with_line_limits(self.line_limits);
        node.modal_depth = self.modal_depth;
        node.refs.modal_depth.set(self.modal_depth);
        if let Some(controller) = self.handle_controller.clone() {
            node = node.with_handle_controller(controller);
        }
        node.rebuild_cached_closures();
        node
    }

    fn update(&self, node: &mut Self::Node) {
        node.state = self.state;
        node.style = self.style.clone();
        node.cursor_brush = Brush::solid(self.cursor_color);
        node.line_limits = self.line_limits;
        node.handle_controller = self.handle_controller.clone();
        node.modal_depth = self.modal_depth;
        node.refs.modal_depth.set(self.modal_depth);
        node.rebuild_cached_closures();

        if node.update_cached_state() {}
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
            | NodeCapabilities::DRAW
            | NodeCapabilities::SEMANTICS
            | NodeCapabilities::POINTER_INPUT
    }

    fn always_update(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
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
                PointerEvent::new(PointerEventKind::Down, at, at)
                    .with_source(PointerSource::Stylus),
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
                    PointerEvent::new(PointerEventKind::Down, at, at)
                        .with_source(PointerSource::Touch),
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
                    PointerEvent::new(PointerEventKind::Down, at, at)
                        .with_source(PointerSource::Touch),
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
                "Empty text should have 0 width, got {}",
                empty_width
            );

            let hi_width =
                crate::text::measure_text(&crate::text::AnnotatedString::from("Hi"), &style).width;
            assert!(
                hi_width > 0.0,
                "Text 'Hi' should have positive width: {}",
                hi_width
            );

            let h_width =
                crate::text::measure_text(&crate::text::AnnotatedString::from("H"), &style).width;
            assert!(h_width > 0.0, "Text 'H' should have positive width");
            assert!(
                h_width < hi_width,
                "'H' width {} should be less than 'Hi' width {}",
                h_width,
                hi_width
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
                "Cursor x {} should equal 'Hi' width {}",
                cursor_x,
                hi_width
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
}

use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{MutableState, mutableStateOf};
use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, FocusState, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    SemanticsConfiguration, SemanticsNode, Size,
    text::{TextFieldLineLimits, TextFieldState, TextRange},
};
use cranpose_ui_graphics::{Brush, Color, Point, ProjectiveTransform};
use cranpose_ui_layout::ceil_to_px;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextFieldHandleMetrics {
    pub focused: bool,
    pub direct_manipulation: bool,
    /// Maps the field's local coordinates to its window.
    pub local_to_window: ProjectiveTransform,
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
    activation: MutableState<u64>,
    geometry: MutableState<u64>,
    gesture_claim: RefCell<Option<Rc<Cell<bool>>>>,
    press_track: Cell<Option<MutableState<Option<PointerPressTrack>>>>,
}

impl TextFieldHandleController {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(TextFieldHandleControllerInner {
                metrics: Cell::new(None),
                activation: mutableStateOf(0u64),
                geometry: mutableStateOf(0u64),
                gesture_claim: RefCell::new(None),
                press_track: Cell::new(None),
            }),
        }
    }

    pub(crate) fn publish(&self, metrics: TextFieldHandleMetrics) {
        let previous = self.inner.metrics.replace(Some(metrics));
        if previous == Some(metrics) {
            return;
        }
        let was_live = previous.map(|metrics| (metrics.focused, metrics.direct_manipulation));
        if was_live != Some((metrics.focused, metrics.direct_manipulation)) {
            self.bump(&self.inner.activation);
        }
        self.bump(&self.inner.geometry);
    }

    fn bump(&self, revision: &MutableState<u64>) {
        revision.update(|value| *value = value.wrapping_add(1));
    }

    /// The field's current handle metrics, subscribing the caller to whether
    /// the handles are live -- to [`TextFieldHandleMetrics::focused`] and
    /// [`TextFieldHandleMetrics::direct_manipulation`], and to nothing else.
    ///
    /// The rest of the metrics describe where the field sits, and a field
    /// moves for reasons that have nothing to do with its handles: a list
    /// scrolling under it, a bounce settling, a layout shifting by a
    /// fraction of a pixel. A caller that only wants to know whether to emit
    /// handles must not be recomposed by any of that, so geometry is not part
    /// of this subscription. Use [`Self::live_metrics`] once the handles are
    /// live and their position has to follow the field, and
    /// [`Self::metrics_now`] outside composition.
    pub fn metrics(&self) -> Option<TextFieldHandleMetrics> {
        let _ = self.inner.activation.value();
        self.inner.metrics.get()
    }

    /// The field's current handle metrics, subscribing the caller to the
    /// field's position as well.
    ///
    /// For a caller that places something at the caret and has to keep it
    /// there while the field moves. Reach for it only past a
    /// [`TextFieldHandleMetrics::focused`] check, because every caller that
    /// holds this subscription recomposes whenever the field moves at all.
    pub fn live_metrics(&self) -> Option<TextFieldHandleMetrics> {
        let _ = self.inner.geometry.value();
        self.inner.metrics.get()
    }

    /// The field's current handle metrics, subscribing the caller to nothing.
    ///
    /// For gesture callbacks and effects, which run after composition and want
    /// the position as it is now rather than as it was when their scope last
    /// composed.
    pub fn metrics_now(&self) -> Option<TextFieldHandleMetrics> {
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
            self.bump(&self.inner.activation);
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

pub(crate) const DEFAULT_SELECTION_COLOR: Color = Color(0.0, 0.5, 1.0, 0.3);

const DEFAULT_LINE_HEIGHT: f32 = 20.0;

/// What Compose's `textFieldMinSize` lays out to find a field's smallest
/// box: one line of ten 'H's.
const MIN_SIZE_TEXT: &str = "HHHHHHHHHH";

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

#[derive(Clone)]
pub(crate) struct TextFieldLayoutHandle {
    state: TextFieldState,
    node_id: Rc<Cell<Option<cranpose_core::NodeId>>>,
    wrap_width: Rc<Cell<Option<f32>>>,
}

impl TextFieldLayoutHandle {
    pub(crate) fn measured_layout(
        &self,
        style: &TextStyle,
    ) -> std::rc::Rc<crate::text::PreparedTextLayout> {
        crate::text::prepare_text_layout_for_node(
            self.node_id.get(),
            &Rc::new(crate::text::AnnotatedString::from(self.state.text())),
            &std::sync::Arc::new(style.clone()),
            crate::text::TextLayoutOptions::default(),
            self.wrap_width.get(),
        )
    }
}

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
            let line_start = before.rfind('\n').map_or(0, |i| i + 1);
            (line_index, line_start)
        }
    }
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn range_visual_line_rects(
    text: &str,
    style: &TextStyle,
    node_id: Option<cranpose_core::NodeId>,
    wrap_width: Option<f32>,
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
            - pan;
        let x1 = crate::text::measure_text(
            &crate::text::AnnotatedString::from(&text[line_start..seg_end]),
            style,
        )
        .width
            - pan;
        let width = x1 - x0;
        if width > 0.0 {
            rects.push(cranpose_ui_graphics::Rect {
                x: x0,
                y: line_idx as f32 * line_height,
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
        refs.focus_node.get(),
        line_limits,
        refs.focus_options.get().show_keyboard_on_focus,
        crate::text_field_handler::CaretGeometryRefs {
            local_to_window: refs.local_to_window.clone(),
            content_origin: refs.content_origin.clone(),
            scroll_offset: refs.scroll_offset.clone(),
            style: style.clone(),
        },
    )
}

fn request_pointer_focus(
    state: TextFieldState,
    refs: &TextFieldRefs,
    line_limits: TextFieldLineLimits,
    style: &TextStyle,
    modal_depth: usize,
) {
    if modal_depth < crate::modal::current_modal_depth() {
        return;
    }
    let routed = refs
        .focus_node
        .get()
        .is_some_and(crate::focus_dispatch::request_focus_in_context);
    if !routed {
        crate::text_field_focus::request_focus(
            refs.is_focused.clone(),
            build_focus_handler(state, refs, line_limits, style),
            modal_depth,
        );
    }
    if !refs.focus_options.get().show_keyboard_on_focus && *refs.is_focused.borrow() {
        crate::text_input_session::notify_text_input_focus_gained();
    }
}

pub(crate) struct TextFieldFocusBridge {
    state: TextFieldState,
    refs: TextFieldRefs,
    style: TextStyle,
    line_limits: TextFieldLineLimits,
}

impl TextFieldFocusBridge {
    /// The focus target of the field with these refs.
    pub(crate) fn handle(
        state: TextFieldState,
        refs: TextFieldRefs,
        style: TextStyle,
        line_limits: TextFieldLineLimits,
    ) -> Rc<dyn crate::focus_dispatch::FocusTargetHandle> {
        Rc::new(Self {
            state,
            refs,
            style,
            line_limits,
        })
    }
}

impl crate::focus_dispatch::FocusTargetHandle for TextFieldFocusBridge {
    fn set_focus_state(&self, state: FocusState) {
        if state.is_focused() {
            crate::text_field_focus::request_focus(
                self.refs.is_focused.clone(),
                build_focus_handler(self.state, &self.refs, self.line_limits, &self.style),
                self.refs.focus_options.get().modal_depth,
            );
        } else if crate::text_field_focus::focused_field_node() == self.refs.node_id.get() {
            crate::text_field_focus::clear_focus();
        }
    }
}

#[derive(Clone, Copy)]
struct TextFieldFocusOptions {
    modal_depth: usize,
    show_keyboard_on_focus: bool,
}

#[derive(Clone)]
pub(crate) struct TextFieldRefs {
    pub is_focused: Rc<RefCell<bool>>,
    /// Where the field's content sits in its node: the rect its layout
    /// placed the field at, after every layout modifier before it.
    pub content_origin: Rc<RefCell<crate::modifier::CoordinatorRect>>,
    pub drag_anchor: Rc<Cell<Option<crate::text_selection::SelectionAnchor>>>,
    pub last_click_time: Rc<Cell<Option<web_time::Instant>>>,
    pub last_click_pos: Rc<Cell<Option<(f32, f32)>>>,
    pub click_count: Rc<Cell<u8>>,
    pub node_id: Rc<Cell<Option<cranpose_core::NodeId>>>,
    /// The node focus and semantics know the field by: the decoration box
    /// around a decorated field, else the field's own node.
    pub focus_node: Rc<Cell<Option<cranpose_core::NodeId>>>,
    pub scroll_offset: Rc<Cell<f32>>,
    pub direct_manipulation: Rc<Cell<bool>>,
    pub local_to_window: Rc<Cell<ProjectiveTransform>>,
    pub line_height: Rc<Cell<f32>>,
    pub wrap_width: Rc<Cell<Option<f32>>>,
    pub press_track: MutableState<Option<PointerPressTrack>>,
    pub gesture_claimed: Rc<Cell<bool>>,
    focus_options: Rc<Cell<TextFieldFocusOptions>>,
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
            content_origin: Rc::default(),
            drag_anchor: Rc::new(Cell::new(None)),
            last_click_time: Rc::new(Cell::new(None::<web_time::Instant>)),
            last_click_pos: Rc::new(Cell::new(None::<(f32, f32)>)),
            click_count: Rc::new(Cell::new(0_u8)),
            node_id: Rc::new(Cell::new(None::<cranpose_core::NodeId>)),
            focus_node: Rc::new(Cell::new(None::<cranpose_core::NodeId>)),
            scroll_offset: Rc::new(Cell::new(0.0_f32)),
            direct_manipulation: Rc::new(Cell::new(false)),
            local_to_window: Rc::new(Cell::new(ProjectiveTransform::identity())),
            line_height: Rc::new(Cell::new(DEFAULT_LINE_HEIGHT)),
            wrap_width: Rc::new(Cell::new(None::<f32>)),
            press_track: mutableStateOf(None::<PointerPressTrack>),
            gesture_claimed: Rc::new(Cell::new(false)),
            focus_options: Rc::new(Cell::new(TextFieldFocusOptions {
                modal_depth: 0,
                show_keyboard_on_focus: true,
            })),
        }
    }

    /// Tells these refs from any others, so the nodes sharing them can be
    /// keyed by them.
    pub(crate) fn key(&self) -> u64 {
        Rc::as_ptr(&self.is_focused) as usize as u64
    }
}

impl PartialEq for TextFieldRefs {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.is_focused, &other.is_focused)
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
    /// [`Self::min_size`] and the density it was taken at.
    min_size: Cell<Option<(f32, Size)>>,
    measured_line_height: Rc<Cell<f32>>,
    measured_wrap_width: Rc<Cell<Option<f32>>>,
    cached_handler: Rc<dyn Fn(PointerEvent)>,
    cached_pan_resolver: TextPanResolver,
    handle_controller: Option<TextFieldHandleController>,
    modal_depth: usize,
    focus_bridge: Option<Rc<dyn crate::focus_dispatch::FocusTargetHandle>>,
    /// A decoration box around the field takes its input, focus and
    /// semantics, sharing its refs; the field only lays out and draws.
    decorated: bool,
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
        Self::with_refs(state, style, TextFieldRefs::new(), false)
    }

    fn with_refs(
        state: TextFieldState,
        style: TextStyle,
        refs: TextFieldRefs,
        decorated: bool,
    ) -> Self {
        let value = state.value();
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
            min_size: Cell::new(None),
            measured_line_height: refs_line_height,
            measured_wrap_width: refs_wrap_width,
            cached_handler,
            cached_pan_resolver,
            handle_controller: None,
            modal_depth: 0,
            focus_bridge: None,
            decorated,
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

    pub(crate) fn create_handler(
        state: TextFieldState,
        refs: TextFieldRefs,
        line_limits: TextFieldLineLimits,
        style: TextStyle,
        modal_depth: usize,
    ) -> Rc<dyn Fn(PointerEvent)> {
        use crate::text_selection::{
            MULTI_TAP_SLOP_PX, MULTI_TAP_TIMEOUT_MS, SelectionAnchor, SelectionGranularity,
            classify_tap_count, resolve_selection_tap_count, tap_selection_granularity,
        };

        Rc::new(move |event: PointerEvent| {
            let content = refs.content_origin.borrow().origin();
            let click_x = (event.position.x - content.x + refs.scroll_offset.get()).max(0.0);
            let click_y = (event.position.y - content.y).max(0.0);

            match event.kind {
                PointerEventKind::Down => {
                    refs.direct_manipulation.set(true);
                    refs.press_track.set(Some(PointerPressTrack {
                        start: event.global_position,
                        position: event.global_position,
                    }));
                    refs.gesture_claimed.set(false);

                    request_pointer_focus(state, &refs, line_limits, &style, modal_depth);

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
                        .map_or(u128::MAX, |last| now.duration_since(last).as_millis());
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
                    let repeat_in_place = refs.last_click_pos.get().is_some_and(|(px, py)| {
                        let dx = event.position.x - px;
                        let dy = event.position.y - py;
                        dx * dx + dy * dy <= MULTI_TAP_SLOP_PX * MULTI_TAP_SLOP_PX
                    });
                    let effective_count = resolve_selection_tap_count(
                        tap_count,
                        refs.click_count.get(),
                        tap_in_selection,
                        repeat_in_place,
                    );

                    let granularity = tap_selection_granularity(effective_count);
                    let anchor = SelectionAnchor::at(&text, pos, granularity);
                    refs.drag_anchor.set(Some(anchor));
                    state.edit(|buffer| match granularity {
                        SelectionGranularity::Caret => buffer.place_cursor_before_char(pos),
                        _ => buffer.select(TextRange::new(anchor.start, anchor.end)),
                    });

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

                        state.set_selection(anchor.dragged_to(&text, current_pos));

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

    pub(crate) fn window_transform_sink(&self) -> Rc<Cell<ProjectiveTransform>> {
        self.refs.local_to_window.clone()
    }

    pub(crate) fn layout_handle(&self) -> TextFieldLayoutHandle {
        TextFieldLayoutHandle {
            state: self.state,
            node_id: self.refs.node_id.clone(),
            wrap_width: self.measured_wrap_width.clone(),
        }
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

    /// Where the field's content sits in its node, set by slice collection
    /// and placed by layout; clicks, the caret and the handles map through it.
    pub(crate) fn set_content_origin(&self, origin: crate::modifier::CoordinatorRect) {
        *self.refs.content_origin.borrow_mut() = origin;
    }

    fn wrap_width(&self, available_width: f32) -> Option<f32> {
        (!self.line_limits.is_single_line() && available_width.is_finite() && available_width > 0.0)
            .then_some(available_width)
    }

    fn measure_text_content(&self, wrap_width: Option<f32>) -> crate::text::TextMetrics {
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
        metrics
    }

    /// The text's size on `density`'s grid: Compose sizes a field's layout
    /// as its paragraph's size, `ceil`ed to whole pixels.
    fn text_size(&self, wrap_width: Option<f32>, density: f32) -> (Size, crate::text::TextMetrics) {
        let metrics = self.measure_text_content(wrap_width);
        (
            Size {
                width: ceil_to_px(metrics.width, density),
                height: ceil_to_px(metrics.height, density),
            },
            metrics,
        )
    }

    /// The smallest box the field takes, as Compose's `textFieldMinSize`
    /// gives it: one line of ten 'H's in its style, so an empty field is as
    /// tall as a line and wide enough to type into.
    fn min_size(&self, density: f32) -> Size {
        if let Some((taken_at, size)) = self.min_size.get()
            && taken_at == density
        {
            return size;
        }
        let metrics = crate::text::measure_text(
            &crate::text::AnnotatedString::from(MIN_SIZE_TEXT),
            &self.style,
        );
        let size = Size {
            width: ceil_to_px(metrics.width, density),
            height: ceil_to_px(metrics.height, density),
        };
        self.min_size.set(Some((density, size)));
        size
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
        if self.decorated {
            return;
        }
        context.invalidate(InvalidationKind::Semantics);
        self.refs.focus_node.set(context.node_id());

        if let Some(node_id) = context.node_id() {
            let bridge = TextFieldFocusBridge::handle(
                self.state,
                self.refs.clone(),
                self.style.clone(),
                self.line_limits,
            );
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

    cranpose_foundation::impl_semantics_node!();

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        (!self.decorated).then_some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        if self.decorated { None } else { Some(self) }
    }
}

impl LayoutModifierNode for TextFieldModifierNode {
    fn measure(
        &self,
        context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        let density = context.density();
        let wrap_width = self.wrap_width(constraints.max_width);
        self.measured_wrap_width.set(wrap_width);
        let (text, metrics) = self.text_size(wrap_width, density);
        let min = self.min_size(density);
        // The minimum joins the constraints' own, as a floor they bound.
        let fit = |length: f32, min: f32, low: f32, high: f32| {
            length.max(min.max(low).min(high)).min(high)
        };
        let size = Size {
            width: fit(
                text.width,
                min.width,
                constraints.min_width,
                constraints.max_width,
            ),
            height: fit(
                text.height,
                min.height,
                constraints.min_height,
                constraints.max_height,
            ),
        };
        self.measured_size.set(size);

        let _ = (self.cached_pan_resolver)(size.width);

        let first = crate::text::measure::text_line_box(&self.style)
            .map(crate::text::LineBox::first_baseline)
            .or_else(|| crate::text::measure::first_baseline(&self.style));
        let lines = cranpose_ui_layout::AlignmentLines::new(
            first,
            first.map(|baseline| {
                baseline + metrics.line_count.saturating_sub(1) as f32 * metrics.line_height
            }),
        );
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(size).with_alignment_lines(lines)
    }

    // Compose's minimum size is a plain layout modifier, which leaves the
    // intrinsics to the text; an empty text is still a line tall.
    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32, density: f32) -> f32 {
        self.text_size(None, density).0.width
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32, density: f32) -> f32 {
        self.text_size(None, density).0.width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        self.intrinsic_height(width, density)
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, density: f32) -> f32 {
        self.intrinsic_height(width, density)
    }
}

impl TextFieldModifierNode {
    fn intrinsic_height(&self, width: f32, density: f32) -> f32 {
        self.text_size(self.wrap_width(width), density)
            .0
            .height
            .max(self.min_size(density).height)
    }
}

/// The field's content viewport: its measured text area, or the draw
/// scope's size before the first measure. The draw scope is the field's
/// content rect, already inside the padding declared before the field.
fn content_viewport(
    measured: cranpose_ui_graphics::Size,
    size: cranpose_foundation::Size,
) -> (f32, f32) {
    let width = if measured.width > 0.0 {
        measured.width
    } else {
        size.width
    };
    let height = if measured.height > 0.0 {
        measured.height
    } else {
        size.height
    };
    (width, height)
}

impl DrawModifierNode for TextFieldModifierNode {
    fn create_draw_closure(
        &self,
    ) -> Option<Rc<dyn Fn(&mut cranpose_ui_graphics::DrawScopeDefault)>> {
        use cranpose_ui_graphics::{DrawPrimitive, DrawScope as _};

        let is_focused = self.refs.is_focused.clone();
        let state = self.state;
        let content_origin = self.refs.content_origin.clone();
        let cursor_brush = self.cursor_brush.clone();
        let style = self.style.clone();
        let cached_line_height = self.measured_line_height.clone();
        let measured_size = self.measured_size.clone();
        let measured_wrap_width = self.measured_wrap_width.clone();
        let node_id = self.refs.node_id.clone();
        let pan_resolver = self.cached_pan_resolver.clone();
        let handle_controller = self.handle_controller.clone();
        let local_to_window = self.refs.local_to_window.clone();
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
                        local_to_window: local_to_window.get(),
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
            let line_height = cached_line_height.get();

            let (viewport_width, viewport_height) = content_viewport(measured_size.get(), size);
            let pan = pan_resolver(viewport_width);

            if let Some(controller) = &handle_controller {
                controller.adopt_gesture_claim(&gesture_claimed);
                controller.adopt_press_track(press_track);
                controller.publish(TextFieldHandleMetrics {
                    focused: true,
                    direct_manipulation: direct_manipulation.get(),
                    local_to_window: local_to_window.get(),
                    padding_left: content_origin.borrow().origin().x,
                    padding_top: content_origin.borrow().origin().y,
                    scroll_offset: pan,
                    line_height,
                    glyph_box: crate::text::glyph_line_box(&style, line_height),
                    wrap_width: measured_wrap_width.get(),
                });
            }
            let clip_bounds = cranpose_ui_graphics::Rect {
                x: 0.0,
                y: 0.0,
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
                    - pan;
                let (box_off, box_h) = crate::text::glyph_line_box(&style, line_height);
                let cursor_y = line_index as f32 * line_height + box_off;

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
            let line_height = cached_line_height.get();
            let (viewport_width, viewport_height) = content_viewport(measured_size.get(), size);
            let pan = pan_resolver(viewport_width);
            let clip_bounds = cranpose_ui_graphics::Rect {
                x: 0.0,
                y: 0.0,
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
        if !self.decorated {
            merge_text_field_semantics(self.state, self.line_limits, config);
        }
    }

    /// Neither modal nor hidden, but the field's text and selection are read
    /// live from its state, which an app may set without touching the node.
    fn reach(&self) -> cranpose_foundation::SemanticsReach {
        cranpose_foundation::SemanticsReach {
            merges_live_state: true,
            ..cranpose_foundation::SemanticsReach::default()
        }
    }
}

/// What an editable field tells accessibility, from the node that takes its
/// input.
pub(crate) fn merge_text_field_semantics(
    state: TextFieldState,
    line_limits: TextFieldLineLimits,
    config: &mut SemanticsConfiguration,
) {
    let text = state.text();
    if config.content_description.is_none() {
        config.content_description = Some(text.clone());
    }
    config.text = Some(text);
    config.is_editable_text = true;
    config.is_clickable = true;
    config.multiline = !matches!(line_limits, TextFieldLineLimits::SingleLine);
    config.set_text = Some(cranpose_foundation::SemanticsSetText::new(move |text| {
        state.set_text(text)
    }));
    config.set_selection = Some(cranpose_foundation::SemanticsSetSelection::new(
        move |anchor, focus| {
            let text = state.text();
            let anchor = floor_char_boundary(&text, anchor);
            let focus = floor_char_boundary(&text, focus);
            state.set_selection(TextRange::new(anchor, focus));
            crate::cursor_animation::reset_cursor_blink();
            crate::request_render_invalidation();
            true
        },
    ));
    config.text_selection = Some(state.selection());
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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
    show_keyboard_on_focus: bool,
    handle_controller: Option<TextFieldHandleController>,
    modal_depth: usize,
    decorator: Option<TextFieldRefs>,
}

impl TextFieldElement {
    /// Creates a new text field element.
    pub fn new(state: TextFieldState, style: TextStyle) -> Self {
        Self {
            state,
            style,
            cursor_color: DEFAULT_CURSOR_COLOR,
            line_limits: TextFieldLineLimits::default(),
            show_keyboard_on_focus: true,
            handle_controller: None,
            modal_depth: 0,
            decorator: None,
        }
    }

    /// Hands the field's input, focus and semantics to the decoration box
    /// that shares `refs`.
    pub(crate) fn decorated_by(mut self, refs: TextFieldRefs) -> Self {
        self.decorator = Some(refs);
        self
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

    /// Sets whether focus opens the software keyboard. A pointer tap always
    /// requests it, including when the field is already focused.
    pub fn with_show_keyboard_on_focus(mut self, show: bool) -> Self {
        self.show_keyboard_on_focus = show;
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
        self.show_keyboard_on_focus.hash(state);
        self.modal_depth.hash(state);
        self.decorator.as_ref().map(TextFieldRefs::key).hash(state);
    }
}

impl PartialEq for TextFieldElement {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.style == other.style
            && self.cursor_color == other.cursor_color
            && self.line_limits == other.line_limits
            && self.show_keyboard_on_focus == other.show_keyboard_on_focus
            && self.modal_depth == other.modal_depth
            && self.decorator == other.decorator
    }
}

impl Eq for TextFieldElement {}

impl ModifierNodeElement for TextFieldElement {
    type Node = TextFieldModifierNode;

    fn create(&self) -> Self::Node {
        let refs = self.decorator.clone().unwrap_or_else(TextFieldRefs::new);
        let decorated = self.decorator.is_some();
        let mut node =
            TextFieldModifierNode::with_refs(self.state, self.style.clone(), refs, decorated)
                .with_cursor_color(self.cursor_color)
                .with_line_limits(self.line_limits);
        node.modal_depth = self.modal_depth;
        node.refs.focus_options.set(TextFieldFocusOptions {
            modal_depth: self.modal_depth,
            show_keyboard_on_focus: self.show_keyboard_on_focus,
        });
        if let Some(controller) = self.handle_controller.clone() {
            node = node.with_handle_controller(controller);
        }
        node.rebuild_cached_closures();
        node
    }

    fn update(&self, node: &mut Self::Node) {
        node.state = self.state;
        if node.style != self.style {
            node.min_size.set(None);
            node.style = self.style.clone();
        }
        node.cursor_brush = Brush::solid(self.cursor_color);
        node.line_limits = self.line_limits;
        node.handle_controller.clone_from(&self.handle_controller);
        node.modal_depth = self.modal_depth;
        let previous_focus_options = node.refs.focus_options.replace(TextFieldFocusOptions {
            modal_depth: self.modal_depth,
            show_keyboard_on_focus: self.show_keyboard_on_focus,
        });
        if self.show_keyboard_on_focus
            && !previous_focus_options.show_keyboard_on_focus
            && *node.refs.is_focused.borrow()
        {
            crate::text_input_session::notify_text_input_focus_gained();
        }
        node.rebuild_cached_closures();

        if node.update_cached_state() {}
    }

    /// A decorated field's node shares its decoration's refs, so it is
    /// never handed to a field decorated by another.
    fn key(&self) -> Option<u64> {
        self.decorator.as_ref().map(TextFieldRefs::key)
    }

    fn capabilities(&self) -> NodeCapabilities {
        let drawn = NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS;
        if self.decorator.is_some() {
            drawn
        } else {
            drawn | NodeCapabilities::POINTER_INPUT
        }
    }

    fn always_update(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "tests/text_field_modifier_node_tests.rs"]
mod tests;

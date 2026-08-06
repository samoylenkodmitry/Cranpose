//! Text field modifier node for editable text input.
//!
//! This module implements the modifier node for `BasicTextField`, following
//! Jetpack Compose's `CoreTextFieldNode` architecture.
//!
//! The node handles:
//! - **Layout**: Measures text content and returns appropriate size
//! - **Draw**: Renders text, cursor, and selection highlights
//! - **Pointer Input**: Handles tap to position cursor, drag for selection
//! - **Semantics**: Provides text content for accessibility
//!
//! # Architecture
//!
//! Unlike display-only `TextModifierNode`, this node:
//! - References a `TextFieldState` for mutable text
//! - Tracks focus state for cursor visibility
//! - Handles pointer events for cursor positioning

use cranpose_core::{mutableStateOf, MutableState};
use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};
use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    SemanticsConfiguration, SemanticsNode, Size,
};
use cranpose_ui_graphics::{Brush, Color, Point};
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Live geometry a `BasicTextField` needs to place and drive its selection
/// handles: whether the field is focused and is under direct manipulation, its
/// on-screen origin (window coordinates) and the metrics that map a window
/// position back to a text offset.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TextFieldHandleMetrics {
    pub focused: bool,
    /// A primary pointer has interacted with the focused field. Mouse, touch,
    /// and pen all expose the same draggable selection mechanics.
    pub direct_manipulation: bool,
    /// Field node's top-left in window coordinates.
    pub node_origin: Point,
    pub padding_left: f32,
    pub padding_top: f32,
    pub scroll_offset: f32,
    pub line_height: f32,
    /// Tight glyph box `(top_offset, height)` inside each line slot — what
    /// the caret, highlight and finger handles anchor to (the reference
    /// selection chrome rides the glyphs, not the paragraph slot).
    pub glyph_box: (f32, f32),
    /// Width the field wrapped its text at (`None` for single-line fields).
    /// Lets the handles resolve the same visual (wrapped) lines the caret does.
    pub wrap_width: Option<f32>,
    /// Live pointer press on the text surface (window space) — the widget's
    /// long-press → slide-to-menu gesture reads this stream.
    pub press: Option<PointerPressTrack>,
}

/// Shared channel by which a `TextFieldModifierNode` publishes its live handle
/// [`TextFieldHandleMetrics`] to the `BasicTextField` composable that renders
/// the handles. Reads subscribe reactively (backed by a revision `MutableState`)
/// so the composable recomposes when the field's focus/geometry changes.
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
    /// The field node's gesture-claim flag, adopted at publish time so the
    /// widget's long-press watcher can take over the live pointer (the node
    /// then stops drag-selecting under it).
    gesture_claim: RefCell<Option<Rc<Cell<bool>>>>,
}

impl TextFieldHandleController {
    /// Creates a controller. Must run with an active runtime (i.e. inside a
    /// composition, via `remember`).
    pub fn new() -> Self {
        Self {
            inner: Rc::new(TextFieldHandleControllerInner {
                metrics: Cell::new(None),
                revision: mutableStateOf(0u64),
                gesture_claim: RefCell::new(None),
            }),
        }
    }

    /// Publishes fresh metrics, waking any reader only when they actually
    /// changed (so a resting frame does not spin recomposition).
    pub(crate) fn publish(&self, metrics: TextFieldHandleMetrics) {
        if self.inner.metrics.get() != Some(metrics) {
            self.inner.metrics.set(Some(metrics));
            self.inner
                .revision
                .update(|value| *value = value.wrapping_add(1));
        }
    }

    /// Reads the latest metrics, subscribing the current recompose scope to
    /// future changes.
    pub fn metrics(&self) -> Option<TextFieldHandleMetrics> {
        let _ = self.inner.revision.value();
        self.inner.metrics.get()
    }

    /// Adopts the field node's gesture-claim flag (idempotent).
    pub(crate) fn adopt_gesture_claim(&self, claim: &Rc<Cell<bool>>) {
        let mut slot = self.inner.gesture_claim.borrow_mut();
        let adopted = slot.as_ref().is_some_and(|held| Rc::ptr_eq(held, claim));
        if !adopted {
            *slot = Some(Rc::clone(claim));
        }
    }

    /// Claims the active press gesture for the widget layer: the node stops
    /// drag-selecting and the press stream drives the menu slide instead.
    pub fn claim_gesture(&self) {
        if let Some(claim) = self.inner.gesture_claim.borrow().as_ref() {
            claim.set(true);
        }
    }

    /// Whether the active press gesture is claimed by the widget layer.
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

/// Default cursor color (white - visible on dark backgrounds)
const DEFAULT_CURSOR_COLOR: Color = Color(1.0, 1.0, 1.0, 1.0);

/// Default selection highlight color (light blue with transparency)
const DEFAULT_SELECTION_COLOR: Color = Color(0.0, 0.5, 1.0, 0.3);

/// Default line height for empty text fields
const DEFAULT_LINE_HEIGHT: f32 = 20.0;

/// Cursor width in pixels
const CURSOR_WIDTH: f32 = 2.0;

/// Computes the horizontal scroll (pan) offset that keeps the cursor visible
/// inside the viewport of a single-line text field.
///
/// Mirrors Jetpack Compose's `TextFieldScrollerPosition.coerceOffset` behavior:
/// - the offset only changes when the cursor would leave the viewport,
/// - the offset is clamped so the text never detaches from the left edge and
///   never scrolls further than needed to show the end of the text (plus the
///   cursor width, so a cursor at the end of the text stays visible).
///
/// All values are in px within the field's content coordinate space.
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
        // Cursor ran past the right edge: pan so it sits at the right edge.
        offset = cursor_x - viewport_width + CURSOR_WIDTH;
    } else if cursor_x < offset {
        // Cursor ran past the left edge: pan so it sits at the left edge.
        offset = cursor_x;
    }
    offset.clamp(0.0, max_offset)
}

/// Intersects `rect` with `bounds`, returning `None` when nothing remains.
///
/// Used to clip selection/cursor/composition primitives to the field's
/// viewport so they never draw outside the field bounds.
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

/// Resolves the caret's visual `(line_index, line_start_byte)` for byte
/// `offset`.
///
/// For a wrapping (multi-line) field this lays the text out at the same wrap
/// width the field measured and returns the VISUAL line the caret sits on, so
/// the drawn caret lands on the same glyph the renderer draws. For a
/// non-wrapping field (single line, or when no wrap width is known yet) it falls
/// back to counting logical `\n` lines. Shared by the in-content caret and the
/// overlay selection handles so both agree.
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
            // Logical `\n` lines never share a boundary byte (the separator
            // sits between them), so affinity cannot change the result here.
            let before = &text[..offset];
            let line_index = before.matches('\n').count();
            let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
            (line_index, line_start)
        }
    }
}

/// Window-space (pre-clip) rects covering byte range `start..end`, one per
/// VISUAL (wrapped) line the range touches, each spanning the full
/// `line_height`.
///
/// Shared by the selection highlight and the composition-preedit underline so
/// both track soft-wrapping exactly as the renderer and caret do. Splitting on
/// logical `\n` alone draws the rect on the wrong line whenever a line above
/// the range soft-wraps (the x stays right, the y lands one visual line too
/// high). Iterating the same wrapped ranges the renderer lays out keeps them in
/// sync.
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

/// Shared references for text field input handling.
///
/// This struct bundles the shared state references passed to the pointer input handler,
/// reducing the argument count for `create_handler` from 8 individual `Rc` parameters
/// to a single struct (fixing clippy::too_many_arguments).
#[derive(Clone)]
pub(crate) struct TextFieldRefs {
    /// Whether this field is currently focused
    pub is_focused: Rc<RefCell<bool>>,
    /// Content offset from left (padding) for accurate click positioning
    pub content_offset: Rc<Cell<f32>>,
    /// Content offset from top (padding) for cursor Y positioning
    pub content_y_offset: Rc<Cell<f32>>,
    /// Drag anchor position (byte offset) for click-drag selection
    pub drag_anchor: Rc<Cell<Option<usize>>>,
    /// Last click time for double/triple-click detection
    pub last_click_time: Rc<Cell<Option<web_time::Instant>>>,
    /// Last click screen position, for multi-tap slop gating
    pub last_click_pos: Rc<Cell<Option<(f32, f32)>>>,
    /// Click count (1=single, 2=double, 3=triple)
    pub click_count: Rc<Cell<u8>>,
    /// Node ID for scoped layout invalidation
    pub node_id: Rc<Cell<Option<cranpose_core::NodeId>>>,
    /// Horizontal scroll (pan) offset in px for single-line fields.
    /// Keeps the cursor visible when the text is wider than the field.
    pub scroll_offset: Rc<Cell<f32>>,
    /// Whether the focused field has been entered through a primary pointer.
    /// This is source-independent: desktop mouse, touch, and pen share the
    /// same direct-manipulation selection UI.
    pub direct_manipulation: Rc<Cell<bool>>,
    /// Field node's top-left in window coordinates, derived from the most recent
    /// pointer event (`global_position - position`). Used to place selection
    /// handles in the top-level overlay, which is in window space.
    pub node_origin: Rc<Cell<Point>>,
    /// Line height from the last measurement. Shared with the node's
    /// `measured_line_height` so the pointer handler maps a tap's `y` to the
    /// correct VISUAL (wrapped) line.
    pub line_height: Rc<Cell<f32>>,
    /// Wrap width the last measurement laid the text out at (`None` for
    /// single-line fields). Shared with the node's `measured_wrap_width` so the
    /// pointer handler resolves the same wrapped lines the renderer draws.
    pub wrap_width: Rc<Cell<Option<f32>>>,
    /// Live primary-pointer press on the text surface (window space), for the
    /// widget layer's long-press → slide-to-menu gesture.
    pub press_track: Rc<Cell<Option<PointerPressTrack>>>,
    /// Set by the widget when its long-press watcher claims the active
    /// gesture: the node then stops drag-selecting on Move and the press
    /// positions feed the menu slide instead.
    pub gesture_claimed: Rc<Cell<bool>>,
}

/// A live primary-pointer press on the text surface, published by the field node
/// for the widget layer (window coordinates).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerPressTrack {
    /// Where the press went down.
    pub start: Point,
    /// The press's current position.
    pub position: Point,
}

impl TextFieldRefs {
    /// Creates a new set of shared references.
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
            press_track: Rc::new(Cell::new(None::<PointerPressTrack>)),
            gesture_claimed: Rc::new(Cell::new(false)),
        }
    }
}

/// Modifier node for editable text fields.
///
/// This node is the core of `BasicTextField`, handling:
/// - Text measurement and layout
/// - Cursor and selection rendering
/// - Pointer input for cursor positioning
use crate::text::TextStyle; // Add import

pub struct TextFieldModifierNode {
    /// The text field state (shared)
    state: TextFieldState,
    /// Shared references for input handling
    refs: TextFieldRefs,
    /// Text style
    style: TextStyle, // Add style
    /// Cursor brush color
    cursor_brush: Brush,
    /// Selection highlight brush
    selection_brush: Brush,
    /// Line limits configuration
    line_limits: TextFieldLineLimits,
    /// Cached text value for change detection
    cached_text: String,
    /// Cached selection for change detection
    cached_selection: TextRange,
    /// Node state for delegation
    node_state: NodeState,
    /// Measured size cache (shared with the draw closure as the pan viewport)
    measured_size: Rc<Cell<Size>>,
    /// Cached line height from last measurement (shared with draw closure)
    measured_line_height: Rc<Cell<f32>>,
    /// Wrap width the last measurement laid the text out at (`None` for
    /// single-line fields, which pan instead of wrapping). Shared with the draw
    /// closure so the caret and selection handles resolve the same *visual*
    /// (wrapped) lines the renderer draws, instead of counting only logical
    /// `\n` lines.
    measured_wrap_width: Rc<Cell<Option<f32>>>,
    /// Cached pointer input handler
    cached_handler: Rc<dyn Fn(PointerEvent)>,
    /// Cached horizontal pan resolver (recomputes + stores the scroll offset)
    cached_pan_resolver: TextPanResolver,
    /// Channel to publish live handle metrics to the `BasicTextField`
    /// composable that renders the finger selection handles. `None` when the
    /// field is used without handle support.
    handle_controller: Option<TextFieldHandleController>,
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

// Re-export from extracted module
use crate::text_field_handler::TextFieldHandler;

impl TextFieldModifierNode {
    /// Creates a new text field modifier node.
    pub fn new(state: TextFieldState, style: TextStyle) -> Self {
        let value = state.value();
        let refs = TextFieldRefs::new();
        let refs_line_height = refs.line_height.clone();
        let refs_wrap_width = refs.wrap_width.clone();
        let line_limits = TextFieldLineLimits::default();
        let cached_handler =
            Self::create_handler(state.clone(), refs.clone(), line_limits, style.clone());
        let cached_pan_resolver =
            Self::create_pan_resolver(state.clone(), refs.clone(), line_limits, style.clone());

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
            // Alias the refs cells so the pointer handler reads the same live
            // line-height / wrap-width the layout writes here — a tap's `y` must
            // resolve to the same VISUAL line the renderer draws.
            measured_line_height: refs_line_height,
            measured_wrap_width: refs_wrap_width,
            cached_handler,
            cached_pan_resolver,
            handle_controller: None,
        }
    }

    /// Creates a node with custom line limits.
    pub fn with_line_limits(mut self, line_limits: TextFieldLineLimits) -> Self {
        self.line_limits = line_limits;
        self.cached_pan_resolver = Self::create_pan_resolver(
            self.state.clone(),
            self.refs.clone(),
            line_limits,
            self.style.clone(),
        );
        self
    }

    /// Installs the controller the field publishes live handle metrics to.
    pub fn with_handle_controller(mut self, controller: TextFieldHandleController) -> Self {
        self.handle_controller = Some(controller);
        self
    }

    /// Creates the horizontal pan resolver closure.
    ///
    /// The resolver takes the content viewport width (px) and returns the
    /// horizontal scroll offset that keeps the cursor visible, storing the
    /// result in `refs.scroll_offset` so pointer input and rendering agree.
    /// It recomputes from the live state so layout, the render scene builder,
    /// and the draw closure all observe the same value within a frame.
    fn create_pan_resolver(
        state: TextFieldState,
        refs: TextFieldRefs,
        line_limits: TextFieldLineLimits,
        style: TextStyle,
    ) -> TextPanResolver {
        Rc::new(move |viewport_width: f32| {
            if !line_limits.is_single_line() {
                // Multi-line fields do not pan horizontally.
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

    /// Creates the pointer input handler closure.
    fn create_handler(
        state: TextFieldState,
        refs: TextFieldRefs,
        line_limits: TextFieldLineLimits,
        style: TextStyle, // Add style
    ) -> Rc<dyn Fn(PointerEvent)> {
        // Tap-count classification plus word/line/paragraph boundaries drive the
        // multi-tap selection granularity gestures.
        use crate::text_selection::{
            classify_tap_count, find_line_boundaries, find_paragraph_boundaries,
            resolve_selection_tap_count, tap_selection_granularity, SelectionGranularity,
            MULTI_TAP_SLOP_PX, MULTI_TAP_TIMEOUT_MS,
        };
        use crate::word_boundaries::find_word_boundaries;

        Rc::new(move |event: PointerEvent| {
            // Seed the field node's window-space origin from this pointer event
            // so the very first handle placement after a tap has a value even
            // before the next layout pass runs. The layout pass
            // (`window_origin_sink`) is the authoritative source that keeps it
            // fresh as the field scrolls; both agree (`global - local` equals
            // the composited window origin at rest).
            refs.node_origin.set(Point {
                x: event.global_position.x - event.position.x,
                y: event.global_position.y - event.position.y,
            });

            // Account for content padding offsets and the horizontal pan
            // offset (single-line fields pan to keep the cursor visible, so
            // clicks must map back into text space).
            let click_x =
                (event.position.x - refs.content_offset.get() + refs.scroll_offset.get()).max(0.0);
            let click_y = (event.position.y - refs.content_y_offset.get()).max(0.0);

            match event.kind {
                PointerEventKind::Down => {
                    // Direct selection mechanics are source-independent:
                    // mouse, touch and pen all expose handles and the same
                    // continuous long-press → slide-to-menu gesture.
                    refs.direct_manipulation.set(true);
                    refs.press_track.set(Some(PointerPressTrack {
                        start: event.global_position,
                        position: event.global_position,
                    }));
                    refs.gesture_claimed.set(false);

                    // Request focus with O(1) handler, passing node_id and line
                    // limits for key handling plus the live geometry cells the
                    // layout keeps fresh, so coordinate-based platform text input
                    // (iOS caret positioning) can read the caret's window rect.
                    let handler = TextFieldHandler::new(
                        state.clone(),
                        refs.node_id.get(),
                        line_limits,
                        crate::text_field_handler::CaretGeometryRefs {
                            node_origin: refs.node_origin.clone(),
                            content_offset: refs.content_offset.clone(),
                            content_y_offset: refs.content_y_offset.clone(),
                            scroll_offset: refs.scroll_offset.clone(),
                            style: style.clone(),
                        },
                    );
                    crate::text_field_focus::request_focus(refs.is_focused.clone(), handler);

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

                    // Classify the press into a 1-based tap count by both the
                    // time since and the distance from the previous press (a tap
                    // far from the last one starts a fresh single tap, matching
                    // Android's double-tap slop).
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

                    // A lone tap that lands INSIDE an existing (non-collapsed)
                    // selection selects the word under the finger (Android/iOS
                    // "tap the selection to re-grab a word"). Tapping the SAME
                    // spot again grows the granularity word → line → paragraph →
                    // word …, keyed on location so it keeps escalating even when
                    // the taps arrive too slowly to count as a rapid multi-tap.
                    // A lone tap elsewhere just places the caret.
                    let selection = state.selection();
                    let tap_in_selection =
                        !selection.collapsed() && pos >= selection.min() && pos <= selection.max();
                    // Same-spot repeat, independent of the multi-tap timeout:
                    // within slop of the previous press.
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
                            // Fourth tap: grow to the whole paragraph.
                            let (start, end) = find_paragraph_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(start, end));
                            });
                            refs.drag_anchor.set(Some(start));
                        }
                        SelectionGranularity::Line => {
                            // Triple tap: select the line.
                            let (line_start, line_end) = find_line_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(line_start, line_end));
                            });
                            refs.drag_anchor.set(Some(line_start));
                        }
                        SelectionGranularity::Word => {
                            // Double tap (or a tap inside an existing selection):
                            // select the word.
                            let (word_start, word_end) = find_word_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(word_start, word_end));
                            });
                            refs.drag_anchor.set(Some(word_start));
                        }
                        SelectionGranularity::Caret => {
                            // Single tap: place the cursor.
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
                    // Keep the live press stream fresh for the widget layer.
                    if let Some(mut track) = refs.press_track.get() {
                        track.position = event.global_position;
                        refs.press_track.set(Some(track));
                        crate::request_render_invalidation();
                    }
                    // A claimed gesture belongs to the widget's menu slide:
                    // the node must not keep drag-selecting under it.
                    if refs.gesture_claimed.get() {
                        event.consume();
                        return;
                    }
                    // If we have a drag anchor, extend selection during drag
                    if let Some(anchor) = refs.drag_anchor.get() {
                        if *refs.is_focused.borrow() {
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

                            // Update selection directly (without undo stack push)
                            state.set_selection(TextRange::new(anchor, current_pos));

                            // Selection change only needs redraw, not layout
                            crate::request_render_invalidation();

                            event.consume();
                        }
                    }
                }
                PointerEventKind::Up => {
                    // Clear drag anchor on mouse up
                    refs.drag_anchor.set(None);
                    refs.press_track.set(None);
                    refs.gesture_claimed.set(false);
                }
                PointerEventKind::Cancel => {
                    refs.press_track.set(None);
                    refs.gesture_claimed.set(false);
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

    /// Returns the is_focused Rc for closure capture.
    pub fn is_focused_rc(&self) -> Rc<RefCell<bool>> {
        self.refs.is_focused.clone()
    }

    /// Returns the content_offset Rc for closure capture.
    pub fn content_offset_rc(&self) -> Rc<Cell<f32>> {
        self.refs.content_offset.clone()
    }

    /// Returns the content_y_offset Rc for closure capture.
    pub fn content_y_offset_rc(&self) -> Rc<Cell<f32>> {
        self.refs.content_y_offset.clone()
    }

    /// Returns the shared cell the field's composited window origin is written
    /// into (window coordinates of the field node's top-left).
    ///
    /// The layout pass writes the field's TRUE on-screen origin here every frame
    /// — resolved through all ancestor placements (a scrolling `LazyColumn` /
    /// `vertical_scroll` offsets its items via placement, which the layout tree
    /// bakes into each node's absolute rect) plus ancestor graphics-layer
    /// translations. The draw closure reads it back to publish handle metrics,
    /// so the finger selection/cursor handles anchor at (and their window→offset
    /// inverse mapping agrees with) the field's real glyphs even while the list
    /// scrolls. Without this the origin was only ever sampled from the last
    /// pointer event and went stale the moment the field scrolled.
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

    /// Returns a clone of the text field state for use in draw closures.
    /// This allows reading selection at DRAW time rather than LAYOUT time.
    pub fn get_state(&self) -> cranpose_foundation::text::TextFieldState {
        self.state.clone()
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

    /// The wrap width a multi-line field lays its text out at, or `None` when
    /// the text must not wrap (single-line fields pan horizontally instead).
    ///
    /// Multi-line fields wrap at the available content width exactly like the
    /// render scene builder, so the measured height reflects every wrapped line
    /// and the field grows to fit its content instead of clipping it.
    fn wrap_width(&self, available_width: f32) -> Option<f32> {
        (!self.line_limits.is_single_line() && available_width.is_finite() && available_width > 0.0)
            .then_some(available_width)
    }

    /// Measures the text content using node-identity-based caching.
    ///
    /// `wrap_width` bounds the layout width so multi-line text wraps; `None`
    /// measures the natural single-line width (single-line fields, intrinsic
    /// width queries).
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

    /// Updates cached state and returns true if changed.
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

    /// Positions cursor at a given x offset within the text.
    /// Uses proper text layout hit testing for accurate proportional font support.
    pub fn position_cursor_at_offset(&self, x_offset: f32) {
        let text = self.state.text();
        if text.is_empty() {
            self.state.edit(|buffer| {
                buffer.place_cursor_at_start();
            });
            return;
        }

        // Use proper text layout hit testing instead of character-based calculation.
        // Map the viewport-relative offset into text space by adding the pan offset.
        let byte_offset = crate::text::get_offset_for_position(
            &crate::text::AnnotatedString::from(text.as_str()),
            &self.style,
            x_offset + self.refs.scroll_offset.get(),
            0.0,
        );

        self.state.edit(|buffer| {
            buffer.place_cursor_before_char(byte_offset);
        });
    }

    // NOTE: Key event handling is done via TextFieldHandler::handle_key() which is
    // registered with the focus system for O(1) dispatch. DO NOT add a handle_key_event()
    // method here - it would be duplicate code that never gets called.
}

impl DelegatableNode for TextFieldModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for TextFieldModifierNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        // Store node_id for scoped layout invalidation (avoids O(app) global invalidation)
        self.refs.node_id.set(context.node_id());

        context.invalidate(InvalidationKind::Layout);
        context.invalidate(InvalidationKind::Draw);
        context.invalidate(InvalidationKind::Semantics);
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
        // Measure the text content, wrapping multi-line fields at the available
        // width so the field grows to fit every wrapped line instead of
        // clipping content past the first line.
        let wrap_width = self.wrap_width(constraints.max_width);
        // Remember the wrap width so the draw closure can resolve the same
        // visual (wrapped) lines when placing the caret and selection handles.
        self.measured_wrap_width.set(wrap_width);
        let text_size = self.measure_text_content(wrap_width);

        // Add minimum height for empty text (cursor needs space)
        let min_height = if text_size.height < 1.0 {
            DEFAULT_LINE_HEIGHT
        } else {
            text_size.height
        };

        // Constrain to provided constraints
        let width = text_size
            .width
            .max(constraints.min_width)
            .min(constraints.max_width);
        let height = min_height
            .max(constraints.min_height)
            .min(constraints.max_height);

        let size = Size { width, height };
        self.measured_size.set(size);

        // Refresh the horizontal pan offset so it is up to date for pointer
        // input and rendering even before the next draw pass runs.
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

/// Content viewport (excludes padding), falling back to the node size when
/// measurement has not run yet. Shared by the field's behind (selection
/// highlight) and overlay (caret, IME underline) draw closures.
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
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // No-op: Cursor and selection are rendered via create_draw_closure() which
        // creates DrawPrimitive::Rect directly. This enables draw-time evaluation
        // of focus state and cursor blink timing.
    }

    fn create_draw_closure(
        &self,
    ) -> Option<Rc<dyn Fn(cranpose_foundation::Size) -> Vec<cranpose_ui_graphics::DrawPrimitive>>>
    {
        use cranpose_ui_graphics::DrawPrimitive;

        // Capture state via Rc clone (cheap) for draw-time evaluation
        let is_focused = self.refs.is_focused.clone();
        let state = self.state.clone();
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
        let press_track = self.refs.press_track.clone();
        let gesture_claimed = self.refs.gesture_claimed.clone();

        Some(Rc::new(move |size| {
            // Check focus at DRAW time
            if !*is_focused.borrow() {
                // Publish an unfocused snapshot so the composable clears any
                // finger handles when the field loses focus.
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
                        press: None,
                    });
                }
                return vec![];
            }

            let mut primitives = Vec::new();

            let text = state.text();
            let selection = state.selection();
            let padding_left = content_offset.get();
            let padding_top = content_y_offset.get();
            // Reuse line_height from the most recent layout measurement
            // instead of re-measuring the full text.
            let line_height = cached_line_height.get();

            let (viewport_width, viewport_height) =
                content_viewport(measured_size.get(), size, padding_left, padding_top);
            // Horizontal pan that keeps the cursor visible (0 for multi-line).
            let pan = pan_resolver(viewport_width);

            // Publish live geometry so the `BasicTextField` composable can place
            // and drive the finger selection handles.
            if let Some(controller) = &handle_controller {
                controller.adopt_gesture_claim(&gesture_claimed);
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
                    press: press_track.get(),
                });
            }
            // Everything the field draws (selection, IME underline, cursor)
            // is clipped to the content viewport so primitives never extend
            // outside the field bounds.
            let clip_bounds = cranpose_ui_graphics::Rect {
                x: padding_left,
                y: padding_top,
                width: viewport_width,
                height: viewport_height,
            };

            // (The selection highlight renders BEHIND the glyphs — see
            // create_behind_draw_closure; a translucent fill over the text
            // tinted the selected glyphs.)

            // Draw composition (IME preedit) underline
            // This shows the user which text is being composed by the input method
            if let Some(comp_range) = state.composition() {
                let comp_start = comp_range.min();
                let comp_end = comp_range.max();

                if comp_start < comp_end && comp_end <= text.len() {
                    // Underline color: slightly transparent white/gray
                    let underline_brush = cranpose_ui_graphics::Brush::solid(
                        cranpose_ui_graphics::Color(0.8, 0.8, 0.8, 0.8),
                    );
                    let underline_height: f32 = 2.0;

                    // Per-visual-line rects, shrunk to a strip at the bottom of
                    // each line — same wrap-aware layout as the selection.
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

            // Draw cursor - check visibility at DRAW time for blinking. The
            // caret exists only for a collapsed selection: with a range
            // selected the edges are marked by the finger handles, and a
            // caret drawn at the range start just thickens the start
            // handle's stem.
            if selection.collapsed() && crate::cursor_animation::is_cursor_visible() {
                let pos = selection.start.min(text.len());
                // Resolve the caret's VISUAL (wrapped) line so it lands on the
                // same glyph the renderer draws — the field wraps long lines, and
                // counting only logical `\n` lines would draw the caret on the
                // wrong line (and, with the full logical-line-prefix width, off
                // the right edge) while typing/the magnifier stay correct.
                // Upstream affinity: a caret placed by a finger at a wrapped
                // line's right edge draws at that line's end, not one line
                // down at the left edge (matching the cursor handle's anchor).
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
                // The caret spans the tight glyph box, not the paragraph
                // slot — the reference caret's ends ride the glyph extents.
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

            primitives
        }))
    }

    fn create_behind_draw_closure(
        &self,
    ) -> Option<Rc<dyn Fn(cranpose_foundation::Size) -> Vec<cranpose_ui_graphics::DrawPrimitive>>>
    {
        use cranpose_ui_graphics::DrawPrimitive;

        let is_focused = self.refs.is_focused.clone();
        let state = self.state.clone();
        let content_offset = self.refs.content_offset.clone();
        let content_y_offset = self.refs.content_y_offset.clone();
        let selection_brush = self.selection_brush.clone();
        let style = self.style.clone();
        let cached_line_height = self.measured_line_height.clone();
        let measured_size = self.measured_size.clone();
        let measured_wrap_width = self.measured_wrap_width.clone();
        let node_id = self.refs.node_id.clone();
        let pan_resolver = self.cached_pan_resolver.clone();

        Some(Rc::new(move |size| {
            if !*is_focused.borrow() {
                return vec![];
            }
            let selection = state.selection();
            if selection.collapsed() {
                return vec![];
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

            // Highlight per VISUAL (wrapped) line so it lands on the same
            // glyphs the renderer draws — BENEATH them (the reference keeps
            // selected glyphs unblended white over the tint).
            let mut primitives = Vec::new();
            // Highlight rects hug the tight glyph box of each line — the
            // reference selection shows GAPS between lines when the
            // paragraph line height exceeds the natural text height.
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
            primitives
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
        // No-op: All pointer handling is done via pointer_input_handler() closure.
        // This follows Jetpack Compose's delegation pattern where the node simply
        // forwards to a delegated pointer input handler (see TextFieldDecoratorModifier.kt:741-747).
        //
        // The cached_handler closure handles:
        // - Focus request on Down
        // - Cursor positioning
        // - Double-click word selection
        // - Triple-click select all
        // - Drag selection
        false
    }

    fn hit_test(&self, x: f32, y: f32) -> bool {
        // Check if point is within measured bounds
        let size = self.measured_size.get();
        x >= 0.0 && x <= size.width && y >= 0.0 && y <= size.height
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        // Return cached handler for pointer input dispatch
        Some(self.cached_handler.clone())
    }
}

// ============================================================================
// TextFieldElement - Creates and updates TextFieldModifierNode
// ============================================================================

/// Element that creates and updates `TextFieldModifierNode` instances.
///
/// This follows the modifier element pattern where the element is responsible for:
/// - Creating new nodes (via `create`)
/// - Updating existing nodes when properties change (via `update`)
/// - Declaring capabilities (LAYOUT | DRAW | SEMANTICS)
#[derive(Clone)]
pub struct TextFieldElement {
    /// The text field state
    state: TextFieldState,
    /// Text style
    style: TextStyle,
    /// Cursor color
    cursor_color: Color,
    /// Line limits configuration
    line_limits: TextFieldLineLimits,
    /// Channel the node publishes live handle metrics to (finger selection
    /// handles). `None` disables handle support.
    handle_controller: Option<TextFieldHandleController>,
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
        // Hash by state Rc pointer identity - matches PartialEq
        // This ensures equal elements hash equal (correctness requirement)
        std::ptr::hash(std::rc::Rc::as_ptr(&self.state.inner), state);
        // Hash cursor color
        self.cursor_color.0.to_bits().hash(state);
        self.cursor_color.1.to_bits().hash(state);
        self.cursor_color.2.to_bits().hash(state);
        self.cursor_color.3.to_bits().hash(state);
        self.style.render_hash().hash(state);
        self.line_limits.hash(state);
    }
}

impl PartialEq for TextFieldElement {
    fn eq(&self, other: &Self) -> bool {
        // Compare by state identity (same Rc), cursor color, and line limits
        // This ensures node reuse when same state is passed, while detecting
        // actual changes that require updates
        self.state == other.state
            && self.style == other.style
            && self.cursor_color == other.cursor_color
            && self.line_limits == other.line_limits
    }
}

impl Eq for TextFieldElement {}

impl ModifierNodeElement for TextFieldElement {
    type Node = TextFieldModifierNode;

    fn create(&self) -> Self::Node {
        let mut node = TextFieldModifierNode::new(self.state.clone(), self.style.clone())
            .with_cursor_color(self.cursor_color)
            .with_line_limits(self.line_limits);
        if let Some(controller) = self.handle_controller.clone() {
            node = node.with_handle_controller(controller);
        }
        node
    }

    fn update(&self, node: &mut Self::Node) {
        // Update the state reference
        node.state = self.state.clone();
        node.style = self.style.clone();
        node.cursor_brush = Brush::solid(self.cursor_color);
        node.line_limits = self.line_limits;
        node.handle_controller = self.handle_controller.clone();

        // Recreate the cached handler with the new state but same refs
        node.cached_handler = TextFieldModifierNode::create_handler(
            node.state.clone(),
            node.refs.clone(),
            node.line_limits,
            self.style.clone(),
        );

        // Recreate the pan resolver so it captures the new state/style/limits
        node.cached_pan_resolver = TextFieldModifierNode::create_pan_resolver(
            node.state.clone(),
            node.refs.clone(),
            node.line_limits,
            self.style.clone(),
        );

        // Check if content changed and update cache
        if node.update_cached_state() {
            // Content changed - node will need layout/draw invalidation
            // This happens automatically through the modifier reconciliation
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
            | NodeCapabilities::DRAW
            | NodeCapabilities::SEMANTICS
            | NodeCapabilities::POINTER_INPUT
    }

    fn always_update(&self) -> bool {
        // Always update to capture new state/handler while preserving focus state
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextStyle;
    use cranpose_core::{DefaultScheduler, Runtime};
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
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

    // Regression: a selection (or preedit) whose logical line sits *below* a
    // soft-wrapped line must highlight on the correct VISUAL line. The old
    // logical-`\n` split placed it one line too high whenever a line above
    // wrapped — the reported "correct x, wrong y line" iOS selection bug.
    #[test]
    fn selection_rects_follow_wrapped_visual_lines() {
        let _app_context = crate::render_state::app_context_test_scope();
        // Monospaced test measurer: 14.0 * 0.6 = 8.4 px per char. Wrap width 30
        // fits 3 chars (25.2) but not 4 (33.6), so "aaaaa" wraps to "aaa"/"aa".
        let text = "aaaaa\nbb";
        let style = TextStyle::default();
        let line_height = 10.0_f32;

        // Select "bb" — logical line 1, but VISUAL line 2 (two visual lines
        // above it: "aaa", "aa").
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

        // A selection spanning the wrap boundary produces one rect per visual
        // line, at consecutive y positions.
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

    // Regression: a finger tap must resolve to the byte offset on the VISUAL
    // (wrapped) line under the finger. The measurer's plain get_offset_for_position
    // maps `y` through logical `\n` lines only, so on wrapped text the caret
    // landed below the finger — the reported "taps miss the y coordinate" bug.
    #[test]
    fn tap_resolves_offset_on_wrapped_visual_line() {
        let _app_context = crate::render_state::app_context_test_scope();
        // Same fixture: wrap width 30 splits "aaaaa" into "aaa"/"aa"; "bb" is the
        // third visual line. line_height 10 → line 2 spans y in [20, 30).
        let text = "aaaaa\nbb";
        let style = TextStyle::default();
        let line_height = 10.0_f32;

        // Tap on visual line 2 ("bb") must land in bytes 6..=8, not in the
        // wrapped first logical line.
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

        // Tap on visual line 1 (the "aa" continuation of the first logical line)
        // must land in bytes 3..=5.
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

        // Single-line (no wrap width): degrades to the one logical line.
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

    /// End-to-end guard for source-independent direct manipulation through the
    /// real pointer handler and draw closure. Keyboard-only focus keeps a clean
    /// caret; touch, mouse, and stylus presses publish handles and a continuous
    /// press stream for long-press → slide-to-menu.
    #[test]
    fn every_primary_pointer_source_publishes_direct_manipulation_metrics() {
        use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
        use cranpose_ui_graphics::Point;

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let state = TextFieldState::new("hello world");
            let controller = TextFieldHandleController::new();
            let mut node = TextFieldModifierNode::new(state.clone(), TextStyle::default())
                .with_handle_controller(controller.clone());
            // Give the field a measured size so the draw closure has geometry.
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

            node.set_focused(true);
            let _ = draw(size);
            let keyboard_metrics = controller
                .metrics()
                .expect("focused field publishes handle metrics");
            assert!(!keyboard_metrics.direct_manipulation);

            handler(
                PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Touch),
            );
            let _ = draw(size);
            let metrics = controller
                .metrics()
                .expect("focused field publishes handle metrics");
            assert!(metrics.focused, "a tap focuses the field");
            assert!(
                metrics.direct_manipulation,
                "a touch tap must expose direct-manipulation handles"
            );
            assert!(metrics.press.is_some(), "touch must publish the live press");

            handler(
                PointerEvent::new(PointerEventKind::Down, at, at).with_source(PointerSource::Mouse),
            );
            let _ = draw(size);
            let metrics = controller
                .metrics()
                .expect("focused field publishes handle metrics");
            assert!(
                metrics.direct_manipulation,
                "a mouse tap must expose the same direct-manipulation handles"
            );
            assert!(metrics.press.is_some(), "mouse must publish the live press");

            handler(
                PointerEvent::new(PointerEventKind::Down, at, at)
                    .with_source(PointerSource::Stylus),
            );
            let _ = draw(size);
            let metrics = controller
                .metrics()
                .expect("focused field publishes handle metrics");
            assert!(
                metrics.direct_manipulation,
                "a stylus contact must expose the same direct-manipulation handles"
            );
            assert!(
                metrics.press.is_some(),
                "stylus must publish the live press"
            );

            crate::text_field_focus::clear_focus();
        });
    }

    /// A double tap on a word must select that word. This regressed after
    /// selection handles began appearing inside `LazyColumn` items in 0.1.39:
    /// the cursor handle shown by the first tap overlapped the text line and
    /// consumed the second tap. The field's own gesture classification (proven
    /// here) is correct — two quick taps at the same spot escalate to a word
    /// selection — so the fix is geometric (keep the handle's touch box off the
    /// text line; see `selection_handle::handle_shape`).
    #[test]
    fn double_tap_selects_the_word_under_the_finger() {
        use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
        use cranpose_ui_graphics::Point;

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let state = TextFieldState::new("hello world");
            let node = TextFieldModifierNode::new(state.clone(), TextStyle::default());
            node.measured_size.set(Size {
                width: 200.0,
                height: 20.0,
            });
            let handler = node
                .pointer_input_handler()
                .expect("field exposes a pointer handler");

            // Two touch taps at the same spot, back to back (well within the
            // multi-tap timeout and slop): near the start of "hello".
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

    /// The multi-tap selection granularity ladder (bug 8): repeated in-place taps
    /// escalate word → line → paragraph, then cycle back to word. Mirrors mature
    /// editors (Android `TextView`, iOS, VS Code).
    #[test]
    fn repeated_taps_escalate_word_line_paragraph_then_cycle() {
        use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
        use cranpose_ui_graphics::Point;

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            // Two lines in the first paragraph, a blank line, then a second
            // paragraph — so line and paragraph selections differ.
            let text = "alpha beta\ngamma delta\n\nsecond para";
            let state = TextFieldState::new(text);
            let node = TextFieldModifierNode::new(state.clone(), TextStyle::default())
                .with_line_limits(TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: usize::MAX,
                });
            node.measured_size.set(Size {
                width: 400.0,
                height: 80.0,
            });
            let handler = node
                .pointer_input_handler()
                .expect("field exposes a pointer handler");

            // Tap in place on the first line ("alpha").
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

            tap(); // 1 → caret
            assert!(state.selection().collapsed(), "first tap places the caret");
            tap(); // 2 → word
            assert_eq!(selected(&state), "alpha", "double tap selects the word");
            tap(); // 3 → line
            assert_eq!(
                selected(&state),
                "alpha beta",
                "triple tap selects the line"
            );
            tap(); // 4 → paragraph
            assert_eq!(
                selected(&state),
                "alpha beta\ngamma delta",
                "fourth tap grows to the paragraph"
            );
            tap(); // 5 → cycles back to word
            assert_eq!(
                selected(&state),
                "alpha",
                "fifth tap cycles back to the word"
            );

            crate::text_field_focus::clear_focus();
        });
    }

    /// A single tap that lands inside an existing selection re-grabs the word
    /// under the finger (Android/iOS behaviour), rather than collapsing to a
    /// caret (bug 8).
    #[test]
    fn single_tap_inside_selection_selects_the_word() {
        use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
        use cranpose_ui_graphics::Point;

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let state = TextFieldState::new("hello world");
            let node = TextFieldModifierNode::new(state.clone(), TextStyle::default());
            node.measured_size.set(Size {
                width: 200.0,
                height: 20.0,
            });
            let handler = node
                .pointer_input_handler()
                .expect("field exposes a pointer handler");

            // Pre-existing broad selection over the whole text.
            state.edit(|buffer| buffer.select(TextRange::new(0, 11)));
            assert!(!state.selection().collapsed());

            // A lone tap over "hello" (fresh tap count) must select that word,
            // not drop the selection.
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

    /// Bug (c) at the handler level: repeated taps at the SAME spot inside an
    /// existing selection climb the granularity ladder word → line → paragraph →
    /// word even when each tap arrives after the multi-tap timeout has lapsed
    /// (the growth is keyed on location, not the double-tap timer). Forcing a
    /// timeout between taps (clearing `last_click_time`) makes the raw tap count
    /// reset to 1 each time, so this exercises the location-based path rather
    /// than the rapid-multi-tap path.
    #[test]
    fn slow_taps_inside_selection_cycle_word_line_paragraph_by_location() {
        use cranpose_foundation::{PointerEvent, PointerEventKind, PointerSource};
        use cranpose_ui_graphics::Point;

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let text = "alpha beta\ngamma delta\n\nsecond para";
            let state = TextFieldState::new(text);
            let node = TextFieldModifierNode::new(state.clone(), TextStyle::default())
                .with_line_limits(TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: usize::MAX,
                });
            node.measured_size.set(Size {
                width: 400.0,
                height: 80.0,
            });
            let handler = node
                .pointer_input_handler()
                .expect("field exposes a pointer handler");

            // A broad pre-existing selection over the whole text.
            state.edit(|buffer| buffer.select(TextRange::new(0, text.len())));

            let at = Point { x: 2.0, y: 4.0 };
            let selected = |state: &TextFieldState| {
                let s = state.selection();
                state.text()[s.min()..s.max()].to_string()
            };
            // Each call forces the multi-tap timer to look expired, so the raw
            // tap count resets to 1 while the tap position stays put.
            let slow_tap = || {
                node.refs.last_click_time.set(None);
                handler(
                    PointerEvent::new(PointerEventKind::Down, at, at)
                        .with_source(PointerSource::Touch),
                );
            };

            slow_tap(); // inside selection → word
            assert_eq!(
                selected(&state),
                "alpha",
                "tap inside selection grabs the word"
            );
            slow_tap(); // same spot → line
            assert_eq!(
                selected(&state),
                "alpha beta",
                "same-spot tap grows to the line even after the timeout"
            );
            slow_tap(); // same spot → paragraph
            assert_eq!(
                selected(&state),
                "alpha beta\ngamma delta",
                "same-spot tap grows to the paragraph"
            );
            slow_tap(); // same spot → cycles back to word
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
            let state2 = TextFieldState::new("Hello"); // Different Rc, same text

            let elem1 = TextFieldElement::new(state1.clone(), TextStyle::default());
            let elem2 = TextFieldElement::new(state1.clone(), TextStyle::default()); // Same state (Rc identity)
            let elem3 = TextFieldElement::new(state2, TextStyle::default()); // Different state

            // Elements are equal only when they share the same state Rc
            // This ensures proper Eq/Hash contract compliance
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
            let initial = TextFieldElement::new(state.clone(), dark_style);
            let updated = TextFieldElement::new(state, light_style.clone());
            let mut node = initial.create();

            updated.update(&mut node);

            assert_eq!(node.text(), "themed text");
            assert_eq!(node.style(), &light_style);
        });
    }

    /// A multi-line field must measure the *wrapped* height at the available
    /// width, so a long transcript grows the field instead of being clipped to
    /// a single line. Regression for the "edits only appear after focus loss"
    /// bug where a wrapped OCR transcript rendered only its first line.
    #[test]
    fn multiline_field_measures_wrapped_height() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let long = "abcd ".repeat(40); // ~200 chars, no explicit newlines
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

    /// Single-line fields pan horizontally instead of wrapping, so they never
    /// derive a wrap width even under a narrow constraint.
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

    /// Test that cursor draw command position is calculated correctly.
    ///
    /// This test verifies that when we measure text width for cursor position:
    /// 1. The cursor x position = width of text before cursor
    /// 2. For text at cursor end, x = full text width
    #[test]
    fn test_cursor_x_position_calculation() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            // Test that text measurement works correctly for cursor positioning
            let style = crate::text::TextStyle::default();

            // Empty text - cursor should be at x=0
            let empty_width =
                crate::text::measure_text(&crate::text::AnnotatedString::from(""), &style).width;
            assert!(
                empty_width.abs() < 0.1,
                "Empty text should have 0 width, got {}",
                empty_width
            );

            // Non-empty text - cursor at end should be at text width
            let hi_width =
                crate::text::measure_text(&crate::text::AnnotatedString::from("Hi"), &style).width;
            assert!(
                hi_width > 0.0,
                "Text 'Hi' should have positive width: {}",
                hi_width
            );

            // Partial text - cursor after 'H' should be at width of 'H'
            let h_width =
                crate::text::measure_text(&crate::text::AnnotatedString::from("H"), &style).width;
            assert!(h_width > 0.0, "Text 'H' should have positive width");
            assert!(
                h_width < hi_width,
                "'H' width {} should be less than 'Hi' width {}",
                h_width,
                hi_width
            );

            // Verify TextFieldState selection tracks cursor correctly
            let state = TextFieldState::new("Hi");
            assert_eq!(
                state.selection().start,
                2,
                "Cursor should be at position 2 (end of 'Hi')"
            );

            // The text before cursor at position 2 in "Hi" is "Hi" itself
            let text = state.text();
            let cursor_pos = state.selection().start;
            let text_before_cursor = &text[..cursor_pos.min(text.len())];
            assert_eq!(text_before_cursor, "Hi");

            // So cursor x = width of "Hi"
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

    /// Test cursor is created when focused node is in slices.
    #[test]
    fn test_focused_node_creates_cursor() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let state = TextFieldState::new("Test");
            let element = TextFieldElement::new(state.clone(), TextStyle::default());
            let node = element.create();

            // Initially not focused
            assert!(!node.is_focused());

            // Set focus
            *node.refs.is_focused.borrow_mut() = true;
            assert!(node.is_focused());

            // Verify the node has correct text
            assert_eq!(node.text(), "Test");

            // Verify selection is at end
            assert_eq!(node.selection().start, 4);
        });
    }
}

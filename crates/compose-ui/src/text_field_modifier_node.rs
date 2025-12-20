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

use compose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};
use compose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    SemanticsConfiguration, SemanticsNode, Size,
};
use compose_ui_graphics::{Brush, Color};
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Default cursor color (white - visible on dark backgrounds)
const DEFAULT_CURSOR_COLOR: Color = Color(1.0, 1.0, 1.0, 1.0);

/// Default selection highlight color (light blue with transparency)
const DEFAULT_SELECTION_COLOR: Color = Color(0.0, 0.5, 1.0, 0.3);

/// Double-click timeout in milliseconds
const DOUBLE_CLICK_MS: u128 = 500;

/// Default line height for empty text fields
const DEFAULT_LINE_HEIGHT: f32 = 20.0;

/// Cursor width in pixels
const CURSOR_WIDTH: f32 = 2.0;

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
    pub last_click_time: Rc<Cell<Option<instant::Instant>>>,
    /// Click count (1=single, 2=double, 3=triple)
    pub click_count: Rc<Cell<u8>>,
    /// Node ID for scoped layout invalidation
    pub node_id: Rc<Cell<Option<compose_core::NodeId>>>,
}

impl TextFieldRefs {
    /// Creates a new set of shared references.
    pub fn new() -> Self {
        Self {
            is_focused: Rc::new(RefCell::new(false)),
            content_offset: Rc::new(Cell::new(0.0_f32)),
            content_y_offset: Rc::new(Cell::new(0.0_f32)),
            drag_anchor: Rc::new(Cell::new(None::<usize>)),
            last_click_time: Rc::new(Cell::new(None::<instant::Instant>)),
            click_count: Rc::new(Cell::new(0_u8)),
            node_id: Rc::new(Cell::new(None::<compose_core::NodeId>)),
        }
    }
}

/// Modifier node for editable text fields.
///
/// This node is the core of `BasicTextField`, handling:
/// - Text measurement and layout
/// - Cursor and selection rendering
/// - Pointer input for cursor positioning
pub struct TextFieldModifierNode {
    /// The text field state (shared)
    state: TextFieldState,
    /// Shared references for input handling
    refs: TextFieldRefs,
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
    /// Measured size cache
    measured_size: Cell<Size>,
    /// Cached pointer input handler
    cached_handler: Rc<dyn Fn(PointerEvent)>,
}

impl std::fmt::Debug for TextFieldModifierNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldModifierNode")
            .field("text", &self.state.text())
            .field("is_focused", &*self.refs.is_focused.borrow())
            .finish()
    }
}

// Re-export from extracted module
use crate::text_field_handler::TextFieldHandler;

impl TextFieldModifierNode {
    /// Creates a new text field modifier node.
    pub fn new(state: TextFieldState) -> Self {
        let value = state.value();
        let refs = TextFieldRefs::new();
        let line_limits = TextFieldLineLimits::default();
        let cached_handler = Self::create_handler(state.clone(), refs.clone(), line_limits);
        
        Self {
            state,
            refs,
            cursor_brush: Brush::solid(DEFAULT_CURSOR_COLOR),
            selection_brush: Brush::solid(DEFAULT_SELECTION_COLOR),
            line_limits,
            cached_text: value.text,
            cached_selection: value.selection,
            node_state: NodeState::new(),
            measured_size: Cell::new(Size {
                width: 0.0,
                height: 0.0,
            }),
            cached_handler,
        }
    }
    
    /// Creates a node with custom line limits.
    pub fn with_line_limits(mut self, line_limits: TextFieldLineLimits) -> Self {
        self.line_limits = line_limits;
        self
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
    ) -> Rc<dyn Fn(PointerEvent)> {
        // Use word_boundaries module for double-click word selection
        use crate::word_boundaries::find_word_boundaries;
        
        Rc::new(move |event: PointerEvent| {
            // Account for content padding offsets
            let click_x = (event.position.x - refs.content_offset.get()).max(0.0);
            let click_y = (event.position.y - refs.content_y_offset.get()).max(0.0);
            
            match event.kind {
                PointerEventKind::Down => {
                    // Request focus with O(1) handler, passing node_id and line_limits for key handling
                    let handler = TextFieldHandler::new(state.clone(), refs.node_id.get(), line_limits);
                    crate::text_field_focus::request_focus(refs.is_focused.clone(), handler);
                    
                    let now = instant::Instant::now();
                    let text = state.text();
                    let pos = crate::text::get_offset_for_position(&text, click_x, click_y);
                    
                    // Detect double-click
                    let is_double_click = if let Some(last) = refs.last_click_time.get() {
                        now.duration_since(last).as_millis() < DOUBLE_CLICK_MS
                    } else {
                        false
                    };
                    
                    if is_double_click {
                        // Increment click count for potential triple-click
                        let count = refs.click_count.get() + 1;
                        refs.click_count.set(count.min(3));
                        
                        if count >= 3 {
                            // Triple-click: select all
                            state.edit(|buffer| {
                                buffer.select_all();
                            });
                            // Set drag anchor to start for select-all drag
                            refs.drag_anchor.set(Some(0));
                        } else if count >= 2 {
                            // Double-click: select word
                            let (word_start, word_end) = find_word_boundaries(&text, pos);
                            state.edit(|buffer| {
                                buffer.select(TextRange::new(word_start, word_end));
                            });
                            // Set drag anchor to word boundaries for word-extend drag
                            refs.drag_anchor.set(Some(word_start));
                        }
                    } else {
                        // Single click: reset click count, place cursor
                        refs.click_count.set(1);
                        refs.drag_anchor.set(Some(pos));
                        state.edit(|buffer| {
                            buffer.place_cursor_before_char(pos);
                        });
                    }
                    
                    refs.last_click_time.set(Some(now));
                    event.consume();
                }
                PointerEventKind::Move => {
                    // If we have a drag anchor, extend selection during drag
                    if let Some(anchor) = refs.drag_anchor.get() {
                        if *refs.is_focused.borrow() {
                            let text = state.text();
                            let current_pos = crate::text::get_offset_for_position(&text, click_x, click_y);
                            
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
                }
                _ => {}
            }
        })
    }

    /// Creates a node with custom cursor color.
    pub fn with_cursor_color(mut self, color: Color) -> Self {
        self.cursor_brush = Brush::solid(color);
        self
    }

    /// Sets the focus state.
    pub fn set_focused(&mut self, focused: bool) {
        let current = *self.refs.is_focused.borrow();
        if current != focused {
            *self.refs.is_focused.borrow_mut() = focused;
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

    /// Returns the current text.
    pub fn text(&self) -> String {
        self.state.text()
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
    pub fn get_state(&self) -> compose_foundation::text::TextFieldState {
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

    /// Measures the text content.
    fn measure_text_content(&self) -> Size {
        let text = self.state.text();
        let metrics = crate::text::measure_text(&text);
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

        // Use proper text layout hit testing instead of character-based calculation
        let byte_offset = crate::text::get_offset_for_position(&text, x_offset, 0.0);

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
    ) -> compose_ui_layout::LayoutModifierMeasureResult {
        // Measure the text content
        let text_size = self.measure_text_content();

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

        compose_ui_layout::LayoutModifierMeasureResult::with_size(size)
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content().width
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content().width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content().height.max(DEFAULT_LINE_HEIGHT)
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content().height.max(DEFAULT_LINE_HEIGHT)
    }
}

impl DrawModifierNode for TextFieldModifierNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // No-op: Cursor and selection are rendered via create_draw_closure() which 
        // creates DrawPrimitive::Rect directly. This enables draw-time evaluation
        // of focus state and cursor blink timing.
    }
    
    fn create_draw_closure(&self) -> Option<Rc<dyn Fn(compose_foundation::Size) -> Vec<compose_ui_graphics::DrawPrimitive>>> {
        use compose_ui_graphics::DrawPrimitive;
        
        // Capture state via Rc clone (cheap) for draw-time evaluation
        let is_focused = self.refs.is_focused.clone();
        let state = self.state.clone();
        let content_offset = self.refs.content_offset.clone();
        let content_y_offset = self.refs.content_y_offset.clone();
        let cursor_brush = self.cursor_brush.clone();
        let selection_brush = self.selection_brush.clone();
        
        Some(Rc::new(move |_size| {
            // Check focus at DRAW time
            if !*is_focused.borrow() {
                return vec![];
            }
            
            let mut primitives = Vec::new();
            
            let text = state.text();
            let selection = state.selection();
            let padding_left = content_offset.get();
            let padding_top = content_y_offset.get();
            let line_height = crate::text::measure_text(&text).line_height;
            
            // Draw selection highlight
            if !selection.collapsed() {
                let sel_start = selection.min();
                let sel_end = selection.max();
                
                let lines: Vec<&str> = text.split('\n').collect();
                let mut byte_offset: usize = 0;
                
                for (line_idx, line) in lines.iter().enumerate() {
                    let line_start = byte_offset;
                    let line_end = byte_offset + line.len();
                    
                    if sel_end > line_start && sel_start < line_end {
                        let sel_start_in_line = sel_start.saturating_sub(line_start);
                        let sel_end_in_line = (sel_end - line_start).min(line.len());
                        
                        let sel_start_x = crate::text::measure_text(&line[..sel_start_in_line]).width + padding_left;
                        let sel_end_x = crate::text::measure_text(&line[..sel_end_in_line]).width + padding_left;
                        let sel_width = sel_end_x - sel_start_x;
                        
                        if sel_width > 0.0 {
                            let sel_rect = compose_ui_graphics::Rect {
                                x: sel_start_x,
                                y: padding_top + line_idx as f32 * line_height,
                                width: sel_width,
                                height: line_height,
                            };
                            primitives.push(DrawPrimitive::Rect { rect: sel_rect, brush: selection_brush.clone() });
                        }
                    }
                    byte_offset = line_end + 1;
                }
            }
            
            // Draw composition (IME preedit) underline
            // This shows the user which text is being composed by the input method
            if let Some(comp_range) = state.composition() {
                let comp_start = comp_range.min();
                let comp_end = comp_range.max();
                
                if comp_start < comp_end && comp_end <= text.len() {
                    let lines: Vec<&str> = text.split('\n').collect();
                    let mut byte_offset: usize = 0;
                    
                    // Underline color: slightly transparent white/gray
                    let underline_brush = compose_ui_graphics::Brush::solid(
                        compose_ui_graphics::Color(0.8, 0.8, 0.8, 0.8)
                    );
                    let underline_height: f32 = 2.0;
                    
                    for (line_idx, line) in lines.iter().enumerate() {
                        let line_start = byte_offset;
                        let line_end = byte_offset + line.len();
                        
                        // Check if composition overlaps this line
                        if comp_end > line_start && comp_start < line_end {
                            let comp_start_in_line = comp_start.saturating_sub(line_start);
                            let comp_end_in_line = (comp_end - line_start).min(line.len());
                            
                            // Clamp to valid UTF-8 boundaries
                            let comp_start_in_line = if line.is_char_boundary(comp_start_in_line) {
                                comp_start_in_line
                            } else {
                                0
                            };
                            let comp_end_in_line = if line.is_char_boundary(comp_end_in_line) {
                                comp_end_in_line
                            } else {
                                line.len()
                            };
                            
                            let comp_start_x = crate::text::measure_text(&line[..comp_start_in_line]).width + padding_left;
                            let comp_end_x = crate::text::measure_text(&line[..comp_end_in_line]).width + padding_left;
                            let comp_width = comp_end_x - comp_start_x;
                            
                            if comp_width > 0.0 {
                                // Draw underline at the bottom of the text line
                                let underline_rect = compose_ui_graphics::Rect {
                                    x: comp_start_x,
                                    y: padding_top + (line_idx as f32 + 1.0) * line_height - underline_height,
                                    width: comp_width,
                                    height: underline_height,
                                };
                                primitives.push(DrawPrimitive::Rect { 
                                    rect: underline_rect, 
                                    brush: underline_brush.clone() 
                                });
                            }
                        }
                        byte_offset = line_end + 1;
                    }
                }
            }
            
            // Draw cursor - check visibility at DRAW time for blinking
            if crate::cursor_animation::is_cursor_visible() {
                
                let pos = selection.start.min(text.len());
                let text_before = &text[..pos];
                let line_index = text_before.matches('\n').count();
                let line_start = text_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let cursor_x = crate::text::measure_text(&text_before[line_start..]).width + padding_left;
                let cursor_y = padding_top + line_index as f32 * line_height;
                
                let cursor_rect = compose_ui_graphics::Rect {
                    x: cursor_x,
                    y: cursor_y,
                    width: CURSOR_WIDTH,
                    height: line_height,
                };
                
                primitives.push(DrawPrimitive::Rect { rect: cursor_rect, brush: cursor_brush.clone() });
            }
            
            primitives
        }))
    }
}

impl SemanticsNode for TextFieldModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        let text = self.state.text();
        config.content_description = Some(text);
        // TODO: Add editable text semantics properties
        // - is_editable = true
        // - text_selection_range = self.state.selection()
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
    /// Cursor color
    cursor_color: Color,
    /// Line limits configuration
    line_limits: TextFieldLineLimits,
}

impl TextFieldElement {
    /// Creates a new text field element.
    pub fn new(state: TextFieldState) -> Self {
        Self {
            state,
            cursor_color: DEFAULT_CURSOR_COLOR,
            line_limits: TextFieldLineLimits::default(),
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
}

impl std::fmt::Debug for TextFieldElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldElement")
            .field("text", &self.state.text())
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
    }
}

impl PartialEq for TextFieldElement {
    fn eq(&self, other: &Self) -> bool {
        // Compare by state identity (same Rc), cursor color, and line limits
        // This ensures node reuse when same state is passed, while detecting
        // actual changes that require updates
        self.state == other.state 
            && self.cursor_color == other.cursor_color
            && self.line_limits == other.line_limits
    }
}

impl Eq for TextFieldElement {}

impl ModifierNodeElement for TextFieldElement {
    type Node = TextFieldModifierNode;

    fn create(&self) -> Self::Node {
        TextFieldModifierNode::new(self.state.clone())
            .with_cursor_color(self.cursor_color)
            .with_line_limits(self.line_limits)
    }

    fn update(&self, node: &mut Self::Node) {
        // Update the state reference
        node.state = self.state.clone();
        node.cursor_brush = Brush::solid(self.cursor_color);
        node.line_limits = self.line_limits;
        
        // Recreate the cached handler with the new state but same refs
        node.cached_handler = TextFieldModifierNode::create_handler(
            node.state.clone(),
            node.refs.clone(),
            node.line_limits,
        );

        // Check if content changed and update cache
        if node.update_cached_state() {
            // Content changed - node will need layout/draw invalidation
            // This happens automatically through the modifier reconciliation
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS | NodeCapabilities::POINTER_INPUT
    }

    fn always_update(&self) -> bool {
        // Always update to capture new state/handler while preserving focus state
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose_core::{DefaultScheduler, Runtime};
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    #[test]
    fn text_field_node_creation() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            let node = TextFieldModifierNode::new(state);
            assert_eq!(node.text(), "Hello");
            assert!(!node.is_focused());
        });
    }

    #[test]
    fn text_field_node_focus() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Test");
            let mut node = TextFieldModifierNode::new(state);
            assert!(!node.is_focused());

            node.set_focused(true);
            assert!(node.is_focused());

            node.set_focused(false);
            assert!(!node.is_focused());
        });
    }

    #[test]
    fn text_field_element_creates_node() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello World");
            let element = TextFieldElement::new(state);

            let node = element.create();
            assert_eq!(node.text(), "Hello World");
        });
    }

    #[test]
    fn text_field_element_equality() {
        with_test_runtime(|| {
            let state1 = TextFieldState::new("Hello");
            let state2 = TextFieldState::new("Hello"); // Different Rc, same text
            
            let elem1 = TextFieldElement::new(state1.clone());
            let elem2 = TextFieldElement::new(state1.clone()); // Same state (Rc identity)
            let elem3 = TextFieldElement::new(state2);         // Different state

            // Elements are equal only when they share the same state Rc
            // This ensures proper Eq/Hash contract compliance
            assert_eq!(elem1, elem2, "Same state should be equal");
            assert_ne!(elem1, elem3, "Different states should not be equal");
        });
    }

    /// Test that cursor draw command position is calculated correctly.
    /// 
    /// This test verifies that when we measure text width for cursor position:
    /// 1. The cursor x position = width of text before cursor
    /// 2. For text at cursor end, x = full text width
    #[test]
    fn test_cursor_x_position_calculation() {
        with_test_runtime(|| {
            // Test that text measurement works correctly for cursor positioning
            
            // Empty text - cursor should be at x=0
            let empty_width = crate::text::measure_text("").width;
            assert!(empty_width.abs() < 0.1, "Empty text should have 0 width, got {}", empty_width);
            
            // Non-empty text - cursor at end should be at text width
            let hi_width = crate::text::measure_text("Hi").width;
            assert!(hi_width > 0.0, "Text 'Hi' should have positive width: {}", hi_width);
            
            // Partial text - cursor after 'H' should be at width of 'H'
            let h_width = crate::text::measure_text("H").width;
            assert!(h_width > 0.0, "Text 'H' should have positive width");
            assert!(h_width < hi_width, "'H' width {} should be less than 'Hi' width {}", h_width, hi_width);
            
            // Verify TextFieldState selection tracks cursor correctly
            let state = TextFieldState::new("Hi");
            assert_eq!(state.selection().start, 2, "Cursor should be at position 2 (end of 'Hi')");
            
            // The text before cursor at position 2 in "Hi" is "Hi" itself
            let text = state.text();
            let cursor_pos = state.selection().start;
            let text_before_cursor = &text[..cursor_pos.min(text.len())];
            assert_eq!(text_before_cursor, "Hi");
            
            // So cursor x = width of "Hi"
            let cursor_x = crate::text::measure_text(text_before_cursor).width;
            assert!((cursor_x - hi_width).abs() < 0.1, 
                "Cursor x {} should equal 'Hi' width {}", cursor_x, hi_width);
        });
    }



    /// Test cursor is created when focused node is in slices.
    #[test]  
    fn test_focused_node_creates_cursor() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Test");
            let element = TextFieldElement::new(state.clone());
            let mut node = element.create();
            
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


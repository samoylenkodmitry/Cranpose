//! Concrete implementations of modifier nodes for common modifiers.
//!
//! This module provides actual implementations of layout and draw modifier nodes
//! that can be used instead of the value-based ModOp system. These nodes follow
//! the Modifier.Node architecture from the roadmap.
//!
//! # Overview
//!
//! The Modifier.Node system provides better performance than value-based modifiers by:
//! - Reusing node instances across recompositions (zero allocations when stable)
//! - Targeted invalidation (only affected phases like layout/draw are invalidated)
//! - Lifecycle hooks (on_attach, on_detach, update) for efficient state management
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use compose_foundation::{modifier_element, ModifierNodeChain, BasicModifierNodeContext};
//! use compose_ui::{PaddingElement, EdgeInsets};
//!
//! let mut chain = ModifierNodeChain::new();
//! let mut context = BasicModifierNodeContext::new();
//!
//! // Create a padding modifier element
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(16.0)))];
//!
//! // Reconcile the chain (attaches new nodes, reuses existing)
//! chain.update_from_slice(&elements, &mut context);
//!
//! // Update with different padding - reuses the same node instance
//! let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(24.0)))];
//! chain.update_from_slice(&elements, &mut context);
//! // Zero allocations on this update!
//! ```
//!
//! # Available Nodes
//!
//! - [`PaddingNode`] / [`PaddingElement`]: Adds padding around content (layout)
//! - [`BackgroundNode`] / [`BackgroundElement`]: Draws a background color (draw)
//! - [`SizeNode`] / [`SizeElement`]: Enforces specific dimensions (layout)
//! - [`ClickableNode`] / [`ClickableElement`]: Handles click/tap interactions (pointer input)
//! - [`AlphaNode`] / [`AlphaElement`]: Applies alpha transparency (draw)
//!
//! # Integration with Value-Based Modifiers
//!
//! Currently, both systems coexist. The value-based `Modifier` API (ModOp enum)
//! is still the primary public API. The node-based system provides an alternative
//! implementation path that will eventually replace value-based modifiers once
//! the migration is complete.

use compose_foundation::{
    Constraints, DrawModifierNode, DrawScope, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, PointerEvent, PointerEventKind,
    PointerInputNode, Size,
};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::modifier::{Color, EdgeInsets, Point, RoundedCornerShape};

fn hash_f32_value<H: Hasher>(state: &mut H, value: f32) {
    state.write_u32(value.to_bits());
}

fn hash_option_f32<H: Hasher>(state: &mut H, value: Option<f32>) {
    match value {
        Some(v) => {
            state.write_u8(1);
            hash_f32_value(state, v);
        }
        None => state.write_u8(0),
    }
}

// ============================================================================
// Padding Modifier Node
// ============================================================================

/// Node that adds padding around its content.
#[derive(Debug)]
pub struct PaddingNode {
    padding: EdgeInsets,
}

impl PaddingNode {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }

    pub fn padding(&self) -> EdgeInsets {
        self.padding
    }
}

impl ModifierNode for PaddingNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }
}

impl LayoutModifierNode for PaddingNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> Size {
        // Convert padding to floating point values
        let horizontal_padding = self.padding.horizontal_sum();
        let vertical_padding = self.padding.vertical_sum();

        // Subtract padding from available space
        let inner_constraints = Constraints {
            min_width: (constraints.min_width - horizontal_padding).max(0.0),
            max_width: (constraints.max_width - horizontal_padding).max(0.0),
            min_height: (constraints.min_height - vertical_padding).max(0.0),
            max_height: (constraints.max_height - vertical_padding).max(0.0),
        };

        // Measure the wrapped content
        let inner_placeable = measurable.measure(inner_constraints);
        let inner_width = inner_placeable.width();
        let inner_height = inner_placeable.height();

        // Add padding back to the result
        Size {
            width: inner_width + horizontal_padding,
            height: inner_height + vertical_padding,
        }
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.min_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        let vertical_padding = self.padding.vertical_sum();
        let inner_height = (height - vertical_padding).max(0.0);
        let inner_width = measurable.max_intrinsic_width(inner_height);
        inner_width + self.padding.horizontal_sum()
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.min_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        let horizontal_padding = self.padding.horizontal_sum();
        let inner_width = (width - horizontal_padding).max(0.0);
        let inner_height = measurable.max_intrinsic_height(inner_width);
        inner_height + self.padding.vertical_sum()
    }
}

/// Element that creates and updates padding nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl Hash for PaddingElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.padding.left);
        hash_f32_value(state, self.padding.top);
        hash_f32_value(state, self.padding.right);
        hash_f32_value(state, self.padding.bottom);
    }
}

impl ModifierNodeElement for PaddingElement {
    type Node = PaddingNode;

    fn create(&self) -> Self::Node {
        PaddingNode::new(self.padding)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.padding != self.padding {
            node.padding = self.padding;
            // Note: In a full implementation, we would invalidate layout here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Background Modifier Node
// ============================================================================

/// Node that draws a background behind its content.
#[derive(Debug)]
pub struct BackgroundNode {
    color: Color,
    shape: Option<RoundedCornerShape>,
}

impl BackgroundNode {
    pub fn new(color: Color) -> Self {
        Self { color, shape: None }
    }

    pub fn color(&self) -> Color {
        self.color
    }

    pub fn shape(&self) -> Option<RoundedCornerShape> {
        self.shape
    }
}

impl ModifierNode for BackgroundNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for BackgroundNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would draw the background color
        // using the draw scope. For now, this is a placeholder.
        // The actual drawing happens in the renderer which reads node state.
    }
}

/// Element that creates and updates background nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl Hash for BackgroundElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.color.0);
        hash_f32_value(state, self.color.1);
        hash_f32_value(state, self.color.2);
        hash_f32_value(state, self.color.3);
    }
}

impl ModifierNodeElement for BackgroundElement {
    type Node = BackgroundNode;

    fn create(&self) -> Self::Node {
        BackgroundNode::new(self.color)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.color != self.color {
            node.color = self.color;
            // Note: In a full implementation, we would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Size Modifier Node
// ============================================================================

/// Node that tracks the latest rounded corner shape.
#[derive(Debug)]
pub struct CornerShapeNode {
    shape: RoundedCornerShape,
}

impl CornerShapeNode {
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self { shape }
    }

    pub fn shape(&self) -> RoundedCornerShape {
        self.shape
    }
}

impl ModifierNode for CornerShapeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for CornerShapeNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {}
}

/// Element that creates and updates corner shape nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct CornerShapeElement {
    shape: RoundedCornerShape,
}

impl CornerShapeElement {
    pub fn new(shape: RoundedCornerShape) -> Self {
        Self { shape }
    }
}

impl Hash for CornerShapeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let radii = self.shape.radii();
        hash_f32_value(state, radii.top_left);
        hash_f32_value(state, radii.top_right);
        hash_f32_value(state, radii.bottom_right);
        hash_f32_value(state, radii.bottom_left);
    }
}

impl ModifierNodeElement for CornerShapeElement {
    type Node = CornerShapeNode;

    fn create(&self) -> Self::Node {
        CornerShapeNode::new(self.shape)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.shape != self.shape {
            node.shape = self.shape;
            // Invalidations are handled lazily for now.
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

// ============================================================================
// Size Modifier Node
// ============================================================================

/// Node that enforces a specific size on its content.
#[derive(Debug)]
pub struct SizeNode {
    width: Option<f32>,
    height: Option<f32>,
}

impl SizeNode {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }
}

impl ModifierNode for SizeNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }
}

impl LayoutModifierNode for SizeNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> Size {
        // Override constraints with explicit sizes if specified
        let width = self
            .width
            .map(|value| value.clamp(constraints.min_width, constraints.max_width));
        let height = self
            .height
            .map(|value| value.clamp(constraints.min_height, constraints.max_height));

        let inner_constraints = Constraints {
            min_width: width.unwrap_or(constraints.min_width),
            max_width: width.unwrap_or(constraints.max_width),
            min_height: height.unwrap_or(constraints.min_height),
            max_height: height.unwrap_or(constraints.max_height),
        };

        // Measure wrapped content with size constraints
        let placeable = measurable.measure(inner_constraints);
        let measured_width = placeable.width();
        let measured_height = placeable.height();

        // Return the specified size or the measured size when not overridden
        Size {
            width: width.unwrap_or(measured_width),
            height: height.unwrap_or(measured_height),
        }
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.width.unwrap_or(0.0)
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.width.unwrap_or(f32::INFINITY)
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.height.unwrap_or(0.0)
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.height.unwrap_or(f32::INFINITY)
    }
}

/// Element that creates and updates size nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeElement {
    width: Option<f32>,
    height: Option<f32>,
}

impl SizeElement {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }
}

impl Hash for SizeElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_option_f32(state, self.width);
        hash_option_f32(state, self.height);
    }
}

impl ModifierNodeElement for SizeElement {
    type Node = SizeNode;

    fn create(&self) -> Self::Node {
        SizeNode::new(self.width, self.height)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.width != self.width || node.height != self.height {
            node.width = self.width;
            node.height = self.height;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}

// ============================================================================
// Clickable Modifier Node
// ============================================================================

/// Node that handles click/tap interactions.
pub struct ClickableNode {
    on_click: Rc<dyn Fn(Point)>,
}

impl std::fmt::Debug for ClickableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableNode").finish()
    }
}

impl ClickableNode {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_click: Rc::new(on_click),
        }
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self { on_click }
    }
}

impl ModifierNode for ClickableNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::PointerInput);
    }
}

impl PointerInputNode for ClickableNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
    ) -> bool {
        if matches!(event.kind, PointerEventKind::Down) {
            let point = Point {
                x: event.position.x,
                y: event.position.y,
            };
            (self.on_click)(point);
            true
        } else {
            false
        }
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        // Always participate in hit testing
        true
    }
}

/// Element that creates and updates clickable nodes.
#[derive(Clone)]
pub struct ClickableElement {
    on_click: Rc<dyn Fn(Point)>,
}

impl ClickableElement {
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_click: Rc::new(on_click),
        }
    }

    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self { on_click }
    }
}

impl std::fmt::Debug for ClickableElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableElement").finish()
    }
}

impl PartialEq for ClickableElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.on_click, &other.on_click)
    }
}

impl Eq for ClickableElement {}

impl Hash for ClickableElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let ptr = Rc::as_ptr(&self.on_click) as *const ();
        (ptr as usize).hash(state);
    }
}

impl ModifierNodeElement for ClickableElement {
    type Node = ClickableNode;

    fn create(&self) -> Self::Node {
        ClickableNode::with_handler(self.on_click.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        // Update the handler
        node.on_click = self.on_click.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

// ============================================================================
// Alpha Modifier Node
// ============================================================================

/// Node that applies alpha transparency to its content.
#[derive(Debug)]
pub struct AlphaNode {
    alpha: f32,
}

impl AlphaNode {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl ModifierNode for AlphaNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for AlphaNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Save the current alpha/layer state
        // 2. Apply the alpha value to the graphics context
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous state
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates alpha nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct AlphaElement {
    alpha: f32,
}

impl AlphaElement {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
        }
    }
}

impl Hash for AlphaElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        hash_f32_value(state, self.alpha);
    }
}

impl ModifierNodeElement for AlphaElement {
    type Node = AlphaNode;

    fn create(&self) -> Self::Node {
        AlphaNode::new(self.alpha)
    }

    fn update(&self, node: &mut Self::Node) {
        let new_alpha = self.alpha.clamp(0.0, 1.0);
        if (node.alpha - new_alpha).abs() > f32::EPSILON {
            node.alpha = new_alpha;
            // In a full implementation, would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

#[cfg(test)]
#[path = "tests/modifier_nodes_tests.rs"]
mod tests;

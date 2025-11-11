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
    Constraints, DrawModifierNode, DrawScope, LayoutModifierNode, Measurable, ModifierElement,
    ModifierNode, ModifierNodeContext, NodeCapabilities, PointerEvent, PointerEventKind,
    PointerInputNode, Size,
};
use std::rc::Rc;

use crate::modifier::{Color, EdgeInsets, Point};

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
#[derive(Debug, Clone)]
pub struct PaddingElement {
    padding: EdgeInsets,
}

impl PaddingElement {
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

impl ModifierElement for PaddingElement {
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
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Background Modifier Node
// ============================================================================

/// Node that draws a background behind its content.
#[derive(Debug)]
pub struct BackgroundNode {
    color: Color,
}

impl BackgroundNode {
    pub fn new(color: Color) -> Self {
        Self { color }
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
#[derive(Debug, Clone)]
pub struct BackgroundElement {
    color: Color,
}

impl BackgroundElement {
    pub fn new(color: Color) -> Self {
        Self { color }
    }
}

impl ModifierElement for BackgroundElement {
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
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
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
#[derive(Debug, Clone)]
pub struct SizeElement {
    width: Option<f32>,
    height: Option<f32>,
}

impl SizeElement {
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }
}

impl ModifierElement for SizeElement {
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
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
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

impl ModifierElement for ClickableElement {
    type Node = ClickableNode;

    fn create(&self) -> Self::Node {
        ClickableNode::with_handler(self.on_click.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        // Update the handler
        node.on_click = self.on_click.clone();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: false,
            has_pointer_input: true,
            has_semantics: false,
        }
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
#[derive(Debug, Clone)]
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

impl ModifierElement for AlphaElement {
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
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// AspectRatio Modifier Node
// ============================================================================

/// Node that enforces a specific aspect ratio on its content.
#[derive(Debug)]
pub struct AspectRatioNode {
    ratio: f32,
    match_height_constraints_first: bool,
}

impl AspectRatioNode {
    pub fn new(ratio: f32, match_height_constraints_first: bool) -> Self {
        Self {
            ratio,
            match_height_constraints_first,
        }
    }
}

impl ModifierNode for AspectRatioNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Layout);
    }
}

impl LayoutModifierNode for AspectRatioNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> Size {
        let size = if self.match_height_constraints_first {
            // Try to match height first, then calculate width
            let height = if constraints.max_height.is_finite() {
                constraints.max_height
            } else if constraints.min_height > 0.0 {
                constraints.min_height
            } else {
                constraints.max_width / self.ratio
            };
            let width = height * self.ratio;
            Size {
                width: width.clamp(constraints.min_width, constraints.max_width),
                height: height.clamp(constraints.min_height, constraints.max_height),
            }
        } else {
            // Try to match width first, then calculate height
            let width = if constraints.max_width.is_finite() {
                constraints.max_width
            } else if constraints.min_width > 0.0 {
                constraints.min_width
            } else {
                constraints.max_height * self.ratio
            };
            let height = width / self.ratio;
            Size {
                width: width.clamp(constraints.min_width, constraints.max_width),
                height: height.clamp(constraints.min_height, constraints.max_height),
            }
        };

        // Create tight constraints for the content
        let content_constraints = Constraints {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        };

        // Measure the content with tight constraints
        measurable.measure(content_constraints);

        size
    }

    fn min_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        if height.is_finite() && height > 0.0 {
            height * self.ratio
        } else {
            measurable.min_intrinsic_width(height)
        }
    }

    fn max_intrinsic_width(&self, measurable: &dyn Measurable, height: f32) -> f32 {
        if height.is_finite() && height > 0.0 {
            height * self.ratio
        } else {
            measurable.max_intrinsic_width(height)
        }
    }

    fn min_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        if width.is_finite() && width > 0.0 {
            width / self.ratio
        } else {
            measurable.min_intrinsic_height(width)
        }
    }

    fn max_intrinsic_height(&self, measurable: &dyn Measurable, width: f32) -> f32 {
        if width.is_finite() && width > 0.0 {
            width / self.ratio
        } else {
            measurable.max_intrinsic_height(width)
        }
    }
}

/// Element that creates and updates aspect ratio nodes.
#[derive(Debug, Clone)]
pub struct AspectRatioElement {
    ratio: f32,
    match_height_constraints_first: bool,
}

impl AspectRatioElement {
    pub fn new(ratio: f32, match_height_constraints_first: bool) -> Self {
        Self {
            ratio,
            match_height_constraints_first,
        }
    }
}

impl ModifierElement for AspectRatioElement {
    type Node = AspectRatioNode;

    fn create(&self) -> Self::Node {
        AspectRatioNode::new(self.ratio, self.match_height_constraints_first)
    }

    fn update(&self, node: &mut Self::Node) {
        if (node.ratio - self.ratio).abs() > f32::EPSILON
            || node.match_height_constraints_first != self.match_height_constraints_first
        {
            node.ratio = self.ratio;
            node.match_height_constraints_first = self.match_height_constraints_first;
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: true,
            has_draw: false,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Border Modifier Node
// ============================================================================

/// Node that draws a border around its content.
#[derive(Debug)]
pub struct BorderNode {
    width: f32,
    color: Color,
    shape: Option<crate::modifier::RoundedCornerShape>,
}

impl BorderNode {
    pub fn new(width: f32, color: Color, shape: Option<crate::modifier::RoundedCornerShape>) -> Self {
        Self {
            width,
            color,
            shape,
        }
    }
}

impl ModifierNode for BorderNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for BorderNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would draw the border
        // using the draw scope with the specified width, color, and shape.
        // For now, this is a placeholder.
        // The actual drawing happens in the renderer which reads node state.
    }
}

/// Element that creates and updates border nodes.
#[derive(Debug, Clone)]
pub struct BorderElement {
    width: f32,
    color: Color,
    shape: Option<crate::modifier::RoundedCornerShape>,
}

impl BorderElement {
    pub fn new(width: f32, color: Color, shape: Option<crate::modifier::RoundedCornerShape>) -> Self {
        Self {
            width,
            color,
            shape,
        }
    }
}

impl ModifierElement for BorderElement {
    type Node = BorderNode;

    fn create(&self) -> Self::Node {
        BorderNode::new(self.width, self.color, self.shape)
    }

    fn update(&self, node: &mut Self::Node) {
        let mut changed = false;
        if (node.width - self.width).abs() > f32::EPSILON {
            node.width = self.width;
            changed = true;
        }
        if node.color != self.color {
            node.color = self.color;
            changed = true;
        }
        if node.shape != self.shape {
            node.shape = self.shape;
            changed = true;
        }
        // In a full implementation, we would invalidate draw if changed
        let _ = changed;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Clip Modifier Node
// ============================================================================

/// Node that clips content to a shape.
#[derive(Debug)]
pub struct ClipNode {
    shape: crate::modifier::RoundedCornerShape,
}

impl ClipNode {
    pub fn new(shape: crate::modifier::RoundedCornerShape) -> Self {
        Self { shape }
    }
}

impl ModifierNode for ClipNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for ClipNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Save the current clip state
        // 2. Apply clipping based on the shape
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous clip state
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates clip nodes.
#[derive(Debug, Clone)]
pub struct ClipElement {
    shape: crate::modifier::RoundedCornerShape,
}

impl ClipElement {
    pub fn new(shape: crate::modifier::RoundedCornerShape) -> Self {
        Self { shape }
    }
}

impl ModifierElement for ClipElement {
    type Node = ClipNode;

    fn create(&self) -> Self::Node {
        ClipNode::new(self.shape)
    }

    fn update(&self, node: &mut Self::Node) {
        if node.shape != self.shape {
            node.shape = self.shape;
            // In a full implementation, would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Rotate Modifier Node
// ============================================================================

/// Node that rotates content by a specified angle.
#[derive(Debug)]
pub struct RotateNode {
    degrees: f32,
}

impl RotateNode {
    pub fn new(degrees: f32) -> Self {
        Self { degrees }
    }
}

impl ModifierNode for RotateNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for RotateNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Save the current transformation matrix
        // 2. Apply rotation transform around the center point
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous transformation
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates rotate nodes.
#[derive(Debug, Clone)]
pub struct RotateElement {
    degrees: f32,
}

impl RotateElement {
    pub fn new(degrees: f32) -> Self {
        Self { degrees }
    }
}

impl ModifierElement for RotateElement {
    type Node = RotateNode;

    fn create(&self) -> Self::Node {
        RotateNode::new(self.degrees)
    }

    fn update(&self, node: &mut Self::Node) {
        if (node.degrees - self.degrees).abs() > f32::EPSILON {
            node.degrees = self.degrees;
            // In a full implementation, would invalidate draw here
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

// ============================================================================
// Scale Modifier Node
// ============================================================================

/// Node that scales content by specified factors.
#[derive(Debug)]
pub struct ScaleNode {
    scale_x: f32,
    scale_y: f32,
}

impl ScaleNode {
    pub fn new(scale_x: f32, scale_y: f32) -> Self {
        Self { scale_x, scale_y }
    }
}

impl ModifierNode for ScaleNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(compose_foundation::InvalidationKind::Draw);
    }
}

impl DrawModifierNode for ScaleNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Save the current transformation matrix
        // 2. Apply scale transform around the center point
        // 3. Draw content via draw_scope.draw_content()
        // 4. Restore previous transformation
        //
        // For now this is a placeholder showing the structure
    }
}

/// Element that creates and updates scale nodes.
#[derive(Debug, Clone)]
pub struct ScaleElement {
    scale_x: f32,
    scale_y: f32,
}

impl ScaleElement {
    pub fn new(scale_x: f32, scale_y: f32) -> Self {
        Self { scale_x, scale_y }
    }
}

impl ModifierElement for ScaleElement {
    type Node = ScaleNode;

    fn create(&self) -> Self::Node {
        ScaleNode::new(self.scale_x, self.scale_y)
    }

    fn update(&self, node: &mut Self::Node) {
        let mut changed = false;
        if (node.scale_x - self.scale_x).abs() > f32::EPSILON {
            node.scale_x = self.scale_x;
            changed = true;
        }
        if (node.scale_y - self.scale_y).abs() > f32::EPSILON {
            node.scale_y = self.scale_y;
            changed = true;
        }
        // In a full implementation, would invalidate draw if changed
        let _ = changed;
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities {
            has_layout: false,
            has_draw: true,
            has_pointer_input: false,
            has_semantics: false,
        }
    }
}

#[cfg(test)]
#[path = "tests/modifier_nodes_tests.rs"]
mod tests;

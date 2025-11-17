//! Text modifier node implementation following Jetpack Compose's TextStringSimpleNode architecture.
//!
//! This module implements text content as a modifier node rather than as a measure policy,
//! matching the Jetpack Compose pattern where text is treated as visual content (like background)
//! rather than as a layout strategy.
//!
//! # Architecture
//!
//! In Jetpack Compose, `BasicText` uses:
//! ```kotlin
//! Layout(modifier.then(TextStringSimpleElement(...)), EmptyMeasurePolicy)
//! ```
//!
//! Where `TextStringSimpleNode` implements:
//! - `LayoutModifierNode` - handles text measurement
//! - `DrawModifierNode` - handles text drawing
//! - `SemanticsModifierNode` - provides text content for accessibility
//!
//! This follows the principle that `MeasurePolicy` is for child layout, while modifier nodes
//! handle content rendering and measurement.

use compose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, SemanticsConfiguration, SemanticsNode, Size,
};
use std::hash::{Hash, Hasher};

/// Node that stores text content and handles measurement, drawing, and semantics.
///
/// This node implements three capabilities:
/// - **Layout**: Measures text and returns appropriate size
/// - **Draw**: Renders the text (placeholder for now)
/// - **Semantics**: Provides text content for accessibility
///
/// Matches Jetpack Compose: `TextStringSimpleNode` in
/// `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/modifiers/TextStringSimpleNode.kt`
#[derive(Debug)]
pub struct TextModifierNode {
    text: String,
    state: NodeState,
}

impl TextModifierNode {
    pub fn new(text: String) -> Self {
        Self {
            text,
            state: NodeState::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Helper to measure text content size.
    fn measure_text_content(&self) -> Size {
        let metrics = crate::text::measure_text(&self.text);
        Size {
            width: metrics.width,
            height: metrics.height,
        }
    }
}

impl DelegatableNode for TextModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TextModifierNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        // Invalidate layout and draw when text node is attached
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

    fn as_semantics_node(&self) -> Option<&dyn SemanticsNode> {
        Some(self)
    }

    fn as_semantics_node_mut(&mut self) -> Option<&mut dyn SemanticsNode> {
        Some(self)
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for TextModifierNode {
    fn measure(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> Size {
        // Measure the text content
        let text_size = self.measure_text_content();

        // Constrain text size to the provided constraints
        let width = text_size
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_size
            .height
            .clamp(constraints.min_height, constraints.max_height);

        // Text is a leaf node - return the text size directly
        // We don't call measurable.measure() because there's no wrapped content
        // (Text uses EmptyMeasurePolicy which has no children)
        Size { width, height }
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content().width
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content().width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content().height
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content().height
    }
}

impl DrawModifierNode for TextModifierNode {
    fn draw(&mut self, _context: &mut dyn ModifierNodeContext, _draw_scope: &mut dyn DrawScope) {
        // In a full implementation, this would:
        // 1. Get the text paragraph layout cache
        // 2. Paint the text using draw_scope canvas
        //
        // For now, this is a placeholder. The actual rendering will be handled
        // by the renderer which can read text from the modifier chain.
        //
        // Future: Implement actual text drawing here using DrawScope
    }
}

impl SemanticsNode for TextModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        // Provide text content for accessibility
        config.content_description = Some(self.text.clone());
    }
}

/// Element that creates and updates TextModifierNode instances.
///
/// This follows the modifier element pattern where the element is responsible for:
/// - Creating new nodes (via `create`)
/// - Updating existing nodes when properties change (via `update`)
/// - Declaring capabilities (LAYOUT | DRAW | SEMANTICS)
///
/// Matches Jetpack Compose: `TextStringSimpleElement` in BasicText.kt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextModifierElement {
    text: String,
}

impl TextModifierElement {
    pub fn new(text: String) -> Self {
        Self { text }
    }
}

impl Hash for TextModifierElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.hash(state);
    }
}

impl ModifierNodeElement for TextModifierElement {
    type Node = TextModifierNode;

    fn create(&self) -> Self::Node {
        TextModifierNode::new(self.text.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        if node.text != self.text {
            node.text = self.text.clone();
            // Text changed - need to invalidate layout, draw, and semantics
            // Note: In the full implementation, we would call context.invalidate here
            // but update() doesn't currently have access to context.
            // The invalidation will happen on the next recomposition when the node
            // is reconciled.
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        // Text nodes participate in layout, drawing, and semantics
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS
    }
}

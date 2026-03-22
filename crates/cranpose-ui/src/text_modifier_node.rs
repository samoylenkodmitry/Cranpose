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

use crate::text::{AnnotatedString, TextLayoutOptions, TextStyle};
use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, InvalidationKind,
    LayoutModifierNode, Measurable, MeasurementProxy, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState, SemanticsConfiguration, SemanticsNode, Size,
};
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

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
    layout: Rc<TextPreparedLayoutOwner>,
    state: NodeState,
}

const PREPARED_LAYOUT_CACHE_CAPACITY: usize = 4;

#[derive(Clone, Debug)]
struct TextPreparedLayoutCacheEntry {
    max_width_bits: Option<u32>,
    layout: crate::text::PreparedTextLayout,
}

#[derive(Debug)]
struct TextPreparedLayoutOwner {
    text: Rc<AnnotatedString>,
    style: TextStyle,
    options: TextLayoutOptions,
    node_id: Cell<Option<cranpose_core::NodeId>>,
    cache: RefCell<Vec<TextPreparedLayoutCacheEntry>>,
}

#[derive(Clone, Debug)]
pub(crate) struct TextPreparedLayoutHandle {
    owner: Rc<TextPreparedLayoutOwner>,
}

impl TextPreparedLayoutOwner {
    fn new(
        text: Rc<AnnotatedString>,
        style: TextStyle,
        options: TextLayoutOptions,
        node_id: Option<cranpose_core::NodeId>,
    ) -> Self {
        Self {
            text,
            style,
            options: options.normalized(),
            node_id: Cell::new(node_id),
            cache: RefCell::new(Vec::new()),
        }
    }

    fn text(&self) -> &str {
        self.text.text.as_str()
    }

    fn annotated_text(&self) -> Rc<AnnotatedString> {
        self.text.clone()
    }

    fn annotated_string(&self) -> AnnotatedString {
        (*self.text).clone()
    }

    fn style(&self) -> &TextStyle {
        &self.style
    }

    fn options(&self) -> TextLayoutOptions {
        self.options
    }

    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        self.node_id.get()
    }

    fn set_node_id(&self, node_id: Option<cranpose_core::NodeId>) {
        if self.node_id.replace(node_id) != node_id {
            self.cache.borrow_mut().clear();
        }
    }

    fn prepare(&self, max_width: Option<f32>) -> crate::text::PreparedTextLayout {
        let normalized_max_width = max_width.filter(|width| width.is_finite() && *width > 0.0);
        let max_width_bits = normalized_max_width.map(f32::to_bits);

        {
            let mut cache = self.cache.borrow_mut();
            if let Some(index) = cache
                .iter()
                .position(|entry| entry.max_width_bits == max_width_bits)
            {
                let entry = cache.remove(index);
                let prepared = entry.layout.clone();
                cache.insert(0, entry);
                return prepared;
            }
        }

        let prepared = crate::text::prepare_text_layout_for_node(
            self.node_id(),
            self.text.as_ref(),
            &self.style,
            self.options,
            normalized_max_width,
        );

        let mut cache = self.cache.borrow_mut();
        cache.insert(
            0,
            TextPreparedLayoutCacheEntry {
                max_width_bits,
                layout: prepared.clone(),
            },
        );
        cache.truncate(PREPARED_LAYOUT_CACHE_CAPACITY);
        prepared
    }

    fn measure_text_content(&self, max_width: Option<f32>) -> Size {
        let prepared = self.prepare(max_width);
        Size {
            width: prepared.metrics.width,
            height: prepared.metrics.height,
        }
    }
}

impl TextPreparedLayoutHandle {
    fn new(owner: Rc<TextPreparedLayoutOwner>) -> Self {
        Self { owner }
    }

    pub(crate) fn prepare(&self, max_width: Option<f32>) -> crate::text::PreparedTextLayout {
        self.owner.prepare(max_width)
    }

    fn measure_text_content(&self, max_width: Option<f32>) -> Size {
        self.owner.measure_text_content(max_width)
    }
}

impl TextModifierNode {
    pub fn new(text: Rc<AnnotatedString>, style: TextStyle, options: TextLayoutOptions) -> Self {
        Self {
            layout: Rc::new(TextPreparedLayoutOwner::new(text, style, options, None)),
            state: NodeState::new(),
        }
    }

    pub fn text(&self) -> &str {
        self.layout.text()
    }

    pub fn annotated_text(&self) -> Rc<AnnotatedString> {
        self.layout.annotated_text()
    }

    pub fn annotated_string(&self) -> AnnotatedString {
        self.layout.annotated_string()
    }

    pub fn style(&self) -> &TextStyle {
        self.layout.style()
    }

    pub fn options(&self) -> TextLayoutOptions {
        self.layout.options()
    }

    fn measure_text_content(&self, max_width: Option<f32>) -> Size {
        self.layout.measure_text_content(max_width)
    }

    pub(crate) fn prepared_layout_handle(&self) -> TextPreparedLayoutHandle {
        TextPreparedLayoutHandle::new(self.layout.clone())
    }
}

impl DelegatableNode for TextModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TextModifierNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.layout.set_node_id(context.node_id());
        // Invalidate layout and draw when text node is attached
        context.invalidate(InvalidationKind::Layout);
        context.invalidate(InvalidationKind::Draw);
        context.invalidate(InvalidationKind::Semantics);
    }

    fn on_detach(&mut self) {
        self.layout.set_node_id(None);
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
        &self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Measure the text content
        let max_width = constraints
            .max_width
            .is_finite()
            .then_some(constraints.max_width);
        let text_size = self.measure_text_content(max_width);

        // Constrain text size to the provided constraints
        let width = text_size
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_size
            .height
            .clamp(constraints.min_height, constraints.max_height);

        // Text is a leaf node - return the text size directly with no offset
        // We don't call measurable.measure() because there's no wrapped content
        // (Text uses EmptyMeasurePolicy which has no children)
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size { width, height })
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content(Some(_width).filter(|w| w.is_finite() && *w > 0.0))
            .height
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content(Some(_width).filter(|w| w.is_finite() && *w > 0.0))
            .height
    }

    fn create_measurement_proxy(&self) -> Option<Box<dyn MeasurementProxy>> {
        Some(Box::new(TextMeasurementProxy {
            layout: self.prepared_layout_handle(),
        }))
    }
}

/// Measurement proxy for TextModifierNode that snapshots live state.
///
/// Phase 2: Instead of reconstructing nodes via `TextModifierNode::new()`, this proxy
/// directly implements measurement logic using the snapshotted text content.
struct TextMeasurementProxy {
    layout: TextPreparedLayoutHandle,
}

impl TextMeasurementProxy {
    /// Measure the text content dimensions.
    /// Matches TextModifierNode::measure_text_content() logic.
    fn measure_text_content(&self, max_width: Option<f32>) -> Size {
        self.layout.measure_text_content(max_width)
    }
}

impl MeasurementProxy for TextMeasurementProxy {
    fn measure_proxy(
        &self,
        _context: &mut dyn ModifierNodeContext,
        _measurable: &dyn Measurable,
        constraints: Constraints,
    ) -> cranpose_ui_layout::LayoutModifierMeasureResult {
        // Directly implement text measurement logic (no node reconstruction)
        let max_width = constraints
            .max_width
            .is_finite()
            .then_some(constraints.max_width);
        let text_size = self.measure_text_content(max_width);

        // Constrain text size to the provided constraints
        let width = text_size
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_size
            .height
            .clamp(constraints.min_height, constraints.max_height);

        // Text is a leaf node - return the text size directly with no offset
        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size { width, height })
    }

    fn min_intrinsic_width_proxy(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn max_intrinsic_width_proxy(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        self.measure_text_content(None).width
    }

    fn min_intrinsic_height_proxy(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content(Some(_width).filter(|w| w.is_finite() && *w > 0.0))
            .height
    }

    fn max_intrinsic_height_proxy(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        self.measure_text_content(Some(_width).filter(|w| w.is_finite() && *w > 0.0))
            .height
    }
}

impl DrawModifierNode for TextModifierNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
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
        config.content_description = Some(self.text().to_string());
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
#[derive(Debug, Clone, PartialEq)]
pub struct TextModifierElement {
    text: Rc<AnnotatedString>,
    style: TextStyle,
    options: TextLayoutOptions,
}

impl TextModifierElement {
    pub fn new(text: Rc<AnnotatedString>, style: TextStyle, options: TextLayoutOptions) -> Self {
        Self {
            text,
            style,
            options: options.normalized(),
        }
    }
}

impl Hash for TextModifierElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.render_hash().hash(state);
        self.style.render_hash().hash(state);
        self.options.hash(state);
    }
}

impl ModifierNodeElement for TextModifierElement {
    type Node = TextModifierNode;

    fn create(&self) -> Self::Node {
        TextModifierNode::new(self.text.clone(), self.style.clone(), self.options)
    }

    fn update(&self, node: &mut Self::Node) {
        let current = node.layout.as_ref();
        if current.text != self.text
            || current.style != self.style
            || current.options != self.options
        {
            node.layout = Rc::new(TextPreparedLayoutOwner::new(
                self.text.clone(),
                self.style.clone(),
                self.options,
                current.node_id(),
            ));
            // Text/Style changed - need to invalidate layout, draw, and semantics
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextUnit;
    use crate::text_layout_result::TextLayoutResult;
    use cranpose_core::NodeId;
    use cranpose_foundation::BasicModifierNodeContext;
    use std::collections::hash_map::DefaultHasher;
    use std::sync::mpsc;

    fn hash_of(element: &TextModifierElement) -> u64 {
        let mut hasher = DefaultHasher::new();
        element.hash(&mut hasher);
        hasher.finish()
    }

    struct RecordingPreparedLayoutMeasurer {
        recorded: std::rc::Rc<std::cell::RefCell<Vec<Option<NodeId>>>>,
    }

    impl crate::text::TextMeasurer for RecordingPreparedLayoutMeasurer {
        fn measure(
            &self,
            _text: &crate::text::AnnotatedString,
            _style: &TextStyle,
        ) -> crate::text::TextMetrics {
            crate::text::TextMetrics {
                width: 12.0,
                height: 18.0,
                line_height: 18.0,
                line_count: 1,
            }
        }

        fn prepare_with_options_for_node(
            &self,
            node_id: Option<NodeId>,
            text: &crate::text::AnnotatedString,
            _style: &TextStyle,
            _options: TextLayoutOptions,
            _max_width: Option<f32>,
        ) -> crate::text::PreparedTextLayout {
            self.recorded.borrow_mut().push(node_id);
            crate::text::PreparedTextLayout {
                text: text.clone(),
                metrics: crate::text::TextMetrics {
                    width: 12.0,
                    height: 18.0,
                    line_height: 18.0,
                    line_count: 1,
                },
                did_overflow: false,
            }
        }

        fn get_offset_for_position(
            &self,
            _text: &crate::text::AnnotatedString,
            _style: &TextStyle,
            _x: f32,
            _y: f32,
        ) -> usize {
            0
        }

        fn get_cursor_x_for_offset(
            &self,
            _text: &crate::text::AnnotatedString,
            _style: &TextStyle,
            _offset: usize,
        ) -> f32 {
            0.0
        }

        fn layout(
            &self,
            _text: &crate::text::AnnotatedString,
            _style: &TextStyle,
        ) -> TextLayoutResult {
            panic!("layout is not used in this test");
        }
    }

    #[test]
    fn hash_changes_when_style_changes() {
        let text = Rc::new(AnnotatedString::from("Hello"));
        let element_a = TextModifierElement::new(
            text.clone(),
            TextStyle::default(),
            TextLayoutOptions::default(),
        );
        let style_b = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(18.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let element_b = TextModifierElement::new(text, style_b, TextLayoutOptions::default());

        assert_ne!(element_a, element_b);
        assert_ne!(hash_of(&element_a), hash_of(&element_b));
    }

    #[test]
    fn hash_matches_for_equal_elements() {
        let style = TextStyle {
            span_style: crate::text::SpanStyle {
                font_size: TextUnit::Sp(14.0),
                letter_spacing: TextUnit::Em(0.1),
                ..Default::default()
            },
            ..Default::default()
        };
        let options = TextLayoutOptions::default();
        let text = Rc::new(AnnotatedString::from("Hash me"));
        let element_a = TextModifierElement::new(text.clone(), style.clone(), options);
        let element_b = TextModifierElement::new(text, style, options);

        assert_eq!(element_a, element_b);
        assert_eq!(hash_of(&element_a), hash_of(&element_b));
    }

    #[test]
    fn measure_uses_attached_node_identity() {
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            crate::text::set_text_measurer(RecordingPreparedLayoutMeasurer {
                recorded: recorded.clone(),
            });

            let mut node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("identity")),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );
            let mut context = BasicModifierNodeContext::new();
            context.set_node_id(Some(77));
            node.on_attach(&mut context);

            let size = node.measure_text_content(Some(96.0));
            tx.send((recorded.borrow().clone(), size.width, size.height))
                .expect("send measurement result");
        });

        let (recorded, width, height) = rx.recv().expect("receive measurement result");
        assert_eq!(recorded, vec![Some(77)]);
        assert_eq!(width, 12.0);
        assert_eq!(height, 18.0);
    }

    #[test]
    fn prepared_layout_cache_reuses_node_snapshot() {
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            crate::text::set_text_measurer(RecordingPreparedLayoutMeasurer {
                recorded: recorded.clone(),
            });

            let mut node = TextModifierNode::new(
                Rc::new(AnnotatedString::from("reuse")),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );
            let mut context = BasicModifierNodeContext::new();
            context.set_node_id(Some(88));
            node.on_attach(&mut context);

            let measured = node.measure_text_content(Some(120.0));
            let prepared = node.prepared_layout_handle().prepare(Some(120.0));
            tx.send((
                recorded.borrow().clone(),
                measured.width,
                measured.height,
                prepared.metrics.width,
                prepared.metrics.height,
            ))
            .expect("send cached layout result");
        });

        let (recorded, measured_width, measured_height, prepared_width, prepared_height) =
            rx.recv().expect("receive cached layout result");
        assert_eq!(recorded, vec![Some(88)]);
        assert_eq!(measured_width, prepared_width);
        assert_eq!(measured_height, prepared_height);
    }
}

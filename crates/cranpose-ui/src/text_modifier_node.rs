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

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn hash_text_unit<H: Hasher>(unit: crate::text::TextUnit, state: &mut H) {
    match unit {
        crate::text::TextUnit::Unspecified => 0u8.hash(state),
        crate::text::TextUnit::Sp(value) => {
            1u8.hash(state);
            hash_f32_bits(value, state);
        }
        crate::text::TextUnit::Em(value) => {
            2u8.hash(state);
            hash_f32_bits(value, state);
        }
    }
}

fn hash_color<H: Hasher>(color: crate::modifier::Color, state: &mut H) {
    hash_f32_bits(color.0, state);
    hash_f32_bits(color.1, state);
    hash_f32_bits(color.2, state);
    hash_f32_bits(color.3, state);
}

fn hash_option_color<H: Hasher>(color: &Option<crate::modifier::Color>, state: &mut H) {
    match color {
        Some(color) => {
            1u8.hash(state);
            hash_color(*color, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_brush<H: Hasher>(brush: &crate::modifier::Brush, state: &mut H) {
    match brush {
        crate::modifier::Brush::Solid(color) => {
            0u8.hash(state);
            hash_color(*color, state);
        }
        crate::modifier::Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            1u8.hash(state);
            colors.len().hash(state);
            for color in colors {
                hash_color(*color, state);
            }
            match stops {
                Some(stops) => {
                    1u8.hash(state);
                    stops.len().hash(state);
                    for stop in stops {
                        hash_f32_bits(*stop, state);
                    }
                }
                None => 0u8.hash(state),
            }
            hash_f32_bits(start.x, state);
            hash_f32_bits(start.y, state);
            hash_f32_bits(end.x, state);
            hash_f32_bits(end.y, state);
            tile_mode.hash(state);
        }
        crate::modifier::Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            2u8.hash(state);
            colors.len().hash(state);
            for color in colors {
                hash_color(*color, state);
            }
            match stops {
                Some(stops) => {
                    1u8.hash(state);
                    stops.len().hash(state);
                    for stop in stops {
                        hash_f32_bits(*stop, state);
                    }
                }
                None => 0u8.hash(state),
            }
            hash_f32_bits(center.x, state);
            hash_f32_bits(center.y, state);
            hash_f32_bits(*radius, state);
            tile_mode.hash(state);
        }
        crate::modifier::Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            3u8.hash(state);
            colors.len().hash(state);
            for color in colors {
                hash_color(*color, state);
            }
            match stops {
                Some(stops) => {
                    1u8.hash(state);
                    stops.len().hash(state);
                    for stop in stops {
                        hash_f32_bits(*stop, state);
                    }
                }
                None => 0u8.hash(state),
            }
            hash_f32_bits(center.x, state);
            hash_f32_bits(center.y, state);
        }
    }
}

fn hash_option_brush<H: Hasher>(brush: &Option<crate::modifier::Brush>, state: &mut H) {
    match brush {
        Some(brush) => {
            1u8.hash(state);
            hash_brush(brush, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_alpha<H: Hasher>(alpha: &Option<f32>, state: &mut H) {
    match alpha {
        Some(alpha) => {
            1u8.hash(state);
            hash_f32_bits(*alpha, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_baseline_shift<H: Hasher>(
    baseline_shift: &Option<crate::text::BaselineShift>,
    state: &mut H,
) {
    match baseline_shift {
        Some(shift) => {
            1u8.hash(state);
            hash_f32_bits(shift.0, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_text_geometric_transform<H: Hasher>(
    transform: &Option<crate::text::TextGeometricTransform>,
    state: &mut H,
) {
    match transform {
        Some(transform) => {
            1u8.hash(state);
            hash_f32_bits(transform.scale_x, state);
            hash_f32_bits(transform.skew_x, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_shadow<H: Hasher>(shadow: &Option<crate::text::Shadow>, state: &mut H) {
    match shadow {
        Some(shadow) => {
            1u8.hash(state);
            hash_color(shadow.color, state);
            hash_f32_bits(shadow.offset.x, state);
            hash_f32_bits(shadow.offset.y, state);
            hash_f32_bits(shadow.blur_radius, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_text_indent<H: Hasher>(indent: &Option<crate::text::TextIndent>, state: &mut H) {
    match indent {
        Some(indent) => {
            1u8.hash(state);
            hash_text_unit(indent.first_line, state);
            hash_text_unit(indent.rest_line, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_option_text_draw_style<H: Hasher>(
    draw_style: &Option<crate::text::TextDrawStyle>,
    state: &mut H,
) {
    match draw_style {
        Some(crate::text::TextDrawStyle::Fill) => {
            1u8.hash(state);
            0u8.hash(state);
        }
        Some(crate::text::TextDrawStyle::Stroke { width }) => {
            1u8.hash(state);
            1u8.hash(state);
            hash_f32_bits(*width, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_text_style<H: Hasher>(style: &TextStyle, state: &mut H) {
    let span = &style.span_style;
    let paragraph = &style.paragraph_style;

    hash_option_color(&span.color, state);
    hash_option_brush(&span.brush, state);
    hash_option_alpha(&span.alpha, state);
    hash_text_unit(span.font_size, state);
    span.font_weight.hash(state);
    span.font_style.hash(state);
    span.font_synthesis.hash(state);
    span.font_family.hash(state);
    span.font_feature_settings.hash(state);
    hash_text_unit(span.letter_spacing, state);
    hash_option_baseline_shift(&span.baseline_shift, state);
    hash_option_text_geometric_transform(&span.text_geometric_transform, state);
    span.locale_list.hash(state);
    hash_option_color(&span.background, state);
    span.text_decoration.hash(state);
    hash_option_shadow(&span.shadow, state);
    span.platform_style.hash(state);
    hash_option_text_draw_style(&span.draw_style, state);

    paragraph.text_align.hash(state);
    paragraph.text_direction.hash(state);
    hash_text_unit(paragraph.line_height, state);
    hash_option_text_indent(&paragraph.text_indent, state);
    paragraph.platform_style.hash(state);
    paragraph.line_height_style.hash(state);
    paragraph.line_break.hash(state);
    paragraph.hyphens.hash(state);
    paragraph.text_motion.hash(state);
}

impl Hash for TextModifierElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.text.hash(state);
        hash_text_style(&self.style, state);
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

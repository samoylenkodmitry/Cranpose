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

use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, DrawScope, InvalidationKind,
    LayoutModifierNode, Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, SemanticsConfiguration, SemanticsNode, Size,
};

use crate::text::{AnnotatedString, TextLayoutOptions, TextStyle};

/// Node that stores text content and handles measurement, drawing, and semantics.
///
/// This node implements three capabilities:
/// - **Layout**: Measures text and returns appropriate size
/// - **Draw**: Supplies prepared text state consumed by scene building
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
    text_generation: u64,
    font_scale_fingerprint: u32,
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
        let text_generation = crate::text::measure::current_text_generation();
        // Keyed on the whole conversion and not on the setting alone: two
        // curves can share a scale and resolve an `Sp` differently.
        let font_scale_fingerprint = crate::current_font_scale_curve().fingerprint();

        {
            let mut cache = self.cache.borrow_mut();
            if let Some(index) = cache.iter().position(|entry| {
                entry.max_width_bits == max_width_bits
                    && entry.text_generation == text_generation
                    && entry.font_scale_fingerprint == font_scale_fingerprint
            }) {
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
                text_generation,
                font_scale_fingerprint,
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
}

impl DrawModifierNode for TextModifierNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // Text drawing is emitted by the scene builder from the retained node
        // state, so the modifier draw hook remains side-effect free.
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
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        // Text nodes participate in layout, drawing, and semantics
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::hash_map::DefaultHasher, sync::mpsc};

    use cranpose_core::NodeId;
    use cranpose_foundation::BasicModifierNodeContext;

    use super::*;
    use crate::{text::TextUnit, text_layout_result::TextLayoutResult};

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
                visual_style: TextStyle::default(),
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

    struct FixedPreparedLayoutMeasurer {
        height: f32,
        line_height: f32,
    }

    struct FontSizePreparedLayoutMeasurer {
        recorded: Rc<RefCell<Vec<f32>>>,
    }

    impl crate::text::TextMeasurer for FontSizePreparedLayoutMeasurer {
        fn measure(
            &self,
            _text: &crate::text::AnnotatedString,
            style: &TextStyle,
        ) -> crate::text::TextMetrics {
            let size = style.resolve_font_size(14.0);
            crate::text::TextMetrics {
                width: size,
                height: size,
                line_height: size,
                line_count: 1,
            }
        }

        fn prepare_with_options_for_node(
            &self,
            _node_id: Option<NodeId>,
            text: &crate::text::AnnotatedString,
            style: &TextStyle,
            _options: TextLayoutOptions,
            _max_width: Option<f32>,
        ) -> crate::text::PreparedTextLayout {
            let size = style.resolve_font_size(14.0);
            self.recorded.borrow_mut().push(size);
            crate::text::PreparedTextLayout {
                text: text.clone(),
                visual_style: style.clone(),
                metrics: crate::text::TextMetrics {
                    width: size,
                    height: size,
                    line_height: size,
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

    impl crate::text::TextMeasurer for FixedPreparedLayoutMeasurer {
        fn measure(
            &self,
            _text: &crate::text::AnnotatedString,
            _style: &TextStyle,
        ) -> crate::text::TextMetrics {
            crate::text::TextMetrics {
                width: 24.0,
                height: self.height,
                line_height: self.line_height,
                line_count: (self.height / self.line_height).round().max(1.0) as usize,
            }
        }

        fn prepare_with_options_for_node(
            &self,
            _node_id: Option<NodeId>,
            text: &crate::text::AnnotatedString,
            _style: &TextStyle,
            _options: TextLayoutOptions,
            _max_width: Option<f32>,
        ) -> crate::text::PreparedTextLayout {
            crate::text::PreparedTextLayout {
                text: text.clone(),
                visual_style: TextStyle::default(),
                metrics: crate::text::TextMetrics {
                    width: 24.0,
                    height: self.height,
                    line_height: self.line_height,
                    line_count: (self.height / self.line_height).round().max(1.0) as usize,
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
            let app_context = crate::AppContext::new();
            app_context.enter(|| {
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
            let app_context = crate::AppContext::new();
            app_context.enter(|| {
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
        });

        let (recorded, measured_width, measured_height, prepared_width, prepared_height) =
            rx.recv().expect("receive cached layout result");
        assert_eq!(recorded, vec![Some(88)]);
        assert_eq!(measured_width, prepared_width);
        assert_eq!(measured_height, prepared_height);
    }

    #[test]
    fn prepared_layout_cache_refreshes_when_text_service_changes() {
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let app_context = crate::AppContext::new();
            app_context.enter(|| {
                crate::text::set_text_measurer(FixedPreparedLayoutMeasurer {
                    height: 30.0,
                    line_height: 10.0,
                });

                let node = TextModifierNode::new(
                    Rc::new(AnnotatedString::from("a\nb\nc")),
                    TextStyle::default(),
                    TextLayoutOptions::default(),
                );

                let first = node.measure_text_content(Some(160.0));
                crate::text::set_text_measurer(FixedPreparedLayoutMeasurer {
                    height: 60.0,
                    line_height: 20.0,
                });
                let second = node.measure_text_content(Some(160.0));
                tx.send((first.height, second.height))
                    .expect("send measurement result");
            });
        });

        let (first_height, second_height) = rx.recv().expect("receive measurement result");
        assert_eq!(first_height, 30.0);
        assert_eq!(second_height, 60.0);
    }

    #[test]
    fn prepared_layout_cache_refreshes_when_system_font_scale_changes() {
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let recorded = Rc::new(RefCell::new(Vec::new()));
            let app_context = crate::AppContext::new();
            app_context.enter(|| {
                crate::text::set_text_measurer(FontSizePreparedLayoutMeasurer {
                    recorded: Rc::clone(&recorded),
                });
                let node = TextModifierNode::new(
                    Rc::new(AnnotatedString::from("scale")),
                    TextStyle {
                        span_style: crate::text::SpanStyle {
                            font_size: TextUnit::Sp(10.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    TextLayoutOptions::default(),
                );

                let first = node.measure_text_content(None);
                crate::set_font_scale(1.5);
                let second = node.measure_text_content(None);
                tx.send((recorded.borrow().clone(), first.height, second.height))
                    .expect("send measurement result");
            });
        });

        let (recorded, first, second) = rx.recv().expect("receive measurement result");
        assert_eq!(recorded, vec![10.0, 15.0]);
        assert_eq!(first, 10.0);
        assert_eq!(second, 15.0);
    }

    #[test]
    fn semantics_uses_source_text_for_scaled_overflow() {
        let node = TextModifierNode::new(
            Rc::new(AnnotatedString::from("Save Cranpose WebP")),
            TextStyle::default(),
            TextLayoutOptions {
                overflow: crate::text::TextOverflow::ScaleDown {
                    min_font_size_sp: 9.0,
                },
                soft_wrap: false,
                max_lines: 1,
                min_lines: 1,
            },
        );
        let mut config = SemanticsConfiguration::default();

        node.merge_semantics(&mut config);

        assert_eq!(
            config.content_description.as_deref(),
            Some("Save Cranpose WebP")
        );
    }
}

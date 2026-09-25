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
    widths: crate::text::measure::PreparedWidths,
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
    measured_max_width: Cell<Option<Option<f32>>>,
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
        measured_max_width: Option<Option<f32>>,
    ) -> Self {
        Self {
            text,
            style,
            options: options.normalized(),
            node_id: Cell::new(node_id),
            measured_max_width: Cell::new(measured_max_width),
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
        let text_generation = crate::text::measure::current_text_generation();
        let font_scale_fingerprint = crate::current_font_scale_curve().fingerprint();

        {
            let mut cache = self.cache.borrow_mut();
            if let Some(index) = cache.iter().position(|entry| {
                entry.widths.hold(normalized_max_width)
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
                widths: crate::text::measure::PreparedWidths::of(
                    self.text.as_ref(),
                    self.options,
                    normalized_max_width,
                    &prepared,
                ),
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

    fn measure_layout(&self, max_width: Option<f32>) -> Size {
        self.measured_max_width.set(Some(max_width));
        self.measure_text_content(max_width)
    }

    fn measured_layout(&self) -> Option<crate::text::PreparedTextLayout> {
        self.measured_max_width
            .get()
            .map(|max_width| self.prepare(max_width))
    }
}

impl TextPreparedLayoutHandle {
    fn new(owner: Rc<TextPreparedLayoutOwner>) -> Self {
        Self { owner }
    }

    pub(crate) fn measured_layout(&self) -> Option<crate::text::PreparedTextLayout> {
        self.owner.measured_layout()
    }
}

impl TextModifierNode {
    pub fn new(text: Rc<AnnotatedString>, style: TextStyle, options: TextLayoutOptions) -> Self {
        Self {
            layout: Rc::new(TextPreparedLayoutOwner::new(
                text, style, options, None, None,
            )),
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
        let max_width = constraints
            .max_width
            .is_finite()
            .then_some(constraints.max_width);
        let text_size = self.layout.measure_layout(max_width);

        let width = text_size
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_size
            .height
            .clamp(constraints.min_height, constraints.max_height);

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
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {}
}

impl SemanticsNode for TextModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        config.content_description = Some(self.text().to_string());
    }

    fn reach(&self) -> cranpose_foundation::SemanticsReach {
        cranpose_foundation::SemanticsReach::default()
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
                current.measured_max_width.get(),
            ));
        }
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS
    }
}

#[cfg(test)]
#[path = "tests/text_modifier_node_tests.rs"]
mod tests;

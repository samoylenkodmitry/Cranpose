use std::{
    cell::{Cell, Ref, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

use cranpose_foundation::{
    Constraints, DelegatableNode, DrawModifierNode, InvalidationKind, LayoutModifierNode,
    Measurable, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities,
    NodeState, SemanticsConfiguration, SemanticsNode, Size,
};
use smallvec::SmallVec;

use crate::{
    density::Density,
    text::{AnnotatedString, TextLayoutOptions, TextStyle},
};

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
    density: Density,
    state: NodeState,
}

const PREPARED_LAYOUT_CACHE_CAPACITY: usize = 4;

#[derive(Clone, Debug)]
struct TextPreparedLayoutCacheEntry {
    widths: crate::text::measure::PreparedWidths,
    text_generation: u64,
    font_scale_fingerprint: u32,
    layout: Rc<crate::text::PreparedTextLayout>,
}

/// A text node's text and its prepared layouts. The node and the slices
/// built from it share one owner for the node's life: an update changes its
/// source in place, so the slices read the new text without being rebuilt.
#[derive(Debug)]
struct TextPreparedLayoutOwner {
    source: RefCell<TextLayoutSource>,
    node_id: Cell<Option<cranpose_core::NodeId>>,
    measured_max_width: Cell<Option<Option<f32>>>,
    cache: RefCell<SmallVec<[TextPreparedLayoutCacheEntry; 1]>>,
}

/// What a text is laid out from.
#[derive(Debug)]
struct TextLayoutSource {
    text: Rc<AnnotatedString>,
    style: Arc<TextStyle>,
    style_hash: u64,
    options: TextLayoutOptions,
    /// Whether layouts come from the text service's shared cache. A text
    /// changed in place, as a ticker's is every frame, lays out unshared.
    shares_layouts: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct TextPreparedLayoutHandle {
    owner: Rc<TextPreparedLayoutOwner>,
}

impl TextPreparedLayoutOwner {
    fn new(text: Rc<AnnotatedString>, style: TextStyle, options: TextLayoutOptions) -> Self {
        let style_hash = style.render_hash();
        Self {
            source: RefCell::new(TextLayoutSource {
                text,
                style: Arc::new(style),
                style_hash,
                options: options.normalized(),
                shares_layouts: true,
            }),
            node_id: Cell::new(None),
            measured_max_width: Cell::new(None),
            cache: RefCell::new(SmallVec::new()),
        }
    }

    /// Lays the text out from `text`, `style` and normalized `options` from
    /// now on, dropping the layouts prepared from the old ones. A style equal
    /// to the current one keeps being shared.
    fn update(&self, text: &Rc<AnnotatedString>, style: &TextStyle, options: TextLayoutOptions) {
        let mut source = self.source.borrow_mut();
        let same_style = *source.style == *style;
        if source.text == *text && same_style && source.options == options {
            return;
        }
        if !same_style {
            source.style = Arc::new(style.clone());
            source.style_hash = style.render_hash();
        }
        source.text = Rc::clone(text);
        source.options = options;
        source.shares_layouts = false;
        drop(source);
        self.cache.borrow_mut().clear();
    }

    fn annotated_text(&self) -> Ref<'_, Rc<AnnotatedString>> {
        Ref::map(self.source.borrow(), |source| &source.text)
    }

    fn style(&self) -> Ref<'_, TextStyle> {
        Ref::map(self.source.borrow(), |source| &*source.style)
    }

    fn options(&self) -> TextLayoutOptions {
        self.source.borrow().options
    }

    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        self.node_id.get()
    }

    fn set_node_id(&self, node_id: Option<cranpose_core::NodeId>) {
        if self.node_id.replace(node_id) != node_id {
            self.cache.borrow_mut().clear();
        }
    }

    fn with_prepared<R>(
        &self,
        max_width: Option<f32>,
        read: impl FnOnce(&Rc<crate::text::PreparedTextLayout>) -> R,
    ) -> R {
        let normalized_max_width = max_width.filter(|width| width.is_finite() && *width > 0.0);
        let (text_generation, font_scale_fingerprint) = crate::render_state::text_layout_stamp();

        {
            let mut cache = self.cache.borrow_mut();
            if let Some(index) = cache.iter().position(|entry| {
                entry.widths.hold(normalized_max_width)
                    && entry.text_generation == text_generation
                    && entry.font_scale_fingerprint == font_scale_fingerprint
            }) {
                cache[..=index].rotate_right(1);
                return read(&cache[0].layout);
            }
        }

        let source = self.source.borrow();
        let prepare = if source.shares_layouts {
            crate::text::prepare_text_layout_for_node
        } else {
            crate::text::measure::prepare_unshared_text_layout_for_node
        };
        let prepared = prepare(
            self.node_id(),
            &source.text,
            &source.style,
            source.options,
            normalized_max_width,
        );
        if Arc::ptr_eq(&prepared.visual_style, &source.style) {
            let _ = prepared.visual_style_hash.set(source.style_hash);
        }
        let widths = crate::text::measure::PreparedWidths::of(
            source.text.as_ref(),
            source.options,
            normalized_max_width,
            &prepared,
        );
        drop(source);

        let mut cache = self.cache.borrow_mut();
        cache.insert(
            0,
            TextPreparedLayoutCacheEntry {
                widths,
                text_generation,
                font_scale_fingerprint,
                layout: prepared,
            },
        );
        cache.truncate(PREPARED_LAYOUT_CACHE_CAPACITY);
        read(&cache[0].layout)
    }

    /// The size of the layout the cache holds for `max_width` and the max
    /// widths that layout holds for; `None` when the cache holds none.
    fn cached_widths(
        &self,
        max_width: Option<f32>,
    ) -> Option<(Size, crate::text::measure::PreparedWidths)> {
        let normalized_max_width = max_width.filter(|width| width.is_finite() && *width > 0.0);
        let (text_generation, font_scale_fingerprint) = crate::render_state::text_layout_stamp();
        self.cache
            .borrow()
            .iter()
            .find(|entry| {
                entry.widths.hold(normalized_max_width)
                    && entry.text_generation == text_generation
                    && entry.font_scale_fingerprint == font_scale_fingerprint
            })
            .map(|entry| {
                (
                    Size::new(entry.layout.metrics.width, entry.layout.metrics.height),
                    entry.widths,
                )
            })
    }

    fn measure_text_content(&self, max_width: Option<f32>) -> Size {
        self.with_prepared(max_width, |prepared| Size {
            width: prepared.metrics.width,
            height: prepared.metrics.height,
        })
    }

    fn measure_layout(&self, max_width: Option<f32>) -> (Size, cranpose_ui_layout::AlignmentLines) {
        self.measured_max_width.set(Some(max_width));
        self.with_prepared(max_width, |prepared| {
            (
                Size::new(prepared.metrics.width, prepared.metrics.height),
                prepared.alignment_lines,
            )
        })
    }

    fn measured_layout(&self) -> Option<Rc<crate::text::PreparedTextLayout>> {
        self.measured_max_width
            .get()
            .map(|max_width| self.with_prepared(max_width, Rc::clone))
    }
}

impl TextPreparedLayoutHandle {
    fn new(owner: Rc<TextPreparedLayoutOwner>) -> Self {
        Self { owner }
    }

    pub(crate) fn annotated_text(&self) -> Ref<'_, Rc<AnnotatedString>> {
        self.owner.annotated_text()
    }

    pub(crate) fn style(&self) -> Ref<'_, TextStyle> {
        self.owner.style()
    }

    pub(crate) fn options(&self) -> TextLayoutOptions {
        self.owner.options()
    }

    pub(crate) fn measured_layout(&self) -> Option<Rc<crate::text::PreparedTextLayout>> {
        self.owner.measured_layout()
    }
}

impl TextModifierNode {
    /// A text node sized on `density`'s device pixel grid.
    pub fn new(
        text: Rc<AnnotatedString>,
        style: TextStyle,
        options: TextLayoutOptions,
        density: Density,
    ) -> Self {
        Self {
            layout: Rc::new(TextPreparedLayoutOwner::new(text, style, options)),
            density,
            state: NodeState::new(),
        }
    }

    /// The text's size rounded up to whole device pixels, as Compose sizes a
    /// text node (`TextLayoutResult.size` is the paragraph's size, `ceil`ed),
    /// so whatever follows it starts on the pixel grid.
    fn pixel_size(&self, size: Size) -> Size {
        Size {
            width: self.density.ceil(size.width),
            height: self.density.ceil(size.height),
        }
    }

    pub fn text(&self) -> Ref<'_, str> {
        Ref::map(self.layout.annotated_text(), |text| text.text.as_str())
    }

    pub fn annotated_text(&self) -> Rc<AnnotatedString> {
        Rc::clone(&self.layout.annotated_text())
    }

    pub fn style(&self) -> Ref<'_, TextStyle> {
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
    /// A text holds while its layout does and the bounds keep its size: it
    /// measures nothing it wraps.
    fn measure_hold(
        &self,
        _density: f32,
        constraints: Constraints,
        size: Size,
        _wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        let max_width = constraints
            .max_width
            .is_finite()
            .then_some(constraints.max_width);
        let (text_size, widths) = self.layout.cached_widths(max_width)?;
        let text_size = self.pixel_size(text_size);
        if size != text_size {
            return None;
        }
        Some(cranpose_ui_layout::ConstraintsHold {
            width: cranpose_ui_layout::AxisHold {
                min: cranpose_ui_layout::BoundRange::up_to(text_size.width),
                max: cranpose_ui_layout::BoundRange::from(text_size.width)
                    .intersect(widths.max_width_range())?,
            },
            height: cranpose_ui_layout::AxisHold::sized(text_size.height),
        })
    }

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
        let (text_size, alignment_lines) = self.layout.measure_layout(max_width);
        let text_size = self.pixel_size(text_size);

        let width = text_size
            .width
            .clamp(constraints.min_width, constraints.max_width);
        let height = text_size
            .height
            .clamp(constraints.min_height, constraints.max_height);

        cranpose_ui_layout::LayoutModifierMeasureResult::with_size(Size { width, height })
            .with_alignment_lines(alignment_lines)
    }

    fn min_intrinsic_width(
        &self,
        _measurable: &dyn Measurable,
        _height: f32,
        _density: f32,
    ) -> f32 {
        self.pixel_size(self.measure_text_content(None)).width
    }

    fn max_intrinsic_width(
        &self,
        _measurable: &dyn Measurable,
        _height: f32,
        _density: f32,
    ) -> f32 {
        self.pixel_size(self.measure_text_content(None)).width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.pixel_size(
            self.measure_text_content(Some(width).filter(|w| w.is_finite() && *w > 0.0)),
        )
        .height
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.pixel_size(
            self.measure_text_content(Some(width).filter(|w| w.is_finite() && *w > 0.0)),
        )
        .height
    }
}

impl DrawModifierNode for TextModifierNode {}

impl SemanticsNode for TextModifierNode {
    fn merge_semantics(&self, config: &mut SemanticsConfiguration) {
        config
            .content_description
            .get_or_insert_with(|| self.text().to_string());
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
    density: Density,
}

impl TextModifierElement {
    /// A text laid out on `density`'s device pixel grid, the composition's
    /// [`crate::density::density`] where a `Text` is composed.
    pub fn new(
        text: Rc<AnnotatedString>,
        style: TextStyle,
        options: TextLayoutOptions,
        density: Density,
    ) -> Self {
        Self {
            text,
            style,
            options: options.normalized(),
            density,
        }
    }
}

impl Hash for TextModifierElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text.render_hash().hash(state);
        self.options.hash(state);
        self.density.density().to_bits().hash(state);
    }
}

impl ModifierNodeElement for TextModifierElement {
    type Node = TextModifierNode;

    fn create(&self) -> Self::Node {
        TextModifierNode::new(
            self.text.clone(),
            self.style.clone(),
            self.options,
            self.density,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        node.density = self.density;
        node.layout.update(&self.text, &self.style, self.options);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::DRAW | NodeCapabilities::SEMANTICS
    }
}

#[cfg(test)]
#[path = "tests/text_modifier_node_tests.rs"]
mod tests;

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
    text::{
        AnnotatedString, TextLayoutOptions, TextStyle, layout_cache::CacheAdmission,
        recent_values::RecentValues,
    },
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
    min_width: f32,
    text_generation: u64,
    font_scale_fingerprint: u32,
    layout: Rc<crate::text::PreparedTextLayout>,
}

impl TextPreparedLayoutCacheEntry {
    // Whether the layout stands for the text laid out between `min_width`
    // and `max_width` as it is: its lines and width the same.
    fn keeps(&self, (min_width, max_width): (f32, f32), stamp: (u64, u32)) -> bool {
        let layout = &self.layout.layout;
        let same = |a: f32, b: f32| a.to_bits() == b.to_bits();
        // The layout's own width breaks it into its own lines.
        same(self.min_width, min_width)
            && (self.text_generation, self.font_scale_fingerprint) == stamp
            && same(layout.width_between(min_width, max_width), layout.width())
            && same(self.layout.size().width, layout.width().min(max_width))
    }
}

/// A text node's text and its prepared layouts. The node and the slices
/// built from it share one owner for the node's life: an update changes its
/// source in place, so the slices read the new text without being rebuilt.
#[derive(Debug)]
struct TextPreparedLayoutOwner {
    source: RefCell<TextLayoutSource>,
    measured_widths: Cell<Option<(f32, f32)>>,
    cache: RefCell<SmallVec<[TextPreparedLayoutCacheEntry; 1]>>,
}

/// What a text is laid out from.
#[derive(Debug)]
struct TextLayoutSource {
    text: Rc<AnnotatedString>,
    style: Arc<TextStyle>,
    style_hash: u64,
    options: TextLayoutOptions,
    // How the text service's shared cache keeps the layouts: those of a
    // text changed in place, as a ticker's is every frame, once it comes
    // back.
    admission: CacheAdmission,
}

#[derive(Clone, Debug)]
pub(crate) struct TextPreparedLayoutHandle {
    owner: Rc<TextPreparedLayoutOwner>,
}

impl TextPreparedLayoutOwner {
    fn new(text: Rc<AnnotatedString>, style: Arc<TextStyle>, options: TextLayoutOptions) -> Self {
        let style_hash = style.render_hash();
        Self {
            source: RefCell::new(TextLayoutSource {
                text,
                style,
                style_hash,
                options: options.normalized(),
                admission: CacheAdmission::First,
            }),
            measured_widths: Cell::new(None),
            cache: RefCell::new(SmallVec::new()),
        }
    }

    /// Lays the text out from `text`, `style` and normalized `options` from
    /// now on, dropping the layouts prepared from the old ones. The source
    /// takes the element's style even when it equals the current one, so
    /// the element, the source and the layouts prepared next share one
    /// copy; the old one leaves with the layouts that hold it.
    fn update(
        &self,
        text: &Rc<AnnotatedString>,
        style: &Arc<TextStyle>,
        options: TextLayoutOptions,
    ) {
        let mut source = self.source.borrow_mut();
        let shared_style = Arc::ptr_eq(&source.style, style);
        let same_style = shared_style || *source.style == **style;
        if source.text == *text && same_style && source.options == options {
            return;
        }
        if !same_style {
            source.style_hash = style.render_hash();
        }
        if !shared_style {
            source.style = Arc::clone(style);
        }
        source.text = Rc::clone(text);
        source.options = options;
        source.admission = CacheAdmission::Changed;
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

    fn with_prepared<R>(
        &self,
        widths: (f32, f32),
        read: impl FnOnce(&Rc<crate::text::PreparedTextLayout>) -> R,
    ) -> R {
        let stamp = crate::render_state::text_layout_stamp();
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(index) = cache.iter().position(|entry| entry.keeps(widths, stamp)) {
                cache[..=index].rotate_right(1);
                return read(&cache[0].layout);
            }
        }

        let source = self.source.borrow();
        let frame = crate::text::measure::TextFrame {
            min_width: widths.0,
            max_width: widths.1,
            options: source.options,
        };
        let prepared = {
            // The cache holds layouts of this source only: the latest one, on
            // the fonts of now, takes the widths when it breaks into the same
            // lines there, as a width animates, or gives its glyphs to the new
            // one.
            let cache = self.cache.borrow();
            let latest = cache
                .first()
                .filter(|entry| (entry.text_generation, entry.font_scale_fingerprint) == stamp)
                .map(|entry| &entry.layout);
            match latest.filter(|latest| latest.layout.holds(widths.0, widths.1)) {
                Some(latest) => latest.at_widths(frame, source.style.paragraph_style.text_align),
                None => {
                    let text = crate::text::measure::SharedText {
                        text: &source.text,
                        style: &source.style,
                        style_hash: source.style_hash,
                    };
                    let shaped = latest.map(|latest| &latest.layout);
                    crate::text::measure::prepare_shared(&text, frame, source.admission, shaped)
                }
            }
        };
        if Arc::ptr_eq(&prepared.visual_style, &source.style) {
            let _ = prepared.visual_style_hash.set(source.style_hash);
        }
        drop(source);

        let mut cache = self.cache.borrow_mut();
        cache.insert(
            0,
            TextPreparedLayoutCacheEntry {
                min_width: widths.0,
                text_generation: stamp.0,
                font_scale_fingerprint: stamp.1,
                layout: prepared,
            },
        );
        cache.truncate(PREPARED_LAYOUT_CACHE_CAPACITY);
        read(&cache[0].layout)
    }

    /// The size of the layout the cache holds for `widths` and the max widths
    /// that keep it; `None` when the cache holds none.
    fn cached_widths(&self, widths: (f32, f32)) -> Option<(Size, cranpose_ui_layout::BoundRange)> {
        let stamp = crate::render_state::text_layout_stamp();
        let cache = self.cache.borrow();
        let entry = cache.iter().find(|entry| entry.keeps(widths, stamp))?;
        let prepared = &entry.layout;
        let size = prepared.size();
        // A paragraph wider than its max measures as the max, which moves
        // with it.
        if size.width < prepared.layout.width() {
            return Some((size, cranpose_ui_layout::BoundRange::exactly(widths.1)));
        }
        let (low, high) = prepared.layout.max_widths_kept(widths.0);
        Some((size, cranpose_ui_layout::BoundRange { low, high }))
    }

    /// The widest word of the text: see [`crate::text::measure::min_intrinsic_text_width`].
    fn min_intrinsic_width(&self) -> f32 {
        let source = self.source.borrow();
        crate::text::measure::min_intrinsic_text_width(&source.text, &source.style)
    }

    fn measure_text_content(&self, widths: (f32, f32)) -> Size {
        self.with_prepared(widths, |prepared| prepared.size())
    }

    fn measure_layout(&self, widths: (f32, f32)) -> (Size, cranpose_ui_layout::AlignmentLines) {
        self.measured_widths.set(Some(widths));
        self.with_prepared(widths, |prepared| {
            (prepared.size(), prepared.alignment_lines())
        })
    }

    fn measured_layout(&self) -> Option<Rc<crate::text::PreparedTextLayout>> {
        self.measured_widths
            .get()
            .map(|widths| self.with_prepared(widths, Rc::clone))
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
        style: impl Into<Arc<TextStyle>>,
        options: TextLayoutOptions,
        density: Density,
    ) -> Self {
        Self {
            layout: Rc::new(TextPreparedLayoutOwner::new(text, style.into(), options)),
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

    /// Returns the plain text of the node.
    pub fn text(&self) -> Ref<'_, str> {
        Ref::map(self.layout.annotated_text(), |text| text.text.as_str())
    }

    /// Returns the node's text with its span styles and annotations.
    pub fn annotated_text(&self) -> Rc<AnnotatedString> {
        Rc::clone(&self.layout.annotated_text())
    }

    /// Returns the style the text lays out with.
    pub fn style(&self) -> Ref<'_, TextStyle> {
        self.layout.style()
    }

    /// Returns the layout options: overflow, soft wrap, and minimum and maximum lines.
    pub fn options(&self) -> TextLayoutOptions {
        self.layout.options()
    }

    fn measure_text_content(&self, widths: (f32, f32)) -> Size {
        self.layout.measure_text_content(widths)
    }

    pub(crate) fn prepared_layout_handle(&self) -> TextPreparedLayoutHandle {
        TextPreparedLayoutHandle::new(Rc::clone(&self.layout))
    }
}

impl DelegatableNode for TextModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TextModifierNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
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
    /// A text holds while its layout does and the bounds keep its size: it
    /// measures nothing it wraps.
    fn measure_hold(
        &self,
        _density: f32,
        constraints: Constraints,
        size: Size,
        _wrapped: cranpose_ui_layout::WrappedHold,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        let (text_size, max_widths) = self.layout.cached_widths(text_widths(constraints))?;
        let text_size = self.pixel_size(text_size);
        if size != text_size {
            return None;
        }
        Some(cranpose_ui_layout::ConstraintsHold {
            width: cranpose_ui_layout::AxisHold {
                min: cranpose_ui_layout::BoundRange::exactly(constraints.min_width),
                max: cranpose_ui_layout::BoundRange::from(text_size.width).intersect(max_widths)?,
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
        let (text_size, alignment_lines) = self.layout.measure_layout(text_widths(constraints));
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
        self.density.ceil(self.layout.min_intrinsic_width())
    }

    fn max_intrinsic_width(
        &self,
        _measurable: &dyn Measurable,
        _height: f32,
        _density: f32,
    ) -> f32 {
        self.pixel_size(self.measure_text_content((0.0, f32::INFINITY)))
            .width
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.pixel_size(self.measure_text_content((0.0, intrinsic_width(width))))
            .height
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, width: f32, _density: f32) -> f32 {
        self.pixel_size(self.measure_text_content((0.0, intrinsic_width(width))))
            .height
    }
}

// The widths a text lays out between under `constraints`: Compose's
// `finalConstraints` read the min and the max width.
fn text_widths(constraints: Constraints) -> (f32, f32) {
    (constraints.min_width.max(0.0), constraints.max_width)
}

// The max width an intrinsic height is asked at; a width that is not positive
// and finite is no limit.
fn intrinsic_width(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        f32::INFINITY
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
#[derive(Debug, Clone)]
pub struct TextModifierElement {
    text: Rc<AnnotatedString>,
    /// One copy for every text composed with an equal style: see
    /// [`shared_text_style`].
    style: Arc<TextStyle>,
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
        Self::with_shared_style(text, shared_text_style(style), options, density)
    }

    /// [`Self::new`] for a style [`shared_text_style`] already shared.
    pub(crate) fn with_shared_style(
        text: Rc<AnnotatedString>,
        style: Arc<TextStyle>,
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

impl PartialEq for TextModifierElement {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && (Arc::ptr_eq(&self.style, &other.style) || self.style == other.style)
            && self.options == other.options
            && self.density == other.density
    }
}

/// How many recent text styles [`shared_text_style`] keeps.
const SHARED_TEXT_STYLES: usize = 16;

// The text styles recently composed, which [`shared_text_style`] shares.
pub(crate) type RecentTextStyles = RecentValues<Arc<TextStyle>, SHARED_TEXT_STYLES>;

/// `style` shared with the texts recently composed with an equal one, so a
/// screen of texts in a few styles keeps a few copies, not one a text, and
/// a node updated to an element of the same style compares pointers. A
/// style none of the kept [`SHARED_TEXT_STYLES`] equals gets a copy of its
/// own, kept first; the last kept one goes.
pub(crate) fn shared_text_style(style: TextStyle) -> Arc<TextStyle> {
    crate::thread_state::with(|state| {
        let mut recent = state.recent_text_styles.borrow_mut();
        if let Some(kept) = recent.find(|kept| **kept == style) {
            return Arc::clone(kept);
        }
        Arc::clone(recent.keep(Arc::new(style)))
    })
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
            Rc::clone(&self.text),
            Arc::clone(&self.style),
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

//! A round-watch scaling list, and the channel it uses to shrink an item.
//!
//! # The structural problem, and the shape chosen for it
//!
//! [`Placement`] is `{ node_id, x, y, z_index }`. It has no scale, no alpha and
//! no transform origin, so a measure policy physically cannot tell the renderer
//! to draw a child at 0.7x — and shrinking rows towards the bezel is the entire
//! point of this widget.
//!
//! Two answers were on the table. Widening `Placement` with a transform is the
//! obvious one, and it is the wrong one: `Placement` is consumed by the layout
//! engine to set a node's position, and a scale arriving that way would be a
//! second transform path competing with the one every renderer backend already
//! honours — `GraphicsLayer` — with a merge rule to invent at every backend.
//!
//! The answer taken instead is [`WearItemTransform`]: a shared cell the list's
//! measure pass writes and the item's own `graphics_layer` resolver reads. It
//! works because of one fact about when that resolver runs — **the layer
//! closure is evaluated at scene-build time, once per node per frame, after
//! measure and layout have finished**. So the scale a frame draws with is the
//! scale that frame measured, not the previous one; the value is never stale.
//! Two further properties fall out of it:
//!
//! - reads inside the resolver are tracked by the draw observer, so a change
//!   invalidates **draw only** — no recomposition, no relayout;
//! - the layer is the same mechanism AOSP uses. `ScalingLazyColumnItemWrapper`
//!   is a `graphicsLayer { alpha; scaleX = scaleY = scale; transformOrigin =
//!   TransformOrigin(0.5f, 0f) }`, and an item is rasterised once at full size
//!   and composited scaled — which is why Wear's scaled rows are softer than
//!   rows redrawn at a smaller font.
//!
//! A cost this was once documented to have, and does not: a shrunk row's touch
//! target is **not** its unscaled box. Hit testing in this engine is done on
//! the render graph, and a layer's `transform_to_parent` is built by
//! `layer_transform_to_parent` from the same resolved `GraphicsLayer` the
//! renderer draws with — scale about the transform origin included.
//! `HitRegion::contains` inverts that transform and tests the point against
//! the node's local bounds, so the target is the drawn quad. A tap in the gap
//! between two shrunken rows hits neither, and a tap outside a shrunken row's
//! narrowed width misses it too. Measured and pinned by
//! `a_shrunken_row_is_only_tappable_where_it_is_drawn` in
//! `cranpose-render-common`'s `scaling_list_scene` test.
//!
//! # How it is virtualised
//!
//! It composes the items the viewport can reach and no others. The obstacle
//! that once kept this list composing everything is that the scaling ramp is
//! stated against the running total of the **full** heights above an item, and
//! auto-centring is stated against the anchored item's height — both of which
//! read like they need every height in the list.
//!
//! Neither does, because of one arithmetic fact. Every slot height is
//! `WearDensity::ceil`ed and the gap is `WearDensity::dp`ed, so **a slot top is
//! always a whole number of device pixels**; and `round_to_px` is
//! `floor(v * d + 0.5) / d`, which commutes with subtracting a whole pixel:
//!
//! ```text
//! round_to_px(x - k/d, d) == round_to_px(x, d) - k/d
//! ```
//!
//! `centre_offset` computes `round_to_px(viewport/2 - slot.top - h/2 - offset)`
//! and the placed top adds `slot.top` straight back, so the whole `slot.top`
//! term cancels and the anchored item's top is
//! `round_to_px(viewport/2 - h/2 - offset)` — its own height and nothing else.
//! From there the measure pass walks outward one item at a time, stacking full
//! heights the way [`stack_into`] does, and stops when a slot can no longer
//! reach the viewport. A shrunken row is always **inside** its unscaled slot
//! (above the centre line the bottom edge is pinned, below it the top edge is),
//! so a slot that misses the viewport cannot draw a pixel, and the walk is
//! exact rather than a heuristic. Pinned by
//! `the_anchored_items_top_does_not_depend_on_the_heights_above_it`.
//!
//! What genuinely does need every height is the **scroll clamp**, which wants
//! the list's total extent. That comes out of a per-index height cache on the
//! state, filled as items are measured, with the mean of the known heights
//! standing in for the rest. The two ends that matter are exact anyway: at the
//! top the first item's centre is on the centre line and `scrolled()` is `0`
//! however wrong the estimate is, and at the bottom `scrolled()` and `travel()`
//! are both `last_centre - first_centre`, so the estimate cancels against
//! itself.
//!
//! The **scroll indicator** needs none of it. Wear measures its thumb in
//! fractional item indices rather than in pixels — see
//! [`crate::round_scroll_indicator::scaling_list_geometry`] — so what it reads
//! is the window this list already placed, plus the count, plus the blank at
//! each end. [`IndicatorState`] is that reading, filled by the measure pass and
//! lent out by [`WearScalingListState::with_indicator_list`]. The height cache
//! reaches it in one place only, the auto-centring spacers, and cannot carry an
//! estimate into a pixel: a spacer counts only while the row at its own end of
//! the list is on screen, and a row on screen has been measured.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::{GraphicsLayer, Modifier, TransformOrigin};
use crate::round_scaling_list::{
    leading_auto_centring_spacer, place_row_with, round_to_px, trailing_auto_centring_spacer,
    CentreAnchor, PlacedRow, ScaleAlpha, ScalingParams,
};
use crate::round_scroll_indicator::{
    scaling_list_items_with, IndicatorItem, ScalingList, ThumbLength,
};
use crate::subcompose_layout::{
    MeasurePolicy as SubcomposeMeasurePolicy, SubcomposeChild, SubcomposeLayoutNode,
    SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
use crate::widgets::wear::density::WearDensity;
use crate::widgets::Layout;
use cranpose_core::{remember, useState, MutableState, NodeId, SlotId};
use cranpose_ui_graphics::{CompositingStrategy, Size};
use cranpose_ui_layout::{Constraints, Measurable, MeasurePolicy, MeasureResult, Placement};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The channel one item's scale and alpha travel down.
///
/// A list hands one of these to each item; the item's `graphics_layer`
/// resolver reads it at scene-build time. Cloning shares the same cell — that
/// is the whole point of the type.
#[derive(Clone, Debug)]
pub struct WearItemTransform {
    cell: Rc<Cell<ScaleAlpha>>,
}

impl WearItemTransform {
    pub fn new() -> Self {
        Self {
            cell: Rc::new(Cell::new(ScaleAlpha::UNCHANGED)),
        }
    }

    pub fn get(&self) -> ScaleAlpha {
        self.cell.get()
    }

    pub fn set(&self, value: ScaleAlpha) {
        self.cell.set(value);
    }
}

impl Default for WearItemTransform {
    fn default() -> Self {
        Self::new()
    }
}

/// Two handles are equal when they are the same cell. Comparing the values
/// instead would let a recomposition swap one item's channel for another's
/// while they happened to be at the same scale.
impl PartialEq for WearItemTransform {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.cell, &other.cell)
    }
}

/// What the list worked out about itself on the last measure pass.
///
/// The scroll indicator needs the travel between the first and last item's
/// centres, not the flat content length: a centring list holds its first and
/// last rows on the centre line rather than against the display edges, so the
/// distance the content can move is centre to centre. Deriving the thumb from
/// a top-aligned content length puts it a fraction out at both ends.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WearScalingLayoutInfo {
    pub item_count: usize,
    pub viewport: f32,
    /// The first item's centre, in viewport coordinates.
    pub first_centre: f32,
    /// The last item's centre, in viewport coordinates.
    pub last_centre: f32,
    /// Top of the first item to the bottom of the last, unscaled.
    pub content: f32,
    /// How many of the items the viewport can actually see — which is now also
    /// how many the list placed, and close to how many it composed.
    pub visible: usize,
    /// How many items the measure pass composed and measured. Always at least
    /// [`Self::visible`], because the walk keeps a small band beyond each edge
    /// so a row entering the viewport is not composed in the frame it appears.
    pub composed: usize,
}

impl WearScalingLayoutInfo {
    /// How far the content can travel: the first item's centre to the last's.
    pub fn travel(self) -> f32 {
        (self.last_centre - self.first_centre).max(0.0)
    }

    /// How far it has travelled, `0.0` at the top.
    pub fn scrolled(self) -> f32 {
        (self.viewport * 0.5 - self.first_centre).clamp(0.0, self.travel())
    }
}

/// The scroll position of a [`WearScalingLazyColumn`].
///
/// It is a centre anchor — which item is on the centre line and by how much it
/// is offset — because that is the coordinate `ScalingLazyListState` keeps and
/// because a scaling list has no meaningful "first visible item": the item at
/// the top of the screen is the one shrunk to nothing.
#[derive(Clone)]
pub struct WearScalingListState {
    anchor: MutableState<CentreAnchor>,
    layout: Rc<RefCell<WearScalingLayoutInfo>>,
    heights: Rc<RefCell<ItemHeights>>,
    indicator: Rc<RefCell<IndicatorState>>,
}

impl PartialEq for WearScalingListState {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor
            && Rc::ptr_eq(&self.layout, &other.layout)
            && Rc::ptr_eq(&self.heights, &other.heights)
            && Rc::ptr_eq(&self.indicator, &other.indicator)
    }
}

/// The list as `ScalingLazyColumnStateAdapter` reads it, in device pixels.
///
/// Wear's indicator does not work in pixels at all: it takes the first and last
/// visible rows as **fractional item indices** and divides by the item count.
/// What it is handed is a `ScalingLazyListLayoutInfo` whose sizes and offsets
/// have already landed on the pixel grid, and it divides those integers, so
/// this is pixels throughout — points would quietly lose the halves that decide
/// which index an end lands on.
///
/// It is filled by the measure pass rather than derived when the indicator
/// draws, because the measure pass is the one place where the density, the
/// viewport and the rows the virtualising walk kept all exist at once.
#[derive(Debug, Default)]
struct IndicatorState {
    visible: Vec<IndicatorItem>,
    total: usize,
    viewport: f32,
    before_padding: f32,
    after_padding: f32,
    /// `currentSizeFraction`, which the adapter recomputes **only** when
    /// `totalItemsCount` changes. It lives here rather than in the widget
    /// because it belongs to one list, for as long as that list: a thumb
    /// re-measured per frame breathes as tall rows scroll past, and Wear's does
    /// not move at all.
    thumb: ThumbLength,
}

impl IndicatorState {
    /// Forgets the window, for a list with nothing in it to describe.
    ///
    /// The measured thumb length survives, because [`ThumbLength`] keys itself
    /// on the item count and an empty list is not a new length.
    fn clear(&mut self) {
        self.visible.clear();
        self.total = 0;
        self.viewport = 0.0;
        self.before_padding = 0.0;
        self.after_padding = 0.0;
    }

    /// Reads a measured window the way `ScalingLazyListLayoutInfo` reports one.
    fn record(
        &mut self,
        window: &[WindowedRow],
        spec: &WearScalingLazyColumnSpec,
        known: &ItemHeights,
        viewport: f32,
        density: WearDensity,
    ) {
        let count = known.len();
        if count == 0 {
            self.clear();
            return;
        }
        let viewport_px = density.to_px(viewport).round();
        self.total = count;
        self.viewport = viewport_px;
        scaling_list_items_with(
            spec.scaling,
            viewport,
            density.density(),
            window.iter().map(|row| (row.top, row.height)),
            &mut self.visible,
        );
        // `scaling_list_items` numbers the rows it is handed from zero, and what
        // it is handed here is a window that starts wherever the walk did.
        if let Some(base) = window.first().map(|row| row.index) {
            for item in &mut self.visible {
                item.index += base;
            }
        }
        // `beforeContentPadding + beforeAutoCenteringPadding`, and its mirror
        // below. The content padding is the one THIS list was given: a widget
        // that reaches for a number an app happens to use is right by
        // coincidence and wrong the moment a second app calls it.
        let mut before = density.to_px(density.dp(spec.content_padding_top));
        let mut after = density.to_px(density.dp(spec.content_padding_bottom));
        if let Some(anchor) = spec.auto_centering {
            // The spacers are stated against the auto-centring params, not
            // against where the list has been scrolled to: they are two items
            // Wear injects around the content once, and scrolling moves the
            // content past them rather than resizing them.
            let index = anchor.index.min(count - 1);
            let spacing = density.dp(spec.item_spacing);
            let centre = (0..index)
                .map(|i| known.height_of(i) + spacing)
                .sum::<f32>()
                + known.height_of(index) * 0.5;
            before += leading_auto_centring_spacer(
                viewport_px,
                density.to_px(centre).round(),
                density.to_px(anchor.offset),
            );
            after += trailing_auto_centring_spacer(
                viewport_px,
                density.to_px(known.height_of(count - 1)).round(),
            );
        }
        self.before_padding = before;
        self.after_padding = after;
    }
}

/// Every item height the list has measured so far, and a stand-in for the rest.
///
/// A virtualised list cannot know the height of an item it has never composed,
/// and the scroll indicator's total extent is a sum over all of them. This is
/// the same bargain `LazyListState` makes with `average_item_size`: remember
/// what has been measured, and let the mean of it speak for what has not.
///
/// The cache is cleared when the item **count** changes and not otherwise. A
/// row whose content changed is re-measured the next time it is composed, so a
/// stale entry can only ever bias the estimate for rows that are off screen —
/// never a placed row's geometry.
#[derive(Default, Debug)]
struct ItemHeights {
    known: Vec<Option<f32>>,
    sum: f32,
    count: usize,
}

impl ItemHeights {
    /// How many items the list has, measured or not.
    fn len(&self) -> usize {
        self.known.len()
    }

    fn resize(&mut self, len: usize) {
        if self.known.len() == len {
            return;
        }
        self.known.clear();
        self.known.resize(len, None);
        self.sum = 0.0;
        self.count = 0;
    }

    fn record(&mut self, index: usize, height: f32) {
        let Some(slot) = self.known.get_mut(index) else {
            return;
        };
        match slot.replace(height) {
            Some(previous) => self.sum += height - previous,
            None => {
                self.sum += height;
                self.count += 1;
            }
        }
    }

    /// The mean of the heights seen so far, or zero before anything is.
    fn estimate(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f32
        }
    }

    fn height_of(&self, index: usize) -> f32 {
        self.known
            .get(index)
            .copied()
            .flatten()
            .unwrap_or_else(|| self.estimate())
    }
}

impl WearScalingListState {
    /// The anchor, read reactively — a scope that reads this recomposes when
    /// the list scrolls.
    pub fn anchor(&self) -> CentreAnchor {
        self.anchor.get()
    }

    pub fn set_anchor(&self, anchor: CentreAnchor) {
        self.anchor.set(anchor);
    }

    /// Scrolls by a distance in layout points, positive towards the end of the
    /// list, re-anchoring on whichever item the centre line lands in.
    ///
    /// The re-anchoring matters: leaving the index alone and letting the offset
    /// grow works until the anchored item scrolls off, after which the ramp is
    /// computed from an item nobody can see.
    pub fn scroll_by(&self, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        let info = *self.layout.borrow();
        let anchor = self.anchor.get();
        self.set_anchor(re_anchor(anchor, delta, info));
    }

    /// What the last measure pass worked out.
    pub fn layout_info(&self) -> WearScalingLayoutInfo {
        *self.layout.borrow()
    }

    /// Reads the list the way `ScalingLazyColumnStateAdapter` reads one, for
    /// [`crate::round_scroll_indicator::scaling_list_geometry`].
    ///
    /// The window is lent to a closure rather than returned because it is a
    /// buffer the measure pass owns and refills; copying it out would put an
    /// allocation per frame inside a draw. The [`ThumbLength`] comes with it
    /// because it is the adapter's own field rather than the caller's — the
    /// thumb is measured once per list and kept — and one list has exactly one.
    pub fn with_indicator_list<R>(
        &self,
        read: impl FnOnce(&mut ThumbLength, ScalingList<'_>) -> R,
    ) -> R {
        let mut indicator = self.indicator.borrow_mut();
        let IndicatorState {
            visible,
            total,
            viewport,
            before_padding,
            after_padding,
            thumb,
        } = &mut *indicator;
        read(
            thumb,
            ScalingList {
                visible: visible.as_slice(),
                total: *total,
                viewport: *viewport,
                before_padding: *before_padding,
                after_padding: *after_padding,
            },
        )
    }
}

/// Moves an anchor by `delta` and settles it on the nearest item.
///
/// Kept as a free function so the arithmetic is testable without composing.
fn re_anchor(anchor: CentreAnchor, delta: f32, info: WearScalingLayoutInfo) -> CentreAnchor {
    let mut anchor = CentreAnchor {
        index: anchor.index,
        offset: anchor.offset + delta,
    };
    if info.item_count == 0 {
        return anchor;
    }
    // Clamp so the centre line stays between the first and last item's
    // centres. Past either it is the auto-centring spacer's job to hold the
    // list still, and a list that keeps counting travels away from its content.
    let travelled = info.scrolled() + delta;
    let travel = info.travel();
    if travelled < 0.0 {
        anchor.offset -= travelled;
    } else if travelled > travel {
        anchor.offset -= travelled - travel;
    }
    anchor
}

/// Remembers a scaling list's scroll position.
#[composable]
pub fn rememberWearScalingListState(initial: CentreAnchor) -> WearScalingListState {
    let anchor = useState(move || initial);
    let layout = remember(|| Rc::new(RefCell::new(WearScalingLayoutInfo::default())))
        .with(|value| value.clone());
    let heights =
        remember(|| Rc::new(RefCell::new(ItemHeights::default()))).with(|value| value.clone());
    let indicator =
        remember(|| Rc::new(RefCell::new(IndicatorState::default()))).with(|value| value.clone());
    WearScalingListState {
        anchor,
        layout,
        heights,
        indicator,
    }
}

/// How a [`WearScalingLazyColumn`] is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearScalingLazyColumnSpec {
    /// The six knobs of the shrink-and-fade ramp.
    pub scaling: ScalingParams,
    /// `Arrangement.spacedBy(4.dp)`, Wear's default.
    pub item_spacing: f32,
    pub content_padding_start: f32,
    pub content_padding_end: f32,
    pub content_padding_top: f32,
    pub content_padding_bottom: f32,
    /// Which item sits on the centre line at rest. `None` places the content
    /// from the top instead, which is what `autoCentering = null` does.
    pub auto_centering: Option<CentreAnchor>,
    /// Whether a faded item is composited through its own surface. `Auto`
    /// isolates whenever alpha is under one, which is correct where a row's
    /// content overlaps itself — a label crossing its own capsule, say — and
    /// costs a render target per faded row. `ModulateAlpha` folds the alpha
    /// into each primitive instead, which is cheaper and visibly different at
    /// exactly those overlaps.
    pub compositing_strategy: CompositingStrategy,
    /// How many items past each edge of the viewport are composed and measured
    /// but not placed.
    ///
    /// The same knob `LazyColumnSpec` carries, and for the same reason: a row
    /// composed in the frame it first becomes visible pays for its whole
    /// subtree in that frame. Composing a band ahead moves that cost off the
    /// frame the row appears in. These rows are deliberately **not placed** —
    /// an off-screen row placed under `CompositingStrategy::Auto` at an alpha
    /// below one still takes a render target the clip then throws away.
    pub beyond_bounds_item_count: usize,
}

impl Default for WearScalingLazyColumnSpec {
    fn default() -> Self {
        Self {
            scaling: ScalingParams::WEAR,
            item_spacing: 4.0,
            content_padding_start: 0.0,
            content_padding_end: 0.0,
            content_padding_top: 0.0,
            content_padding_bottom: 0.0,
            auto_centering: Some(CentreAnchor::default()),
            compositing_strategy: CompositingStrategy::Auto,
            beyond_bounds_item_count: 2,
        }
    }
}

impl WearScalingLazyColumnSpec {
    pub fn content_padding(mut self, horizontal: f32, vertical: f32) -> Self {
        self.content_padding_start = horizontal;
        self.content_padding_end = horizontal;
        self.content_padding_top = vertical;
        self.content_padding_bottom = vertical;
        self
    }

    pub fn item_spacing(mut self, spacing: f32) -> Self {
        self.item_spacing = spacing;
        self
    }

    pub fn scaling(mut self, scaling: ScalingParams) -> Self {
        self.scaling = scaling;
        self
    }

    pub fn compositing_strategy(mut self, strategy: CompositingStrategy) -> Self {
        self.compositing_strategy = strategy;
        self
    }

    pub fn auto_centering(mut self, anchor: Option<CentreAnchor>) -> Self {
        self.auto_centering = anchor;
        self
    }

    pub fn beyond_bounds_item_count(mut self, count: usize) -> Self {
        self.beyond_bounds_item_count = count;
        self
    }
}

/// The slot an item is composed into.
///
/// A caller declares items through this rather than calling composables
/// directly, because each item needs its own transform channel and its own
/// group key, and both are bookkeeping the list can do and a caller should not
/// have to. The closure body runs when the item is composed, not when it is
/// declared — this collects factories, exactly as `LazyColumn`'s interval
/// content does.
#[derive(Default)]
pub struct WearScalingListScope {
    items: Vec<Rc<dyn Fn()>>,
}

impl WearScalingListScope {
    /// One item.
    pub fn item<F>(&mut self, content: F)
    where
        F: Fn() + 'static,
    {
        self.items.push(Rc::new(content));
    }

    /// `count` items, each handed its index.
    pub fn items<F>(&mut self, count: usize, item: F)
    where
        F: Fn(usize) + Clone + 'static,
    {
        for index in 0..count {
            let item = item.clone();
            self.item(move || item(index));
        }
    }

    /// How many items have been declared.
    pub fn count(&self) -> usize {
        self.items.len()
    }
}

/// The item factories a list was built with.
///
/// Compared by identity, so a rebuilt list is always a changed argument. That
/// is the same bargain `Canvas` makes with its draw closure: the content cannot
/// be compared by value, so it is never skipped.
#[derive(Clone)]
pub struct WearScalingListContent {
    items: Rc<Vec<Rc<dyn Fn()>>>,
}

impl PartialEq for WearScalingListContent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Default for WearScalingListContent {
    fn default() -> Self {
        Self {
            items: Rc::new(Vec::new()),
        }
    }
}

/// One item of a scaling list, wrapped in the layer that shrinks and fades it.
///
/// `TransformOrigin` is `(0.5, 0.0)` — the horizontal centre and the top edge —
/// on every item, exactly as AOSP sets it. The pinning of whichever edge faces
/// the centre line is **not** done with the origin; it is already in the `top`
/// the measure pass placed the item at. Doing both pins the row twice and
/// doubles the drift.
#[composable]
pub fn WearScalingItem<F>(
    modifier: Modifier,
    transform: WearItemTransform,
    compositing_strategy: CompositingStrategy,
    content: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    let layer_transform = transform.clone();
    let layered = modifier.graphics_layer(move || {
        let value = layer_transform.get();
        GraphicsLayer {
            alpha: value.alpha,
            scale: value.scale,
            transform_origin: TransformOrigin::new(0.5, 0.0),
            compositing_strategy,
            ..GraphicsLayer::default()
        }
    });
    Layout(layered, WearItemMeasurePolicy, content)
}

/// An item is a plain box around its content: one child, measured loosely, sized
/// to it.
#[derive(Clone, Debug, PartialEq)]
struct WearItemMeasurePolicy;

impl MeasurePolicy for WearItemMeasurePolicy {
    fn measure(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = self.measure_into(measurables, constraints, &mut placements);
        MeasureResult::new(size, placements)
    }

    fn measure_into(
        &self,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> Size {
        placements.clear();
        let mut width = constraints.min_width;
        let mut height = constraints.min_height;
        for measurable in measurables {
            let placeable = measurable.measure(constraints);
            width = width.max(placeable.width());
            height = height.max(placeable.height());
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }
        Size::new(
            width.clamp(constraints.min_width, constraints.max_width),
            height.clamp(constraints.min_height, constraints.max_height),
        )
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}

/// A round-watch list that shrinks and fades its rows towards the bezel.
///
/// ```rust,ignore
/// WearScalingLazyColumn(
///     Modifier::empty().fill_max_size(),
///     state.clone(),
///     WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
///     |scope| {
///         scope.item(|| ListHeader(Modifier::empty(), header_spec, || Text(...)));
///         scope.items(rows.len(), move |index| { ... });
///     },
/// );
/// ```
pub fn WearScalingLazyColumn<F>(
    modifier: Modifier,
    state: WearScalingListState,
    spec: WearScalingLazyColumnSpec,
    content: F,
) -> NodeId
where
    F: FnOnce(&mut WearScalingListScope),
{
    let mut scope = WearScalingListScope::default();
    content(&mut scope);
    WearScalingLazyColumnNode(
        modifier,
        state,
        spec,
        WearScalingListContent {
            items: Rc::new(scope.items),
        },
    )
}

/// What the last composition handed the measure policy.
///
/// A subcomposing list reads its inputs during measure, not during
/// composition, so they travel in a cell rather than in a policy value. The
/// policy itself is remembered once and never rebuilt, which is what lets the
/// slot table survive from frame to frame.
#[derive(Default)]
struct WearScalingListInputs {
    spec: WearScalingLazyColumnSpec,
    anchor: CentreAnchor,
    content: WearScalingListContent,
}

/// The node half of [`WearScalingLazyColumn`], with its items already declared.
#[composable]
pub fn WearScalingLazyColumnNode(
    modifier: Modifier,
    state: WearScalingListState,
    spec: WearScalingLazyColumnSpec,
    content: WearScalingListContent,
) -> NodeId {
    let transforms = remember(|| Rc::new(RefCell::new(Vec::<WearItemTransform>::new())))
        .with(|value| value.clone());
    let inputs = remember(|| Rc::new(RefCell::new(WearScalingListInputs::default())))
        .with(|value| value.clone());

    // Reading the anchor here is what makes a scroll re-measure the list. The
    // per-item scale does NOT come back through composition — it rides the
    // transform channel, which the scene build reads directly. What this body
    // no longer does is compose the items: they are subcomposed inside the
    // measure pass, so a scroll costs the window and not the list.
    let anchor = if spec.auto_centering.is_some() {
        state.anchor()
    } else {
        CentreAnchor {
            index: 0,
            offset: 0.0,
        }
    };
    transforms
        .borrow_mut()
        .resize_with(content.items.len(), WearItemTransform::new);
    let inputs_changed = {
        let mut current = inputs.borrow_mut();
        let changed =
            current.spec != spec || current.anchor != anchor || current.content != content;
        if changed {
            current.spec = spec;
            current.anchor = anchor;
            current.content = content;
        }
        changed
    };

    let policy: Rc<SubcomposeMeasurePolicy> = remember({
        let inputs = inputs.clone();
        let transforms = transforms.clone();
        let layout = state.layout.clone();
        let heights = state.heights.clone();
        let indicator = state.indicator.clone();
        move || {
            let policy: Rc<SubcomposeMeasurePolicy> = Rc::new(
                move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                    measure_wear_scaling_list(
                        scope,
                        constraints,
                        &inputs.borrow(),
                        &transforms,
                        &layout,
                        &heights,
                        &indicator,
                    )
                },
            );
            policy
        }
    })
    .with(|policy| policy.clone());

    let modifier = modifier.clip_to_bounds();
    // The state's layout cell is allocated once per remembered state and lives
    // exactly as long as it, so its address is a stable identity for the node.
    let list_id = Rc::as_ptr(&state.layout) as usize;
    let node_id = cranpose_core::with_current_composer(|composer| {
        composer.with_key(&(list_id, "WearScalingLazyColumnNode"), |composer| {
            composer.emit_node({
                let modifier = modifier.clone();
                let policy = Rc::clone(&policy);
                move || SubcomposeLayoutNode::with_content_type_policy(modifier, policy)
            })
        })
    });
    // Items are composed during measure, so they need the call site's locals
    // and source scope to reach them.
    let captured_context =
        cranpose_core::with_current_composer(|composer| composer.capture_composition_context());
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        if !node.modifier().structural_eq(&modifier) {
            node.set_modifier(modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        if inputs_changed {
            node.request_measure_recompose();
        }
    }) {
        debug_assert!(false, "failed to update WearScalingLazyColumn node: {err}");
    }
    node_id
}

/// One item's place in the column, once it has been composed and measured.
struct WindowedRow {
    index: usize,
    /// The item's root nodes and where each sits inside the item.
    roots: Vec<(NodeId, f32, f32)>,
    /// The outward walk's cursor for this row: the drawn edge of the row
    /// between it and the anchor, plus the gap. Its full height is stated from
    /// here, and the ramp is read off that box.
    top: f32,
    /// Unscaled height, already on the pixel grid.
    height: f32,
    /// Where the row is actually drawn, and by how much it shrank and faded.
    placed: PlacedRow,
}

/// Composes, measures and places only the items the viewport can reach.
fn measure_wear_scaling_list(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    inputs: &WearScalingListInputs,
    transforms: &Rc<RefCell<Vec<WearItemTransform>>>,
    layout: &Rc<RefCell<WearScalingLayoutInfo>>,
    heights: &Rc<RefCell<ItemHeights>>,
    indicator: &Rc<RefCell<IndicatorState>>,
) -> MeasureResult {
    let spec = inputs.spec;
    let top_aligned = spec.auto_centering.is_none();
    let scale = WearDensity::current().density();
    let density = WearDensity::new(scale, 1.0);
    let width = if constraints.max_width.is_finite() {
        constraints.max_width
    } else {
        constraints.min_width
    };
    // Wear scales against the FULL component height: `calculateItemInfo`
    // passes `viewPortStartPx = 0, viewPortEndPx = viewportHeightPx` and
    // `viewportHeightPx` is `constraints.maxHeight`, with no padding taken
    // off. A row near the top of the padded area is therefore already well
    // down the ramp.
    let viewport = if constraints.max_height.is_finite() {
        constraints.max_height
    } else {
        constraints.min_height
    };

    let inset = density.dp(spec.content_padding_start) + density.dp(spec.content_padding_end);
    let item_width = (width - inset).max(0.0);
    let child_constraints = Constraints {
        min_width: 0.0,
        max_width: item_width,
        min_height: 0.0,
        max_height: f32::INFINITY,
    };
    let spacing = density.dp(spec.item_spacing);
    let count = inputs.content.items.len();

    transforms
        .borrow_mut()
        .resize_with(count, WearItemTransform::new);
    heights.borrow_mut().resize(count);

    if count == 0 {
        *layout.borrow_mut() = WearScalingLayoutInfo {
            item_count: 0,
            viewport,
            ..WearScalingLayoutInfo::default()
        };
        indicator.borrow_mut().clear();
        return scope
            .layout_with_placement_builder(width, viewport, |placements| placements.clear());
    }

    // Wear pools a slot per row and reuses it for whichever row needs it next;
    // the limit is what keeps a long list from retaining every row it ever saw.
    scope.set_reusable_pool_limits(REUSABLE_SLOTS, REUSABLE_SLOTS);

    let anchor = inputs.anchor;
    let start = if top_aligned {
        0
    } else {
        anchor.index.min(count - 1)
    };

    let mut window: Vec<WindowedRow> = Vec::new();
    let anchored = compose_and_measure_item(
        scope,
        start,
        inputs,
        transforms,
        heights,
        &density,
        child_constraints,
    );

    // Auto-centring absorbs the vertical content padding; a top-aligned
    // list does not. Wear's `LazyColumn` places item 0 one
    // `beforeContentPadding` down, and `ScalingLazyListState.scrollToItem`
    // asks for `beforeContentPaddingPx - viewportCenterLinePx + size/2`,
    // whose first term exists to take that back
    // (`ScalingLazyListState.kt:499`). The auto-centring spacer is sized
    // without seeing the padding, so it is exactly one padding short of
    // holding the anchor on the centre line by itself -- but it is never
    // the thing that decides, because the initial scroll is never clamped
    // by it: the spacer is `centreLine - size/2` of content above the
    // anchor and the scroll asks for `centreLine - padding - size/2`, which
    // is less. So the anchored item's centre lands on the centre line, and
    // the padding does not move the column at all.
    //
    // Measured on `sdk_gwear` at density 2, viewport 454, 34dp vertical
    // padding: Settings' header row is [36,175]..[418,279] in both the
    // framebuffer and the accessibility tree, centre 227 = 454/2, and
    // Credits' `"Version 1.0.0-debug"` is [60,211]..[394,243], centre 227.
    //
    // The centred form drops the `slot.top` `centre_offset` would subtract and
    // add straight back: every slot top is a whole number of device pixels, and
    // `round_to_px` commutes with a whole pixel. See the module docs.
    let anchored_top = if top_aligned {
        density.dp(spec.content_padding_top)
    } else {
        round_to_px(
            viewport * 0.5 - anchored.height * 0.5 - anchor.offset,
            scale,
        )
    };
    let anchored_height = anchored.height;
    // The walk stacks the DRAWN boxes, which is what `layoutInfo` does and not
    // what the `LazyColumn` underneath does: its cursor advances by
    // `PlacedRow::reported_height`, the scaled size the layout reports, rather
    // than by the row's full one. See `round_scaling_list`'s module docs. The
    // anchored row is never scaled, so both accounts seed the walk identically.
    let place = |top: f32, height: f32| {
        place_row_with(spec.scaling, viewport, top, height, scale).unwrap_or(PlacedRow {
            top,
            height,
            reported_height: height,
            scale: 1.0,
            alpha: 1.0,
        })
    };
    let anchored_placed = place(anchored_top, anchored_height);
    window.push(WindowedRow {
        index: start,
        roots: anchored.roots,
        top: anchored_top,
        height: anchored_height,
        placed: anchored_placed,
    });

    // Upward, until a row can no longer reach the top edge. A shrunken row is
    // always inside the box its cursor gave it, so a box that misses the
    // viewport draws nothing at all and the walk can stop on it.
    let mut edge = anchored_top;
    let mut budget = spec.beyond_bounds_item_count;
    let mut index = start;
    while index > 0 {
        index -= 1;
        let bottom = edge - spacing;
        if bottom <= 0.0 {
            if budget == 0 {
                break;
            }
            budget -= 1;
        }
        let item = compose_and_measure_item(
            scope,
            index,
            inputs,
            transforms,
            heights,
            &density,
            child_constraints,
        );
        // The ramp is read off the row's FULL box hanging from `bottom`; the
        // cursor then drops by only what the row was shrunk to.
        let top = bottom - item.height;
        let placed = place(top, item.height);
        edge = bottom - placed.reported_height;
        window.push(WindowedRow {
            index,
            roots: item.roots,
            top,
            height: item.height,
            placed,
        });
    }

    // Downward, the same walk against the bottom edge.
    let mut edge = anchored_top + anchored_placed.reported_height;
    let mut budget = spec.beyond_bounds_item_count;
    for index in (start + 1)..count {
        let top = edge + spacing;
        if top >= viewport {
            if budget == 0 {
                break;
            }
            budget -= 1;
        }
        let item = compose_and_measure_item(
            scope,
            index,
            inputs,
            transforms,
            heights,
            &density,
            child_constraints,
        );
        let placed = place(top, item.height);
        edge = top + placed.reported_height;
        window.push(WindowedRow {
            index,
            roots: item.roots,
            top,
            height: item.height,
            placed,
        });
    }

    // The walk runs outward from the anchor; the tree is read in index order by
    // hit testing, semantics and every golden that counts rows, so the window
    // is put back in list order before anything is placed.
    window.sort_unstable_by_key(|row| row.index);

    let composed = window.len();
    let left = density.dp(spec.content_padding_start);
    let mut visible = 0usize;
    let result = {
        let handles = transforms.borrow();
        scope.layout_with_placement_builder(width, viewport, |placements| {
            placements.clear();
            for item in &window {
                let row = item.placed;
                if let Some(transform) = handles.get(item.index) {
                    transform.set(ScaleAlpha {
                        scale: row.scale,
                        alpha: row.alpha,
                    });
                }
                // Only the rows the viewport can see are placed. The beyond-bounds
                // rows stay composed and measured — that is what they are for —
                // but placing one costs an offscreen render target under
                // `CompositingStrategy::Auto` for a rectangle the clip discards.
                if row.top >= viewport || row.top + row.height <= 0.0 {
                    continue;
                }
                visible += 1;
                for &(node_id, offset, root_width) in &item.roots {
                    let x = left + density.centre(item_width, root_width);
                    placements.push(Placement::new(node_id, x, row.top + offset, 0));
                }
            }
        })
    };

    let known = heights.borrow();
    indicator
        .borrow_mut()
        .record(&window, &spec, &known, viewport, density);

    let before: f32 = (0..start).map(|i| known.height_of(i) + spacing).sum();
    let after: f32 = ((start + 1)..count)
        .map(|i| spacing + known.height_of(i))
        .sum();
    let first_top = anchored_top - before;
    let last_bottom = anchored_top + anchored_height + after;
    *layout.borrow_mut() = WearScalingLayoutInfo {
        item_count: count,
        viewport,
        first_centre: first_top + known.height_of(0) * 0.5,
        last_centre: last_bottom - known.height_of(count - 1) * 0.5,
        content: last_bottom - first_top,
        visible,
        composed,
    };

    result
}

/// How many recycled row slots the list keeps warm.
const REUSABLE_SLOTS: usize = 32;

/// One item, composed into its slot and measured.
struct MeasuredItem {
    roots: Vec<(NodeId, f32, f32)>,
    height: f32,
}

fn compose_and_measure_item(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    index: usize,
    inputs: &WearScalingListInputs,
    transforms: &Rc<RefCell<Vec<WearItemTransform>>>,
    heights: &Rc<RefCell<ItemHeights>>,
    density: &WearDensity,
    child_constraints: Constraints,
) -> MeasuredItem {
    let transform = transforms
        .borrow()
        .get(index)
        .cloned()
        .unwrap_or_else(WearItemTransform::new);
    let strategy = inputs.spec.compositing_strategy;
    let item = inputs.content.items[index].clone();
    let children: Vec<SubcomposeChild> = scope.subcompose(SlotId(index as u64), move || {
        let item = item.clone();
        WearScalingItem(Modifier::empty(), transform.clone(), strategy, move || {
            item()
        });
    });

    let mut roots = Vec::with_capacity(children.len());
    let mut stacked = 0.0f32;
    for child in children {
        let placeable = scope.measure(child, child_constraints);
        roots.push((placeable.node_id(), stacked, placeable.width()));
        stacked += placeable.height();
    }
    // The slot height is ceiled to a whole device pixel, as Compose's integral
    // layout does, before anything stacks on top of it. That is also what makes
    // every slot top a whole pixel, which the anchored-top arithmetic relies on.
    let height = density.ceil(stacked);
    heights.borrow_mut().record(index, height);
    MeasuredItem { roots, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(count: usize, viewport: f32, first: f32, last: f32) -> WearScalingLayoutInfo {
        WearScalingLayoutInfo {
            item_count: count,
            viewport,
            first_centre: first,
            last_centre: last,
            content: last - first,
            visible: count,
            composed: count,
        }
    }

    #[test]
    fn a_transform_handle_is_the_cell_not_the_value() {
        let one = WearItemTransform::new();
        let shared = one.clone();
        let other = WearItemTransform::new();
        assert_eq!(one, shared);
        assert_ne!(one, other, "two fresh cells are two channels");
        shared.set(ScaleAlpha {
            scale: 0.7,
            alpha: 0.5,
        });
        assert_eq!(one.get().scale, 0.7, "a clone writes through");
    }

    #[test]
    fn travel_is_measured_between_the_first_and_last_centres() {
        // A centring list holds its first and last rows on the centre line, so
        // the content can only move as far as the gap between those centres.
        let info = info(3, 454.0, 227.0, 627.0);
        assert_eq!(info.travel(), 400.0);
        assert_eq!(info.scrolled(), 0.0, "at rest the first row is centred");
    }

    #[test]
    fn scrolling_past_an_end_stops_instead_of_counting_on() {
        let info = info(3, 454.0, 227.0, 627.0);
        let anchor = CentreAnchor {
            index: 0,
            offset: 0.0,
        };
        let up = re_anchor(anchor, -50.0, info);
        assert_eq!(up.offset, 0.0, "already at the top");
        let down = re_anchor(anchor, 120.0, info);
        assert_eq!(down.offset, 120.0);
        let past = re_anchor(anchor, 900.0, info);
        assert_eq!(past.offset, 400.0, "clamped to the whole travel");
    }

    #[test]
    fn an_empty_list_does_not_divide_by_its_own_travel() {
        let empty = WearScalingLayoutInfo::default();
        assert_eq!(empty.travel(), 0.0);
        assert_eq!(empty.scrolled(), 0.0);
        let anchor = CentreAnchor::default();
        assert_eq!(re_anchor(anchor, 30.0, empty).offset, 30.0);
    }

    #[test]
    fn the_spec_defaults_are_the_ones_wear_ships() {
        let spec = WearScalingLazyColumnSpec::default();
        assert_eq!(spec.item_spacing, 4.0, "Arrangement.spacedBy(4.dp)");
        assert_eq!(
            spec.auto_centering,
            Some(CentreAnchor {
                index: 1,
                offset: 0.0
            }),
            "AutoCenteringParams(itemIndex = 1, itemOffset = 0)"
        );
        assert_eq!(spec.scaling, ScalingParams::WEAR);
    }
}

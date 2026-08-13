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
//! # What this list is and is not
//!
//! It composes every item and places only the ones the viewport can see. It is
//! not virtualised over `LazyListState`: the scaling ramp needs the running
//! total of the **full** heights above an item, and the centre anchor needs the
//! anchored item's height, so a from-scratch virtualising pass is a separate
//! piece of work from the transform channel this module exists to settle. On
//! the screens this was built against — 9 items and 26 items — every item is
//! composed by Wear as well, because auto-centring plus the 5 % over-compose
//! band reaches almost the whole list.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::{GraphicsLayer, Modifier, TransformOrigin};
use crate::round_scaling_list::{
    centre_offset, place_row_with, stack_into, CentreAnchor, ScaleAlpha, ScalingParams, Slot,
};
use crate::widgets::wear::density::WearDensity;
use crate::widgets::Layout;
use cranpose_core::{remember, useState, MutableState, NodeId};
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
    /// How many of the items the viewport can actually see. Every item is
    /// placed either way; this is what a caller counts, not what the list drew.
    pub visible: usize,
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
}

impl PartialEq for WearScalingListState {
    fn eq(&self, other: &Self) -> bool {
        self.anchor == other.anchor && Rc::ptr_eq(&self.layout, &other.layout)
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
    WearScalingListState { anchor, layout }
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
    // Reading the anchor here is what makes a scroll re-measure the list. The
    // per-item scale does NOT come back through composition — it rides the
    // transform channel, which the scene build reads directly.
    let anchor = if spec.auto_centering.is_some() {
        state.anchor()
    } else {
        CentreAnchor {
            index: 0,
            offset: 0.0,
        }
    };
    {
        let mut transforms = transforms.borrow_mut();
        transforms.resize_with(content.items.len(), WearItemTransform::new);
    }
    let policy = WearScalingListPolicy {
        spec,
        anchor,
        top_aligned: spec.auto_centering.is_none(),
        density: WearDensity::current().density(),
        transforms: transforms.clone(),
        layout: state.layout.clone(),
    };
    let strategy = spec.compositing_strategy;
    let handles = transforms;
    Layout(modifier.clip_to_bounds(), policy, move || {
        for (index, item) in content.items.iter().enumerate() {
            let transform = handles.borrow()[index].clone();
            let item = item.clone();
            cranpose_core::with_key(&index, || {
                WearScalingItem(Modifier::empty(), transform.clone(), strategy, move || {
                    item();
                });
            });
        }
    })
}

#[derive(Clone)]
struct WearScalingListPolicy {
    spec: WearScalingLazyColumnSpec,
    anchor: CentreAnchor,
    top_aligned: bool,
    density: f32,
    transforms: Rc<RefCell<Vec<WearItemTransform>>>,
    layout: Rc<RefCell<WearScalingLayoutInfo>>,
}

impl PartialEq for WearScalingListPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.spec == other.spec
            && self.anchor == other.anchor
            && self.top_aligned == other.top_aligned
            && self.density == other.density
            && Rc::ptr_eq(&self.transforms, &other.transforms)
            && Rc::ptr_eq(&self.layout, &other.layout)
    }
}

impl MeasurePolicy for WearScalingListPolicy {
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
        let density = WearDensity::new(self.density, 1.0);
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

        let inset =
            density.dp(self.spec.content_padding_start) + density.dp(self.spec.content_padding_end);
        let item_width = (width - inset).max(0.0);
        let child_constraints = Constraints {
            min_width: 0.0,
            max_width: item_width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };

        let mut placeables = Vec::with_capacity(measurables.len());
        for measurable in measurables {
            placeables.push(measurable.measure(child_constraints));
        }

        let mut slots: Vec<Slot> = Vec::with_capacity(placeables.len());
        stack_into(
            placeables.iter().map(|p| density.ceil(p.height())),
            density.dp(self.spec.item_spacing),
            &mut slots,
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
        let offset = if self.top_aligned {
            density.dp(self.spec.content_padding_top)
        } else {
            centre_offset(&slots, viewport, self.anchor, self.density)
        };

        let transforms = self.transforms.borrow();
        let mut visible = 0usize;
        for (index, (slot, placeable)) in slots.iter().zip(placeables.iter()).enumerate() {
            let top = slot.top + offset;
            let row = place_row_with(self.spec.scaling, viewport, top, slot.height, self.density)
                .unwrap_or(crate::round_scaling_list::PlacedRow {
                    top,
                    height: slot.height,
                    scale: 1.0,
                    alpha: 1.0,
                });
            if let Some(transform) = transforms.get(index) {
                transform.set(ScaleAlpha {
                    scale: row.scale,
                    alpha: row.alpha,
                });
            }
            if row.top < viewport && row.top + row.height > 0.0 {
                visible += 1;
            }
            // Every item is placed, including the ones off screen. Leaving one
            // unplaced looks like a saving and is not: the node keeps the
            // position it last had, so anything reading the tree — a hit test,
            // a golden, the next frame's ramp — sees a stale rectangle rather
            // than an absent one. The list clips to its bounds, so an off-screen
            // row costs a clipped draw and nothing else.
            let x = density.dp(self.spec.content_padding_start)
                + density.centre(item_width, placeable.width());
            placements.push(Placement::new(placeable.node_id(), x, row.top, 0));
        }

        *self.layout.borrow_mut() = WearScalingLayoutInfo {
            item_count: slots.len(),
            viewport,
            first_centre: slots.first().map(|s| s.centre() + offset).unwrap_or(0.0),
            last_centre: slots.last().map(|s| s.centre() + offset).unwrap_or(0.0),
            content: slots
                .last()
                .zip(slots.first())
                .map(|(last, first)| last.bottom() - first.top)
                .unwrap_or(0.0),
            visible,
        };

        Size::new(width, viewport)
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
        let spacing = self.spec.item_spacing * measurables.len().saturating_sub(1) as f32;
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .sum::<f32>()
            + spacing
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        self.min_intrinsic_height(measurables, width)
    }
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

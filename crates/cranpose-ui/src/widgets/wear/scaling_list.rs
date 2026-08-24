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
//! `Density::ceil`ed and the gap is `Density::dp`ed, so **a slot top is
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
//! heights the way `stack_into` does, and stops when a slot can no longer
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
//! each end. `IndicatorState` is that reading, filled by the measure pass and
//! lent out by [`WearScalingListState::with_indicator_list`]. The height cache
//! reaches it in one place only, the auto-centring spacers, and cannot carry an
//! estimate into a pixel: a spacer counts only while the row at its own end of
//! the list is on screen, and a row on screen has been measured.

#![allow(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    internal::FrameCallbackRegistration, remember, rememberMutableStateOf, MutableState, NodeId,
    SlotId,
};
use cranpose_foundation::{
    lazy::{LazyItems, LazyLayoutKey},
    VelocityTracker1D, DRAG_THRESHOLD, MAX_FLING_VELOCITY,
};
use cranpose_ui_graphics::{CompositingStrategy, Point, Rect, Size};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    composable,
    density::Density,
    fling_animation::FlingAnimation,
    modifier::{GraphicsLayer, Modifier, PointerEventKind, PointerInputScope, TransformOrigin},
    round_scaling_list::{
        leading_auto_centring_spacer, place_row_with, round_to_px, trailing_auto_centring_spacer,
        CentreAnchor, PlacedRow, ScaleAlpha, ScalingParams,
    },
    round_scroll_indicator::{scaling_list_items_with, IndicatorItem, ScalingList, ThumbLength},
    subcompose_layout::{
        MeasurePolicy as SubcomposeMeasurePolicy, SubcomposeChild, SubcomposeLayoutNode,
        SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
    },
    widgets::Layout,
};

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

/// One row from the last scaling-list measure pass, in the list's local
/// coordinate space after scaling and placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearScalingItemInfo {
    pub index: usize,
    pub bounds: Rect,
    pub centre: f32,
    pub unscaled_height: f32,
    pub scale: f32,
    pub alpha: f32,
}

impl WearScalingItemInfo {
    pub fn contains(self, point: Point) -> bool {
        point.x >= self.bounds.x
            && point.x < self.bounds.x + self.bounds.width
            && point.y >= self.bounds.y
            && point.y < self.bounds.y + self.bounds.height
    }
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
#[derive(Clone, Copy)]
pub struct WearScalingListState {
    anchor: MutableState<CentreAnchor>,
    inner: MutableState<Rc<WearScalingListInner>>,
}

struct WearScalingListInner {
    layout: Rc<RefCell<WearScalingLayoutInfo>>,
    items: Rc<RefCell<Vec<WearScalingItemInfo>>>,
    heights: Rc<RefCell<ItemHeights>>,
    indicator: Rc<RefCell<IndicatorState>>,
    /// The frame callback closing an animated scroll, cancelled by the next
    /// scroll of any kind so a finger always wins against a running animation.
    scroll_animation: RefCell<Option<FrameCallbackRegistration>>,
    /// The facts about the list a screen reacts to, published on change rather
    /// than every frame.
    ///
    /// The whole [`WearScalingLayoutInfo`] moves on every scroll frame, so a
    /// screen that observed it would recompose sixty times a second to learn
    /// things that did not change. These are written only when they actually
    /// change, so "the list has 12 rows" or "there is nothing further down"
    /// costs a recomposition when it becomes true and nothing while it stays
    /// true.
    summary: MutableState<WearScalingListSummary>,
}

/// The observable part of a scaling list's layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WearScalingListSummary {
    /// How many rows the list holds.
    pub item_count: usize,
    /// How many of them the viewport can see.
    pub visible_item_count: usize,
    /// Whether there is anything further down to reach.
    pub can_scroll_forward: bool,
    /// Whether there is anything further up to reach.
    pub can_scroll_backward: bool,
}

impl PartialEq for WearScalingListState {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
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
        density: Density,
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
    fn inner(&self) -> Rc<WearScalingListInner> {
        self.inner.get_non_reactive()
    }

    fn id(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.inner.runtime_state_id().hash(&mut hasher);
        hasher.finish()
    }

    /// The anchor, read reactively — a scope that reads this recomposes when
    /// the list scrolls.
    pub fn anchor(&self) -> CentreAnchor {
        self.anchor.get()
    }

    pub fn set_anchor(&self, anchor: CentreAnchor) {
        self.anchor.set(anchor);
    }

    /// Stops a running [`Self::animate_scroll_to_item`].
    ///
    /// Every other way of moving the list calls this first: a finger, a rotary
    /// crown or a snap always wins against an animation the screen started, so
    /// the list never fights the user.
    pub fn cancel_scroll_animation(&self) {
        self.inner().scroll_animation.borrow_mut().take();
    }

    /// Scrolls by a distance in layout points, positive towards the end of the
    /// list, re-anchoring on whichever item the centre line lands in.
    ///
    /// The re-anchoring matters: leaving the index alone and letting the offset
    /// grow works until the anchored item scrolls off, after which the ramp is
    /// computed from an item nobody can see.
    pub fn scroll_by(&self, delta: f32) -> f32 {
        if !delta.is_finite() {
            return 0.0;
        }
        let inner = self.inner();
        inner.scroll_animation.borrow_mut().take();
        let info = *inner.layout.borrow();
        let anchor = self.anchor.get();
        let available_before = info.scrolled();
        let available_after = (info.travel() - available_before).max(0.0);
        let applied = delta.clamp(-available_before, available_after);
        if applied == 0.0 {
            return 0.0;
        }
        let items = inner.items.borrow();
        let target = info.viewport * 0.5 + applied;
        let next = items
            .iter()
            .min_by(|left, right| {
                (left.centre - target)
                    .abs()
                    .total_cmp(&(right.centre - target).abs())
            })
            .map_or_else(
                || re_anchor(anchor, applied, info),
                |item| CentreAnchor {
                    index: item.index,
                    offset: info.viewport * 0.5 - item.centre + applied,
                },
            );
        drop(items);
        self.set_anchor(next);
        applied
    }

    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        self.scroll_by(delta)
    }

    /// What the last measure pass worked out.
    ///
    /// Read outside composition — during draw, input, or a diagnostic — because
    /// every field of it moves on every scroll frame. A screen reacting to the
    /// list reads [`Self::summary`] and its named parts instead, which change
    /// only when they mean something different.
    pub fn layout_info(&self) -> WearScalingLayoutInfo {
        *self.inner().layout.borrow()
    }

    /// The facts about the list a screen reacts to. Reactive: a scope that
    /// reads this recomposes when one of them changes, and not while the list
    /// merely scrolls.
    pub fn summary(&self) -> WearScalingListSummary {
        self.inner().summary.value()
    }

    /// How many rows the list holds. Reactive.
    pub fn item_count(&self) -> usize {
        self.summary().item_count
    }

    /// How many rows the viewport can see. Reactive.
    pub fn visible_item_count(&self) -> usize {
        self.summary().visible_item_count
    }

    /// Whether there is anything further down to reach. Reactive — this is what
    /// a "scroll to top" button and an end-of-list loader watch.
    pub fn can_scroll_forward(&self) -> bool {
        self.summary().can_scroll_forward
    }

    /// Whether there is anything further up to reach. Reactive.
    pub fn can_scroll_backward(&self) -> bool {
        self.summary().can_scroll_backward
    }

    /// Puts `index` on the centre line at once.
    ///
    /// The position of a scaling list *is* a centre anchor, so this is exact
    /// whether or not the target has ever been measured: the next measure pass
    /// lays the list out around it. `offset` shifts the row off the centre line
    /// by that many points, positive downwards.
    pub fn scroll_to_item(&self, index: usize, offset: f32) {
        self.cancel_scroll_animation();
        let count = self.inner().heights.borrow().len();
        let index = if count == 0 {
            index
        } else {
            index.min(count - 1)
        };
        self.set_anchor(CentreAnchor {
            index,
            offset: if offset.is_finite() { offset } else { 0.0 },
        });
    }

    /// How far `index` is from the centre line right now, in points.
    ///
    /// Exact for a row the last measure pass placed. For a row further away
    /// than that, the distance is estimated from the heights the list has seen,
    /// which is what makes an animated scroll to a far row start moving in the
    /// right direction and at a sensible speed; each frame re-asks, so the
    /// estimate is corrected as the real rows arrive.
    pub fn distance_to_item(&self, index: usize, offset: f32) -> f32 {
        let inner = self.inner();
        let info = *inner.layout.borrow();
        let centre_line = info.viewport * 0.5;
        let offset = if offset.is_finite() { offset } else { 0.0 };
        if let Some(item) = inner
            .items
            .borrow()
            .iter()
            .find(|item| item.index == index)
            .copied()
        {
            return item.centre - offset - centre_line;
        }

        let anchor = self.anchor.get_non_reactive();
        let heights = inner.heights.borrow();
        let spacing_free = signed_span(&heights, anchor.index, index);
        spacing_free - anchor.offset - offset
    }

    /// Whether `index` is a row this list has.
    pub fn contains_item(&self, index: usize) -> bool {
        index < self.inner().heights.borrow().len()
    }

    /// Brings `index` to the centre line over several frames.
    ///
    /// Each frame asks again how far away the row is and closes a fixed
    /// fraction of what is left, so the animation corrects itself as the real
    /// rows between here and there are measured — a list scrolled to its
    /// thousandth row does not have to guess right on the first frame. Any
    /// other scroll cancels it; see [`Self::cancel_scroll_animation`].
    pub fn animate_scroll_to_item(&self, index: usize, offset: f32) {
        self.cancel_scroll_animation();
        if !self.contains_item(index) {
            return;
        }
        self.step_scroll_animation(index, if offset.is_finite() { offset } else { 0.0 });
    }

    fn step_scroll_animation(&self, index: usize, offset: f32) {
        let Some(runtime) = cranpose_core::current_runtime_handle() else {
            // Without a runtime there are no frames to animate over; land on
            // the row rather than doing nothing.
            self.scroll_to_item(index, offset);
            return;
        };
        let state = *self;
        let registration = runtime.frame_clock().with_frame_nanos(move |_| {
            let inner = state.inner();
            inner.scroll_animation.borrow_mut().take();
            let remaining = state.distance_to_item(index, offset);
            if remaining.abs() <= SCROLL_ANIMATION_EPSILON {
                state.scroll_to_item(index, offset);
                return;
            }
            let step = remaining * SCROLL_ANIMATION_FRACTION;
            let step = if step.abs() < SCROLL_ANIMATION_MIN_STEP {
                SCROLL_ANIMATION_MIN_STEP.copysign(remaining)
            } else {
                step
            };
            let applied = state.scroll_by(step);
            if applied == 0.0 {
                // The list will not move any further in that direction; the row
                // is as close to the centre as this list can bring it.
                return;
            }
            state.step_scroll_animation(index, offset);
        });
        *self.inner().scroll_animation.borrow_mut() = Some(registration);
    }

    /// Rows placed by the last measure pass, with their transformed bounds.
    pub fn visible_items(&self) -> Vec<WearScalingItemInfo> {
        self.inner().items.borrow().clone()
    }

    /// Returns the topmost placed row containing `point`.
    pub fn item_at(&self, point: Point) -> Option<WearScalingItemInfo> {
        self.inner()
            .items
            .borrow()
            .iter()
            .rev()
            .copied()
            .find(|item| item.contains(point))
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
        let inner = self.inner();
        let mut indicator = inner.indicator.borrow_mut();
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

/// How much of the remaining distance an animated scroll closes each frame.
/// A geometric approach settles quickly without the overshoot a spring would
/// add to a list whose rows are still being measured underneath it.
const SCROLL_ANIMATION_FRACTION: f32 = 0.22;
/// The slowest an animated scroll may crawl, so the last few points do not take
/// a visible tail of frames.
const SCROLL_ANIMATION_MIN_STEP: f32 = 1.0;
/// Close enough to the centre line to snap and stop.
const SCROLL_ANIMATION_EPSILON: f32 = 0.5;

/// The distance from the top of item `from` to the top of item `to`, positive
/// when `to` is further down the list.
///
/// Heights the list has not measured are the mean of the ones it has, which is
/// what `ScalingLazyListState` does for the same question.
fn signed_span(heights: &ItemHeights, from: usize, to: usize) -> f32 {
    if from == to {
        return 0.0;
    }
    let (low, high) = if from < to { (from, to) } else { (to, from) };
    let span: f32 = (low..high).map(|index| heights.height_of(index)).sum();
    if from < to {
        span
    } else {
        -span
    }
}

/// Remembers a scaling list's scroll position.
#[composable]
pub fn rememberWearScalingListState(initial: CentreAnchor) -> WearScalingListState {
    let anchor = rememberMutableStateOf(move || initial);
    let inner = remember(|| {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("rememberWearScalingListState requires an active runtime");
        MutableState::with_runtime(
            Rc::new(WearScalingListInner {
                layout: Rc::new(RefCell::new(WearScalingLayoutInfo::default())),
                items: Rc::new(RefCell::new(Vec::new())),
                heights: Rc::new(RefCell::new(ItemHeights::default())),
                indicator: Rc::new(RefCell::new(IndicatorState::default())),
                scroll_animation: RefCell::new(None),
                summary: MutableState::with_runtime(
                    WearScalingListSummary::default(),
                    runtime.clone(),
                ),
            }),
            runtime,
        )
    })
    .with(|state| *state);
    WearScalingListState { anchor, inner }
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
    items: Vec<WearScalingListItem>,
}

/// One declared row: what identifies it, what shape it is, and what it draws.
#[derive(Clone)]
pub(crate) struct WearScalingListItem {
    /// The caller's key, or `None` to be keyed by position.
    pub(crate) key: Option<u64>,
    /// Which rows may reuse each other's composition slots. Rows of the same
    /// content type are interchangeable; rows of different types are not, and
    /// reusing one as the other throws away the whole subtree.
    pub(crate) content_type: Option<u64>,
    pub(crate) content: Rc<dyn Fn()>,
}

impl WearScalingListScope {
    /// One item.
    ///
    /// `key` is the row's identity. Without one the row is identified by its
    /// position, so inserting or removing a row above it hands its remembered
    /// state — a swipe displacement, an expanded flag, a running animation — to
    /// whichever row moves into its slot. `content_type` groups rows that may
    /// reuse each other's slots: a header and a track row that share a pool
    /// throw away the whole subtree on every reuse.
    pub fn item_keyed<F>(&mut self, key: Option<u64>, content_type: Option<u64>, content: F)
    where
        F: Fn() + 'static,
    {
        self.items.push(WearScalingListItem {
            key,
            content_type,
            content: Rc::new(content),
        });
    }

    /// One row.
    pub fn item<F>(&mut self, content: F)
    where
        F: Fn() + 'static,
    {
        self.item_keyed(None, None, content);
    }

    /// A run of rows, each handed its index.
    ///
    /// A count is enough for the ordinary list — `scope.items(rows.len(), ..)`.
    /// Pass a [`LazyItems`] to name the identities or reuse classes described
    /// on [`item_keyed`](Self::item_keyed).
    pub fn items<I, F>(&mut self, items: I, item: F)
    where
        I: Into<LazyItems>,
        F: Fn(usize) + Clone + 'static,
    {
        let items_spec = items.into();
        let key = items_spec.key_fn();
        let content_type = items_spec.content_type_fn();
        for index in 0..items_spec.count() {
            let item = item.clone();
            self.item_keyed(
                key.as_ref().map(|key| key(index)),
                content_type
                    .as_ref()
                    .map(|content_type| content_type(index)),
                move || item(index),
            );
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
    items: Rc<Vec<WearScalingListItem>>,
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

fn wear_scaling_list_input(
    modifier: Modifier,
    state: WearScalingListState,
    fling: Rc<FlingAnimation>,
) -> Modifier {
    let rotary_state = state;
    let touch_state = state;
    let touch_fling = Rc::clone(&fling);
    modifier
        .on_rotary_scroll_event(move |event| {
            let delta = if event.vertical_scroll_pixels != 0.0 {
                event.vertical_scroll_pixels
            } else {
                event.horizontal_scroll_pixels
            };
            rotary_state.dispatch_raw_delta(delta) != 0.0
        })
        .pointer_input(state.id(), move |scope: PointerInputScope| {
            let fling = Rc::clone(&touch_fling);
            async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        let mut pointer = None;
                        let mut down = Point::new(0.0, 0.0);
                        let mut last = Point::new(0.0, 0.0);
                        let mut dragging = false;
                        let mut velocity = VelocityTracker1D::new();
                        loop {
                            let event = events.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down if pointer.is_none() => {
                                    fling.cancel();
                                    pointer = Some(event.id);
                                    down = event.position;
                                    last = event.position;
                                    dragging = false;
                                    velocity.reset();
                                    if let Some(time) = event.time_ms {
                                        velocity.add_data_point(time, event.position.y);
                                    }
                                }
                                PointerEventKind::Move if pointer == Some(event.id) => {
                                    if event.is_consumed() {
                                        pointer = None;
                                        dragging = false;
                                        velocity.reset();
                                        continue;
                                    }
                                    let dx = event.position.x - down.x;
                                    let dy = event.position.y - down.y;
                                    if !dragging && dy.abs() > DRAG_THRESHOLD && dy.abs() > dx.abs()
                                    {
                                        dragging = true;
                                    } else if !dragging
                                        && dx.abs() > DRAG_THRESHOLD
                                        && dx.abs() >= dy.abs()
                                    {
                                        pointer = None;
                                        velocity.reset();
                                        continue;
                                    }
                                    if dragging {
                                        let consumed = touch_state
                                            .dispatch_raw_delta(last.y - event.position.y);
                                        last = event.position;
                                        if let Some(time) = event.time_ms {
                                            velocity.add_data_point(time, event.position.y);
                                        }
                                        if consumed != 0.0 {
                                            event.consume();
                                        }
                                    }
                                }
                                PointerEventKind::Up if pointer == Some(event.id) => {
                                    if dragging {
                                        if let Some(time) = event.time_ms {
                                            velocity.add_data_point(time, event.position.y);
                                        }
                                        let speed = -velocity
                                            .calculate_velocity_with_max(MAX_FLING_VELOCITY);
                                        let fling_state = touch_state;
                                        fling.start_fling(
                                            0.0,
                                            speed,
                                            crate::current_density(),
                                            move |delta| fling_state.dispatch_raw_delta(delta),
                                            || {},
                                        );
                                        event.consume();
                                    }
                                    pointer = None;
                                    dragging = false;
                                    velocity.reset();
                                }
                                PointerEventKind::Cancel if pointer == Some(event.id) => {
                                    pointer = None;
                                    dragging = false;
                                    velocity.reset();
                                }
                                PointerEventKind::Scroll => {
                                    let consumed =
                                        touch_state.dispatch_raw_delta(event.scroll_delta.y);
                                    if consumed != 0.0 {
                                        event.consume();
                                    }
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }
        })
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
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(size, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
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
    let fling = remember(|| {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("WearScalingLazyColumn requires an active runtime");
        Rc::new(FlingAnimation::new(runtime))
    })
    .with(Rc::clone);

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
        let inner = state.inner();
        let layout = Rc::clone(&inner.layout);
        let items = Rc::clone(&inner.items);
        let heights = Rc::clone(&inner.heights);
        let indicator = Rc::clone(&inner.indicator);
        let outputs = WearScalingMeasureOutputs {
            transforms,
            layout,
            items,
            heights,
            indicator,
            summary: inner.summary,
        };
        move || {
            let policy: Rc<SubcomposeMeasurePolicy> = Rc::new(
                move |scope: &mut SubcomposeMeasureScopeImpl<'_>, constraints: Constraints| {
                    measure_wear_scaling_list(scope, constraints, &inputs.borrow(), &outputs)
                },
            );
            policy
        }
    })
    .with(|policy| policy.clone());

    let modifier = wear_scaling_list_input(modifier, state, fling).clip_to_bounds();
    // The state's layout cell is allocated once per remembered state and lives
    // exactly as long as it, so its address is a stable identity for the node.
    let list_id = state.id();
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
    // Read while the composition is still running, same as `Layout`: measurement
    // happens after it and cannot reach a composition local. Reading here also
    // subscribes, so a subtree given a different grid recomposes and re-captures.
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        if !node.modifier().structural_eq(&modifier) {
            node.set_modifier(modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
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

struct WearScalingMeasureOutputs {
    transforms: Rc<RefCell<Vec<WearItemTransform>>>,
    layout: Rc<RefCell<WearScalingLayoutInfo>>,
    items: Rc<RefCell<Vec<WearScalingItemInfo>>>,
    heights: Rc<RefCell<ItemHeights>>,
    indicator: Rc<RefCell<IndicatorState>>,
    summary: MutableState<WearScalingListSummary>,
}

impl WearScalingMeasureOutputs {
    /// Records what the measure pass worked out, and republishes the observable
    /// summary only where it differs from what a screen was last told.
    fn publish(&self, info: WearScalingLayoutInfo) {
        *self.layout.borrow_mut() = info;
        let travel = info.travel();
        let scrolled = info.scrolled();
        let summary = WearScalingListSummary {
            item_count: info.item_count,
            visible_item_count: info.visible,
            can_scroll_forward: travel - scrolled > SCROLL_EPSILON,
            can_scroll_backward: scrolled > SCROLL_EPSILON,
        };
        if self.summary.get_non_reactive() != summary {
            self.summary.set(summary);
        }
    }
}

/// Below this the list is at an end: a fraction of a point of travel left is
/// rounding, not somewhere to scroll to.
const SCROLL_EPSILON: f32 = 0.5;

/// Composes, measures and places only the items the viewport can reach.
fn measure_wear_scaling_list(
    scope: &mut SubcomposeMeasureScopeImpl<'_>,
    constraints: Constraints,
    inputs: &WearScalingListInputs,
    outputs: &WearScalingMeasureOutputs,
) -> MeasureResult {
    let WearScalingMeasureOutputs {
        transforms,
        items,
        heights,
        indicator,
        ..
    } = outputs;
    let spec = inputs.spec;
    let top_aligned = spec.auto_centering.is_none();
    // The grid comes from the scope this measure pass is running with, not the
    // host default -- a subtree given a different grid is measured on it.
    let scale = scope.density();
    let density = Density::new(scale, 1.0);
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
        items.borrow_mut().clear();
        outputs.publish(WearScalingLayoutInfo {
            item_count: 0,
            viewport,
            ..WearScalingLayoutInfo::default()
        });
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
        edge = bottom - item.height;
        window.push(WindowedRow {
            index,
            roots: item.roots,
            top,
            height: item.height,
            placed,
        });
    }

    // Downward, the same walk against the bottom edge.
    let mut edge = anchored_top + anchored_height;
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
        edge = top + item.height;
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
        let mut placed_items = items.borrow_mut();
        placed_items.clear();
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
                placed_items.push(WearScalingItemInfo {
                    index: item.index,
                    bounds: Rect {
                        x: left + item_width * (1.0 - row.scale) * 0.5,
                        y: row.top,
                        width: item_width * row.scale,
                        height: row.height,
                    },
                    centre: item.top + item.height * 0.5,
                    unscaled_height: item.height,
                    scale: row.scale,
                    alpha: row.alpha,
                });
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
    outputs.publish(WearScalingLayoutInfo {
        item_count: count,
        viewport,
        first_centre: first_top + known.height_of(0) * 0.5,
        last_centre: last_bottom - known.height_of(count - 1) * 0.5,
        content: last_bottom - first_top,
        visible,
        composed,
    });

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
    density: &Density,
    child_constraints: Constraints,
) -> MeasuredItem {
    let transform = transforms
        .borrow()
        .get(index)
        .cloned()
        .unwrap_or_else(WearItemTransform::new);
    let strategy = inputs.spec.compositing_strategy;
    let item = inputs.content.items[index].clone();
    // A user key names the item; without one the row is keyed by position. The
    // two are tagged apart so a caller's key can never collide with an index,
    // which is the same rule the flat lazy list follows.
    let key = match item.key {
        Some(key) => LazyLayoutKey::User(key),
        None => LazyLayoutKey::Index(index),
    };
    let slot_id = SlotId(key.to_slot_id());
    let identity = item.key.map(|_| key.to_slot_id());
    scope.update_content_type(slot_id, item.content_type);
    let content = Rc::clone(&item.content);
    let children: Vec<SubcomposeChild> = scope.subcompose(slot_id, move || {
        let content = Rc::clone(&content);
        crate::lazy_item::ProvideLazyItemKey(identity, || {
            WearScalingItem(Modifier::empty(), transform.clone(), strategy, move || {
                content()
            });
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
    fn a_declared_row_carries_its_key_and_content_type() {
        let mut scope = WearScalingListScope::default();
        scope.item_keyed(Some(7), Some(1), || {});
        scope.items(
            LazyItems::new(3)
                .key(|index: usize| 100 + index as u64)
                .content_type(|index: usize| (index % 2) as u64),
            |_| {},
        );
        assert_eq!(scope.count(), 4);
        assert_eq!(scope.items[0].key, Some(7));
        assert_eq!(scope.items[0].content_type, Some(1));
        assert_eq!(
            scope.items[1..]
                .iter()
                .map(|item| item.key)
                .collect::<Vec<_>>(),
            [Some(100), Some(101), Some(102)]
        );
        assert_eq!(
            scope.items[1..]
                .iter()
                .map(|item| item.content_type)
                .collect::<Vec<_>>(),
            [Some(0), Some(1), Some(0)]
        );
    }

    #[test]
    fn a_row_declared_without_a_key_is_identified_by_its_position() {
        let mut scope = WearScalingListScope::default();
        scope.items(2, |_| {});
        assert!(scope.items.iter().all(|item| item.key.is_none()));
        assert!(scope.items.iter().all(|item| item.content_type.is_none()));
    }

    /// A caller's key and an index can name the same number, and a slot table
    /// that let them collide would hand one row's composition to another.
    #[test]
    fn a_user_key_and_an_index_never_name_the_same_slot() {
        assert_ne!(
            LazyLayoutKey::User(3).to_slot_id(),
            LazyLayoutKey::Index(3).to_slot_id()
        );
    }

    fn summary_for(info: WearScalingLayoutInfo) -> (bool, bool) {
        let travel = info.travel();
        let scrolled = info.scrolled();
        (
            travel - scrolled > SCROLL_EPSILON,
            scrolled > SCROLL_EPSILON,
        )
    }

    #[test]
    fn a_list_at_its_top_can_only_scroll_forward() {
        // Viewport 200, first centre on the centre line: nothing scrolled yet.
        let (forward, backward) = summary_for(info(10, 200.0, 100.0, 500.0));
        assert!(forward);
        assert!(!backward);
    }

    #[test]
    fn a_list_at_its_end_can_only_scroll_backward() {
        // Last centre on the centre line: the whole travel is behind it.
        let (forward, backward) = summary_for(info(10, 200.0, -300.0, 100.0));
        assert!(!forward);
        assert!(backward);
    }

    #[test]
    fn a_list_that_fits_can_scroll_neither_way() {
        let (forward, backward) = summary_for(info(1, 200.0, 100.0, 100.0));
        assert!(!forward);
        assert!(!backward);
    }

    #[test]
    fn an_unmeasured_height_is_the_mean_of_the_measured_ones() {
        let mut heights = ItemHeights::default();
        heights.resize(4);
        heights.record(0, 30.0);
        heights.record(1, 50.0);
        assert_eq!(heights.height_of(0), 30.0);
        assert_eq!(heights.height_of(3), 40.0, "the mean of 30 and 50");
        assert_eq!(signed_span(&heights, 0, 2), 80.0);
        assert_eq!(signed_span(&heights, 2, 0), -80.0);
        assert_eq!(signed_span(&heights, 2, 2), 0.0);
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

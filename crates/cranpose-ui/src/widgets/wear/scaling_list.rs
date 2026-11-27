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

#![expect(non_snake_case, reason = "names follow Compose's casing")]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, NodeId, SlotId, remember};
use cranpose_foundation::lazy::{LazyItems, LazyLayoutKey};
use cranpose_ui_graphics::{CompositingStrategy, Point, Rect, Size};
use cranpose_ui_layout::{
    Constraints, Measurables, MeasurePolicy, MeasureResult, MeasureScope, Placement, round_to_px,
};

mod indicator;
mod input;
mod item_heights;
mod state;
use indicator::IndicatorState;
use input::wear_scaling_list_input;
use item_heights::ItemHeights;
pub use state::{WearScalingListState, WearScalingListSummary, rememberWearScalingListState};
#[cfg(test)]
use state::{re_anchor, signed_span};

use crate::{
    composable,
    density::Density,
    fling_animation::FlingAnimation,
    layout_direction::LayoutDirection,
    modifier::{GraphicsLayer, Modifier, TransformOrigin},
    round_scaling_list::{CentreAnchor, PlacedRow, ScaleAlpha, ScalingParams, place_row_with},
    subcompose_layout::{
        MeasurePolicy as SubcomposeMeasurePolicy, SubcomposeChild, SubcomposeLayoutNode,
        SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
    },
    widgets::{Layout, PaddingValues},
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
    /// Creates a channel that leaves the item unscaled and fully opaque.
    pub fn new() -> Self {
        Self {
            cell: Rc::new(Cell::new(ScaleAlpha::UNCHANGED)),
        }
    }

    /// Returns the scale and alpha last set.
    pub fn get(&self) -> ScaleAlpha {
        self.cell.get()
    }

    /// Sets the scale and alpha that the item draws with.
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
    /// Number of items in the list.
    pub item_count: usize,
    /// Height of the viewport, in logical pixels.
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
    /// Index of the item in the list.
    pub index: usize,
    /// Bounds of the item after scaling and placement, in the list's local coordinates.
    pub bounds: Rect,
    /// Center of the item along the scroll axis, in the list's local coordinates.
    pub centre: f32,
    /// Height of the item before scaling, in logical pixels.
    pub unscaled_height: f32,
    /// Scale factor applied to the item. 1.0 means unscaled.
    pub scale: f32,
    /// Opacity of the item, from 0.0 (transparent) to 1.0 (opaque).
    pub alpha: f32,
}

impl WearScalingItemInfo {
    /// Returns whether `point`, in the list's local coordinates, lies inside the bounds of the
    /// item.
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

/// How a [`WearScalingLazyColumn`] is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WearScalingLazyColumnSpec {
    /// The six knobs of the shrink-and-fade ramp.
    pub scaling: ScalingParams,
    /// `Arrangement.spacedBy(4.dp)`, Wear's default.
    pub item_spacing: f32,
    /// Compose's `contentPadding`: the top and bottom edges add space before
    /// the first item and after the last, the start and end edges inset every
    /// item.
    pub content_padding: PaddingValues,
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
            content_padding: PaddingValues::default(),
            auto_centering: Some(CentreAnchor::default()),
            compositing_strategy: CompositingStrategy::Auto,
            beyond_bounds_item_count: 2,
        }
    }
}

impl WearScalingLazyColumnSpec {
    /// Pads the content on all four edges, as Compose's `contentPadding`.
    pub fn content_padding(mut self, padding: PaddingValues) -> Self {
        self.content_padding = padding;
        self
    }

    /// Sets the space between items, in dp.
    pub fn item_spacing(mut self, spacing: f32) -> Self {
        self.item_spacing = spacing;
        self
    }

    /// Sets the parameters of the shrink-and-fade ramp.
    pub fn scaling(mut self, scaling: ScalingParams) -> Self {
        self.scaling = scaling;
        self
    }

    /// Sets whether a faded item is composited through its own surface.
    pub fn compositing_strategy(mut self, strategy: CompositingStrategy) -> Self {
        self.compositing_strategy = strategy;
        self
    }

    /// Sets which item sits on the center line at rest. `None` places the content from the top.
    pub fn auto_centering(mut self, anchor: Option<CentreAnchor>) -> Self {
        self.auto_centering = anchor;
        self
    }

    /// Sets how many items past each edge of the viewport are composed and measured but not placed.
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
    pub(crate) key: Option<u64>,
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
    let layered = modifier.graphics_layer(move || {
        let value = transform.get();
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
        measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let measurement = self.measure_into(scope, measurables, constraints, &mut placements);
        MeasureResult::new(measurement, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        measurables: Measurables<'_>,
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> cranpose_ui_layout::Measurement {
        placements.clear();
        let mut width = constraints.min_width;
        let mut height = constraints.min_height;
        for measurable in measurables {
            let placeable = measurable.measure(constraints);
            width = width.max(placeable.width());
            height = height.max(placeable.height());
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0.0));
        }
        Size::new(
            width.clamp(constraints.min_width, constraints.max_width),
            height.clamp(constraints.min_height, constraints.max_height),
        )
        .into()
    }

    fn min_intrinsic_width(&self, measurables: Measurables<'_>, height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: Measurables<'_>, height: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: Measurables<'_>, width: f32) -> f32 {
        measurables
            .iter()
            .map(|m| m.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: Measurables<'_>, width: f32) -> f32 {
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
///     WearScalingLazyColumnSpec::default().content_padding(PaddingValues::symmetric(18.0, 34.0)),
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
#[derive(Default, PartialEq)]
struct WearScalingListInputs {
    spec: WearScalingLazyColumnSpec,
    anchor: CentreAnchor,
    content: WearScalingListContent,
    direction: LayoutDirection,
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
        .with(|value| Rc::clone(value));
    let inputs = remember(|| Rc::new(RefCell::new(WearScalingListInputs::default())))
        .with(|value| Rc::clone(value));
    let fling = remember(|| {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("WearScalingLazyColumn requires an active runtime");
        Rc::new(FlingAnimation::new(runtime))
    })
    .with(Rc::clone);

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
    let direction = crate::layout_direction();
    let inputs_changed = {
        let mut current = inputs.borrow_mut();
        let next = WearScalingListInputs {
            spec,
            anchor,
            content,
            direction,
        };
        let changed = *current != next;
        if changed {
            *current = next;
        }
        changed
    };

    let policy: Rc<SubcomposeMeasurePolicy> = remember({
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
    .with(|policy| Rc::clone(policy));

    let caller_modifier_changed = crate::widgets::layout::caller_modifier_changed(&modifier);
    let modifier = wear_scaling_list_input(modifier, state, fling).clip_to_bounds();
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
    let captured_context =
        cranpose_core::with_current_composer(|composer| composer.capture_composition_context());
    let composed_density = crate::density::density();
    if let Err(err) = cranpose_core::with_node_mut(node_id, |node: &mut SubcomposeLayoutNode| {
        if caller_modifier_changed {
            node.set_modifier(modifier.clone());
        }
        node.set_measure_policy(Rc::clone(&policy));
        node.set_captured_context(captured_context);
        node.set_density(composed_density);
        node.set_layout_direction(direction);
        if inputs_changed {
            node.invalidate_subcomposition();
        }
    }) {
        debug_assert!(false, "failed to update WearScalingLazyColumn node: {err}");
    }
    node_id
}

/// One item's place in the column, once it has been composed and measured.
pub(super) struct WindowedRow {
    pub(super) index: usize,
    roots: Vec<(NodeId, f32, f32)>,
    pub(super) top: f32,
    pub(super) height: f32,
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
        if self.summary.is_alive() && self.summary.get_non_reactive() != summary {
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
    let scale = scope.density();
    let density = Density::new(scale, 1.0);
    let width = if constraints.max_width.is_finite() {
        constraints.max_width
    } else {
        constraints.min_width
    };
    let viewport = if constraints.max_height.is_finite() {
        constraints.max_height
    } else {
        constraints.min_height
    };

    let inset = density.dp(spec.content_padding.start) + density.dp(spec.content_padding.end);
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
        return scope.layout_with_placement_builder(width, viewport, Vec::clear);
    }

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

    let anchored_top = if top_aligned {
        density.dp(spec.content_padding.top)
    } else {
        round_to_px(
            viewport * 0.5 - anchored.height * 0.5 - anchor.offset,
            scale,
        )
    };
    let anchored_height = anchored.height;
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

    window.sort_unstable_by_key(|row| row.index);

    let composed = window.len();
    let left = density.dp(spec.content_padding.physical(inputs.direction).left);
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
                    placements.push(Placement::new(node_id, x, row.top + offset, 0.0));
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
    let key = match item.key {
        Some(key) => LazyLayoutKey::User(key),
        None => LazyLayoutKey::Index(index),
    };
    let slot_id = SlotId(key.to_slot_id());
    let identity = item.key.map(|_| key.to_slot_id());
    scope.update_content_type(slot_id, item.content_type);
    let content = Rc::clone(&item.content);
    let children = scope.subcompose_roots(slot_id, (), move || {
        let content = Rc::clone(&content);
        crate::lazy_item::ProvideLazyItemKey(identity, || {
            WearScalingItem(Modifier::empty(), transform.clone(), strategy, move || {
                content();
            });
        });
    });

    let mut roots = Vec::with_capacity(children.len());
    let mut stacked = 0.0f32;
    for child in children {
        let placeable = scope.measure(SubcomposeChild::new(child), child_constraints);
        roots.push((placeable.node_id(), stacked, placeable.width()));
        stacked += placeable.height();
    }
    let height = density.ceil(stacked);
    heights.borrow_mut().record(index, height);
    MeasuredItem { roots, height }
}

#[cfg(test)]
#[path = "tests/scaling_list_tests.rs"]
mod tests;

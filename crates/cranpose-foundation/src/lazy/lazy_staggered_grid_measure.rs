//! Measures lazy staggered grids. Items go, in index order, into the lane
//! that ends first, or across the whole line, and keep that lane while the
//! grid scrolls. A pass starts at the item the viewport was anchored to and
//! composes only the items near the viewport, wherever the grid scrolled.
//!
//! The items from the anchor on lie after the ones before them in their
//! lanes. The ones before the anchor lie before the ones after them, so the
//! items in view stay put when the items a backward scroll brings in turn
//! out longer or shorter than the average size they had. The lanes line up
//! again from the first item once a scroll reaches it.

use std::ops::Range;

use smallvec::{SmallVec, smallvec};

use super::{
    lazy_list_measure::{
        BeyondItem, LazyListMeasureConfig, MAX_UNBOUNDED_REALIZED_ITEMS, publish_without_items,
    },
    lazy_list_measured_item::{LazyListMeasureResult, LazyListMeasuredItem},
    lazy_list_state::{LazyListLayoutInfo, LazyListState},
};

/// The lane of an item that takes the whole line.
const FULL_LINE: u16 = u16::MAX;

/// A pass measures again when it scrolls back from past either end or lines
/// the lanes up at the first item, at most this many times in all.
const MAX_FILLS: usize = 3;

/// Per lane, a main-axis position: where the lane's next item starts, or
/// where its first item after some point starts.
type LanePositions = SmallVec<[f32; 8]>;

#[derive(Clone, Copy, Debug)]
struct Slot {
    lane: u16,
    start: f32,
    size: f32,
}

impl Slot {
    fn end(self) -> f32 {
        self.start + self.size
    }

    fn lane(self) -> Option<usize> {
        (self.lane != FULL_LINE).then_some(usize::from(self.lane))
    }
}

/// Where the items of a lazy staggered grid sit along its main axis, from
/// the first item to the furthest one a pass reached. An item keeps its lane
/// once it has one; an item no pass measured has the average size.
#[derive(Debug, Default)]
pub struct StaggeredLanes {
    lanes: usize,
    spacing: f32,
    slots: Vec<Slot>,
    /// Whether every item lies after the ones before it in its lane, so the
    /// first items start at 0. A backward fill that measures an item at a
    /// new size breaks that until the lanes line up again.
    aligned: bool,
}

/// How a lazy staggered grid's measure pass gets its items.
pub trait LazyStaggeredItemSource {
    /// Whether the item at `index` takes the whole line.
    fn is_full_line(&self, index: usize) -> bool;

    /// Composes and measures the item at `index`, as wide as `lane`, or as
    /// the whole line when `lane` is `None`.
    fn measure(&mut self, index: usize, lane: Option<usize>) -> LazyListMeasuredItem;

    /// Keeps the item at `index`, which starts `distance` past the
    /// viewport, composed ahead of a scroll, as a list does with
    /// [`super::LazyItemSource::keep_beyond`], in `lane` or across the whole
    /// line when `lane` is `None`.
    fn keep_beyond(&mut self, index: usize, lane: Option<usize>, distance: f32) -> BeyondItem;

    /// The item that holds the focus, which stays placed wherever the grid
    /// scrolled it.
    fn focused_item(&self) -> Option<usize>;
}

/// The lanes of a staggered grid's measure pass, by where each starts across
/// the grid.
#[derive(Clone, Debug, PartialEq)]
pub struct StaggeredGridMeasureConfig {
    /// The start of each lane across the grid, in layout pixels. The number of entries is the
    /// number of lanes.
    pub lane_offsets: SmallVec<[f32; 8]>,
}

/// What one fill of the viewport found.
struct Fill {
    /// The items in view, in index order.
    visible: Vec<LazyListMeasuredItem>,
    /// Where the content ends, when the items ran out above the bottom.
    end: Option<f32>,
}

impl StaggeredLanes {
    /// Starts over when the lanes or the spacing between items change, and
    /// forgets the items past `items_count`.
    fn prepare(&mut self, lanes: usize, spacing: f32, items_count: usize) {
        if self.lanes != lanes || self.spacing != spacing {
            self.lanes = lanes;
            self.spacing = spacing;
            self.slots.clear();
            self.aligned = true;
        }
        self.slots.truncate(items_count);
    }

    /// Where each lane's next item starts after the items before `index`.
    fn ends_before(&self, index: usize) -> LanePositions {
        let mut ends: SmallVec<[Option<f32>; 8]> = smallvec![None; self.lanes];
        let mut open = self.lanes;
        for slot in self.slots[..index.min(self.slots.len())].iter().rev() {
            let next = slot.end() + self.spacing;
            let Some(lane) = slot.lane() else {
                fill_unset(&mut ends, next);
                break;
            };
            if ends[lane].is_none() {
                ends[lane] = Some(next);
                open -= 1;
                if open == 0 {
                    break;
                }
            }
        }
        ends.iter().map(|end| end.unwrap_or(0.0)).collect()
    }

    /// Where each lane's first item from `index` on starts; for a lane with
    /// none, where its next item would start.
    fn starts_from(&self, index: usize) -> LanePositions {
        let mut starts: SmallVec<[Option<f32>; 8]> = smallvec![None; self.lanes];
        let mut open = self.lanes;
        for slot in self.slots.iter().skip(index) {
            let Some(lane) = slot.lane() else {
                fill_unset(&mut starts, slot.start);
                open = 0;
                break;
            };
            if starts[lane].is_none() {
                starts[lane] = Some(slot.start);
                open -= 1;
                if open == 0 {
                    break;
                }
            }
        }
        if open == 0 {
            return starts.iter().map(|start| start.unwrap_or(0.0)).collect();
        }
        let ends = self.ends_before(self.slots.len());
        starts
            .iter()
            .zip(&ends)
            .map(|(start, &end)| start.unwrap_or(end))
            .collect()
    }

    /// Gives every item up to `index` a lane, the items no pass reached
    /// yet at the average size.
    fn reach<S: LazyStaggeredItemSource>(&mut self, index: usize, estimate: f32, source: &S) {
        if index < self.slots.len() {
            return;
        }
        let mut ends = self.ends_before(self.slots.len());
        for item in self.slots.len()..=index {
            let (lane, start) = place(&ends, source.is_full_line(item));
            let slot = Slot {
                lane,
                start,
                size: estimate,
            };
            advance(&mut ends, slot, self.spacing);
            self.slots.push(slot);
        }
    }

    /// Measures, from `anchor` on, the items that start above `bottom` and
    /// reach below `top`, each lane from where its first item from `anchor`
    /// on starts (`starts`), and places the items after them by the sizes
    /// they have.
    fn fill_down<S: LazyStaggeredItemSource>(
        &mut self,
        anchor: usize,
        mut ends: LanePositions,
        window: Window,
        items_count: usize,
        source: &mut S,
        visible: &mut Vec<LazyListMeasuredItem>,
    ) -> Option<f32> {
        let mut index = anchor;
        while index < items_count && ends.iter().any(|&end| end < window.bottom) {
            let known = self.slots.get(index).copied();
            let (lane, start) = match known {
                Some(slot) => (slot.lane, start_in(&ends, slot.lane)),
                None => place(&ends, source.is_full_line(index)),
            };
            let mut slot = Slot {
                lane,
                start,
                size: known.map_or(window.estimate, |slot| slot.size),
            };
            if start < window.bottom && slot.end() > window.top {
                let mut item = source.measure(index, slot.lane());
                // An item that starts above the viewport keeps its end, so
                // the items after it in its lane stay put.
                if slot.start < window.top && item.main_axis_size != slot.size {
                    slot.start = slot.end() - item.main_axis_size;
                    self.aligned = false;
                }
                slot.size = item.main_axis_size;
                if slot.end() > window.top {
                    window.show(&mut item, slot);
                    visible.push(item);
                }
            }
            match self.slots.get_mut(index) {
                Some(stored) => *stored = slot,
                None => self.slots.push(slot),
            }
            advance(&mut ends, slot, self.spacing);
            index += 1;
        }
        let end = (index == items_count).then(|| start_in(&ends, FULL_LINE) - self.spacing);
        for slot in self.slots.iter_mut().skip(index) {
            slot.start = start_in(&ends, slot.lane);
            advance(&mut ends, *slot, self.spacing);
        }
        end
    }

    /// Measures, back from `anchor`, the items before it that reach below
    /// `top`, each ending where the item after it in its lane starts, from
    /// where each lane's first item from `anchor` on starts (`next`), and
    /// adds them to `visible` from the last. Answers whether it went back to
    /// the first item, so nothing lies above the items it placed.
    fn fill_up<S: LazyStaggeredItemSource>(
        &mut self,
        anchor: usize,
        mut next: LanePositions,
        window: Window,
        source: &mut S,
        visible: &mut Vec<LazyListMeasuredItem>,
    ) -> bool {
        for index in (0..anchor.min(self.slots.len())).rev() {
            if next.iter().all(|&start| start <= window.top) {
                return false;
            }
            let stored = self.slots[index];
            let end = match stored.lane() {
                Some(lane) => next[lane],
                None => next.iter().copied().fold(f32::INFINITY, f32::min),
            } - self.spacing;
            let mut slot = Slot {
                start: end - stored.size,
                ..stored
            };
            if end > window.top && slot.start < window.bottom {
                let mut item = source.measure(index, slot.lane());
                if item.main_axis_size != slot.size {
                    self.aligned = false;
                }
                slot.size = item.main_axis_size;
                slot.start = end - slot.size;
                if slot.start < window.bottom {
                    window.show(&mut item, slot);
                    visible.push(item);
                }
            }
            self.slots[index] = slot;
            match slot.lane() {
                Some(lane) => next[lane] = slot.start,
                None => next.fill(slot.start),
            }
        }
        true
    }

    /// Lines the lanes up from the first item, by the sizes the items have.
    fn align(&mut self) {
        let mut ends: LanePositions = smallvec![0.0; self.lanes];
        for slot in &mut self.slots {
            slot.start = start_in(&ends, slot.lane);
            advance(&mut ends, *slot, self.spacing);
        }
        self.aligned = true;
    }

    /// The lane of the item at `index`; `None` for an item across the
    /// whole line or one no pass reached.
    pub fn lane_of(&self, index: usize) -> Option<usize> {
        self.slots.get(index).and_then(|slot| slot.lane())
    }

    /// The main-axis extent of the items the passes placed, and the rest at
    /// the average size spread over the lanes.
    fn estimated_extent(&self, items_count: usize, estimate: f32) -> f32 {
        let placed = self.slots.iter().map(|slot| slot.end()).fold(0.0, f32::max);
        let left = items_count.saturating_sub(self.slots.len()) as f32;
        placed + left * (estimate + self.spacing) / self.lanes.max(1) as f32
    }
}

/// The part of the content a fill shows, and how its items get there.
#[derive(Clone, Copy)]
struct Window<'l> {
    top: f32,
    bottom: f32,
    estimate: f32,
    lane_offsets: &'l [f32],
}

impl Window<'_> {
    /// Puts `item` at `slot`'s start and its lane's offset across the grid.
    fn show(&self, item: &mut LazyListMeasuredItem, slot: Slot) {
        let cross = slot.lane().map_or(0.0, |lane| self.lane_offsets[lane]);
        item.offset = slot.start;
        item.child_cross_offsets = smallvec![cross; item.node_ids.len()];
    }
}

fn fill_unset(positions: &mut [Option<f32>], value: f32) {
    for position in positions.iter_mut().filter(|position| position.is_none()) {
        *position = Some(value);
    }
}

/// The lane and the start of an item after `ends`: the lane that ends
/// first, the first of them on a tie, or the whole line after all of them.
fn place(ends: &LanePositions, full_line: bool) -> (u16, f32) {
    if full_line {
        return (FULL_LINE, start_in(ends, FULL_LINE));
    }
    let mut lane = 0;
    for (candidate, &end) in ends.iter().enumerate() {
        if end < ends[lane] {
            lane = candidate;
        }
    }
    (lane as u16, ends[lane])
}

fn start_in(ends: &LanePositions, lane: u16) -> f32 {
    if lane == FULL_LINE {
        ends.iter().copied().fold(0.0, f32::max)
    } else {
        ends[usize::from(lane)]
    }
}

fn advance(ends: &mut LanePositions, slot: Slot, spacing: f32) {
    let next = slot.end() + spacing;
    match slot.lane() {
        Some(lane) => ends[lane] = next,
        None => ends.fill(next),
    }
}

/// What one measure pass works with.
struct Pass<'g> {
    items_count: usize,
    /// The items it may show: all of them, or the first ones up to the cap a
    /// list has when the viewport is unbounded.
    realized: usize,
    viewport: f32,
    unbounded: bool,
    before: f32,
    after: f32,
    estimate: f32,
    lane_offsets: &'g [f32],
}

impl<'g> Pass<'g> {
    fn new(
        items_count: usize,
        viewport: f32,
        estimate: f32,
        config: &LazyListMeasureConfig,
        grid: &'g StaggeredGridMeasureConfig,
    ) -> Self {
        let unbounded = viewport.is_infinite();
        if unbounded && items_count > MAX_UNBOUNDED_REALIZED_ITEMS {
            log::warn!(
                "LazyStaggeredGrid: unbounded viewport with {items_count} items; realizing only the first {MAX_UNBOUNDED_REALIZED_ITEMS}. \
                 Wrap the grid in a constrained container to restore virtualization."
            );
        }
        Self {
            items_count,
            realized: if unbounded {
                items_count.min(MAX_UNBOUNDED_REALIZED_ITEMS)
            } else {
                items_count
            },
            viewport,
            unbounded,
            before: config.before_content_padding,
            after: config.after_content_padding,
            estimate,
            lane_offsets: &grid.lane_offsets,
        }
    }

    /// The part of the content the viewport shows at `scroll`.
    fn window(&self, scroll: f32) -> Window<'g> {
        Window {
            top: scroll - self.before,
            bottom: scroll - self.before + self.viewport,
            estimate: self.estimate,
            lane_offsets: self.lane_offsets,
        }
    }

    /// Where the scroll comes back to from past either end.
    fn clamp(&self, scroll: f32, end: Option<f32>, reached_start: bool) -> f32 {
        if self.unbounded {
            return scroll;
        }
        let mut scroll = match end {
            Some(end) => scroll.min(end + self.after + self.before - self.viewport),
            None => scroll,
        };
        if reached_start {
            scroll = scroll.max(0.0);
        }
        scroll
    }
}

/// Measures a lazy staggered grid and returns the items to place, at their
/// offsets from the viewport's start and with their lanes' offsets across
/// it. `lanes` keeps the items' lanes from pass to pass. The scroll position
/// in `state` names the first item the viewport shows and how far the
/// viewport starts past that item's start.
pub fn measure_lazy_staggered_grid<S: LazyStaggeredItemSource>(
    items_count: usize,
    lanes: &mut StaggeredLanes,
    grid: &StaggeredGridMeasureConfig,
    state: &LazyListState,
    viewport_size: f32,
    config: &LazyListMeasureConfig,
    source: &mut S,
) -> LazyListMeasureResult {
    if items_count == 0 || viewport_size <= 0.0 {
        return publish_without_items(state, items_count, viewport_size, config);
    }
    let lane_count = grid.lane_offsets.len().clamp(1, usize::from(FULL_LINE) - 1);
    lanes.prepare(lane_count, config.spacing, items_count);
    let snapshot = state.begin_measure_pass();
    let pass = Pass::new(
        items_count,
        viewport_size,
        snapshot.average_item_size,
        config,
        grid,
    );
    let (anchor, offset) = snapshot.pending_scroll_to.unwrap_or((
        snapshot.first_visible_item_index,
        snapshot.first_visible_item_scroll_offset,
    ));
    let anchor = anchor.min(items_count - 1);
    lanes.reach(anchor, pass.estimate, source);
    let scroll = if pass.unbounded {
        0.0
    } else {
        lanes.slots[anchor].start + offset - snapshot.pending_scroll_delta
    };
    let (fill, scroll) = fill_viewport(lanes, &pass, anchor, scroll, source);
    publish(lanes, &pass, config, fill, scroll, anchor, state, source)
}

/// Fills the viewport at `scroll` from `anchor`, and again where the scroll
/// comes back from past either end or the lanes line up. Answers the fill
/// and the scroll it shows.
fn fill_viewport<S: LazyStaggeredItemSource>(
    lanes: &mut StaggeredLanes,
    pass: &Pass,
    anchor: usize,
    mut scroll: f32,
    source: &mut S,
) -> (Fill, f32) {
    let mut fill = Fill {
        visible: Vec::new(),
        end: None,
    };
    for attempt in 1..=MAX_FILLS {
        let window = pass.window(scroll);
        // The first item of each lane from the anchor on stays where it was,
        // so the items before it end there and the ones after follow it.
        let starts = lanes.starts_from(anchor);
        fill.visible.clear();
        let reached_start =
            lanes.fill_up(anchor, starts.clone(), window, source, &mut fill.visible);
        fill.visible.reverse();
        fill.end = lanes.fill_down(
            anchor,
            starts,
            window,
            pass.realized,
            source,
            &mut fill.visible,
        );
        let anchored = lanes.slots[anchor].start;
        if reached_start && !lanes.aligned {
            lanes.align();
        }
        // The viewport stays on the anchor while the lanes line up.
        let target = pass.clamp(
            scroll + lanes.slots[anchor].start - anchored,
            fill.end,
            reached_start,
        );
        if attempt == MAX_FILLS || (target - scroll).abs() <= 0.5 {
            break;
        }
        scroll = target;
    }
    (fill, scroll)
}

/// Puts the items of `fill` at their offsets in the viewport, keeps the
/// items beside them composed, and publishes the scroll position and the
/// layout to `state`.
#[expect(clippy::too_many_arguments)]
fn publish<S: LazyStaggeredItemSource>(
    lanes: &mut StaggeredLanes,
    pass: &Pass,
    config: &LazyListMeasureConfig,
    fill: Fill,
    scroll: f32,
    anchor: usize,
    state: &LazyListState,
    source: &mut S,
) -> LazyListMeasureResult {
    let mut visible = fill.visible;
    for item in &mut visible {
        item.offset += pass.before - scroll;
    }
    let first = visible.first().map_or(anchor, |item| item.index);
    let last = visible.last().map_or(first, |item| item.index);
    let placed = place_beyond(lanes, pass, config, scroll, first..last + 1, source);
    let first_offset = scroll - lanes.slots[first].start;
    match visible.first() {
        Some(item) => state.update_scroll_position_with_key(first, first_offset, item.key),
        None => state.update_scroll_position(first, first_offset),
    }
    state.cache_item_sizes(visible.iter().map(|item| (item.index, item.main_axis_size)));
    let content = fill
        .end
        .unwrap_or_else(|| lanes.estimated_extent(pass.items_count, pass.estimate));
    let total_content_size = content + pass.before + pass.after;
    let viewport = if pass.unbounded {
        total_content_size
    } else {
        pass.viewport
    };
    let mut visible_items_info = state.spare_items_info();
    visible_items_info.extend(visible.iter().map(LazyListMeasuredItem::to_item_info));
    state.update_layout_info(LazyListLayoutInfo {
        visible_items_info,
        total_items_count: pass.items_count,
        raw_viewport_size: pass.viewport,
        is_infinite_viewport: pass.unbounded,
        viewport_size: viewport,
        viewport_start_offset: 0.0,
        viewport_end_offset: viewport,
        before_content_padding: pass.before,
        after_content_padding: pass.after,
        main_axis_item_spacing: config.spacing,
        snap_anchor_offset: 0.0,
        reverse_layout: config.reverse_layout,
    });
    state.update_scroll_bounds();
    let can_scroll_forward = !pass.unbounded
        && (last + 1 < pass.items_count
            || visible
                .iter()
                .any(|item| item.offset + item.main_axis_size > viewport - pass.after + 0.5));
    // The items beyond the viewport are placed, but not in view.
    visible.extend(placed);
    visible.sort_unstable_by_key(|item| item.index);
    LazyListMeasureResult {
        visible_items: visible,
        first_visible_item_index: first,
        first_visible_item_scroll_offset: first_offset,
        viewport_size: viewport,
        total_content_size,
        can_scroll_forward,
        can_scroll_backward: first > 0 || first_offset > 0.0,
    }
}

/// Keeps the items on either side of `shown`, the items in view, composed,
/// as many as the configuration asks for, nearest first, and answers the
/// ones the source places, with the focused item, at their offsets in the
/// viewport.
fn place_beyond<S: LazyStaggeredItemSource>(
    lanes: &mut StaggeredLanes,
    pass: &Pass,
    config: &LazyListMeasureConfig,
    scroll: f32,
    shown: Range<usize>,
    source: &mut S,
) -> Vec<LazyListMeasuredItem> {
    let count = config.beyond_bounds_item_count;
    let mut placed = Vec::new();
    let after = (shown.end..pass.items_count).take(count);
    keep_side(lanes, pass, scroll, after, source, &mut placed);
    let before = (0..shown.start).rev().take(count);
    keep_side(lanes, pass, scroll, before, source, &mut placed);
    if let Some(focused) = source
        .focused_item()
        .filter(|index| *index < pass.items_count && !shown.contains(index))
        .filter(|index| placed.iter().all(|item| item.index != *index))
    {
        lanes.reach(focused, pass.estimate, source);
        let item = source.measure(focused, lanes.slots[focused].lane());
        placed.push(place_at(lanes, pass, scroll, item));
    }
    placed
}

/// Keeps `indices`, nearest the viewport first, composed until the source
/// declines one, and adds the ones it places to `placed`.
fn keep_side<S: LazyStaggeredItemSource>(
    lanes: &mut StaggeredLanes,
    pass: &Pass,
    scroll: f32,
    indices: impl Iterator<Item = usize>,
    source: &mut S,
    placed: &mut Vec<LazyListMeasuredItem>,
) {
    let mut distance = 0.0;
    for index in indices {
        lanes.reach(index, pass.estimate, source);
        match source.keep_beyond(index, lanes.slots[index].lane(), distance) {
            BeyondItem::Declined => break,
            BeyondItem::Kept => {}
            BeyondItem::Placed(item) => placed.push(place_at(lanes, pass, scroll, item)),
        }
        distance += lanes.slots[index].size;
    }
}

/// Puts `item`, measured outside the viewport, at its lane's place, sized as
/// measured.
fn place_at(
    lanes: &mut StaggeredLanes,
    pass: &Pass,
    scroll: f32,
    mut item: LazyListMeasuredItem,
) -> LazyListMeasuredItem {
    let slot = &mut lanes.slots[item.index];
    slot.size = item.main_axis_size;
    pass.window(scroll).show(&mut item, *slot);
    item.offset += pass.before - scroll;
    item
}

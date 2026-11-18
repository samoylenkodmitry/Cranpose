// The items a lazy list measured, kept for the next pass of the list.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{SlotId, collections::map::HashMap};
use cranpose_foundation::lazy::{
    LazyItemRoots, LazyListMeasureResult, LazyListMeasuredItem, SmallNodeVec,
};
use cranpose_ui_layout::Axis;

use super::LazyItemMeasureContext;

// The most items the kept list of placed items has room for; a larger one
// grew in a pass that placed far more items than the list shows.
const KEPT_ITEM_LIST_CAPACITY: usize = 256;

#[derive(Default)]
pub(super) struct LazyMeasuredItemCache {
    axis: Option<Axis>,
    cross_axis_bits: u32,
    telemetry: LazyCacheTelemetry,
    pub(super) entries: HashMap<usize, CachedLazyMeasuredItem>,
    focused: Option<(u64, usize)>,
    // The roots of the items that the pass let go of, which the items it
    // measures fill again; empty between passes.
    rooms: Vec<Rc<LazyItemRoots>>,
    // The list of the items the last pass placed, emptied, for the next.
    item_list: Vec<LazyListMeasuredItem>,
}

pub(super) struct CachedLazyMeasuredItem {
    pub(super) item: LazyListMeasuredItem,
    /// The cross size the item was measured at. A grid cell's changes with
    /// its span, its column and the grid's width.
    cross_axis_bits: u32,
}

#[derive(Clone, Copy, Default)]
struct LazyCacheTelemetry {
    candidate_hits: usize,
    candidate_misses: usize,
    exact_reuses: usize,
    exact_misses: usize,
    dirty_children: usize,
    uncached_measures: usize,
}

impl LazyCacheTelemetry {
    fn has_events(self) -> bool {
        self.candidate_hits > 0
            || self.candidate_misses > 0
            || self.exact_reuses > 0
            || self.exact_misses > 0
            || self.dirty_children > 0
            || self.uncached_measures > 0
    }
}

impl LazyMeasuredItemCache {
    /// The index the item in `slot` was last measured at while focused, the
    /// first place to look for it: a focused item scrolled far from the
    /// viewport is found there without scanning the list.
    pub(super) fn focused_index(&self, slot: u64) -> Option<usize> {
        self.focused
            .filter(|(focused, _)| *focused == slot)
            .map(|(_, index)| index)
    }

    pub(super) fn remember_focused(&mut self, slot: u64, index: usize) {
        self.focused = Some((slot, index));
    }

    fn retain_constraint_scope(&mut self, axis: Axis, cross_axis_size: f32) {
        let cross_axis_bits = normalized_axis_bits(cross_axis_size);
        if self.entries.is_empty() {
            self.axis = Some(axis);
            self.cross_axis_bits = cross_axis_bits;
            return;
        }
        if self.axis != Some(axis) || self.cross_axis_bits != cross_axis_bits {
            self.clear();
            self.axis = Some(axis);
            self.cross_axis_bits = cross_axis_bits;
        }
    }

    pub(super) fn clear(&mut self) {
        for (_, cached) in self.entries.drain() {
            keep_room(&mut self.rooms, cached.item.roots);
        }
    }

    /// Drops what a new measure pass cannot reuse: everything, when the
    /// list's axis or cross size changed, and otherwise the items whose slots
    /// are not `retained`.
    pub(super) fn begin_pass(
        &mut self,
        axis: Axis,
        cross_axis_size: f32,
        retained: impl Fn(SlotId) -> bool,
    ) {
        self.retain_constraint_scope(axis, cross_axis_size);
        self.retain_retained_slots(retained);
    }

    /// Keeps only the items whose slots the subcompose state still holds.
    /// An item whose composition was disposed can never be reused, and its
    /// entry would keep the item's whole measured tree alive: an endless list
    /// scrolled past thousands of items kept them all.
    pub(super) fn retain_retained_slots(&mut self, is_retained: impl Fn(SlotId) -> bool) {
        let gone = self
            .entries
            .extract_if(|_, cached| !is_retained(SlotId(cached.item.key)));
        for (_, cached) in gone {
            keep_room(&mut self.rooms, cached.item.roots);
        }
    }

    pub(super) fn get(
        &self,
        item: &LazyItemMeasureContext,
        node_ids: &SmallNodeVec,
    ) -> Option<&CachedLazyMeasuredItem> {
        self.candidate(item)
            .filter(|cached| cached.item.roots.node_ids == *node_ids)
    }

    pub(super) fn contains_index(&self, index: usize) -> bool {
        self.entries.contains_key(&index)
    }

    pub(super) fn candidate(
        &self,
        item: &LazyItemMeasureContext,
    ) -> Option<&CachedLazyMeasuredItem> {
        let cached = self.entries.get(&item.index)?;
        if cached.item.key != item.key_slot_id
            || cached.item.content_type != item.content_type
            || cached.cross_axis_bits != normalized_axis_bits(item.cross_axis_size)
            || cached.item.roots.measurements.is_none()
        {
            return None;
        }
        Some(cached)
    }

    pub(super) fn remove(&mut self, index: usize) {
        if let Some(cached) = self.entries.remove(&index) {
            keep_room(&mut self.rooms, cached.item.roots);
        }
    }

    pub(super) fn insert(&mut self, item: LazyListMeasuredItem, cross_axis_size: f32) {
        let index = item.index;
        let cached = CachedLazyMeasuredItem {
            item,
            cross_axis_bits: normalized_axis_bits(cross_axis_size),
        };
        if let Some(replaced) = self.entries.insert(index, cached) {
            keep_room(&mut self.rooms, replaced.item.roots);
        }
    }

    // The roots of an item the pass let go of, for an item it measures.
    pub(super) fn take_room(&mut self) -> Option<Rc<LazyItemRoots>> {
        self.rooms.pop()
    }

    pub(super) fn take_item_list(&mut self) -> Vec<LazyListMeasuredItem> {
        std::mem::take(&mut self.item_list)
    }

    // Keeps `items`, emptied, as the list the next pass fills.
    pub(super) fn keep_item_list(&mut self, mut items: Vec<LazyListMeasuredItem>) {
        items.clear();
        if items.capacity() <= KEPT_ITEM_LIST_CAPACITY {
            self.item_list = items;
        }
    }

    pub(super) fn record_candidate_hit(&mut self) {
        self.telemetry.candidate_hits += 1;
    }

    fn record_candidate_miss(&mut self) {
        self.telemetry.candidate_misses += 1;
    }

    pub(super) fn record_exact_reuse(&mut self) {
        self.telemetry.exact_reuses += 1;
    }

    pub(super) fn record_exact_miss(&mut self) {
        self.telemetry.exact_misses += 1;
    }

    pub(super) fn record_dirty_children(&mut self) {
        self.telemetry.dirty_children += 1;
    }

    fn take_telemetry(&mut self) -> LazyCacheTelemetry {
        std::mem::take(&mut self.telemetry)
    }

    // Records `count` items a pass kept as they were measured, as the
    // composing pass records each one it reuses.
    pub(super) fn record_retained_reuses(&mut self, count: usize) {
        self.telemetry.candidate_hits += count;
        self.telemetry.exact_reuses += count;
    }

    pub(super) fn record_uncached_measure(&mut self) {
        self.telemetry.uncached_measures += 1;
    }
}

// Notes that `cache` held no measurement of the item at `index` to keep, and
// lets go of the one it held there, which does not fit.
pub(super) fn note_candidate_miss(cache: &RefCell<LazyMeasuredItemCache>, index: usize) {
    let mut cache = cache.borrow_mut();
    cache.record_candidate_miss();
    cache.remove(index);
}

// Ends a measure pass of the list that placed `result`: lets go of the roots
// no item took again, keeps the list of the placed items for the next pass
// and logs what the pass found in the cache.
pub(super) fn end_lazy_cache_pass(
    result: LazyListMeasureResult,
    measured_item_cache: &Rc<RefCell<LazyMeasuredItemCache>>,
) {
    log_lazy_cache_telemetry(&result, measured_item_cache);
    let mut cache = measured_item_cache.borrow_mut();
    cache.rooms.clear();
    cache.keep_item_list(result.visible_items);
}

fn log_lazy_cache_telemetry(
    result: &LazyListMeasureResult,
    measured_item_cache: &Rc<RefCell<LazyMeasuredItemCache>>,
) {
    if !cranpose_core::DIAGNOSTICS.lazy_cache_telemetry.is_set() {
        return;
    }

    let telemetry = measured_item_cache.borrow_mut().take_telemetry();
    if !telemetry.has_events() {
        return;
    }

    let message = format!(
        "[lazy-cache-telemetry] first={} offset={:.2} visible={} candidate_hits={} candidate_misses={} exact_reuses={} exact_misses={} dirty_children={} uncached_measures={} cache_entries={}",
        result.first_visible_item_index,
        result.first_visible_item_scroll_offset,
        result.visible_items.len(),
        telemetry.candidate_hits,
        telemetry.candidate_misses,
        telemetry.exact_reuses,
        telemetry.exact_misses,
        telemetry.dirty_children,
        telemetry.uncached_measures,
        measured_item_cache.borrow().entries.len(),
    );
    log::warn!("{message}");
}

// Keeps `roots` for an item to fill again when nothing else holds them.
fn keep_room(rooms: &mut Vec<Rc<LazyItemRoots>>, mut roots: Rc<LazyItemRoots>) {
    if Rc::get_mut(&mut roots).is_some() {
        rooms.push(roots);
    }
}

fn normalized_axis_bits(size: f32) -> u32 {
    if size.is_finite() && size >= 0.0 {
        size.to_bits()
    } else {
        f32::INFINITY.to_bits()
    }
}

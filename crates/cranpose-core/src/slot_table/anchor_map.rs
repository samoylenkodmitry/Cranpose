use crate::AnchorId;
use std::cell::Cell;

use super::SlotTableDebugStats;

const UNSET_LOCATION: isize = isize::MAX;
const FREE_LOCATION: isize = isize::MAX - 1;
const PAGE_SIZE: usize = 256;

type AnchorPage = Box<[isize; PAGE_SIZE]>;

pub(crate) struct AnchorMap {
    pages: Vec<Option<AnchorPage>>,
    next_anchor_id: Cell<usize>,
    free_anchor_ids: Vec<usize>,
    dirty: bool,
    live_locations: usize,
}

impl Default for AnchorMap {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            next_anchor_id: Cell::new(1),
            free_anchor_ids: Vec::new(),
            dirty: false,
            live_locations: 0,
        }
    }
}

impl AnchorMap {
    pub(crate) fn debug_heap_bytes(&self) -> usize {
        let page_refs_bytes = self.pages.capacity() * std::mem::size_of::<Option<AnchorPage>>();
        let page_bytes =
            self.pages.iter().flatten().count() * PAGE_SIZE * std::mem::size_of::<isize>();
        let free_bytes = self.free_anchor_ids.capacity() * std::mem::size_of::<usize>();
        page_refs_bytes + page_bytes + free_bytes
    }

    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.anchors_len = self.live_locations;
        stats.anchors_cap = self.live_locations + self.free_anchor_ids.capacity();
        stats.gap_metadata_len = 0;
        stats.gap_metadata_cap = 0;
        stats.free_anchor_ids_len = self.free_anchor_ids.len();
        stats.free_anchor_ids_cap = self.free_anchor_ids.capacity();
    }

    pub(crate) fn take_dirty(&mut self) -> bool {
        let dirty = self.dirty;
        self.dirty = false;
        dirty
    }

    pub(crate) fn allocate_anchor(&mut self) -> AnchorId {
        if let Some(id) = self.free_anchor_ids.pop() {
            self.ensure_page(id);
            *self.page_slot_mut(id) = UNSET_LOCATION;
            return AnchorId(id);
        }

        let id = self.next_anchor_id.get();
        self.next_anchor_id.set(id + 1);
        self.ensure_page(id);
        AnchorId(id)
    }

    pub(crate) fn free_anchor(&mut self, anchor: AnchorId) {
        if !anchor.is_valid() || anchor.0 == 0 {
            return;
        }

        let Some(location) = self.page_slot(anchor.0) else {
            self.free_anchor_ids.push(anchor.0);
            return;
        };

        if Self::is_live_location(location) {
            self.live_locations = self.live_locations.saturating_sub(1);
        }

        *self.page_slot_mut(anchor.0) = FREE_LOCATION;
        self.free_anchor_ids.push(anchor.0);
    }

    pub(crate) fn register_anchor(
        &mut self,
        anchor: AnchorId,
        index: usize,
        gap_start: usize,
        size: usize,
    ) {
        debug_assert!(anchor.is_valid(), "attempted to register invalid anchor");
        if anchor.0 == 0 {
            return;
        }

        self.ensure_page(anchor.0);
        let previous = self.page_slot(anchor.0).unwrap_or(UNSET_LOCATION);
        if previous == FREE_LOCATION {
            self.remove_free_anchor_id(anchor.0);
        }
        if !Self::is_live_location(previous) {
            self.live_locations += 1;
        }
        let location = Self::encode_index(index, gap_start, size);
        *self.page_slot_mut(anchor.0) = location;
    }

    pub(crate) fn resolve_anchor(
        &self,
        anchor: AnchorId,
        gap_start: usize,
        size: usize,
    ) -> Option<usize> {
        let location = self.page_slot(anchor.0)?;
        Self::is_live_location(location).then(|| Self::decode_index(location, gap_start, size))
    }

    pub(crate) fn update_for_gap_move(
        &mut self,
        anchor: AnchorId,
        index: usize,
        new_gap_start: usize,
        size: usize,
    ) {
        if !anchor.is_valid() || anchor.0 == 0 {
            return;
        }

        self.ensure_page(anchor.0);
        let previous = self.page_slot(anchor.0).unwrap_or(UNSET_LOCATION);
        if Self::is_live_location(previous) {
            *self.page_slot_mut(anchor.0) = Self::encode_index(index, new_gap_start, size);
        }
    }

    pub(crate) fn rebuild_positions(
        &mut self,
        live_anchors: impl IntoIterator<Item = (AnchorId, usize)>,
        gap_start: usize,
        size: usize,
    ) {
        let live_anchors = live_anchors
            .into_iter()
            .filter(|(anchor, _)| anchor.is_valid() && anchor.0 != 0)
            .collect::<Vec<_>>();
        let max_anchor = live_anchors
            .iter()
            .fold(0usize, |max_anchor, (anchor, _)| max_anchor.max(anchor.0));

        self.pages = Vec::new();
        self.live_locations = 0;
        self.free_anchor_ids = Vec::new();
        if max_anchor != 0 {
            self.pages
                .resize_with(Self::page_index(max_anchor) + 1, || None);
        }

        for (anchor, index) in live_anchors {
            self.ensure_page(anchor.0);
            *self.page_slot_mut(anchor.0) = Self::encode_index(index, gap_start, size);
            self.live_locations += 1;
        }

        self.next_anchor_id.set(max_anchor.saturating_add(1).max(1));
    }

    fn encode_index(index: usize, gap_start: usize, size: usize) -> isize {
        if index < gap_start {
            index as isize
        } else {
            -((size - index) as isize)
        }
    }

    fn decode_index(location: isize, gap_start: usize, size: usize) -> usize {
        if location >= 0 {
            let index = location as usize;
            debug_assert!(index < gap_start || gap_start == size);
            index
        } else {
            size - location.unsigned_abs()
        }
    }

    fn ensure_page(&mut self, anchor_id: usize) {
        if anchor_id == 0 {
            return;
        }

        let page_index = Self::page_index(anchor_id);
        if self.pages.len() <= page_index {
            self.pages.resize_with(page_index + 1, || None);
        }
        if self.pages[page_index].is_none() {
            self.pages[page_index] = Some(Box::new([UNSET_LOCATION; PAGE_SIZE]));
        }
    }

    fn page_slot(&self, anchor_id: usize) -> Option<isize> {
        let page = self.pages.get(Self::page_index(anchor_id))?.as_ref()?;
        Some(page[Self::page_offset(anchor_id)])
    }

    fn page_slot_mut(&mut self, anchor_id: usize) -> &mut isize {
        self.ensure_page(anchor_id);
        let page_index = Self::page_index(anchor_id);
        let page = self.pages[page_index].as_mut().expect("anchor page");
        &mut page[Self::page_offset(anchor_id)]
    }

    fn remove_free_anchor_id(&mut self, anchor_id: usize) {
        if let Some(index) = self
            .free_anchor_ids
            .iter()
            .position(|&free_anchor_id| free_anchor_id == anchor_id)
        {
            self.free_anchor_ids.swap_remove(index);
        }
    }

    fn page_index(anchor_id: usize) -> usize {
        anchor_id / PAGE_SIZE
    }

    fn page_offset(anchor_id: usize) -> usize {
        anchor_id % PAGE_SIZE
    }

    fn is_live_location(location: isize) -> bool {
        location != UNSET_LOCATION && location != FREE_LOCATION
    }
}

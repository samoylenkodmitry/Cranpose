use std::rc::Rc;

use cranpose_core::collections::{bounded_lru::BoundedLruCache, map::HashMap};
use cranpose_render_common::raster_cache::LayerRasterCacheKey;

use crate::{
    draw_pass::ResolvedCompositeKind, frame::DeviceRect, frame_graph::FrameTextureDescriptor,
    geometry::offscreen_byte_size, idle_pool::IDLE_FRAMES, offscreen::OffscreenTarget,
};

const MAX_ENTRIES: usize = 4096;
const MAX_BYTES: u64 = 96 * 1024 * 1024;

#[derive(Clone)]
pub(crate) enum RetainedContent {
    Surface,
    Composite(ResolvedCompositeKind),
}

#[derive(Clone)]
pub(crate) struct Retained {
    pub(crate) texture: Rc<OffscreenTarget>,
    /// The part of the texture the entry holds: all of it when none, a
    /// member's place when the texture is an atlas other entries share.
    pub(crate) region: Option<DeviceRect>,
    pub(crate) content: RetainedContent,
}

impl Retained {
    pub(crate) fn surface(texture: Rc<OffscreenTarget>) -> Self {
        Self {
            texture,
            region: None,
            content: RetainedContent::Surface,
        }
    }

    /// The surface at `region` of an atlas.
    pub(crate) fn surface_in(texture: Rc<OffscreenTarget>, region: DeviceRect) -> Self {
        Self {
            texture,
            region: Some(region),
            content: RetainedContent::Surface,
        }
    }

    pub(crate) fn composite(texture: Rc<OffscreenTarget>, kind: ResolvedCompositeKind) -> Self {
        Self {
            texture,
            region: None,
            content: RetainedContent::Composite(kind),
        }
    }

    /// The pixels the entry holds.
    fn area(&self) -> u64 {
        match self.region {
            Some(region) => (region.width * region.height) as u64,
            None => u64::from(self.texture.width) * u64::from(self.texture.height),
        }
    }
}

struct Allocation {
    bytes: u64,
    holders: usize,
    transient: Option<FrameTextureDescriptor>,
    /// The pixels the texture's holders hold, and the most they held.
    held: u64,
    peak: u64,
}

#[derive(Default)]
struct AllocationLedger {
    records: HashMap<usize, Allocation>,
    bytes: u64,
}

impl AllocationLedger {
    fn attach(
        &mut self,
        id: usize,
        bytes: u64,
        transient: Option<FrameTextureDescriptor>,
        held: u64,
    ) {
        let record = self.records.entry(id).or_insert(Allocation {
            bytes,
            holders: 0,
            transient,
            held: 0,
            peak: 0,
        });
        record.holders += 1;
        record.held += held;
        record.peak = record.peak.max(record.held);
        if record.holders == 1 {
            self.bytes = self.bytes.saturating_add(record.bytes);
        }
    }

    fn detach(&mut self, id: usize, held: u64) -> Option<Allocation> {
        let record = self.records.get_mut(&id)?;
        record.holders -= 1;
        record.held = record.held.saturating_sub(held);
        if record.holders > 0 {
            return None;
        }
        let record = self.records.remove(&id)?;
        self.bytes = self.bytes.saturating_sub(record.bytes);
        Some(record)
    }

    /// Whether the texture's holders keep under a quarter of the pixels
    /// they once held: an atlas whose members mostly left.
    fn sparse(&self, id: usize) -> bool {
        self.records
            .get(&id)
            .is_some_and(|record| record.held.saturating_mul(4) < record.peak)
    }
}

fn texture_id(texture: &Rc<OffscreenTarget>) -> usize {
    Rc::as_ptr(texture) as usize
}

struct Entry {
    retained: Retained,
    used: u64,
}

pub(crate) struct LayerCache {
    entries: BoundedLruCache<LayerRasterCacheKey, Entry>,
    ledger: AllocationLedger,
    retired: Vec<(Option<FrameTextureDescriptor>, Rc<OffscreenTarget>)>,
    /// Shared textures whose holders mostly left: their last entries go
    /// at the frame's end, so the texture goes back to its pool.
    sparse: Vec<usize>,
    max_bytes: u64,
    frame: u64,
}

impl LayerCache {
    pub(crate) fn new() -> Self {
        Self::with_budget(MAX_BYTES)
    }

    fn with_budget(max_bytes: u64) -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_ENTRIES),
            ledger: AllocationLedger::default(),
            retired: Vec::new(),
            sparse: Vec::new(),
            max_bytes,
            frame: 0,
        }
    }

    pub(crate) fn get(&mut self, key: &LayerRasterCacheKey) -> Option<Retained> {
        let frame = self.frame;
        self.entries.get_mut(key).map(|entry| {
            entry.used = frame;
            entry.retained.clone()
        })
    }

    pub(crate) fn fits(&self, width: u32, height: u32) -> bool {
        offscreen_byte_size(width, height) <= self.max_bytes
    }

    pub(crate) fn insert(
        &mut self,
        key: LayerRasterCacheKey,
        retained: Retained,
        transient: Option<FrameTextureDescriptor>,
    ) -> bool {
        if !self.fits(retained.texture.width, retained.texture.height) {
            return false;
        }
        if let Some(pending) = self
            .retired
            .iter()
            .position(|(_, texture)| Rc::ptr_eq(texture, &retained.texture))
        {
            self.retired.swap_remove(pending);
        }
        let bytes = offscreen_byte_size(retained.texture.width, retained.texture.height);
        self.ledger.attach(
            texture_id(&retained.texture),
            bytes,
            transient,
            retained.area(),
        );
        while self.ledger.bytes > self.max_bytes {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.release(evicted.retained);
        }
        let used = self.frame;
        if let Some((_, replaced)) = self.entries.push(key, Entry { retained, used }) {
            self.release(replaced.retained);
        }
        true
    }

    /// Ends a frame, releasing every raster no frame has read for
    /// [`IDLE_FRAMES`].
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        let oldest_kept = self.frame.saturating_sub(IDLE_FRAMES);
        while self
            .entries
            .peek_lru()
            .is_some_and(|(_, entry)| entry.used < oldest_kept)
        {
            let Some((_, idle)) = self.entries.pop_lru() else {
                break;
            };
            self.release(idle.retained);
        }
        while !self.sparse.is_empty() {
            let sparse = std::mem::take(&mut self.sparse);
            let leaving: Vec<LayerRasterCacheKey> = self
                .entries
                .iter()
                .filter(|(_, entry)| sparse.contains(&texture_id(&entry.retained.texture)))
                .map(|(key, _)| *key)
                .collect();
            for key in leaving {
                self.remove(&key);
            }
        }
    }

    fn release(&mut self, entry: Retained) {
        let id = texture_id(&entry.texture);
        match self.ledger.detach(id, entry.area()) {
            Some(allocation) => self.retired.push((allocation.transient, entry.texture)),
            None if self.ledger.sparse(id) && !self.sparse.contains(&id) => self.sparse.push(id),
            None => {}
        }
    }

    /// Releases every raster: they may hold a placeholder drawn while a
    /// pipeline compiled.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn clear(&mut self) {
        while let Some((_, entry)) = self.entries.pop_lru() {
            self.release(entry.retained);
        }
    }

    pub(crate) fn remove(&mut self, key: &LayerRasterCacheKey) {
        if let Some(entry) = self.entries.pop(key) {
            self.release(entry.retained);
        }
    }

    pub(crate) fn take_released(
        &mut self,
    ) -> impl Iterator<Item = (Option<FrameTextureDescriptor>, OffscreenTarget)> + '_ {
        self.retired
            .extract_if(.., |(_, texture)| Rc::strong_count(texture) == 1)
            .filter_map(|(transient, texture)| {
                Rc::into_inner(texture).map(|target| (transient, target))
            })
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.ledger.bytes
    }
}

#[cfg(test)]
#[path = "tests/layer_cache_tests.rs"]
mod tests;

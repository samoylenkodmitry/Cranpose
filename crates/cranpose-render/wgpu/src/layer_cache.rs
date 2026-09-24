use std::{collections::HashMap, rc::Rc};

use cranpose_render_common::{
    bounded_lru_cache::BoundedLruCache, raster_cache::LayerRasterCacheKey,
};

use crate::{
    draw_pass::ResolvedCompositeKind, frame_graph::FrameTextureDescriptor,
    geometry::offscreen_byte_size, offscreen::OffscreenTarget,
};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: u64 = 96 * 1024 * 1024;

#[derive(Clone)]
pub(crate) enum RetainedContent {
    Surface,
    Composite(ResolvedCompositeKind),
}

#[derive(Clone)]
pub(crate) struct Retained {
    pub(crate) texture: Rc<OffscreenTarget>,
    pub(crate) content: RetainedContent,
}

impl Retained {
    pub(crate) fn surface(texture: Rc<OffscreenTarget>) -> Self {
        Self {
            texture,
            content: RetainedContent::Surface,
        }
    }

    pub(crate) fn composite(texture: Rc<OffscreenTarget>, kind: ResolvedCompositeKind) -> Self {
        Self {
            texture,
            content: RetainedContent::Composite(kind),
        }
    }
}

struct Allocation {
    bytes: u64,
    holders: usize,
    transient: Option<FrameTextureDescriptor>,
}

#[derive(Default)]
struct AllocationLedger {
    records: HashMap<usize, Allocation>,
    bytes: u64,
}

impl AllocationLedger {
    fn attach(&mut self, id: usize, bytes: u64, transient: Option<FrameTextureDescriptor>) {
        let record = self.records.entry(id).or_insert(Allocation {
            bytes,
            holders: 0,
            transient,
        });
        record.holders += 1;
        if record.holders == 1 {
            self.bytes = self.bytes.saturating_add(record.bytes);
        }
    }

    fn detach(&mut self, id: usize) -> Option<Allocation> {
        let record = self.records.get_mut(&id)?;
        record.holders -= 1;
        if record.holders > 0 {
            return None;
        }
        let record = self.records.remove(&id)?;
        self.bytes = self.bytes.saturating_sub(record.bytes);
        Some(record)
    }
}

fn texture_id(texture: &Rc<OffscreenTarget>) -> usize {
    Rc::as_ptr(texture) as usize
}

pub(crate) struct LayerCache {
    entries: BoundedLruCache<LayerRasterCacheKey, Retained>,
    ledger: AllocationLedger,
    retired: Vec<(Option<FrameTextureDescriptor>, Rc<OffscreenTarget>)>,
    max_bytes: u64,
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
            max_bytes,
        }
    }

    pub(crate) fn get(&mut self, key: &LayerRasterCacheKey) -> Option<Retained> {
        self.entries.get(key).cloned()
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
        self.ledger
            .attach(texture_id(&retained.texture), bytes, transient);
        while self.ledger.bytes > self.max_bytes {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.release(evicted);
        }
        if let Some((_, replaced)) = self.entries.push(key, retained) {
            self.release(replaced);
        }
        true
    }

    fn release(&mut self, entry: Retained) {
        if let Some(allocation) = self.ledger.detach(texture_id(&entry.texture)) {
            self.retired.push((allocation.transient, entry.texture));
        }
    }

    pub(crate) fn remove(&mut self, key: &LayerRasterCacheKey) {
        if let Some(entry) = self.entries.pop(key) {
            self.release(entry);
        }
    }

    pub(crate) fn take_released(
        &mut self,
    ) -> Vec<(Option<FrameTextureDescriptor>, OffscreenTarget)> {
        let mut released = Vec::new();
        let retired = std::mem::take(&mut self.retired);
        self.retired = retired
            .into_iter()
            .filter_map(|(transient, texture)| match Rc::try_unwrap(texture) {
                Ok(texture) => {
                    released.push((transient, texture));
                    None
                }
                Err(texture) => Some((transient, texture)),
            })
            .collect();
        released
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

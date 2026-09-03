use std::rc::Rc;

use cranpose_render_common::{
    bounded_lru_cache::BoundedLruCache, raster_cache::LayerRasterCacheKey,
};

use crate::{geometry::offscreen_byte_size, offscreen::OffscreenTarget};

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: u64 = 96 * 1024 * 1024;

/// Textures of isolated layers whose pixels are a pure function of their
/// content: keyed by the layer's content and effect hashes, size and scale,
/// evicted least-recently-used once the byte budget is exceeded.
pub(crate) struct LayerCache {
    entries: BoundedLruCache<LayerRasterCacheKey, Rc<OffscreenTarget>>,
    bytes: u64,
    evicted: Vec<Rc<OffscreenTarget>>,
}

impl LayerCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_ENTRIES),
            bytes: 0,
            evicted: Vec::new(),
        }
    }

    pub(crate) fn get(&mut self, key: &LayerRasterCacheKey) -> Option<Rc<OffscreenTarget>> {
        self.entries.get(key).cloned()
    }

    pub(crate) fn insert(&mut self, key: LayerRasterCacheKey, texture: Rc<OffscreenTarget>) {
        let bytes = offscreen_byte_size(texture.width, texture.height);
        while self.bytes.saturating_add(bytes) > MAX_BYTES {
            let Some((_, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self
                .bytes
                .saturating_sub(offscreen_byte_size(evicted.width, evicted.height));
            self.evicted.push(evicted);
        }
        if let Some((_, replaced)) = self.entries.push(key, texture) {
            self.bytes = self
                .bytes
                .saturating_sub(offscreen_byte_size(replaced.width, replaced.height));
            self.evicted.push(replaced);
        }
        self.bytes = self.bytes.saturating_add(bytes);
    }

    /// Textures dropped from the cache since the last call, once no frame
    /// still holds them, so their memory returns to the offscreen pool.
    pub(crate) fn take_released(&mut self) -> Vec<OffscreenTarget> {
        let mut released = Vec::new();
        let mut still_held = Vec::new();
        for texture in self.evicted.drain(..) {
            match Rc::try_unwrap(texture) {
                Ok(texture) => released.push(texture),
                Err(texture) => still_held.push(texture),
            }
        }
        self.evicted = still_held;
        released
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }
}

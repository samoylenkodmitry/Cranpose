use cranpose_core::collections::bounded_lru::BoundedLruCache;

use super::{
    MAX_TEXTURE_CACHE_ITEMS,
    image_draws::{CachedImageTexture, ImageBindGroups},
};

const MAX_IMAGE_TEXTURE_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_EXTERNAL_TEXTURE_BINDINGS: usize = 8;

// The renderer's uploaded image textures, each keyed by its image id and
// held to a byte budget, and the bindings of the external textures draws
// sample in place.
pub(super) struct ImageTextures {
    cache: BoundedLruCache<u64, CachedImageTexture>,
    bytes: usize,
    external: BoundedLruCache<u64, ImageBindGroups>,
}

impl ImageTextures {
    pub(super) fn new() -> Self {
        Self {
            cache: BoundedLruCache::with_capacity_at_least_one(MAX_TEXTURE_CACHE_ITEMS),
            bytes: 0,
            external: BoundedLruCache::with_capacity_at_least_one(MAX_EXTERNAL_TEXTURE_BINDINGS),
        }
    }

    pub(super) fn get(&mut self, id: u64) -> Option<&CachedImageTexture> {
        self.cache.get(&id)
    }

    // Keeps `texture` for image `id`, then evicts the least recently drawn
    // textures while the cache holds more than its byte budget and more than
    // one texture.
    pub(super) fn insert(&mut self, id: u64, texture: CachedImageTexture) {
        let bytes = texture.bytes;
        if let Some(replaced) = self.cache.put(id, texture) {
            self.bytes = self.bytes.saturating_sub(replaced.bytes);
        }
        self.bytes += bytes;
        while self.bytes > MAX_IMAGE_TEXTURE_CACHE_BYTES && self.cache.len() > 1 {
            let Some((_, evicted)) = self.cache.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.bytes);
        }
    }

    pub(super) fn external(&mut self, key: u64) -> Option<&ImageBindGroups> {
        self.external.get(&key)
    }

    pub(super) fn insert_external(&mut self, key: u64, bind_groups: ImageBindGroups) {
        self.external.put(key, bind_groups);
    }

    pub(super) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.cache.cap().get()
    }
}

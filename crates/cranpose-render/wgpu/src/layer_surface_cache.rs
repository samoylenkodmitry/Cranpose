use crate::gpu_stats::FrameStats;
use crate::offscreen::OffscreenTarget;
use crate::surface_executor::{offscreen_byte_size, CachedLayerSurface};
use cranpose_render_common::bounded_lru_cache::BoundedLruCache;
use cranpose_render_common::raster_cache::LayerRasterCacheKey;
use cranpose_ui_graphics::Rect;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_LAYER_SURFACE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_LAYER_SURFACE_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY: usize = 256;

pub(crate) struct LayerSurfaceCache {
    entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>,
    identity: HashMap<usize, LayerRasterCacheKey>,
    bytes: u64,
    seen_this_frame: HashSet<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayerSurfaceCacheDebugStats {
    pub(crate) entries_len: usize,
    pub(crate) entries_cap: usize,
    pub(crate) identity_len: usize,
    pub(crate) identity_cap: usize,
    pub(crate) seen_this_frame_len: usize,
    pub(crate) seen_this_frame_cap: usize,
}

impl LayerSurfaceCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_LAYER_SURFACE_CACHE_ITEMS),
            identity: HashMap::new(),
            bytes: 0,
            seen_this_frame: HashSet::new(),
        }
    }

    pub(crate) fn get(
        &mut self,
        key: &LayerRasterCacheKey,
        frame_stats: &FrameStats,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        if let Some(stable_id) = key.stable_id() {
            self.seen_this_frame.insert(stable_id);
        }
        let cached = self.entries.get(key)?;
        let (width, height) = key.pixel_size();
        frame_stats.record_layer_cache_hit(width, height);
        Some((cached.target.clone(), cached.logical_rect))
    }

    pub(crate) fn insert(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
        frame_stats: &FrameStats,
    ) -> Rc<OffscreenTarget> {
        let byte_size = offscreen_byte_size(target.width, target.height);
        if let Some(stable_id) = key.stable_id() {
            self.seen_this_frame.insert(stable_id);
            if let Some(previous_key) = self.identity.insert(stable_id, key) {
                if previous_key != key {
                    self.remove(&previous_key);
                }
            }
        }

        while self.bytes + byte_size > MAX_LAYER_SURFACE_CACHE_BYTES {
            let Some((evicted_key, evicted_entry)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted_entry.byte_size);
            self.remove_identity_for_key(&evicted_key);
            frame_stats.record_layer_cache_eviction();
        }

        let cached = CachedLayerSurface {
            target: Rc::new(target),
            logical_rect,
            byte_size,
        };
        let cached_handle = cached.target.clone();
        if let Some((replaced_key, replaced_entry)) = self.entries.push(key, cached) {
            self.bytes = self.bytes.saturating_sub(replaced_entry.byte_size);
            if replaced_key != key {
                frame_stats.record_layer_cache_eviction();
            }
            self.remove_identity_for_key(&replaced_key);
        }
        self.bytes = self.bytes.saturating_add(byte_size);
        cached_handle
    }

    pub(crate) fn finish_frame(&mut self, frame_stats: &FrameStats) {
        self.seen_this_frame.clear();
        if self.seen_this_frame.capacity() > RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY {
            self.seen_this_frame
                .shrink_to(RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY);
        }

        self.identity.retain(|_, key| self.entries.contains(key));
        frame_stats.layer_cache_size.set(self.entries.len() as u32);
        frame_stats.layer_cache_bytes.set(self.bytes);
    }

    pub(crate) fn debug_stats(&self) -> LayerSurfaceCacheDebugStats {
        LayerSurfaceCacheDebugStats {
            entries_len: self.entries.len(),
            entries_cap: self.entries.cap().get(),
            identity_len: self.identity.len(),
            identity_cap: self.identity.capacity(),
            seen_this_frame_len: self.seen_this_frame.len(),
            seen_this_frame_cap: self.seen_this_frame.capacity(),
        }
    }

    fn remove(&mut self, key: &LayerRasterCacheKey) {
        let Some(entry) = self.entries.pop(key) else {
            return;
        };
        self.bytes = self.bytes.saturating_sub(entry.byte_size);
        self.remove_identity_for_key(key);
    }

    fn remove_identity_for_key(&mut self, key: &LayerRasterCacheKey) {
        if let Some(stable_id) = key.stable_id() {
            if self.identity.get(&stable_id) == Some(key) {
                self.identity.remove(&stable_id);
            }
        }
    }
}

impl Default for LayerSurfaceCache {
    fn default() -> Self {
        Self::new()
    }
}

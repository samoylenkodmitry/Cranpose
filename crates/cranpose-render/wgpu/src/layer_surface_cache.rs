use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use cranpose_render_common::{
    bounded_lru_cache::BoundedLruCache,
    raster_cache::{LayerRasterCacheIdentity, LayerRasterCacheKey},
};
use cranpose_ui_graphics::Rect;

use crate::{
    gpu_stats::FrameStats,
    offscreen::OffscreenTarget,
    surface_executor::{offscreen_byte_size, CachedLayerSurface},
};

const MAX_LAYER_SURFACE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_LAYER_SURFACE_CACHE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SCENE_RANGE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_SCENE_RANGE_CACHE_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_SCENE_RANGE_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY: usize = 256;

/// Env-gated cache-key diagnostics (`CRANPOSE_LAYER_CACHE_DIAG=1`): logs every
/// miss with the key that missed, and every insert with the key it landed
/// under, so a cache that misses without eviction pressure names the field
/// that varied — or shows that the two key spaces never met.
pub(crate) fn cache_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_LAYER_CACHE_DIAG").is_some())
}

/// Takes out the values no other holder still references, and keeps the rest
/// for a later call.
fn take_unshared<T>(pending: &mut Vec<Rc<T>>) -> Vec<T> {
    let mut free = Vec::new();
    let mut held = Vec::new();
    for value in pending.drain(..) {
        match Rc::try_unwrap(value) {
            Ok(owned) => free.push(owned),
            Err(shared) => held.push(shared),
        }
    }
    *pending = held;
    free
}

pub(crate) struct LayerSurfaceCache {
    entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>,
    scene_range_entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>,
    identity: HashMap<LayerRasterCacheIdentity, LayerRasterCacheKey>,
    bytes: u64,
    scene_range_bytes: u64,
    seen_this_frame: HashSet<usize>,
    /// Surfaces this cache no longer keys, waiting to go back to the offscreen
    /// pool.
    ///
    /// A layer whose backdrop moves changes its key every frame, so its entry
    /// is replaced every frame. Dropping the entry here would free the texture
    /// to the allocator while the pool stays empty of that size, and the next
    /// acquire creates the texture again: a scrolling list with a frosted bar
    /// allocated twelve targets and 15 MB per frame on a Mali G76 with a pool
    /// of twenty-six unused targets. The sizes repeat exactly, so the pool
    /// serves every one of them once the surfaces come back.
    recycled: Vec<Rc<OffscreenTarget>>,
    /// Keys stored this frame, checked at `finish_frame` against the keys the
    /// render paths were probed for and missed.
    #[cfg(debug_assertions)]
    inserted_this_frame: HashSet<LayerRasterCacheKey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayerSurfaceCacheDebugStats {
    pub(crate) entries_len: usize,
    pub(crate) entries_cap: usize,
    pub(crate) scene_range_entries_len: usize,
    pub(crate) scene_range_entries_cap: usize,
    pub(crate) identity_len: usize,
    pub(crate) identity_cap: usize,
    pub(crate) seen_this_frame_len: usize,
    pub(crate) seen_this_frame_cap: usize,
}

impl LayerSurfaceCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(MAX_LAYER_SURFACE_CACHE_ITEMS),
            scene_range_entries: BoundedLruCache::with_capacity_at_least_one(
                MAX_SCENE_RANGE_CACHE_ITEMS,
            ),
            identity: HashMap::new(),
            bytes: 0,
            scene_range_bytes: 0,
            seen_this_frame: HashSet::new(),
            recycled: Vec::new(),
            #[cfg(debug_assertions)]
            inserted_this_frame: HashSet::new(),
        }
    }

    fn recycle(&mut self, entry: CachedLayerSurface) {
        self.recycled.push(entry.target);
    }

    /// Hands back the surfaces no other holder still references.
    ///
    /// A surface stays in the list while the frame that composites from it is
    /// still recorded, and is offered again on a later frame.
    pub(crate) fn take_recycled(&mut self) -> Vec<OffscreenTarget> {
        take_unshared(&mut self.recycled)
    }

    pub(crate) fn get(
        &mut self,
        key: &LayerRasterCacheKey,
        frame_stats: &FrameStats,
    ) -> Option<(Rc<OffscreenTarget>, Rect)> {
        if let Some(stable_id) = key.stable_id() {
            self.seen_this_frame.insert(stable_id);
        }
        let cached = if key.is_scene_range() {
            self.scene_range_entries.get(key)?
        } else {
            self.entries.get(key)?
        };
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
        #[cfg(debug_assertions)]
        self.inserted_this_frame.insert(key);

        if key.is_scene_range() {
            return self.insert_scene_range(key, target, logical_rect, frame_stats);
        }

        let byte_size = offscreen_byte_size(target.width, target.height);
        if let Some(stable_id) = key.stable_id() {
            self.seen_this_frame.insert(stable_id);
            let identity = key.identity().expect("stable cache key must have identity");
            if let Some(previous_key) = self.identity.insert(identity, key) {
                if previous_key != key {
                    if cache_diag_enabled() {
                        log::warn!(
                            "[layer-cache-diag] rekey {identity:?} prev={previous_key:?} new={key:?}"
                        );
                    }
                    self.remove(&previous_key);
                }
            } else if cache_diag_enabled() {
                log::warn!("[layer-cache-diag] insert {identity:?} key={key:?}");
            }
        } else if cache_diag_enabled() {
            log::warn!("[layer-cache-diag] insert anonymous key={key:?}");
        }

        while self.bytes + byte_size > MAX_LAYER_SURFACE_CACHE_BYTES {
            let Some((evicted_key, evicted_entry)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted_entry.byte_size);
            self.recycle(evicted_entry);
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
            self.recycle(replaced_entry);
            if replaced_key != key {
                frame_stats.record_layer_cache_eviction();
            }
            self.remove_identity_for_key(&replaced_key);
        }
        self.bytes = self.bytes.saturating_add(byte_size);
        cached_handle
    }

    /// Fails the frame if the render paths were probed for a key that nothing
    /// went on to store.
    ///
    /// A miss is a one-off cost by design: whatever missed gets rendered and
    /// stored under the key that missed, so the next frame hits it. A miss on
    /// a key no path ever stores under is a miss that repeats every frame for
    /// the life of the layer, and in the counters it is indistinguishable from
    /// honest eviction pressure. Issue #478 was exactly that -- the retained
    /// lookup probed a full-surface key for every backdrop-carrying layer,
    /// which is stored under a source-content key and never under the one
    /// probed -- and it hid behind a plausible-looking hit rate for as long as
    /// nothing compared the two sets. Every scene any test renders now does.
    #[cfg(debug_assertions)]
    fn assert_every_miss_was_stored(&self, frame_stats: &FrameStats) {
        let missed = frame_stats.take_missed_layer_cache_keys();
        let Some(unstored) = missed
            .iter()
            .find(|key| !self.inserted_this_frame.contains(key))
        else {
            return;
        };
        panic!(
            "layer cache was probed for a key nothing stored: {unstored:?}\n\
             a miss must be paid once, not every frame -- the probing path and \
             the storing path have to name the surface the same way. Run with \
             CRANPOSE_LAYER_CACHE_DIAG=1 to see which path probed it."
        );
    }

    pub(crate) fn finish_frame(&mut self, frame_stats: &FrameStats) {
        #[cfg(debug_assertions)]
        self.assert_every_miss_was_stored(frame_stats);

        self.seen_this_frame.clear();
        if self.seen_this_frame.capacity() > RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY {
            self.seen_this_frame
                .shrink_to(RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY);
        }

        #[cfg(debug_assertions)]
        self.inserted_this_frame.clear();

        self.identity.retain(|_, key| self.entries.contains(key));
        frame_stats
            .layer_cache_size
            .set((self.entries.len() + self.scene_range_entries.len()) as u32);
        frame_stats
            .layer_cache_bytes
            .set(self.bytes + self.scene_range_bytes);
    }

    pub(crate) fn debug_stats(&self) -> LayerSurfaceCacheDebugStats {
        LayerSurfaceCacheDebugStats {
            entries_len: self.entries.len(),
            entries_cap: self.entries.cap().get(),
            scene_range_entries_len: self.scene_range_entries.len(),
            scene_range_entries_cap: self.scene_range_entries.cap().get(),
            identity_len: self.identity.len(),
            identity_cap: self.identity.capacity(),
            seen_this_frame_len: self.seen_this_frame.len(),
            seen_this_frame_cap: self.seen_this_frame.capacity(),
        }
    }

    fn remove(&mut self, key: &LayerRasterCacheKey) {
        if key.is_scene_range() {
            self.remove_scene_range(key);
            return;
        }

        let Some(entry) = self.entries.pop(key) else {
            return;
        };
        self.bytes = self.bytes.saturating_sub(entry.byte_size);
        self.recycle(entry);
        self.remove_identity_for_key(key);
    }

    fn remove_identity_for_key(&mut self, key: &LayerRasterCacheKey) {
        if let Some(identity) = key.identity() {
            if self.identity.get(&identity) == Some(key) {
                self.identity.remove(&identity);
            }
        }
    }

    fn insert_scene_range(
        &mut self,
        key: LayerRasterCacheKey,
        target: OffscreenTarget,
        logical_rect: Rect,
        frame_stats: &FrameStats,
    ) -> Rc<OffscreenTarget> {
        let byte_size = offscreen_byte_size(target.width, target.height);

        while self.scene_range_bytes + byte_size > MAX_SCENE_RANGE_CACHE_BYTES {
            let Some((_, evicted_entry)) = self.scene_range_entries.pop_lru() else {
                break;
            };
            self.scene_range_bytes = self
                .scene_range_bytes
                .saturating_sub(evicted_entry.byte_size);
            self.recycle(evicted_entry);
            frame_stats.record_layer_cache_eviction();
        }

        let cached = CachedLayerSurface {
            target: Rc::new(target),
            logical_rect,
            byte_size,
        };
        let cached_handle = cached.target.clone();
        if let Some((_, replaced_entry)) = self.scene_range_entries.push(key, cached) {
            self.scene_range_bytes = self
                .scene_range_bytes
                .saturating_sub(replaced_entry.byte_size);
            self.recycle(replaced_entry);
            frame_stats.record_layer_cache_eviction();
        }
        self.scene_range_bytes = self.scene_range_bytes.saturating_add(byte_size);
        cached_handle
    }

    fn remove_scene_range(&mut self, key: &LayerRasterCacheKey) {
        let Some(entry) = self.scene_range_entries.pop(key) else {
            return;
        };
        self.scene_range_bytes = self.scene_range_bytes.saturating_sub(entry.byte_size);
        self.recycle(entry);
    }
}

impl Default for LayerSurfaceCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{take_unshared, MAX_SCENE_RANGE_CACHE_BYTES, MAX_SCENE_RANGE_CACHE_ENTRY_BYTES};
    use crate::surface_executor::offscreen_byte_size;

    /// A layer whose backdrop moves replaces its cache entry every frame. The
    /// surface it drops has to come back for the offscreen pool, or the
    /// renderer creates a texture of the same size again on the next frame.
    #[test]
    fn a_dropped_surface_comes_back() {
        let mut pending = vec![Rc::new(7u32), Rc::new(9u32)];
        let mut taken = take_unshared(&mut pending);
        taken.sort_unstable();
        assert_eq!(taken, vec![7, 9]);
        assert!(pending.is_empty());
    }

    /// A surface the recorded frame still composites from stays held, and is
    /// offered again once its last holder lets go.
    #[test]
    fn a_surface_in_use_waits_for_its_last_holder() {
        let held = Rc::new(7u32);
        let mut pending = vec![Rc::clone(&held), Rc::new(9u32)];
        assert_eq!(take_unshared(&mut pending), vec![9]);
        assert_eq!(pending.len(), 1);
        drop(held);
        assert_eq!(take_unshared(&mut pending), vec![7]);
        assert!(pending.is_empty());
    }

    #[test]
    fn scene_range_cache_keeps_multiple_visible_scale_buckets() {
        let scaled_shader_ranges = [(1572, 66), (1181, 1198), (328, 390), (514, 268), (475, 189)];
        let unscaled_shader_ranges = [(1160, 48), (872, 885), (242, 288), (380, 198), (351, 139)];
        let required_bytes: u64 = scaled_shader_ranges
            .into_iter()
            .chain(unscaled_shader_ranges)
            .map(|(width, height)| {
                let bytes = offscreen_byte_size(width, height);
                assert!(
                    bytes <= MAX_SCENE_RANGE_CACHE_ENTRY_BYTES,
                    "each reusable range must fit the per-entry scene-range budget"
                );
                bytes
            })
            .sum();

        assert!(
            required_bytes <= MAX_SCENE_RANGE_CACHE_BYTES,
            "shader/backdrop drag should not evict reusable scene ranges between scale buckets: required={required_bytes} budget={MAX_SCENE_RANGE_CACHE_BYTES}"
        );
    }
}

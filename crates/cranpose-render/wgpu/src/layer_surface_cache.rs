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
    surface_executor::{CachedLayerSurface, offscreen_byte_size},
};

const MAX_LAYER_SURFACE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_LAYER_SURFACE_CACHE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SCENE_RANGE_CACHE_ITEMS: usize = 256;
pub(crate) const MAX_SCENE_RANGE_CACHE_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_SCENE_RANGE_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const RETAINED_LAYER_SEEN_THIS_FRAME_CAPACITY: usize = 256;

pub(crate) fn cache_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_LAYER_CACHE_DIAG").is_some())
}

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

const UNREWARDED_STORES_BEFORE_BACKOFF: u32 = 3;
const ADMISSION_BACKOFF_BASE_FRAMES: u64 = 8;
const ADMISSION_BACKOFF_MAX_FRAMES: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AdmissionScope {
    Identity(LayerRasterCacheIdentity),
    Place {
        kind_slot: usize,
        local_bounds_bits: [u32; 4],
        pixel_size: (u32, u32),
    },
}

fn admission_scope(key: &LayerRasterCacheKey) -> AdmissionScope {
    key.identity()
        .map(AdmissionScope::Identity)
        .unwrap_or_else(|| AdmissionScope::Place {
            kind_slot: key.kind_slot(),
            local_bounds_bits: key.local_bounds_bits(),
            pixel_size: key.pixel_size(),
        })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AdmissionBackoff {
    unrewarded_stores: u32,
    resume_at_frame: u64,
}

/// Speculative (repeat-admitted) stores that never get served are pure
/// cost: the entry is rendered, kept, and replaced by the next key before
/// anything reads it. Content that holds for a couple of frames and then
/// moves on -- a stepped animation, a starfield that advances at half the
/// presented rate -- passes the repeat check every time and would be
/// re-stored forever. The ledger counts, per node identity (or per place for
/// keys without one: kind, bounds and size), consecutive frames that stored
/// without a hit, and once
/// that streak reaches [`UNREWARDED_STORES_BEFORE_BACKOFF`] refuses further
/// repeat admissions for a window that doubles per extra unrewarded store up
/// to [`ADMISSION_BACKOFF_MAX_FRAMES`]. A hit clears the scope, so content
/// that settles is cached again at most one window later.
#[derive(Debug, Default)]
struct AdmissionLedger {
    frame: u64,
    backoff: HashMap<AdmissionScope, AdmissionBackoff>,
    stored_this_frame: HashSet<AdmissionScope>,
    hit_this_frame: HashSet<AdmissionScope>,
}

impl AdmissionLedger {
    fn note_hit(&mut self, key: &LayerRasterCacheKey) {
        self.hit_this_frame.insert(admission_scope(key));
    }

    fn note_store(&mut self, key: &LayerRasterCacheKey) {
        self.stored_this_frame.insert(admission_scope(key));
    }

    fn is_backed_off(&self, key: &LayerRasterCacheKey) -> bool {
        self.backoff
            .get(&admission_scope(key))
            .is_some_and(|backoff| backoff.resume_at_frame > self.frame)
    }

    fn finish_frame(&mut self) {
        let hit_this_frame = std::mem::take(&mut self.hit_this_frame);
        for scope in self.stored_this_frame.drain() {
            if hit_this_frame.contains(&scope) {
                continue;
            }
            let backoff = self.backoff.entry(scope).or_default();
            backoff.unrewarded_stores = backoff.unrewarded_stores.saturating_add(1);
            if backoff.unrewarded_stores >= UNREWARDED_STORES_BEFORE_BACKOFF {
                let doublings = (backoff.unrewarded_stores - UNREWARDED_STORES_BEFORE_BACKOFF)
                    .min(u32::BITS - 1);
                let window =
                    (ADMISSION_BACKOFF_BASE_FRAMES << doublings).min(ADMISSION_BACKOFF_MAX_FRAMES);
                backoff.resume_at_frame = self.frame + 1 + window;
            }
        }
        for scope in hit_this_frame {
            self.backoff.remove(&scope);
        }
        let frame = self.frame;
        self.backoff
            .retain(|_, backoff| backoff.resume_at_frame + ADMISSION_BACKOFF_MAX_FRAMES > frame);
        self.frame += 1;
    }
}

pub(crate) struct LayerSurfaceCache {
    entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>,
    scene_range_entries: BoundedLruCache<LayerRasterCacheKey, CachedLayerSurface>,
    identity: HashMap<LayerRasterCacheIdentity, LayerRasterCacheKey>,
    bytes: u64,
    scene_range_bytes: u64,
    seen_this_frame: HashSet<usize>,
    recycled: Vec<Rc<OffscreenTarget>>,
    admission: AdmissionLedger,
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
            admission: AdmissionLedger::default(),
            #[cfg(debug_assertions)]
            inserted_this_frame: HashSet::new(),
        }
    }

    /// Whether a repeat-admitted store for this key would only feed the
    /// unrewarded-store streak its scope is already backing off from.
    pub(crate) fn repeat_admission_backed_off(&self, key: &LayerRasterCacheKey) -> bool {
        self.admission.is_backed_off(key)
    }

    fn recycle(&mut self, entry: CachedLayerSurface) {
        self.recycled.push(entry.target);
    }

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
        frame_stats.record_layer_cache_hit(key, width, height);
        self.admission.note_hit(key);
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
        self.admission.note_store(&key);

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
        self.admission.finish_frame();

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
        if let Some(identity) = key.identity()
            && self.identity.get(&identity) == Some(key)
        {
            self.identity.remove(&identity);
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
mod admission_ledger_tests {
    use cranpose_render_common::raster_cache::{LayerRasterCacheKey, ScaleBucket};
    use cranpose_ui_graphics::Rect;

    use super::{
        ADMISSION_BACKOFF_BASE_FRAMES, ADMISSION_BACKOFF_MAX_FRAMES, AdmissionLedger,
        UNREWARDED_STORES_BEFORE_BACKOFF,
    };

    fn prefix_key(content_hash: u64) -> LayerRasterCacheKey {
        LayerRasterCacheKey::prefix_snapshot(
            content_hash,
            4,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            (10, 10),
            ScaleBucket::from_scale(1.0),
        )
    }

    fn card_key(node: usize, content_hash: u64) -> LayerRasterCacheKey {
        LayerRasterCacheKey::source_content(
            Some(node),
            content_hash,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            (10, 10),
            ScaleBucket::from_scale(1.0),
        )
    }

    #[test]
    fn stores_that_are_never_served_back_off_after_the_streak() {
        let mut ledger = AdmissionLedger::default();
        for frame in 0..UNREWARDED_STORES_BEFORE_BACKOFF as u64 {
            assert!(!ledger.is_backed_off(&prefix_key(frame)));
            ledger.note_store(&prefix_key(frame));
            ledger.finish_frame();
        }
        assert!(ledger.is_backed_off(&prefix_key(99)));
        for _ in 0..ADMISSION_BACKOFF_BASE_FRAMES {
            assert!(ledger.is_backed_off(&prefix_key(99)));
            ledger.finish_frame();
        }
        assert!(!ledger.is_backed_off(&prefix_key(99)));
    }

    #[test]
    fn a_served_store_keeps_admission_open() {
        let mut ledger = AdmissionLedger::default();
        for frame in 0..8u64 {
            ledger.note_store(&prefix_key(frame));
            ledger.note_hit(&prefix_key(frame));
            ledger.finish_frame();
            assert!(!ledger.is_backed_off(&prefix_key(frame + 1)));
        }
    }

    #[test]
    fn the_window_doubles_per_unrewarded_store_and_caps() {
        let mut ledger = AdmissionLedger::default();
        let mut windows = Vec::new();
        for round in 0..6u64 {
            let store_frames = if round == 0 {
                UNREWARDED_STORES_BEFORE_BACKOFF as u64
            } else {
                1
            };
            for _ in 0..store_frames {
                ledger.note_store(&prefix_key(round));
                ledger.finish_frame();
            }
            let mut window = 0u64;
            while ledger.is_backed_off(&prefix_key(round)) {
                ledger.finish_frame();
                window += 1;
            }
            windows.push(window);
        }
        assert_eq!(windows[0], ADMISSION_BACKOFF_BASE_FRAMES);
        assert_eq!(windows[1], ADMISSION_BACKOFF_BASE_FRAMES * 2);
        assert_eq!(windows[2], ADMISSION_BACKOFF_BASE_FRAMES * 4);
        assert_eq!(*windows.last().unwrap(), ADMISSION_BACKOFF_MAX_FRAMES);
    }

    #[test]
    fn a_hit_clears_the_backoff_and_the_streak() {
        let mut ledger = AdmissionLedger::default();
        for frame in 0..UNREWARDED_STORES_BEFORE_BACKOFF as u64 {
            ledger.note_store(&prefix_key(frame));
            ledger.finish_frame();
        }
        assert!(ledger.is_backed_off(&prefix_key(7)));
        ledger.note_hit(&prefix_key(7));
        ledger.finish_frame();
        assert!(!ledger.is_backed_off(&prefix_key(7)));
        ledger.note_store(&prefix_key(8));
        ledger.finish_frame();
        assert!(
            !ledger.is_backed_off(&prefix_key(8)),
            "one unrewarded store after a hit must not reopen the backoff"
        );
    }

    #[test]
    fn scopes_are_independent_per_node_identity() {
        let mut ledger = AdmissionLedger::default();
        for frame in 0..UNREWARDED_STORES_BEFORE_BACKOFF as u64 {
            ledger.note_store(&card_key(1, frame));
            ledger.note_store(&card_key(2, frame));
            ledger.note_hit(&card_key(2, frame));
            ledger.finish_frame();
        }
        assert!(ledger.is_backed_off(&card_key(1, 50)));
        assert!(!ledger.is_backed_off(&card_key(2, 50)));
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::{MAX_SCENE_RANGE_CACHE_BYTES, MAX_SCENE_RANGE_CACHE_ENTRY_BYTES, take_unshared};
    use crate::surface_executor::offscreen_byte_size;

    #[test]
    fn a_dropped_surface_comes_back() {
        let mut pending = vec![Rc::new(7u32), Rc::new(9u32)];
        let mut taken = take_unshared(&mut pending);
        taken.sort_unstable();
        assert_eq!(taken, vec![7, 9]);
        assert!(pending.is_empty());
    }

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

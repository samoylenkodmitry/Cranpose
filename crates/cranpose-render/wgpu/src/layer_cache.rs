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
mod tests {
    use cranpose_render_common::raster_cache::ScaleBucket;
    use cranpose_ui_graphics::{BlendMode, Rect};

    use super::*;
    use crate::{effect_renderer::CompositeSampleMode, frame_graph::upload_test_device};

    fn descriptor(width: u32) -> FrameTextureDescriptor {
        FrameTextureDescriptor::render_attachment("test", width, 1, wgpu::TextureFormat::Rgba8Unorm)
    }

    fn key(index: u64) -> LayerRasterCacheKey {
        LayerRasterCacheKey::prefix_snapshot(
            index,
            1,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            (1, 1),
            ScaleBucket::from_scale(1.0),
        )
    }

    fn texture(device: &wgpu::Device, width: u32) -> Rc<OffscreenTarget> {
        Rc::new(OffscreenTarget::new(
            device,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            1,
        ))
    }

    fn pin(cache: &mut LayerCache, index: u64, atlas: &Rc<OffscreenTarget>) {
        assert!(cache.insert(
            key(index),
            Retained::composite(Rc::clone(atlas), blit()),
            Some(descriptor(64)),
        ));
    }

    fn blit() -> ResolvedCompositeKind {
        ResolvedCompositeKind::Blit {
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            rounded_mask: None,
            sample_mode: CompositeSampleMode::Nearest,
            source_viewport: None,
        }
    }

    #[test]
    fn a_shared_texture_is_charged_once_and_retired_by_its_last_holder() {
        let mut ledger = AllocationLedger::default();
        ledger.attach(7, 1000, Some(descriptor(10)));
        ledger.attach(7, 1000, Some(descriptor(10)));
        ledger.attach(9, 50, None);
        assert_eq!(ledger.bytes, 1050);
        assert!(ledger.detach(7).is_none());
        assert_eq!(ledger.bytes, 1050);
        let retired = ledger
            .detach(7)
            .expect("the last holder retires the texture");
        assert_eq!(retired.bytes, 1000);
        assert_eq!(retired.transient, Some(descriptor(10)));
        assert_eq!(ledger.bytes, 50);
        assert!(ledger.detach(7).is_none());
        let retired = ledger.detach(9).expect("a lone holder retires its texture");
        assert_eq!(retired.transient, None);
        assert_eq!(ledger.bytes, 0);
    }

    #[test]
    fn a_texture_charged_again_after_retirement_starts_a_new_record() {
        let mut ledger = AllocationLedger::default();
        ledger.attach(3, 20, None);
        ledger.detach(3);
        ledger.attach(3, 40, Some(descriptor(4)));
        assert_eq!(ledger.bytes, 40);
        assert_eq!(
            ledger.detach(3).map(|record| record.transient),
            Some(Some(descriptor(4)))
        );
    }

    #[test]
    fn entries_replaying_one_stage_texture_pay_for_it_once_and_retire_it_once() {
        let (_lock, device, _queue) = upload_test_device();
        let mut cache = LayerCache::new();
        let atlas = texture(&device, 64);
        let atlas_bytes = offscreen_byte_size(64, 1);
        pin(&mut cache, 1, &atlas);
        pin(&mut cache, 2, &atlas);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.bytes(), atlas_bytes);
        cache.insert(key(1), Retained::surface(texture(&device, 8)), None);
        assert!(cache.take_released().is_empty());
        assert_eq!(cache.bytes(), atlas_bytes + offscreen_byte_size(8, 1));
        cache.insert(key(2), Retained::surface(texture(&device, 8)), None);
        assert_eq!(cache.bytes(), 2 * offscreen_byte_size(8, 1));
        assert!(
            cache.take_released().is_empty(),
            "a texture a frame still holds must wait"
        );
        drop(atlas);
        let released = cache.take_released();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].0, Some(descriptor(64)));
        assert!(cache.take_released().is_empty());
    }

    #[test]
    fn the_budget_evicts_the_least_recently_used_entry_and_returns_its_surface() {
        let (_lock, device, _queue) = upload_test_device();
        let bytes = offscreen_byte_size(64, 1);
        let mut cache = LayerCache::with_budget(2 * bytes);
        for index in 1..=3 {
            cache.insert(key(index), Retained::surface(texture(&device, 64)), None);
        }
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.bytes(), 2 * bytes);
        assert!(cache.get(&key(1)).is_none());
        let released = cache.take_released();
        assert_eq!(released.len(), 1);
        assert_eq!(released[0].0, None);
        assert!(cache.get(&key(2)).is_some());
        cache.insert(key(4), Retained::surface(texture(&device, 64)), None);
        assert!(
            cache.get(&key(3)).is_none(),
            "the untouched entry leaves first"
        );
        assert!(cache.get(&key(2)).is_some());
        assert_eq!(cache.take_released().len(), 1);
    }

    #[test]
    fn a_texture_over_the_budget_is_refused_without_evicting_anything() {
        let (_lock, device, _queue) = upload_test_device();
        let bytes = offscreen_byte_size(64, 1);
        let mut cache = LayerCache::with_budget(bytes);
        assert!(cache.insert(key(1), Retained::surface(texture(&device, 64)), None));
        assert!(!cache.fits(65, 1));
        assert!(!cache.insert(key(2), Retained::surface(texture(&device, 65)), None));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), bytes);
        assert!(cache.get(&key(1)).is_some());
        assert!(cache.take_released().is_empty());
    }

    #[test]
    fn a_texture_retained_again_while_pending_retirement_is_pending_no_more() {
        let (_lock, device, _queue) = upload_test_device();
        let mut cache = LayerCache::new();
        let atlas = texture(&device, 64);
        let atlas_bytes = offscreen_byte_size(64, 1);
        pin(&mut cache, 1, &atlas);
        assert!(cache.insert(key(1), Retained::surface(texture(&device, 8)), None));
        assert_eq!(cache.bytes(), offscreen_byte_size(8, 1));
        pin(&mut cache, 2, &atlas);
        assert_eq!(cache.bytes(), atlas_bytes + offscreen_byte_size(8, 1));
        drop(atlas);
        assert!(
            cache.take_released().is_empty(),
            "the revived texture is held by its entry, not by the pending list"
        );
        assert!(cache.insert(key(2), Retained::surface(texture(&device, 8)), None));
        let released = cache.take_released();
        assert_eq!(released.len(), 1, "one retirement, one alias");
        assert_eq!(released[0].0, Some(descriptor(64)));
        assert!(cache.take_released().is_empty());
    }
}

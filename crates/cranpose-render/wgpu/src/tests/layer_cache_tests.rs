use cranpose_render_common::raster_cache::RasterScale;
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
        RasterScale::from_scale(1.0),
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
fn a_removed_entry_is_retired_and_its_texture_released_once_unshared() {
    let (_lock, device, _queue) = upload_test_device();
    let mut cache = LayerCache::with_budget(1 << 20);
    let atlas = texture(&device, 64);
    pin(&mut cache, 1, &atlas);
    cache.remove(&key(2));
    assert_eq!(cache.len(), 1, "removing an absent key changes nothing");
    cache.remove(&key(1));
    assert_eq!(cache.len(), 0);
    assert_eq!(
        cache.bytes(),
        0,
        "the removed entry no longer counts against the budget"
    );
    assert!(
        cache.take_released().is_empty(),
        "a texture still held elsewhere is not handed back yet"
    );
    drop(atlas);
    let released = cache.take_released();
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].0, Some(descriptor(64)));
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

#[test]
fn many_small_surfaces_stay_cached_within_the_byte_budget() {
    let (_lock, device, _queue) = upload_test_device();
    let mut cache = LayerCache::new();
    let surfaces = 1000;
    for index in 0..surfaces {
        assert!(cache.insert(key(index), Retained::surface(texture(&device, 1)), None));
    }
    assert_eq!(
        cache.len(),
        surfaces as usize,
        "a screen of more isolated layers than an entry count allows would evict each \
         surface just before the frame that reads it, every frame"
    );
    assert!(cache.get(&key(0)).is_some());
}

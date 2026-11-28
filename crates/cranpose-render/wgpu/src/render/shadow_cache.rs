use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    NodeId,
    collections::{bounded_lru::BoundedLruCache, map::HashMap},
};
use cranpose_ui_graphics::{BlendMode, FxHasher, Rect};

use super::{expand_rect, mask_rect};
use crate::{
    capture_hash::{CaptureWindow, hash_shadow_casters},
    effect_renderer::RoundedCompositeMask,
    geometry::{DevicePixelBounds, anchored_device_rect, offscreen_byte_size},
    idle_pool::IDLE_FRAMES,
    offscreen::OffscreenTarget,
    scene::{CompositorScene, DrawOp, DrawOpKind, RunDraw, ShadowDraw, SnapAnchor, TextDraw},
};

const MAX_SHADOW_SURFACE_CACHE_ITEMS: usize = 512;

const MAX_SHADOW_SURFACE_CACHE_BYTES: u64 = 384 * 1024 * 1024;

pub(super) fn skip_shadow_draws() -> bool {
    cranpose_core::DIAGNOSTICS.skip_shadows.flag()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ShadowSurfaceCacheKey {
    pub(super) content_hash: u64,
    pixel_size: [u32; 2],
    root_scale_bits: u32,
    blur_radius_bits: u32,
}

struct CachedShadowSurface {
    target: Rc<OffscreenTarget>,
    byte_size: u64,
    // The last frame that drew the surface.
    used: u64,
}

// The blurred shadow surfaces, kept for the frames that draw them again,
// bounded by count and by bytes. A surface no frame has drawn for
// [`IDLE_FRAMES`] goes, and so does the surface of a text that an earlier
// frame drew once the text draws a new one: an edited or animated text
// keeps one surface, not one for each frame it changed in.
pub(super) struct ShadowSurfaces {
    cache: BoundedLruCache<ShadowSurfaceCacheKey, CachedShadowSurface>,
    // The surface each text's shadow drew last.
    owners: HashMap<NodeId, ShadowSurfaceCacheKey>,
    bytes: u64,
    frame: u64,
}

impl ShadowSurfaces {
    pub(super) fn new() -> Self {
        Self {
            cache: BoundedLruCache::with_capacity_at_least_one(MAX_SHADOW_SURFACE_CACHE_ITEMS),
            owners: HashMap::default(),
            bytes: 0,
            frame: 0,
        }
    }

    pub(super) fn get(
        &mut self,
        key: &ShadowSurfaceCacheKey,
        owner: Option<NodeId>,
    ) -> Option<&Rc<OffscreenTarget>> {
        let frame = self.frame;
        let cached = self.cache.get_mut(key)?;
        cached.used = frame;
        if let Some(owner) = owner {
            self.owners.insert(owner, *key);
        }
        Some(&cached.target)
    }

    // Keeps `target` for `key`, evicting the least recently drawn surfaces
    // until the bytes fit. Returns the surface `owner` drew before, which
    // goes unless this frame drew it: a label of the same text elsewhere
    // shares it.
    pub(super) fn insert(
        &mut self,
        key: ShadowSurfaceCacheKey,
        target: Rc<OffscreenTarget>,
        owner: Option<NodeId>,
    ) -> Option<Rc<OffscreenTarget>> {
        let replaced = owner
            .and_then(|owner| self.owners.insert(owner, key))
            .filter(|earlier| *earlier != key)
            .and_then(|earlier| self.remove_undrawn(&earlier));
        let byte_size = offscreen_byte_size(target.width, target.height);
        while self.bytes + byte_size > MAX_SHADOW_SURFACE_CACHE_BYTES {
            let Some((_, evicted)) = self.cache.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.byte_size);
        }
        let cached = CachedShadowSurface {
            target,
            byte_size,
            used: self.frame,
        };
        if let Some((_, replaced)) = self.cache.push(key, cached) {
            self.bytes = self.bytes.saturating_sub(replaced.byte_size);
        }
        self.bytes = self.bytes.saturating_add(byte_size);
        replaced
    }

    // Drops the surface under `key` unless this frame drew it.
    fn remove_undrawn(&mut self, key: &ShadowSurfaceCacheKey) -> Option<Rc<OffscreenTarget>> {
        if self.cache.peek(key)?.used == self.frame {
            return None;
        }
        let removed = self.cache.pop(key)?;
        self.bytes = self.bytes.saturating_sub(removed.byte_size);
        Some(removed.target)
    }

    // Ends a frame, dropping every surface no frame has drawn for
    // [`IDLE_FRAMES`].
    pub(super) fn end_frame(&mut self) {
        self.frame += 1;
        let oldest_kept = self.frame.saturating_sub(IDLE_FRAMES);
        while self
            .cache
            .peek_lru()
            .is_some_and(|(_, cached)| cached.used < oldest_kept)
        {
            let Some((_, idle)) = self.cache.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(idle.byte_size);
        }
        let cache = &self.cache;
        self.owners.retain(|_, key| cache.contains(key));
    }

    pub(super) fn clear(&mut self) {
        self.cache.clear();
        self.owners.clear();
        self.bytes = 0;
    }
}

// The text whose shadow `shadow` is: the one owner of its kept surface.
pub(super) fn shadow_owner(shadow: &ShadowDraw) -> Option<NodeId> {
    match (&shadow.shapes, shadow.texts.as_slice()) {
        (None, [text]) => Some(text.node_id),
        _ => None,
    }
}

// The key of the surface of `shadow` that covers `device_bounds`: its
// casters by everything their pixels depend on, placed from the surface's
// corner, so a shadow moved by whole pixels finds its blur.
pub(super) fn shadow_surface_cache_key(
    shadow: &ShadowDraw,
    device_bounds: DevicePixelBounds,
    pixel_radius: f32,
    root_scale: f32,
) -> ShadowSurfaceCacheKey {
    let mut hasher = FxHasher::default();
    let surface = CaptureWindow {
        x: device_bounds.x,
        y: device_bounds.y,
        width: device_bounds.width as f32,
        height: device_bounds.height as f32,
    };
    hash_shadow_casters(shadow, surface, root_scale, &mut hasher);
    ShadowSurfaceCacheKey {
        content_hash: hasher.finish(),
        pixel_size: [device_bounds.width, device_bounds.height],
        root_scale_bits: root_scale.to_bits(),
        blur_radius_bits: pixel_radius.to_bits(),
    }
}

// Where a shadow's casters draw: its shapes' bounds and the parts of its
// texts' boxes where their glyphs put ink.
pub(crate) fn shadow_draw_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shadow
        .shapes
        .as_ref()
        .map(|run| run.bounds)
        .into_iter()
        .chain(
            shadow
                .texts
                .iter()
                .map(|text| text.glyph_reach(text.rect, text.scale)),
        )
        .reduce(|a, b| a.union(b))
}

/// The rounded mask a shadow's composite applies, in the scene's device
/// pixels: an inner shadow masks itself to its fill shape, and a shadow
/// lowered out of a clipped layer masks itself to that layer's rounded clip.
pub(crate) fn shadow_composite_mask(
    shadow: &ShadowDraw,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    inner_shadow_composite_mask(shadow, root_scale).or_else(|| {
        shadow.rounded_clip.map(|clip| RoundedCompositeMask {
            rect: mask_rect(anchored_device_rect(clip.rect, snap_anchor, root_scale)),
            radii: clip.radii.map(|radius| radius * root_scale),
        })
    })
}

/// A scene holding just a shadow's own draws, in the order they arrive, so
/// the shadow source renders through the same pass encoder as everything
/// else.
pub(super) fn shadow_scene(
    shapes: Option<&RunDraw>,
    texts: &[TextDraw],
    input_margin: f32,
) -> CompositorScene {
    let mut scene = CompositorScene::new();
    if let Some(run) = shapes {
        scene.push_run(run.clone());
    }
    for text in texts {
        let z_index = scene.next_z();
        scene.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Text(scene.texts.len()),
        });
        let mut source_text = text.clone();
        source_text.clip = source_text
            .clip
            .map(|clip| expand_rect(clip, input_margin, input_margin));
        scene.texts.push(source_text);
        scene.next_z += 1;
    }
    scene
}

/// The whole device pixels a logical rect covers, or `None` when it covers
/// none or more than a texture can hold.
pub(super) fn device_pixel_bounds(
    rect: Rect,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<DevicePixelBounds> {
    let x = (rect.x * root_scale).floor();
    let y = (rect.y * root_scale).floor();
    let right = ((rect.x + rect.width) * root_scale).ceil();
    let bottom = ((rect.y + rect.height) * root_scale).ceil();
    let width = (right - x).max(0.0) as u32;
    let height = (bottom - y).max(0.0) as u32;
    if width == 0 || height == 0 || width > max_texture_dim || height > max_texture_dim {
        return None;
    }
    Some(DevicePixelBounds {
        x,
        y,
        width,
        height,
    })
}

fn inner_shadow_composite_mask(
    shadow: &ShadowDraw,
    root_scale: f32,
) -> Option<RoundedCompositeMask> {
    let run = shadow.shapes.as_ref()?;
    if !run
        .tables()
        .shapes
        .iter()
        .any(|record| record.blend_mode() == BlendMode::DstOut)
    {
        return None;
    }
    let fill = run.tables().shapes.get(0)?;
    let rect = run.placement.translated_bounds(fill.stored_rect());
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let resolved =
        cranpose_ui_graphics::RoundedCornerShape::with_radii(cranpose_ui_graphics::CornerRadii {
            top_left: fill.radii[0],
            top_right: fill.radii[1],
            bottom_right: fill.radii[2],
            bottom_left: fill.radii[3],
        })
        .resolve(rect.width, rect.height);
    let radii = [
        resolved.top_left * root_scale,
        resolved.top_right * root_scale,
        resolved.bottom_left * root_scale,
        resolved.bottom_right * root_scale,
    ];

    Some(RoundedCompositeMask {
        rect: mask_rect(anchored_device_rect(
            rect,
            run.placement.snap_anchor,
            root_scale,
        )),
        radii,
    })
}

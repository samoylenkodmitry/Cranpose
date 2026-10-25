use std::{
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_ui_graphics::{BlendMode, FxHasher, Rect};

use super::{expand_rect, hash_run_item_with_clip, mask_rect};
use crate::{
    debug_toggles::DebugToggle,
    effect_renderer::RoundedCompositeMask,
    geometry::{DevicePixelBounds, anchored_device_rect},
    offscreen::OffscreenTarget,
    scene::{CompositorScene, DrawOp, DrawOpKind, RunDraw, ShadowDraw, SnapAnchor, TextDraw},
};

pub(super) const MAX_SHADOW_SURFACE_CACHE_ITEMS: usize = 512;

pub(super) const MAX_SHADOW_SURFACE_CACHE_BYTES: u64 = 384 * 1024 * 1024;

static SKIP_SHADOWS: DebugToggle = DebugToggle::new("CRANPOSE_SKIP_SHADOWS");

pub(super) fn skip_shadow_draws() -> bool {
    SKIP_SHADOWS.flag()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ShadowSurfaceCacheKey {
    pub(super) content_hash: u64,
    pixel_size: [u32; 2],
    root_scale_bits: u32,
    blur_radius_bits: u32,
}

pub(super) struct CachedShadowSurface {
    pub(super) target: Rc<OffscreenTarget>,
    pub(super) byte_size: u64,
}

pub(crate) fn shadow_content_hash(shadow: &ShadowDraw, root_scale: f32) -> u64 {
    let mut hasher = FxHasher::default();
    let origin = shape_shadow_bounds(shadow).unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });
    let device = anchored_device_rect(
        origin,
        shadow
            .shapes
            .as_ref()
            .and_then(|run| run.placement.snap_anchor),
        root_scale,
    );
    for run in shadow.shapes.iter().chain(&shadow.post_blur_cutouts) {
        hash_run_item_with_clip(
            run,
            run.placement.clip,
            device.x.floor(),
            device.y.floor(),
            root_scale,
            &mut hasher,
        );
    }
    hasher.finish()
}

pub(super) fn shape_shadow_surface_cache_key(
    shadow: &ShadowDraw,
    device_bounds: DevicePixelBounds,
    pixel_radius: f32,
    root_scale: f32,
) -> Option<ShadowSurfaceCacheKey> {
    (root_scale.is_finite() && root_scale > 0.0).then(|| ShadowSurfaceCacheKey {
        content_hash: shadow_content_hash(shadow, root_scale),
        pixel_size: [device_bounds.width, device_bounds.height],
        root_scale_bits: root_scale.to_bits(),
        blur_radius_bits: pixel_radius.to_bits(),
    })
}

fn shape_shadow_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shadow.shapes.as_ref().map(|run| run.bounds)
}

pub(crate) fn shadow_draw_bounds(shadow: &ShadowDraw) -> Option<Rect> {
    shape_shadow_bounds(shadow)
        .into_iter()
        .chain(shadow.texts.iter().map(|text| text.rect))
        .reduce(|a, b| Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x + a.width).max(b.x + b.width) - a.x.min(b.x),
            height: (a.y + a.height).max(b.y + b.height) - a.y.min(b.y),
        })
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

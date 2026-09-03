use std::hash::{Hash, Hasher};

use cranpose_render_common::geometry::blur_extent_margin;
use cranpose_ui_graphics::{FxHasher, Point, Rect, RenderHash};

use crate::{
    draw_pass::{ResolvedComposite, ResolvedCompositeKind, SourceContent},
    effect_renderer::{CompositeSampleMode, RoundedCompositeMask},
    render::{
        hash_f32_for_cache, hash_shadow_device_offset, hash_shadow_device_rect,
        hash_shape_shadow_item, shadow_draw_bounds, shape_shadow_content_hash,
    },
    scene::{CompositorScene, DrawOp, DrawOpKind, ImageDraw, ShadowDraw, SnapAnchor, TextDraw},
};

/// A device rect a capture reads, in the device space of the scene whose
/// ops and composites are hashed against it.
#[derive(Clone, Copy)]
pub(crate) struct CaptureWindow {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

type DeviceTuple = (f32, f32, f32, f32);

impl CaptureWindow {
    fn origin(self, scale: f32) -> Point {
        Point::new(self.x / scale, self.y / scale)
    }

    fn touches_logical(self, rect: Rect, margin: f32, scale: f32) -> bool {
        self.touches_device((
            rect.x * scale - margin,
            rect.y * scale - margin,
            rect.width * scale + 2.0 * margin,
            rect.height * scale + 2.0 * margin,
        ))
    }

    fn touches_device(self, (x, y, width, height): DeviceTuple) -> bool {
        x < self.x + self.width
            && x + width > self.x
            && y < self.y + self.height
            && y + height > self.y
    }
}

const OP_MARGIN: f32 = 1.0;

/// A fresh hasher for a capture's input.
pub(crate) fn capture_hasher() -> FxHasher {
    FxHasher::default()
}

/// Hashes every op of `ops` that touches `window` into `state`, in order,
/// with its geometry relative to the window's origin, so a window moving
/// rigidly over the same ops hashes the same.
pub(crate) fn hash_capture_ops<H: Hasher>(
    scene: &CompositorScene,
    ops: &[DrawOp],
    window: CaptureWindow,
    scale: f32,
    state: &mut H,
) {
    let origin = window.origin(scale);
    for op in ops {
        match op.kind {
            DrawOpKind::Shape(index) => {
                let shape = &scene.shapes[index];
                if window.touches_logical(shape.rect, OP_MARGIN, scale) {
                    0u8.hash(state);
                    hash_shape_shadow_item(
                        shape,
                        &scene.brushes,
                        shape.blend_mode,
                        origin.x,
                        origin.y,
                        scale,
                        state,
                    );
                }
            }
            DrawOpKind::Image(index) => {
                let image = &scene.images[index];
                if window.touches_logical(image.rect, OP_MARGIN, scale) {
                    1u8.hash(state);
                    hash_image(image, origin, scale, state);
                }
            }
            DrawOpKind::Text(index) => {
                let text = &scene.texts[index];
                if window.touches_logical(text.rect, OP_MARGIN, scale) {
                    2u8.hash(state);
                    hash_text(text, origin, scale, state);
                }
            }
            DrawOpKind::Shadow(index) => {
                let shadow = &scene.shadow_draws[index];
                let margin = blur_extent_margin(shadow.blur_radius) * scale + OP_MARGIN;
                if shadow_draw_bounds(shadow)
                    .is_some_and(|bounds| window.touches_logical(bounds, margin, scale))
                {
                    3u8.hash(state);
                    hash_shadow(shadow, origin, scale, state);
                }
            }
        }
    }
}

fn hash_anchor<H: Hasher>(anchor: Option<SnapAnchor>, origin: Point, scale: f32, state: &mut H) {
    match anchor {
        Some(anchor) => {
            1u8.hash(state);
            hash_shadow_device_offset(anchor.origin.x, origin.x, scale, state);
            hash_shadow_device_offset(anchor.origin.y, origin.y, scale, state);
            hash_f32_for_cache(anchor.device_pixel_step, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, origin: Point, scale: f32, state: &mut H) {
    match rect {
        Some(rect) => {
            1u8.hash(state);
            hash_shadow_device_rect(rect, origin.x, origin.y, scale, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_optional_render_hash<H: Hasher, T: RenderHash>(value: Option<&T>, state: &mut H) {
    match value {
        Some(value) => {
            1u8.hash(state);
            value.render_hash().hash(state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_text<H: Hasher>(text: &TextDraw, origin: Point, scale: f32, state: &mut H) {
    hash_shadow_device_rect(text.rect, origin.x, origin.y, scale, state);
    hash_anchor(text.snap_anchor, origin, scale, state);
    text.text.render_hash().hash(state);
    text.color.render_hash().hash(state);
    text.text_style.render_hash().hash(state);
    hash_f32_for_cache(text.font_size, state);
    hash_f32_for_cache(text.scale, state);
    text.layout_options.hash(state);
    hash_optional_rect(text.clip, origin, scale, state);
}

fn hash_image<H: Hasher>(image: &ImageDraw, origin: Point, scale: f32, state: &mut H) {
    hash_shadow_device_rect(image.rect, origin.x, origin.y, scale, state);
    hash_shadow_device_rect(image.local_rect, origin.x, origin.y, scale, state);
    for point in image.quad {
        hash_shadow_device_offset(point[0], origin.x, scale, state);
        hash_shadow_device_offset(point[1], origin.y, scale, state);
    }
    hash_anchor(image.snap_anchor, origin, scale, state);
    image.image.render_hash().hash(state);
    hash_f32_for_cache(image.alpha, state);
    hash_optional_render_hash(image.color_filter.as_ref(), state);
    image.sampling.hash(state);
    hash_optional_rect(image.clip, origin, scale, state);
    hash_optional_render_hash(image.src_rect.as_ref(), state);
    image.blend_mode.hash(state);
    image.motion_context_animated.hash(state);
}

fn hash_shadow<H: Hasher>(shadow: &ShadowDraw, origin: Point, scale: f32, state: &mut H) {
    shape_shadow_content_hash(
        &shadow.shapes,
        &shadow.post_blur_cutouts,
        &shadow.brushes,
        scale,
    )
    .hash(state);
    hash_optional_rect(shadow_draw_bounds(shadow), origin, scale, state);
    for (shape, _) in shadow.shapes.iter().chain(&shadow.post_blur_cutouts) {
        hash_shadow_device_rect(shape.rect, origin.x, origin.y, scale, state);
        hash_anchor(shape.snap_anchor, origin, scale, state);
    }
    for text in &shadow.texts {
        hash_text(text, origin, scale, state);
    }
    hash_f32_for_cache(shadow.blur_radius, state);
    hash_optional_rect(shadow.clip, origin, scale, state);
    hash_optional_rect(shadow.occluder, origin, scale, state);
    match shadow.rounded_clip {
        Some(clip) => {
            1u8.hash(state);
            hash_shadow_device_rect(clip.rect, origin.x, origin.y, scale, state);
            hash_radii(clip.radii, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_radii<H: Hasher>(radii: [f32; 4], state: &mut H) {
    for radius in radii {
        hash_f32_for_cache(radius, state);
    }
}

fn hash_device_tuple<H: Hasher>(
    (x, y, width, height): DeviceTuple,
    window: CaptureWindow,
    state: &mut H,
) {
    hash_f32_for_cache(x - window.x, state);
    hash_f32_for_cache(y - window.y, state);
    hash_f32_for_cache(width, state);
    hash_f32_for_cache(height, state);
}

fn hash_optional_tuple<H: Hasher>(
    tuple: Option<DeviceTuple>,
    window: CaptureWindow,
    state: &mut H,
) {
    match tuple {
        Some(tuple) => {
            1u8.hash(state);
            hash_device_tuple(tuple, window, state);
        }
        None => 0u8.hash(state),
    }
}

fn hash_mask<H: Hasher>(mask: Option<RoundedCompositeMask>, window: CaptureWindow, state: &mut H) {
    match mask {
        Some(mask) => {
            1u8.hash(state);
            hash_device_tuple(
                (mask.rect[0], mask.rect[1], mask.rect[2], mask.rect[3]),
                window,
                state,
            );
            hash_radii(mask.radii, state);
        }
        None => 0u8.hash(state),
    }
}

const SOURCE_SPACE: CaptureWindow = CaptureWindow {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

/// Hashes every resolved composite that touches `window`: what its texture
/// holds and where it lands relative to the window. Returns false when one
/// of them is drawn anew every frame, so nothing reading it can be reused.
pub(crate) fn hash_capture_composites<H: Hasher>(
    composites: &[ResolvedComposite],
    window: CaptureWindow,
    state: &mut H,
) -> bool {
    for composite in composites {
        if !window.touches_device(composite.dest) {
            continue;
        }
        let SourceContent::Retained(content) = composite.content else {
            return false;
        };
        content.hash(state);
        hash_device_tuple(composite.dest, window, state);
        hash_optional_tuple(composite.scissor, window, state);
        hash_composite_kind(&composite.kind, window, state);
    }
    true
}

fn hash_composite_kind<H: Hasher>(
    kind: &ResolvedCompositeKind,
    window: CaptureWindow,
    state: &mut H,
) {
    match kind {
        ResolvedCompositeKind::Blit {
            alpha,
            blend_mode,
            rounded_mask,
            sample_mode,
            source_viewport,
        } => {
            0u8.hash(state);
            hash_f32_for_cache(*alpha, state);
            blend_mode.hash(state);
            hash_mask(*rounded_mask, window, state);
            (*sample_mode == CompositeSampleMode::Nearest).hash(state);
            hash_optional_tuple(*source_viewport, SOURCE_SPACE, state);
        }
        ResolvedCompositeKind::Shader {
            shader,
            layer_pixel_rect,
            source_region,
            rounded_mask,
            alpha,
        } => {
            1u8.hash(state);
            shader.render_hash().hash(state);
            hash_radii(*layer_pixel_rect, state);
            hash_optional_tuple(*source_region, SOURCE_SPACE, state);
            hash_mask(*rounded_mask, window, state);
            hash_f32_for_cache(*alpha, state);
        }
        ResolvedCompositeKind::Projective {
            dest_quad,
            alpha,
            blend_mode,
            ..
        } => {
            2u8.hash(state);
            for point in dest_quad {
                hash_f32_for_cache(point[0] - window.x, state);
                hash_f32_for_cache(point[1] - window.y, state);
            }
            hash_f32_for_cache(*alpha, state);
            blend_mode.hash(state);
        }
    }
}

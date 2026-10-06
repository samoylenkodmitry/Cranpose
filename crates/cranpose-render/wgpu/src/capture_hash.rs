use std::hash::{Hash, Hasher};

use cranpose_render_common::geometry::blur_reach;
use cranpose_ui_graphics::{FxHasher, Point, Rect, RenderHash};

use crate::{
    draw_pass::{ResolvedComposite, ResolvedCompositeKind, SourceContent},
    effect_renderer::{CompositeSampleMode, RoundedCompositeMask},
    opaque_prefix::capture_solid_rect,
    render::{
        hash_device_offset, hash_device_rect, hash_f32_for_cache, hash_run_item_with_clip,
        hash_text_gradient_phase_for_cache, resolve_image_geometry, shadow_draw_bounds,
        text_raster_geometry_for_draw,
    },
    rrect_shadow::{device_clip, device_shadow, rrect_shadow_bounds},
    scene::{
        CompositorScene, DrawOp, DrawOpKind, ImageDraw, RRectShadowDraw, ShadowDraw, TextDraw,
    },
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
    fn clipped_logical(self, clip: Option<Rect>, scale: f32) -> Option<Rect> {
        clip.map(|clip| {
            let window = Rect {
                x: self.x / scale,
                y: self.y / scale,
                width: self.width / scale,
                height: self.height / scale,
            };
            clip.intersect(window).unwrap_or(clip)
        })
    }

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
    let capture = Rect {
        x: window.x,
        y: window.y,
        width: window.width,
        height: window.height,
    };
    for op in ops {
        if let Some((color, rect, clip)) = capture_solid_rect(scene, op, capture, scale) {
            4u8.hash(state);
            for channel in color {
                channel.to_bits().hash(state);
            }
            hash_device_tuple((rect.x, rect.y, rect.width, rect.height), window, state);
            hash_optional_tuple(
                clip.map(|rect| (rect.x, rect.y, rect.width, rect.height)),
                window,
                state,
            );
            continue;
        }
        match op.kind {
            DrawOpKind::Run(index) => {
                let run = &scene.runs[index];
                if window.touches_logical(run.bounds, OP_MARGIN, scale) {
                    0u8.hash(state);
                    hash_run_item_with_clip(
                        run,
                        if run.placement.clip_rounded() {
                            run.placement.clip
                        } else {
                            window.clipped_logical(run.placement.clip, scale)
                        },
                        window.x,
                        window.y,
                        scale,
                        state,
                    );
                }
            }
            DrawOpKind::Image(index) => {
                let image = &scene.images[index];
                if window.touches_logical(image.rect, OP_MARGIN, scale) {
                    1u8.hash(state);
                    hash_image(image, window, scale, state);
                }
            }
            DrawOpKind::Text(index) => {
                let text = &scene.texts[index];
                if window.touches_logical(text.rect, OP_MARGIN, scale) {
                    2u8.hash(state);
                    hash_text(text, window, scale, state);
                }
            }
            DrawOpKind::RRectShadow(index) => {
                let draw = &scene.rrect_shadows[index];
                if rrect_shadow_bounds(draw, scale)
                    .is_some_and(|bounds| window.touches_logical(bounds, OP_MARGIN, scale))
                {
                    5u8.hash(state);
                    hash_rrect_shadow(draw, window, scale, state);
                }
            }
            DrawOpKind::Shadow(index) => {
                let shadow = &scene.shadow_draws[index];
                let margin = blur_reach(shadow.blur_radius, scale) * scale + OP_MARGIN;
                if shadow_draw_bounds(shadow)
                    .is_some_and(|bounds| window.touches_logical(bounds, margin, scale))
                {
                    3u8.hash(state);
                    hash_shadow(shadow, window, scale, state);
                }
            }
        }
    }
}

fn clipped_device_tuple((x, y, width, height): DeviceTuple, window: CaptureWindow) -> DeviceTuple {
    let device = Rect {
        x,
        y,
        width,
        height,
    };
    let capture = Rect {
        x: window.x,
        y: window.y,
        width: window.width,
        height: window.height,
    };
    device
        .intersect(capture)
        .map_or((window.x, window.y, 0.0, 0.0), |rect| {
            (rect.x, rect.y, rect.width, rect.height)
        })
}

fn hash_scissored_rect<H: Hasher>(
    rect: Rect,
    clip: Option<Rect>,
    window: CaptureWindow,
    scale: f32,
    state: &mut H,
) {
    let visible = match clip {
        Some(clip) => rect.intersect(clip),
        None => Some(rect),
    };
    hash_optional_tuple(
        visible.map(|rect| {
            let device = crate::geometry::canonicalized_scaled_rect(rect, scale);
            clipped_device_tuple((device.x, device.y, device.width, device.height), window)
        }),
        window,
        state,
    );
}

fn hash_optional_rect<H: Hasher>(rect: Option<Rect>, origin: Point, scale: f32, state: &mut H) {
    match rect {
        Some(rect) => {
            1u8.hash(state);
            hash_device_rect(rect, origin.x, origin.y, scale, state);
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

fn hash_text<H: Hasher>(text: &TextDraw, window: CaptureWindow, scale: f32, state: &mut H) {
    let origin = window.origin(scale);
    let Some((logical_rect, raster_rect, clip, text_scale, static_text_motion)) =
        text_raster_geometry_for_draw(text, scale)
    else {
        0u8.hash(state);
        return;
    };
    1u8.hash(state);
    static_text_motion.hash(state);
    hash_f32_for_cache(raster_rect.width, state);
    hash_f32_for_cache(raster_rect.height, state);
    if static_text_motion {
        hash_f32_for_cache(raster_rect.x - window.x, state);
        hash_f32_for_cache(raster_rect.y - window.y, state);
    } else {
        hash_device_offset(logical_rect.x, origin.x, scale, state);
        hash_device_offset(logical_rect.y, origin.y, scale, state);
        hash_f32_for_cache(raster_rect.x.fract(), state);
        hash_f32_for_cache(raster_rect.y.fract(), state);
    }
    hash_f32_for_cache(text_scale, state);
    let visible = match clip {
        Some(clip) => logical_rect.intersect(clip),
        None => Some(logical_rect),
    }
    .is_some_and(|rect| window.touches_logical(rect, 0.0, scale));
    visible.hash(state);
    hash_text_gradient_phase_for_cache(text, raster_rect, state);
    text.text.render_hash().hash(state);
    text.color.render_hash().hash(state);
    text.text_style.render_hash().hash(state);
    hash_f32_for_cache(text.font_size, state);
    text.layout_options.hash(state);
    if static_text_motion && text.text.text().contains('\n') {
        hash_optional_rect(window.clipped_logical(clip, scale), origin, scale, state);
    } else {
        let draw_rect = Rect {
            x: if static_text_motion {
                raster_rect.x / scale
            } else {
                logical_rect.x
            },
            y: if static_text_motion {
                raster_rect.y / scale
            } else {
                logical_rect.y
            },
            width: raster_rect.width / scale,
            height: raster_rect.height / scale,
        };
        hash_scissored_rect(draw_rect, clip, window, scale, state);
    }
}

fn hash_image<H: Hasher>(image: &ImageDraw, window: CaptureWindow, scale: f32, state: &mut H) {
    let geometry = resolve_image_geometry(image, scale);
    hash_scissored_rect(geometry.rect, geometry.clip, window, scale, state);
    for point in geometry.device_quad(scale) {
        hash_f32_for_cache(point[0] - window.x, state);
        hash_f32_for_cache(point[1] - window.y, state);
    }
    image.image.render_hash().hash(state);
    hash_f32_for_cache(image.alpha, state);
    hash_optional_render_hash(image.color_filter.as_ref(), state);
    geometry.sampling.hash(state);

    hash_optional_render_hash(image.src_rect.as_ref(), state);
    image.blend_mode.hash(state);
    image.motion_context_animated.hash(state);
}

/// A round rect shadow by everything its pixels depend on, its device
/// geometry and clip taken against the capture's origin.
fn hash_rrect_shadow<H: Hasher>(
    draw: &RRectShadowDraw,
    window: CaptureWindow,
    scale: f32,
    state: &mut H,
) {
    let shadow = device_shadow(draw, scale);
    let bounds = shadow.bounds;
    hash_device_tuple(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        window,
        state,
    );
    let hole = shadow
        .hole
        .map_or([0.0, -1.0], |hole| [hole.inset, hole.radius]);
    for value in shadow
        .radii
        .into_iter()
        .chain([shadow.umbra_inset, shadow.distance_correction])
        .chain(hole)
    {
        hash_f32_for_cache(value, state);
    }
    for channel in [
        draw.color.r(),
        draw.color.g(),
        draw.color.b(),
        draw.color.a(),
    ] {
        channel.to_bits().hash(state);
    }
    hash_optional_tuple(
        device_clip(draw, scale).map(|[left, top, right, bottom]| {
            clipped_device_tuple((left, top, right - left, bottom - top), window)
        }),
        window,
        state,
    );
}

fn hash_shadow<H: Hasher>(shadow: &ShadowDraw, window: CaptureWindow, scale: f32, state: &mut H) {
    let anchor = shadow
        .shapes
        .as_ref()
        .and_then(|run| run.placement.snap_anchor);
    for run in shadow.shapes.iter().chain(&shadow.post_blur_cutouts) {
        hash_run_item_with_clip(run, run.placement.clip, window.x, window.y, scale, state);
    }
    for text in &shadow.texts {
        hash_text(text, window, scale, state);
    }
    hash_f32_for_cache(shadow.blur_radius, state);
    hash_optional_tuple(
        shadow_draw_bounds(shadow)
            .map(|bounds| crate::render::anchored_rect_to_device(bounds, anchor, scale)),
        window,
        state,
    );
    hash_optional_tuple(
        shadow.clip.map(|rect| {
            clipped_device_tuple(
                crate::render::anchored_rect_to_device(rect, anchor, scale),
                window,
            )
        }),
        window,
        state,
    );
    hash_mask(
        crate::render::shadow_composite_mask(shadow, anchor, scale),
        window,
        state,
    );
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
pub(crate) fn hash_capture_composites<'a, H: Hasher>(
    mut drawn: &'a [ResolvedComposite],
    mut pending: &'a [ResolvedComposite],
    window: CaptureWindow,
    state: &mut H,
) -> bool {
    while !drawn.is_empty() || !pending.is_empty() {
        let stream = if drawn.first().is_some_and(|first| {
            pending
                .first()
                .is_none_or(|next| first.z_index <= next.z_index)
        }) {
            &mut drawn
        } else {
            &mut pending
        };
        let (composite, rest) = stream
            .split_first()
            .expect("one composite stream is nonempty");
        *stream = rest;
        if !window.touches_device(composite.dest)
            || composite
                .scissor
                .is_some_and(|clip| !window.touches_device(clip))
        {
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
            source_logical_size,
            substrate_regions,
            rounded_mask,
            alpha,
        } => {
            1u8.hash(state);
            shader.render_hash().hash(state);
            hash_radii(*layer_pixel_rect, state);
            hash_optional_tuple(*source_region, SOURCE_SPACE, state);
            for region in substrate_regions {
                hash_optional_tuple(*region, SOURCE_SPACE, state);
            }
            source_logical_size.is_some().hash(state);
            if let Some((width, height)) = source_logical_size {
                hash_f32_for_cache(*width, state);
                hash_f32_for_cache(*height, state);
            }
            hash_mask(*rounded_mask, window, state);
            hash_f32_for_cache(*alpha, state);
        }
        ResolvedCompositeKind::Projective {
            dest_quad,
            alpha,
            blend_mode,
            source_region,
            ..
        } => {
            2u8.hash(state);
            for point in dest_quad {
                hash_f32_for_cache(point[0] - window.x, state);
                hash_f32_for_cache(point[1] - window.y, state);
            }
            hash_f32_for_cache(*alpha, state);
            blend_mode.hash(state);
            hash_optional_tuple(*source_region, SOURCE_SPACE, state);
        }
    }
}

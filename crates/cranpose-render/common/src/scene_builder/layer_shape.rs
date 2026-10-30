use std::borrow::Cow;

use cranpose_ui::{ModifierNodeSlices, Rect};
use cranpose_ui_graphics::{
    CornerRadii, GraphicsLayer, LayerShape, RoundedCornerShape, rounded_corner_alpha_mask_effect,
};

const ROUNDED_CLIP_EDGE_FEATHER: f32 = 1.0;

/// The graphics layer of a node with `slices` and bounds `local_bounds`:
/// the chain's own, its clip shaped as [`shape_clip`] shapes it. Lent by
/// the slices unless the shaping changes it, and returned straight from
/// them when nothing shapes it: a layer kept in a local first is copied out.
#[inline]
pub(super) fn shaped_graphics_layer(
    slices: &ModifierNodeSlices,
    local_bounds: Rect,
) -> Option<Cow<'_, GraphicsLayer>> {
    match clip_corners(clip_shape(slices), local_bounds) {
        None => slices.graphics_layer(),
        Some(corners) => rounded_graphics_layer(slices, corners, local_bounds),
    }
}

#[inline(never)]
fn rounded_graphics_layer(
    slices: &ModifierNodeSlices,
    corners: (RoundedCornerShape, CornerRadii),
    local_bounds: Rect,
) -> Option<Cow<'_, GraphicsLayer>> {
    let mut graphics_layer = slices.graphics_layer();
    round_clip(&mut graphics_layer, corners, local_bounds);
    graphics_layer
}

/// Shapes the clip of `graphics_layer`, a layer's own, to `corner_shape`, the
/// shape of a layer that clips to its bounds `local_bounds`. In place: the
/// layer moved in and out of a call is hundreds of bytes copied each way,
/// and a lent layer is copied only when the shaping changes it.
pub(super) fn shape_clip(
    graphics_layer: &mut Option<Cow<'_, GraphicsLayer>>,
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) {
    if let Some(corners) = clip_corners(corner_shape, local_bounds) {
        round_clip(graphics_layer, corners, local_bounds);
    }
}

/// `corner_shape`, the shape a layer that clips to its bounds `local_bounds`
/// rounds its clip with, and its radii there: `None` when it does not clip
/// to its bounds or its corners are square.
fn clip_corners(
    corner_shape: Option<RoundedCornerShape>,
    local_bounds: Rect,
) -> Option<(RoundedCornerShape, CornerRadii)> {
    let corner_shape = corner_shape?;
    let radii = corner_shape.resolve(local_bounds.width, local_bounds.height);
    let square = radii.top_left <= f32::EPSILON
        && radii.top_right <= f32::EPSILON
        && radii.bottom_right <= f32::EPSILON
        && radii.bottom_left <= f32::EPSILON;
    (!square).then_some((corner_shape, radii))
}

/// Rounds the clip of `graphics_layer` with `corners`, as [`clip_corners`]
/// found them for a layer at `local_bounds`.
fn round_clip(
    graphics_layer: &mut Option<Cow<'_, GraphicsLayer>>,
    (corner_shape, radii): (RoundedCornerShape, CornerRadii),
    local_bounds: Rect,
) {
    let properties = graphics_layer.get_or_insert_default().to_mut();
    if let Some(existing) = properties.render_effect.take() {
        let rounded_clip = rounded_corner_alpha_mask_effect(
            local_bounds.width,
            local_bounds.height,
            radii,
            ROUNDED_CLIP_EDGE_FEATHER,
        );
        properties.render_effect = Some(existing.then(rounded_clip));
    } else {
        properties.shape = LayerShape::Rounded(corner_shape);
        properties.clip = true;
    }
}

// The corner shape the layer of a node with `slices` rounds its clip with:
// `None` when it does not clip to its bounds.
pub(super) fn clip_shape(slices: &ModifierNodeSlices) -> Option<RoundedCornerShape> {
    slices.corner_shape().filter(|_| slices.clip_to_bounds())
}

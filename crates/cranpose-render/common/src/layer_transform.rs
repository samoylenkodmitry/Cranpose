use cranpose_ui_graphics::{GraphicsLayer, Point, Rect};

use crate::graph::{ProjectiveTransform, quad_bounds};

pub(crate) fn layer_scale_x(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_x
}

pub(crate) fn layer_scale_y(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_y
}

pub fn layer_uniform_scale(layer: &GraphicsLayer) -> f32 {
    layer_scale_x(layer).min(layer_scale_y(layer))
}

pub(crate) fn layer_affine_is_identity(layer: &GraphicsLayer) -> bool {
    layer.scale == 1.0
        && layer.scale_x == 1.0
        && layer.scale_y == 1.0
        && layer.translation_x == 0.0
        && layer.translation_y == 0.0
}

pub fn apply_layer_affine_to_rect(rect: Rect, layer_bounds: Rect, layer: &GraphicsLayer) -> Rect {
    if layer_affine_is_identity(layer) {
        return rect;
    }
    let scale_x = layer_scale_x(layer);
    let scale_y = layer_scale_y(layer);
    let (pivot_x, pivot_y) = layer_rotation_pivot(layer_bounds, layer);
    Rect {
        x: pivot_x + (rect.x - pivot_x) * scale_x + layer.translation_x,
        y: pivot_y + (rect.y - pivot_y) * scale_y + layer.translation_y,
        width: rect.width * scale_x,
        height: rect.height * scale_y,
    }
}

/// Point counterpart of [`apply_layer_affine_to_rect`]: scale about the layer
/// pivot, then translate. Rotation/perspective are deliberately excluded — the
/// affine space is the one SDF shapes are evaluated in, with rotation carried
/// by the deformed quad instead.
pub fn apply_layer_affine_to_point(
    point: Point,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
) -> Point {
    if layer_affine_is_identity(layer) {
        return point;
    }
    let (pivot_x, pivot_y) = layer_rotation_pivot(layer_bounds, layer);
    Point::new(
        pivot_x + (point.x - pivot_x) * layer_scale_x(layer) + layer.translation_x,
        pivot_y + (point.y - pivot_y) * layer_scale_y(layer) + layer.translation_y,
    )
}

fn layer_rotation_pivot(layer_bounds: Rect, layer: &GraphicsLayer) -> (f32, f32) {
    (
        layer_bounds.x + layer_bounds.width * layer.transform_origin.pivot_fraction_x,
        layer_bounds.y + layer_bounds.height * layer.transform_origin.pivot_fraction_y,
    )
}

pub(crate) fn layer_scales_or_rotates(layer: &GraphicsLayer) -> bool {
    layer_scale_x(layer) != 1.0 || layer_scale_y(layer) != 1.0 || layer_has_rotation(layer)
}

fn layer_has_rotation(layer: &GraphicsLayer) -> bool {
    layer.rotation_x.abs() > f32::EPSILON
        || layer.rotation_y.abs() > f32::EPSILON
        || layer.rotation_z.abs() > f32::EPSILON
}

/// Where the layer's turns -- about x, then y, then z -- take the x and y
/// axes of its plane, in three dimensions.
fn rotation_axes(layer: &GraphicsLayer) -> [[f32; 3]; 2] {
    let (sin_x, cos_x) = layer.rotation_x.to_radians().sin_cos();
    let (sin_y, cos_y) = layer.rotation_y.to_radians().sin_cos();
    let (sin_z, cos_z) = layer.rotation_z.to_radians().sin_cos();
    [
        [cos_z * cos_y, sin_z * cos_y, -sin_y],
        [
            cos_z * sin_x * sin_y - sin_z * cos_x,
            sin_z * sin_x * sin_y + cos_z * cos_x,
            sin_x * cos_y,
        ],
    ]
}

/// How far the camera a tilted layer is seen from stands before its plane.
fn camera_distance(layer: &GraphicsLayer) -> f32 {
    const CAMERA_DISTANCE_SCALE: f32 = 72.0;
    (layer.camera_distance * CAMERA_DISTANCE_SCALE).max(1.0)
}

fn apply_rotation_and_perspective(
    point: [f32; 2],
    pivot: (f32, f32),
    layer: &GraphicsLayer,
) -> [f32; 2] {
    if !layer_has_rotation(layer) {
        return point;
    }
    let (x, y) = (point[0] - pivot.0, point[1] - pivot.1);
    let [x_axis, y_axis] = rotation_axes(layer);
    let [turned_x, turned_y, turned_z] = [0, 1, 2].map(|axis| x * x_axis[axis] + y * y_axis[axis]);
    let camera_distance = camera_distance(layer);
    let perspective = camera_distance / (camera_distance - turned_z).max(1.0);
    [
        pivot.0 + turned_x * perspective,
        pivot.1 + turned_y * perspective,
    ]
}

fn apply_layer_to_point(point: [f32; 2], pivot: (f32, f32), layer: &GraphicsLayer) -> [f32; 2] {
    let scaled = [
        pivot.0 + (point[0] - pivot.0) * layer_scale_x(layer),
        pivot.1 + (point[1] - pivot.1) * layer_scale_y(layer),
    ];
    let rotated = apply_rotation_and_perspective(scaled, pivot, layer);
    [
        rotated[0] + layer.translation_x,
        rotated[1] + layer.translation_y,
    ]
}

pub fn apply_layer_to_quad(rect: Rect, layer_bounds: Rect, layer: &GraphicsLayer) -> [[f32; 2]; 4] {
    let quad = [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ];
    if layer_affine_is_identity(layer) && !layer_has_rotation(layer) {
        return quad;
    }
    let pivot = layer_rotation_pivot(layer_bounds, layer);
    quad.map(|point| apply_layer_to_point(point, pivot, layer))
}

pub fn apply_layer_to_rect(rect: Rect, layer_bounds: Rect, layer: &GraphicsLayer) -> Rect {
    quad_bounds(apply_layer_to_quad(rect, layer_bounds, layer))
}

pub fn layer_transform_to_parent(
    local_bounds: Rect,
    placement: Point,
    layer: &GraphicsLayer,
) -> ProjectiveTransform {
    // Local space keeps the node's origin at `placement`, wherever the
    // layer's bounds sit in it, so a layer bounded at a coordinator away
    // from the node's origin clips and pivots there without moving content.
    //
    // The matrix is the layer's own scale, turn and camera, not a fit to
    // where they take its corners: a fit's rounding grows with the distance
    // from the origin, and a turned cell far down a screen came out skewed
    // by it, too skewed to draw in place.
    let (pivot_x, pivot_y) = layer_rotation_pivot(local_bounds, layer);
    let [x_axis, y_axis] = rotation_axes(layer);
    let (scale_x, scale_y) = (layer_scale_x(layer), layer_scale_y(layer));
    let camera_distance = camera_distance(layer);
    let turn = ProjectiveTransform::from_homogeneous([
        [x_axis[0] * scale_x, y_axis[0] * scale_y, 0.0],
        [x_axis[1] * scale_x, y_axis[1] * scale_y, 0.0],
        [
            -x_axis[2] * scale_x / camera_distance,
            -y_axis[2] * scale_y / camera_distance,
            1.0,
        ],
    ]);
    let placed = ProjectiveTransform::translation(-pivot_x, -pivot_y)
        .then(turn)
        .then(ProjectiveTransform::translation(pivot_x, pivot_y))
        .then(ProjectiveTransform::translation(
            placement.x + layer.translation_x,
            placement.y + layer.translation_y,
        ));
    ProjectiveTransform::from_homogeneous(placed.matrix())
}

#[cfg(test)]
#[path = "tests/layer_transform_tests.rs"]
mod tests;

use cranpose_ui_graphics::{GraphicsLayer, Point, Rect};

use crate::graph::{quad_bounds, ProjectiveTransform};

pub(crate) fn layer_scale_x(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_x
}

pub(crate) fn layer_scale_y(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_y
}

pub fn layer_uniform_scale(layer: &GraphicsLayer) -> f32 {
    layer_scale_x(layer).min(layer_scale_y(layer))
}

/// True when the layer's affine part moves nothing: unit scale and zero
/// translation. A plain draw canvas — the layer under every game scene —
/// looks like this, and the per-primitive transform helpers below run once
/// per recorded shape per frame, so the scan matters at scene scale.
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

fn layer_has_rotation(layer: &GraphicsLayer) -> bool {
    layer.rotation_x.abs() > f32::EPSILON
        || layer.rotation_y.abs() > f32::EPSILON
        || layer.rotation_z.abs() > f32::EPSILON
}

fn apply_rotation_and_perspective(
    point: [f32; 2],
    pivot: (f32, f32),
    layer: &GraphicsLayer,
) -> [f32; 2] {
    if !layer_has_rotation(layer) {
        return point;
    }

    let mut x = point[0] - pivot.0;
    let mut y = point[1] - pivot.1;
    let mut z = 0.0f32;

    let (sin_x, cos_x) = layer.rotation_x.to_radians().sin_cos();
    let (sin_y, cos_y) = layer.rotation_y.to_radians().sin_cos();
    let (sin_z, cos_z) = layer.rotation_z.to_radians().sin_cos();

    let y_rot_x = y * cos_x - z * sin_x;
    let z_rot_x = y * sin_x + z * cos_x;
    y = y_rot_x;
    z = z_rot_x;

    let x_rot_y = x * cos_y + z * sin_y;
    let z_rot_y = -x * sin_y + z * cos_y;
    x = x_rot_y;
    z = z_rot_y;

    let x_rot_z = x * cos_z - y * sin_z;
    let y_rot_z = x * sin_z + y * cos_z;
    x = x_rot_z;
    y = y_rot_z;

    const CAMERA_DISTANCE_SCALE: f32 = 72.0;
    let camera_distance = (layer.camera_distance * CAMERA_DISTANCE_SCALE).max(1.0);
    let denom = (camera_distance - z).max(1.0);
    let perspective = camera_distance / denom;

    [pivot.0 + x * perspective, pivot.1 + y * perspective]
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
    let placement_rect = Rect {
        x: placement.x,
        y: placement.y,
        width: local_bounds.width,
        height: local_bounds.height,
    };
    ProjectiveTransform::from_rect_to_quad(
        local_bounds,
        apply_layer_to_quad(placement_rect, placement_rect, layer),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{Rect, TransformOrigin};

    #[test]
    fn layer_transform_to_parent_maps_local_bounds_to_positioned_bounds() {
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 18.0,
        };
        let placement = Point { x: 12.0, y: 9.0 };
        let transform =
            layer_transform_to_parent(local_bounds, placement, &GraphicsLayer::default());

        assert_eq!(
            transform.bounds_for_rect(local_bounds),
            Rect {
                x: 12.0,
                y: 9.0,
                width: 30.0,
                height: 18.0,
            }
        );
    }

    #[test]
    fn layer_transform_to_parent_applies_rotation_about_transform_origin() {
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let layer = GraphicsLayer {
            rotation_z: 90.0,
            transform_origin: TransformOrigin::CENTER,
            ..Default::default()
        };

        let transform = layer_transform_to_parent(local_bounds, Point::default(), &layer);
        let mapped = transform.bounds_for_rect(local_bounds);

        assert!((mapped.width - 40.0).abs() < 0.01);
        assert!((mapped.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn layer_transform_to_parent_scales_about_transform_origin() {
        let local_bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 36.0,
            height: 36.0,
        };
        let placement = Point { x: 54.0, y: 416.0 };
        let small = layer_transform_to_parent(
            local_bounds,
            placement,
            &GraphicsLayer {
                scale: 0.85,
                transform_origin: TransformOrigin::CENTER,
                ..Default::default()
            },
        )
        .bounds_for_rect(local_bounds);
        let large = layer_transform_to_parent(
            local_bounds,
            placement,
            &GraphicsLayer {
                scale: 1.15,
                transform_origin: TransformOrigin::CENTER,
                ..Default::default()
            },
        )
        .bounds_for_rect(local_bounds);

        let small_center_y = small.y + small.height * 0.5;
        let large_center_y = large.y + large.height * 0.5;
        assert!(
            (small_center_y - large_center_y).abs() < 0.01,
            "scale must not move the layer center when transform origin is centered"
        );
    }
}

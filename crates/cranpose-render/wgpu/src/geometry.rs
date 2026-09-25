use cranpose_render_common::graph::quad_bounds;
use cranpose_ui_graphics::{Point, Rect};

use crate::{offscreen::composition_bytes_per_pixel, scene::SnapAnchor};

const QUAD_AXIS_ALIGNMENT_TOLERANCE: f32 = 1e-4;
const DEVICE_SNAP_SUBPIXEL_STEPS: f64 = 16.0;

pub(crate) fn offscreen_byte_size(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * composition_bytes_per_pixel()
}

/// Whole device pixels: an integral origin and a pixel size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DevicePixelBounds {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn anchored_device_rect(
    rect: Rect,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
) -> Rect {
    snap_anchor
        .and_then(|anchor| {
            axis_aligned_quad_rect(canonicalized_anchored_scaled_quad(
                [
                    [rect.x, rect.y],
                    [rect.x + rect.width, rect.y],
                    [rect.x, rect.y + rect.height],
                    [rect.x + rect.width, rect.y + rect.height],
                ],
                anchor,
                root_scale,
            ))
        })
        .unwrap_or_else(|| canonicalized_scaled_rect(rect, root_scale))
}

pub(crate) fn translation_stable_anchored_device_pixel_bounds(
    rect: Rect,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<DevicePixelBounds> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let device_rect = anchored_device_rect(rect, snap_anchor, root_scale);
    let min_x = device_rect.x.floor();
    let min_y = device_rect.y.floor();
    let width = (device_rect.width.ceil() + 1.0).max(0.0) as u32;
    let height = (device_rect.height.ceil() + 1.0).max(0.0) as u32;
    if width == 0 || height == 0 || width > max_texture_dim || height > max_texture_dim {
        return None;
    }

    Some(DevicePixelBounds {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

pub(crate) fn translate_quad(quad: [[f32; 2]; 4], delta: Point) -> [[f32; 2]; 4] {
    quad.map(|[x, y]| [x + delta.x, y + delta.y])
}

pub(crate) fn scaled_quad(quad: [[f32; 2]; 4], scale: f32) -> [[f32; 2]; 4] {
    quad.map(|[x, y]| [x * scale, y * scale])
}

pub(crate) fn canonicalize_device_coordinate(value: f32) -> f32 {
    if !value.is_finite() {
        return value;
    }
    ((f64::from(value) * DEVICE_SNAP_SUBPIXEL_STEPS).round() / DEVICE_SNAP_SUBPIXEL_STEPS) as f32
}

pub(crate) fn canonicalized_scaled_rect(rect: Rect, scale: f32) -> Rect {
    let left = canonicalize_device_coordinate(rect.x * scale);
    let top = canonicalize_device_coordinate(rect.y * scale);
    let right = canonicalize_device_coordinate((rect.x + rect.width) * scale);
    let bottom = canonicalize_device_coordinate((rect.y + rect.height) * scale);
    Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

pub(crate) fn canonicalized_scaled_quad(quad: [[f32; 2]; 4], scale: f32) -> [[f32; 2]; 4] {
    quad.map(|[x, y]| {
        [
            canonicalize_device_coordinate(x * scale),
            canonicalize_device_coordinate(y * scale),
        ]
    })
}

pub(crate) fn canonicalized_anchored_scaled_quad(
    quad: [[f32; 2]; 4],
    anchor: SnapAnchor,
    root_scale: f32,
) -> [[f32; 2]; 4] {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return canonicalized_scaled_quad(quad, root_scale);
    }
    let origin = snapped_anchor_device_origin(anchor, root_scale);
    quad.map(|[x, y]| {
        [
            origin.x + canonicalize_device_coordinate((x - anchor.origin.x) * root_scale),
            origin.y + canonicalize_device_coordinate((y - anchor.origin.y) * root_scale),
        ]
    })
}

pub(crate) fn snapped_anchor_device_origin(anchor: SnapAnchor, root_scale: f32) -> Point {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return Point::default();
    }
    let device_pixel_step = anchor_device_pixel_step(anchor);
    let snapped = |origin: f32| {
        let snap_units = f64::from(origin) * f64::from(root_scale) / f64::from(device_pixel_step);
        let canonical_snap_units =
            (snap_units * DEVICE_SNAP_SUBPIXEL_STEPS).round() / DEVICE_SNAP_SUBPIXEL_STEPS;
        (canonical_snap_units.round() * f64::from(device_pixel_step)) as f32
    };
    Point::new(snapped(anchor.origin.x), snapped(anchor.origin.y))
}

fn anchor_device_pixel_step(anchor: SnapAnchor) -> f32 {
    if anchor.device_pixel_step.is_finite() && anchor.device_pixel_step > 0.0 {
        anchor.device_pixel_step
    } else {
        1.0
    }
}

pub(crate) fn snap_delta_for_anchor(anchor: SnapAnchor, root_scale: f32) -> Point {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return Point::default();
    }
    let device_pixel_step = anchor_device_pixel_step(anchor);
    let snapped_axis_delta = |origin: f32| {
        let root_scale = f64::from(root_scale);
        let device_pixel_step = f64::from(device_pixel_step);
        let snap_units = f64::from(origin) * root_scale / device_pixel_step;
        let canonical_snap_units =
            (snap_units * DEVICE_SNAP_SUBPIXEL_STEPS).round() / DEVICE_SNAP_SUBPIXEL_STEPS;
        let snapped_logical = canonical_snap_units.round() * device_pixel_step / root_scale;
        (snapped_logical - f64::from(origin)) as f32
    };
    Point::new(
        snapped_axis_delta(anchor.origin.x),
        snapped_axis_delta(anchor.origin.y),
    )
}

fn quad_is_axis_aligned_rect(quad: [[f32; 2]; 4]) -> bool {
    (quad[0][1] - quad[1][1]).abs() <= QUAD_AXIS_ALIGNMENT_TOLERANCE
        && (quad[2][1] - quad[3][1]).abs() <= QUAD_AXIS_ALIGNMENT_TOLERANCE
        && (quad[0][0] - quad[2][0]).abs() <= QUAD_AXIS_ALIGNMENT_TOLERANCE
        && (quad[1][0] - quad[3][0]).abs() <= QUAD_AXIS_ALIGNMENT_TOLERANCE
}

pub(crate) fn axis_aligned_quad_rect(dest_quad: [[f32; 2]; 4]) -> Option<Rect> {
    if !quad_is_axis_aligned_rect(dest_quad) {
        return None;
    }

    let min_x = dest_quad[0][0].min(dest_quad[2][0]);
    let max_x = dest_quad[1][0].max(dest_quad[3][0]);
    let min_y = dest_quad[0][1].min(dest_quad[1][1]);
    let max_y = dest_quad[2][1].max(dest_quad[3][1]);

    if !min_x.is_finite()
        || !max_x.is_finite()
        || !min_y.is_finite()
        || !max_y.is_finite()
        || max_x <= min_x
        || max_y <= min_y
    {
        return None;
    }

    Some(Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

/// Where a segment's device space lands in the device space of the target
/// it draws into: an invertible affine map, with the inverse the shape stage
/// maps fragments back through. Only a layer drawn in place under its rigid
/// transform moves its segments; every other segment keeps the identity,
/// and every consumer takes its untransformed arithmetic for that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SegmentTransform {
    linear: [f32; 4],
    translation: [f32; 2],
    inverse: [f32; 4],
}

impl SegmentTransform {
    pub(crate) const IDENTITY: Self = Self {
        linear: [1.0, 0.0, 0.0, 1.0],
        translation: [0.0, 0.0],
        inverse: [1.0, 0.0, 0.0, 1.0],
    };

    /// The map `p -> linear * p + translation`, `linear` row-major; `None`
    /// when `linear` is singular.
    pub(crate) fn affine(linear: [f32; 4], translation: [f32; 2]) -> Option<Self> {
        let [a, b, c, d] = linear;
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant.abs() <= f32::EPSILON {
            return None;
        }
        let reciprocal = 1.0 / determinant;
        Some(Self {
            linear,
            translation,
            inverse: [
                d * reciprocal,
                -b * reciprocal,
                -c * reciprocal,
                a * reciprocal,
            ],
        })
    }

    pub(crate) fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }

    /// `self` applied first, then `outer`.
    pub(crate) fn then(self, outer: Self) -> Self {
        Self {
            linear: multiply_linear(outer.linear, self.linear),
            translation: outer.map(self.translation),
            inverse: multiply_linear(self.inverse, outer.inverse),
        }
    }

    fn map(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d] = self.linear;
        [
            a * x + b * y + self.translation[0],
            c * x + d * y + self.translation[1],
        ]
    }

    fn unmap(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d] = self.inverse;
        let (x, y) = (x - self.translation[0], y - self.translation[1]);
        [a * x + b * y, c * x + d * y]
    }

    /// The target-space bounds of a rect of the segment's device space.
    pub(crate) fn target_bounds(self, rect: Rect) -> Rect {
        corner_bounds(rect, |corner| self.map(corner))
    }

    /// The segment-space bounds of a rect of the target's device space.
    pub(crate) fn segment_bounds(self, rect: Rect) -> Rect {
        corner_bounds(rect, |corner| self.unmap(corner))
    }

    /// The linear part (row-major) and translation of the map, and the
    /// linear part of its inverse, as the viewport uniform carries them.
    pub(crate) fn uniform_parts(self) -> ([f32; 4], [f32; 2], [f32; 4]) {
        (self.linear, self.translation, self.inverse)
    }
}

fn multiply_linear([a, b, c, d]: [f32; 4], [e, f, g, h]: [f32; 4]) -> [f32; 4] {
    [a * e + b * g, a * f + b * h, c * e + d * g, c * f + d * h]
}

fn corner_bounds(rect: Rect, map: impl Fn([f32; 2]) -> [f32; 2]) -> Rect {
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    quad_bounds([
        map([rect.x, rect.y]),
        map([right, rect.y]),
        map([rect.x, bottom]),
        map([right, bottom]),
    ])
}

#[cfg(test)]
#[path = "tests/geometry_tests.rs"]
mod tests;

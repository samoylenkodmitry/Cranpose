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

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::Rect;

    use super::{
        axis_aligned_quad_rect, canonicalize_device_coordinate, canonicalized_scaled_quad,
        canonicalized_scaled_rect, translation_stable_anchored_device_pixel_bounds,
    };
    use crate::rect_to_quad;

    #[test]
    fn translation_stable_device_bounds_preserve_offscreen_source_origin() {
        let bounds = translation_stable_anchored_device_pixel_bounds(
            Rect {
                x: -12.25,
                y: 8.25,
                width: 34.5,
                height: 10.25,
            },
            None,
            2.0,
            4096,
        )
        .expect("bounds");

        assert_eq!(bounds.x, -25.0);
        assert_eq!(bounds.y, 16.0);
        assert_eq!(bounds.width, 70);
        assert_eq!(bounds.height, 22);
    }

    #[test]
    fn translation_stable_device_bounds_keep_size_across_subpixel_phases() {
        let rect_at = |x: f32| Rect {
            x,
            y: 8.25,
            width: 34.5,
            height: 10.25,
        };
        let scale = 130.0 / 96.0;
        let base =
            translation_stable_anchored_device_pixel_bounds(rect_at(-12.25), None, scale, 4096)
                .expect("base bounds");
        for step in 1..=12 {
            let moved = translation_stable_anchored_device_pixel_bounds(
                rect_at(-12.25 + step as f32),
                None,
                scale,
                4096,
            )
            .expect("moved bounds");
            assert_eq!((base.width, base.height), (moved.width, moved.height));
        }
    }

    #[test]
    fn anchored_translation_stable_bounds_move_one_pixel_at_fractional_densities() {
        for scale in [1.25, 130.0 / 96.0] {
            let mut origin_y = 127.600_006_f32;
            let mut previous_y = None;

            for step in 0..10 {
                let rect = Rect {
                    x: 40.0,
                    y: origin_y - 18.0,
                    width: 60.0,
                    height: 60.0,
                };
                let anchor = crate::scene::SnapAnchor::rigid(cranpose_ui_graphics::Point::new(
                    0.0, origin_y,
                ));
                let bounds = translation_stable_anchored_device_pixel_bounds(
                    rect,
                    Some(anchor),
                    scale,
                    4096,
                )
                .expect("anchored shadow bounds");
                if let Some(previous_y) = previous_y {
                    assert_eq!(
                        bounds.y,
                        previous_y - 1.0,
                        "anchored bounds jumped at step {step} with scale {scale}"
                    );
                }
                previous_y = Some(bounds.y);
                origin_y -= 1.0 / scale;
            }
        }
    }

    #[test]
    fn rigid_snap_keeps_half_pixel_phase_across_one_device_pixel_steps() {
        let scale = 1.25;
        let logical_device_pixel = 1.0 / scale;
        let mut origin = 127.600_006;
        let mut previous_device_origin = None;

        for step in 0..10 {
            let anchor =
                crate::scene::SnapAnchor::rigid(cranpose_ui_graphics::Point::new(0.0, origin));
            let delta = super::snap_delta_for_anchor(anchor, scale);
            let snapped_device_origin = (origin + delta.y) * scale;
            assert_eq!(
                snapped_device_origin.fract(),
                0.0,
                "step {step} did not snap to a device pixel: origin={origin:?} delta={:?}",
                delta.y
            );
            if let Some(previous) = previous_device_origin {
                assert_eq!(
                    previous - snapped_device_origin,
                    1.0,
                    "step {step} changed the half-pixel rounding direction"
                );
            }
            previous_device_origin = Some(snapped_device_origin);
            origin -= logical_device_pixel;
        }
    }

    #[test]
    fn device_coordinate_canonicalization_absorbs_half_pixel_float_noise() {
        assert_eq!(canonicalize_device_coordinate(338.499_94), 338.5);
        assert_eq!(canonicalize_device_coordinate(338.500_06), 338.5);
        assert_eq!(canonicalize_device_coordinate(f32::INFINITY), f32::INFINITY);
    }

    #[test]
    fn scaled_geometry_canonicalization_preserves_edges_and_quad_topology() {
        let rect = Rect {
            x: 10.000_02,
            y: 20.399_96,
            width: 30.0,
            height: 40.000_03,
        };
        let scaled = canonicalized_scaled_rect(rect, 1.25);
        assert_eq!(scaled.x, 12.5);
        assert_eq!(scaled.y, 25.5);
        assert_eq!(scaled.width, 37.5);
        assert_eq!(scaled.height, 50.0);

        assert_eq!(
            canonicalized_scaled_quad(crate::rect_to_quad(rect), 1.25),
            crate::rect_to_quad(scaled)
        );
    }

    #[test]
    fn axis_aligned_quad_rect_returns_rect_for_cardinal_quad() {
        let quad = rect_to_quad(Rect {
            x: 12.0,
            y: 9.0,
            width: 8.0,
            height: 10.0,
        });

        assert_eq!(
            axis_aligned_quad_rect(quad),
            Some(Rect {
                x: 12.0,
                y: 9.0,
                width: 8.0,
                height: 10.0,
            })
        );
    }

    #[test]
    fn axis_aligned_quad_rect_rejects_skewed_quad() {
        let quad = [[12.0, 9.0], [20.0, 9.5], [12.0, 19.0], [20.0, 19.0]];

        assert_eq!(axis_aligned_quad_rect(quad), None);
    }
}

use crate::effect_renderer::CompositeSampleMode;
use crate::scene::SnapAnchor;
use crate::surface_plan::TranslatedContentAxes;
use cranpose_render_common::primitive_emit::resolve_clip;
use cranpose_ui_graphics::{Point, Rect};

pub(crate) const MAX_EFFECT_LAYER_SURFACE_BYTES: u64 = 8 * 1024 * 1024;
const QUAD_AXIS_ALIGNMENT_TOLERANCE: f32 = 1e-4;
const COMPOSITE_DEST_SNAP_TOLERANCE: f32 = 1e-4;
const DEVICE_SNAP_SUBPIXEL_STEPS: f64 = 16.0;

/// Slack absorbed before the ceil, so a rect that already covers a whole
/// number of device pixels cannot gain one to float error: `51.0 / 1.4 * 1.4`
/// is `51.000004`, and a bare ceil turns that into 52. Orders of magnitude
/// below any real sub-pixel coverage.
const SURFACE_SIZE_CEIL_EPSILON: f32 = 1e-3;

pub(crate) fn surface_target_size(rect: Rect, root_scale: f32, max_dim: u32) -> (u32, u32) {
    (
        (rect.width * root_scale - SURFACE_SIZE_CEIL_EPSILON)
            .ceil()
            .clamp(1.0, max_dim as f32) as u32,
        (rect.height * root_scale - SURFACE_SIZE_CEIL_EPSILON)
            .ceil()
            .clamp(1.0, max_dim as f32) as u32,
    )
}

/// The surface rect that `width`x`height` device pixels cover exactly at
/// `scale`.
///
/// `surface_target_size` CEILS each axis, but the composite maps the WHOLE
/// texture onto the destination quad — `layer_surface_dest_quad` maps the
/// surface's logical rect through the layer transform, so texture pixel
/// `(width, height)` lands on the rect's far corner no matter where the
/// content actually ended. A rect that is not a whole number of device pixels
/// therefore has its ceil padding stretched across the quad, which compresses
/// the content toward the quad's origin by up to half a device pixel.
///
/// Growing the rect to the pixels that were allocated anyway makes that
/// mapping exact. The texture is unchanged — this only names the area it
/// already covers — so nothing here spends memory. Only growth is allowed: a
/// size clamped by the texture-dimension limit must not shrink the rect and
/// clip the content off.
pub(crate) fn device_pixel_exact_surface_rect(
    rect: Rect,
    scale: f32,
    width: u32,
    height: u32,
) -> Rect {
    if !scale.is_finite() || scale <= 0.0 {
        return rect;
    }
    Rect {
        x: rect.x,
        y: rect.y,
        width: (width as f32 / scale).max(rect.width),
        height: (height as f32 / scale).max(rect.height),
    }
}

pub(crate) fn offscreen_byte_size(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * 4
}

pub(crate) fn surface_pixel_rect(rect: Rect, root_scale: f32) -> Rect {
    canonicalized_scaled_rect(rect, root_scale)
}

pub(crate) fn local_effect_pixel_rect(width: u32, height: u32) -> [f32; 4] {
    [0.0, 0.0, width as f32, height as f32]
}

/// The effect rect a runtime shader must receive: the LAYER's content rect
/// in surface-local pixels. Effect surfaces are padded (blur reach, rim
/// glow, shadow headroom) and may be clipped (viewport, scroll clips), so
/// the surface bounds themselves are NOT the effect geometry — a glass
/// shader handed the padded surface as its rect paints its optics into the
/// padding band around the widget and scales its dp mapping by the padding
/// ratio. `content_rect` and `surface_rect` share one logical space.
#[track_caller]
pub(crate) fn content_effect_pixel_rect(
    content_rect: Option<Rect>,
    surface_rect: Rect,
    width: u32,
    height: u32,
) -> [f32; 4] {
    if cranpose_core::env_flag!("CRANPOSE_BACKDROP_DIAG") {
        eprintln!(
            "[effect-rect-diag] caller={} content={content_rect:?} surface={surface_rect:?} tex=({width},{height})",
            std::panic::Location::caller()
        );
    }
    let Some(content) = content_rect else {
        return local_effect_pixel_rect(width, height);
    };
    if surface_rect.width <= f32::EPSILON || surface_rect.height <= f32::EPSILON {
        return local_effect_pixel_rect(width, height);
    }
    let scale_x = width as f32 / surface_rect.width;
    let scale_y = height as f32 / surface_rect.height;
    [
        canonicalize_device_coordinate((content.x - surface_rect.x) * scale_x),
        canonicalize_device_coordinate((content.y - surface_rect.y) * scale_y),
        canonicalize_device_coordinate(content.width * scale_x),
        canonicalize_device_coordinate(content.height * scale_y),
    ]
}

pub(crate) fn visible_layer_rect(
    rect: Rect,
    clip: Option<Rect>,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<Rect> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let viewport_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: width as f32 / root_scale,
        height: height as f32 / root_scale,
    };
    let clipped_rect = resolve_clip(Some(viewport_rect), Some(rect))?;
    resolve_clip(Some(clipped_rect), clip)
}

pub(crate) fn clamp_effect_surface_scale(
    rect: Rect,
    minimum_scale: f32,
    desired_scale: f32,
    max_texture_dim: u32,
) -> f32 {
    let safe_minimum_scale = if minimum_scale.is_finite() && minimum_scale > 0.0 {
        minimum_scale
    } else {
        1.0
    };
    let mut scale = if desired_scale.is_finite() && desired_scale > 0.0 {
        desired_scale
    } else {
        safe_minimum_scale
    };

    scale = scale
        .min(max_texture_dim as f32 / rect.width.max(1.0))
        .min(max_texture_dim as f32 / rect.height.max(1.0));

    let area = rect.width.max(1.0) * rect.height.max(1.0);
    let max_scale_by_bytes = ((MAX_EFFECT_LAYER_SURFACE_BYTES as f32) / (area * 4.0)).sqrt();
    scale = scale.min(max_scale_by_bytes);

    scale.max(safe_minimum_scale)
}

pub(crate) fn fit_capture_rect_to_scale_budget_for_axes(
    rect: Rect,
    required_rect: Rect,
    target_scale: f32,
    max_texture_dim: u32,
    translated_axes: TranslatedContentAxes,
) -> Rect {
    let (trim_width, trim_height) = match (translated_axes.x, translated_axes.y) {
        (true, false) => (true, false),
        (false, true) => (false, true),
        _ => (true, true),
    };
    fit_capture_rect_to_scale_budget_with_axis_trimming(
        rect,
        required_rect,
        target_scale,
        max_texture_dim,
        trim_width,
        trim_height,
    )
}

fn fit_capture_rect_to_scale_budget_with_axis_trimming(
    rect: Rect,
    required_rect: Rect,
    target_scale: f32,
    max_texture_dim: u32,
    trim_width: bool,
    trim_height: bool,
) -> Rect {
    if !target_scale.is_finite() || target_scale <= 0.0 {
        return rect;
    }
    let Some(required_rect) = resolve_clip(Some(rect), Some(required_rect)) else {
        return rect;
    };

    let max_target_extent = max_texture_dim as f32 / target_scale;
    let max_target_pixels = (MAX_EFFECT_LAYER_SURFACE_BYTES / 4) as f32;
    let mut fitted = rect;

    let target_width = (fitted.width.max(1.0) * target_scale).ceil().max(1.0);
    let max_height_by_bytes = (max_target_pixels / target_width).floor() / target_scale;
    let max_height_for_width = max_target_extent.min(max_height_by_bytes);
    if trim_height
        && fitted.height > max_height_for_width
        && max_height_for_width >= required_rect.height
    {
        let required_bottom = required_rect.y + required_rect.height;
        let new_y = fitted.y.max(required_bottom - max_height_for_width);
        fitted.height = (required_bottom - new_y).max(required_rect.height);
        fitted.y = new_y;
    }

    let target_height = (fitted.height.max(1.0) * target_scale).ceil().max(1.0);
    let max_width_by_bytes = (max_target_pixels / target_height).floor() / target_scale;
    let max_width_for_height = max_target_extent.min(max_width_by_bytes);
    if trim_width
        && fitted.width > max_width_for_height
        && max_width_for_height >= required_rect.width
    {
        let required_right = required_rect.x + required_rect.width;
        let new_x = fitted.x.max(required_right - max_width_for_height);
        fitted.width = (required_right - new_x).max(required_rect.width);
        fitted.x = new_x;
    }

    fitted
}

use super::backend::DevicePixelBounds;

pub(crate) fn device_pixel_bounds_for_rect(
    rect: Rect,
    viewport_width: u32,
    viewport_height: u32,
    root_scale: f32,
) -> Option<DevicePixelBounds> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let min_x = (rect.x * root_scale).floor().max(0.0);
    let min_y = (rect.y * root_scale).floor().max(0.0);
    let max_x = ((rect.x + rect.width) * root_scale)
        .ceil()
        .min(viewport_width as f32);
    let max_y = ((rect.y + rect.height) * root_scale)
        .ceil()
        .min(viewport_height as f32);
    let width = (max_x - min_x).max(0.0) as u32;
    let height = (max_y - min_y).max(0.0) as u32;
    if width == 0 || height == 0 {
        return None;
    }

    Some(DevicePixelBounds {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

/// Device-pixel bounds whose size depends only on the rect's size, never on its
/// subpixel phase. Floor/ceil bounds grow or shrink by one pixel as content
/// translates across the device-pixel grid, which would change raster cache
/// keys on every scroll step; the +1 slack column/row covers the worst-case
/// phase instead so one cached raster size serves every translation.
pub(crate) fn translation_stable_anchored_device_pixel_bounds(
    rect: Rect,
    snap_anchor: Option<SnapAnchor>,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<DevicePixelBounds> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let device_rect = snap_anchor
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
        .unwrap_or_else(|| canonicalized_scaled_rect(rect, root_scale));
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

pub(crate) fn target_quad(width: u32, height: u32) -> [[f32; 2]; 4] {
    [
        [0.0, 0.0],
        [width as f32, 0.0],
        [0.0, height as f32],
        [width as f32, height as f32],
    ]
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
    let device_pixel_step =
        if anchor.device_pixel_step.is_finite() && anchor.device_pixel_step > 0.0 {
            anchor.device_pixel_step
        } else {
            1.0
        };
    let snapped_device_origin = |origin: f32| {
        let snap_units = f64::from(origin) * f64::from(root_scale) / f64::from(device_pixel_step);
        let canonical_snap_units =
            (snap_units * DEVICE_SNAP_SUBPIXEL_STEPS).round() / DEVICE_SNAP_SUBPIXEL_STEPS;
        (canonical_snap_units.round() * f64::from(device_pixel_step)) as f32
    };
    let anchor_x = snapped_device_origin(anchor.origin.x);
    let anchor_y = snapped_device_origin(anchor.origin.y);
    quad.map(|[x, y]| {
        [
            anchor_x + canonicalize_device_coordinate((x - anchor.origin.x) * root_scale),
            anchor_y + canonicalize_device_coordinate((y - anchor.origin.y) * root_scale),
        ]
    })
}

pub(crate) fn snap_delta_for_anchor(anchor: SnapAnchor, root_scale: f32) -> Point {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return Point::default();
    }
    let device_pixel_step =
        if anchor.device_pixel_step.is_finite() && anchor.device_pixel_step > 0.0 {
            anchor.device_pixel_step
        } else {
            1.0
        };
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

pub(crate) fn snap_motion_stable_dest_quad(
    dest_quad: [[f32; 2]; 4],
    sample_mode: CompositeSampleMode,
) -> [[f32; 2]; 4] {
    if sample_mode != CompositeSampleMode::Box4 || !quad_is_axis_aligned_rect(dest_quad) {
        return dest_quad;
    }

    let delta_x = dest_quad[0][0].round() - dest_quad[0][0];
    let delta_y = dest_quad[0][1].round() - dest_quad[0][1];
    if delta_x.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && delta_y.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
    {
        return dest_quad;
    }

    dest_quad.map(|[x, y]| [x + delta_x, y + delta_y])
}

/// Translates an axis-aligned composite as one unit so its stable transform
/// pivot lands exactly on the canonical device pixel selected for that pivot.
pub(crate) fn snap_dest_quad_to_stable_point(
    dest_quad: [[f32; 2]; 4],
    stable_point: [f32; 2],
) -> [[f32; 2]; 4] {
    if !quad_is_axis_aligned_rect(dest_quad)
        || !stable_point[0].is_finite()
        || !stable_point[1].is_finite()
    {
        return dest_quad;
    }

    let delta_x = canonicalize_device_coordinate(stable_point[0]).round() - stable_point[0];
    let delta_y = canonicalize_device_coordinate(stable_point[1]).round() - stable_point[1];
    if delta_x.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
        && delta_y.abs() <= COMPOSITE_DEST_SNAP_TOLERANCE
    {
        return dest_quad;
    }

    dest_quad.map(|[x, y]| [x + delta_x, y + delta_y])
}

pub(crate) fn quantize_motion_stable_target_scale(
    target_scale: f32,
    sample_mode: CompositeSampleMode,
) -> f32 {
    if sample_mode != CompositeSampleMode::Box4 || !target_scale.is_finite() {
        return target_scale;
    }

    if target_scale < 2.0 {
        target_scale
    } else {
        target_scale.floor().max(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        axis_aligned_quad_rect, canonicalize_device_coordinate, canonicalized_scaled_quad,
        canonicalized_scaled_rect, clamp_effect_surface_scale, content_effect_pixel_rect,
        device_pixel_exact_surface_rect, fit_capture_rect_to_scale_budget_for_axes,
        offscreen_byte_size, quantize_motion_stable_target_scale, snap_dest_quad_to_stable_point,
        snap_motion_stable_dest_quad, surface_target_size,
        translation_stable_anchored_device_pixel_bounds, MAX_EFFECT_LAYER_SURFACE_BYTES,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::rect_to_quad;
    use crate::surface_plan::TranslatedContentAxes;
    use cranpose_ui_graphics::Rect;

    #[test]
    fn box4_motion_stable_dest_quad_snaps_axis_aligned_translation() {
        let quad = [[12.33, 8.66], [20.33, 8.66], [12.33, 18.66], [20.33, 18.66]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Box4),
            [[12.0, 9.0], [20.0, 9.0], [12.0, 19.0], [20.0, 19.0]]
        );
    }

    #[test]
    fn animated_scale_can_snap_around_a_stable_center() {
        let center = [72.0, 433.6];
        let snapped_center = [72.0, 434.0];

        for scale in [0.85, 0.93, 1.0, 1.07, 1.15] {
            let half_extent = 18.0 * scale;
            let quad = [
                [center[0] - half_extent, center[1] - half_extent],
                [center[0] + half_extent, center[1] - half_extent],
                [center[0] - half_extent, center[1] + half_extent],
                [center[0] + half_extent, center[1] + half_extent],
            ];
            let snapped = snap_dest_quad_to_stable_point(quad, center);
            let actual_center = [
                (snapped[0][0] + snapped[3][0]) * 0.5,
                (snapped[0][1] + snapped[3][1]) * 0.5,
            ];

            assert_eq!(
                actual_center, snapped_center,
                "scale {scale} shifted the layer"
            );
        }
    }

    #[test]
    fn linear_dest_quad_preserves_fractional_translation() {
        let quad = [[12.33, 8.66], [20.33, 8.66], [12.33, 18.66], [20.33, 18.66]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Linear),
            quad
        );
    }

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
    fn effect_pixel_rect_is_stable_under_accumulated_rigid_translation() {
        let mut translation = 2_352.801;
        let mut expected = None;

        for step in 0..10 {
            let surface = Rect {
                x: 20.0,
                y: translation,
                width: 120.0,
                height: 60.0,
            };
            let content = Rect {
                x: 24.0,
                y: translation + 7.2,
                width: 112.0,
                height: 48.0,
            };
            let actual = content_effect_pixel_rect(Some(content), surface, 150, 75);
            if let Some(expected) = expected {
                assert_eq!(actual, expected, "effect rect drifted at step {step}");
            } else {
                expected = Some(actual);
            }
            translation += 0.8;
        }
    }

    #[test]
    fn box4_non_axis_aligned_quad_stays_unsnapped() {
        let quad = [[12.33, 8.66], [20.33, 9.16], [12.33, 18.66], [20.33, 19.16]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Box4),
            quad
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

    #[test]
    fn box4_target_scale_quantizes_to_integer_texel_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Box4),
            4.0
        );
    }

    #[test]
    fn box4_target_scale_preserves_subunit_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(0.72, CompositeSampleMode::Box4),
            0.72
        );
    }

    #[test]
    fn box4_target_scale_quantizes_normal_hidpi_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(1.354, CompositeSampleMode::Box4),
            1.354
        );
    }

    #[test]
    fn box4_target_scale_quantization_does_not_break_texture_fit_after_clamp() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 240.0,
            height: 6_000.0,
        };
        let max_dim = 4_096.0;
        let clamped_scale = 9.0_f32
            .min(max_dim / rect.width.max(1.0))
            .min(max_dim / rect.height.max(1.0));
        let quantized_scale =
            quantize_motion_stable_target_scale(clamped_scale, CompositeSampleMode::Box4);
        let (_, height) = surface_target_size(rect, quantized_scale, max_dim as u32);

        assert!(
            (quantized_scale - clamped_scale).abs() < f32::EPSILON,
            "quantization must preserve the max-texture clamp when the capture already needs sub-1 scaling"
        );
        assert_eq!(height, max_dim as u32);
    }

    #[test]
    fn fit_capture_rect_to_scale_budget_trims_hidden_leading_content_before_downscale() {
        let rect = Rect {
            x: -67.0,
            y: -1953.0,
            width: 1119.0,
            height: 2761.0,
        };
        let required_rect = Rect {
            x: -67.0,
            y: 0.0,
            width: 1119.0,
            height: 808.0,
        };

        let fitted = fit_capture_rect_to_scale_budget_for_axes(
            rect,
            required_rect,
            1.25,
            8192,
            TranslatedContentAxes::default(),
        );
        let (width, height) = surface_target_size(fitted, 1.25, 8192);

        assert_eq!(width, 1399);
        assert_eq!(height, 1499);
        assert!(
            fitted.y > rect.y,
            "hidden leading content must shrink before dropping root-scale capture density"
        );
        assert_eq!(fitted.x, rect.x);
        assert_eq!(fitted.y + fitted.height, rect.y + rect.height);
    }

    #[test]
    fn fit_capture_rect_to_scale_budget_keeps_rect_when_target_scale_fits() {
        let rect = Rect {
            x: -20.0,
            y: -64.0,
            width: 240.0,
            height: 360.0,
        };
        let required_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 220.0,
            height: 296.0,
        };

        assert_eq!(
            fit_capture_rect_to_scale_budget_for_axes(
                rect,
                required_rect,
                1.25,
                8192,
                TranslatedContentAxes::default(),
            ),
            rect
        );
    }

    #[test]
    fn vertical_capture_budget_fit_preserves_horizontal_rect() {
        let rect = Rect {
            x: -67.0,
            y: -213.0,
            width: 1119.0,
            height: 1055.0,
        };
        let required_rect = Rect {
            x: 29.0,
            y: -213.0,
            width: 1023.0,
            height: 1055.0,
        };

        let fitted = fit_capture_rect_to_scale_budget_for_axes(
            rect,
            required_rect,
            1.355,
            8192,
            TranslatedContentAxes { x: false, y: true },
        );

        assert_eq!(fitted.x, rect.x);
        assert_eq!(fitted.width, rect.width);
    }

    #[test]
    fn vertical_capture_budget_fit_anchors_trimmed_height_to_required_viewport() {
        let rect = Rect {
            x: -96.0,
            y: -2_016.46,
            width: 1_148.0,
            height: 2_856.0,
        };
        let required_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 1_052.0,
            height: 808.0,
        };

        let fitted = fit_capture_rect_to_scale_budget_for_axes(
            rect,
            required_rect,
            1.355,
            8192,
            TranslatedContentAxes { x: false, y: true },
        );

        assert_eq!(fitted.x, rect.x);
        assert_eq!(fitted.width, rect.width);
        assert!(
            ((fitted.y + fitted.height) - 808.0).abs() < 0.001,
            "trimmed vertical captures must keep the viewport bottom stable instead of preserving phase-shifted trailing content"
        );
    }

    /// Raising a zoomed layer's surface density must stay inside the existing
    /// 8 MB budget: `clamp_effect_surface_scale` only clamps downward, so the
    /// larger desired scale is bounded, not honoured.
    #[test]
    fn zoom_raised_surface_scale_stays_inside_the_byte_budget() {
        // A full-screen zoomable at 3x device scale pinched to 4x.
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 390.0,
            height: 844.0,
        };
        let desired_scale = 3.0 * 4.0;

        let clamped = clamp_effect_surface_scale(rect, 1.0, desired_scale, 8192);
        let (width, height) = surface_target_size(rect, clamped, 8192);

        assert!(
            clamped < desired_scale,
            "a full-screen layer is already budget-bound, so the zoom scale must be clamped"
        );
        // `surface_target_size` ceils each axis, so allow that one row/column.
        let ceil_slack = ((width as u64) + (height as u64) + 1) * 4;
        assert!(
            offscreen_byte_size(width, height) <= MAX_EFFECT_LAYER_SURFACE_BYTES + ceil_slack,
            "{width}x{height} = {} bytes exceeds the surface budget",
            offscreen_byte_size(width, height)
        );
    }

    /// The other side of the budget: a widget-sized zoomable does get the full
    /// effective density, which is the whole point of the fix.
    #[test]
    fn zoom_raised_surface_scale_is_honoured_when_it_fits_the_budget() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        };
        let desired_scale = 3.0 * 4.0;

        assert_eq!(
            clamp_effect_surface_scale(rect, 1.0, desired_scale, 8192),
            desired_scale
        );
    }

    #[test]
    fn linear_target_scale_preserves_fractional_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Linear),
            4.72
        );
    }

    /// The composite maps the WHOLE texture onto the quad built from the
    /// surface rect, so a rect that stops short of the ceil'd texture has its
    /// padding stretched across the quad and its content compressed toward the
    /// quad's origin. The rect must name the pixels that were allocated.
    #[test]
    fn surface_rect_covers_exactly_the_pixels_that_were_allocated() {
        let rect = Rect {
            x: 12.0,
            y: 9.0,
            width: 36.0,
            height: 37.0,
        };
        let scale = 1.15;
        let (width, height) = surface_target_size(rect, scale, 8192);
        assert_eq!((width, height), (42, 43));

        let exact = device_pixel_exact_surface_rect(rect, scale, width, height);

        assert_eq!(
            exact.x, rect.x,
            "only the extent is padded, never the origin"
        );
        assert_eq!(exact.y, rect.y);
        assert!((exact.width * scale - width as f32).abs() < 1e-3);
        assert!((exact.height * scale - height as f32).abs() < 1e-3);
        assert!(exact.width >= rect.width && exact.height >= rect.height);
    }

    /// Padding the rect must not cost a byte: the texture it names is the one
    /// `surface_target_size` already ceil'd to.
    #[test]
    fn padding_the_surface_rect_does_not_grow_the_texture() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 36.0,
            height: 36.0,
        };
        for step in 0..32 {
            let scale = 1.0 + step as f32 * 0.05;
            let (width, height) = surface_target_size(rect, scale, 8192);
            let exact = device_pixel_exact_surface_rect(rect, scale, width, height);
            assert_eq!(
                surface_target_size(exact, scale, 8192),
                (width, height),
                "scale={scale} re-sized the texture"
            );
        }
    }

    /// A size the texture-dimension limit clamped is SMALLER than the rect
    /// asked for; shrinking the rect to match would clip the content off.
    #[test]
    fn a_clamped_surface_size_never_shrinks_the_rect() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 4000.0,
        };

        let exact = device_pixel_exact_surface_rect(rect, 4.0, 8192, 8192);

        assert_eq!(exact.width, 4000.0);
        assert_eq!(exact.height, 4000.0);
    }

    #[test]
    fn a_nonsense_scale_leaves_the_surface_rect_alone() {
        let rect = Rect {
            x: 3.0,
            y: 4.0,
            width: 10.0,
            height: 20.0,
        };

        assert_eq!(device_pixel_exact_surface_rect(rect, 0.0, 10, 20), rect);
        assert_eq!(
            device_pixel_exact_surface_rect(rect, f32::NAN, 10, 20),
            rect
        );
    }
}

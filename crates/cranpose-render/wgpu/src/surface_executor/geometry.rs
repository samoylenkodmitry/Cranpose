use crate::effect_renderer::CompositeSampleMode;
use crate::scene::SnapAnchor;
use crate::surface_plan::TranslatedContentAxes;
use cranpose_render_common::primitive_emit::resolve_clip;
use cranpose_ui_graphics::{Point, Rect};

pub(crate) const MAX_EFFECT_LAYER_SURFACE_BYTES: u64 = 8 * 1024 * 1024;
const QUAD_AXIS_ALIGNMENT_TOLERANCE: f32 = 1e-4;
const COMPOSITE_DEST_SNAP_TOLERANCE: f32 = 1e-4;

pub(crate) fn surface_target_size(rect: Rect, root_scale: f32, max_dim: u32) -> (u32, u32) {
    (
        (rect.width * root_scale).ceil().clamp(1.0, max_dim as f32) as u32,
        (rect.height * root_scale).ceil().clamp(1.0, max_dim as f32) as u32,
    )
}

pub(crate) fn offscreen_byte_size(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * 4
}

pub(crate) fn surface_pixel_rect(rect: Rect, root_scale: f32) -> Rect {
    Rect {
        x: rect.x * root_scale,
        y: rect.y * root_scale,
        width: rect.width * root_scale,
        height: rect.height * root_scale,
    }
}

pub(crate) fn local_effect_pixel_rect(width: u32, height: u32) -> [f32; 4] {
    [0.0, 0.0, width as f32, height as f32]
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
pub(crate) fn translation_stable_device_pixel_bounds(
    rect: Rect,
    root_scale: f32,
    max_texture_dim: u32,
) -> Option<DevicePixelBounds> {
    if !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let min_x = (rect.x * root_scale).floor();
    let min_y = (rect.y * root_scale).floor();
    let width = ((rect.width * root_scale).ceil() + 1.0).max(0.0) as u32;
    let height = ((rect.height * root_scale).ceil() + 1.0).max(0.0) as u32;
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
    Point::new(
        ((anchor.origin.x * root_scale) / device_pixel_step).round() * device_pixel_step
            / root_scale
            - anchor.origin.x,
        ((anchor.origin.y * root_scale) / device_pixel_step).round() * device_pixel_step
            / root_scale
            - anchor.origin.y,
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
        axis_aligned_quad_rect, fit_capture_rect_to_scale_budget_for_axes,
        quantize_motion_stable_target_scale, snap_motion_stable_dest_quad, surface_target_size,
        translation_stable_device_pixel_bounds,
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
    fn linear_dest_quad_preserves_fractional_translation() {
        let quad = [[12.33, 8.66], [20.33, 8.66], [12.33, 18.66], [20.33, 18.66]];

        assert_eq!(
            snap_motion_stable_dest_quad(quad, CompositeSampleMode::Linear),
            quad
        );
    }

    #[test]
    fn translation_stable_device_bounds_preserve_offscreen_source_origin() {
        let bounds = translation_stable_device_pixel_bounds(
            Rect {
                x: -12.25,
                y: 8.25,
                width: 34.5,
                height: 10.25,
            },
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
        let base = translation_stable_device_pixel_bounds(rect_at(-12.25), scale, 4096)
            .expect("base bounds");
        for step in 1..=12 {
            let moved =
                translation_stable_device_pixel_bounds(rect_at(-12.25 + step as f32), scale, 4096)
                    .expect("moved bounds");
            assert_eq!((base.width, base.height), (moved.width, moved.height));
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

    #[test]
    fn linear_target_scale_preserves_fractional_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Linear),
            4.72
        );
    }
}

use crate::effect_renderer::CompositeSampleMode;
use crate::scene::SnapAnchor;
use cranpose_render_common::primitive_emit::resolve_clip;
use cranpose_ui_graphics::{Point, Rect};

const MAX_EFFECT_LAYER_SURFACE_BYTES: u64 = 8 * 1024 * 1024;
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
        axis_aligned_quad_rect, quantize_motion_stable_target_scale, snap_motion_stable_dest_quad,
        surface_target_size,
    };
    use crate::effect_renderer::CompositeSampleMode;
    use crate::rect_to_quad;
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
    fn linear_target_scale_preserves_fractional_density() {
        assert_eq!(
            quantize_motion_stable_target_scale(4.72, CompositeSampleMode::Linear),
            4.72
        );
    }
}

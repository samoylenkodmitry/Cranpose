//! Glass shapeshift frames: the primary shape of a glass element and the
//! nearby glass shapes that join it in one field.

use cranpose_ui_graphics::{Rect, RuntimeShader};

/// A liquid shapeshift frame: the primary shape plus any number of nearby
/// glass shapes, ALL smooth-unioned into one field — liquid glass glues to
/// whatever glass it passes near (a growing menu necks with a neighboring
/// button, the drag lens merges with the search circle). Up to
/// [`GlassMorph::MAX_SHAPES`] extra shapes; an angular wobble makes the
/// mid-flight field bubble like a droplet. All geometry is node-local dp:
/// `(center_x, center_y, width, height, corner_radius)`; radius sentinel
/// `-1` means capsule; `-2` means SUBTRACT capsule — the shape carves a
/// smooth hole in the field (the growing menu leaves its anchor button
/// crisp on top until it swallows it).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GlassMorph {
    /// The glass node's own size in dp. The shader receives all morph
    /// geometry in dp and derives px-per-dp from the renderer-injected node
    /// pixel rect divided by this — geometry then lands correctly at ANY
    /// render scale (live window density, robot captures at 1.0, fractional
    /// desktop scales). The authoring widget always knows its node size.
    pub node_size: (f32, f32),
    /// The primary glass shape as `(center_x, center_y, width, height, corner_radius)`, node-local,
    /// in dp. A corner radius of `-1` means a capsule.
    pub primary: (f32, f32, f32, f32, f32),
    /// Nearby glass shapes participating in the field.
    pub shapes: Vec<(f32, f32, f32, f32, f32)>,
    /// Smooth-union glue radius (dp): shapes within it neck together.
    pub glue: f32,
    /// Wobble amplitude (dp) and phase (radians).
    pub wobble_amplitude: f32,
    /// The phase of the wobble, in radians.
    pub wobble_phase: f32,
    /// Viscous leading-edge bulge: while the shape travels or inflates, its
    /// side facing `bulge_direction` (radians, math convention) swells like a
    /// pulled droplet. Amplitude in dp, usually driven by morph velocity.
    pub bulge_amplitude: f32,
    /// The direction of the bulge, in radians in the math convention. It points toward the swelling
    /// side.
    pub bulge_direction: f32,
    /// Blends the primary rounded-rectangle field toward an ellipse. This is
    /// used by expanding droplets whose broad phase has continuous curvature
    /// rather than the straight sides of a capsule.
    pub ellipse_blend: f32,
    /// Smooths the primary capsule's straight-to-arc joins over this distance in dp.
    /// A positive value replaces its corner radius with the maximum capsule radius.
    /// The span is capped by half the difference between the primary side lengths.
    /// Zero or a nonfinite value preserves the primary outline.
    pub capsule_smoothing_dp: f32,
    /// Area-preserving affine strain applied to the primary shape. Extra
    /// scene shapes remain fixed so nearby glass can join the travelling
    /// droplet without being dragged through its local deformation.
    pub deformation: Option<GlassDeformation>,
    /// Offset (dp) of the optical-zoom axis from the primary SDF center. A
    /// leaning droplet's silhouette shifts toward its travel side while its
    /// curvature apex stays over the content it rides; anchoring the
    /// magnification there keeps the face filled by that content instead of
    /// pulling in whatever lies beyond it.
    pub zoom_anchor: (f32, f32),
}

/// A normalized motion axis and reciprocal scales for incompressible glass.
/// Construction derives the cross-axis scale, so callers cannot describe a
/// deformation that changes the bubble's area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassDeformation {
    axis: (f32, f32),
    along: f32,
}

impl GlassDeformation {
    /// Creates a deformation that scales by `along` along `axis` and by the reciprocal of `along`
    /// across it, so the area stays the same. `axis` is normalized, and a zero axis becomes the x
    /// axis.
    pub fn incompressible(axis: (f32, f32), along: f32) -> Self {
        let length = (axis.0 * axis.0 + axis.1 * axis.1).sqrt();
        let axis = if length > f32::EPSILON {
            (axis.0 / length, axis.1 / length)
        } else {
            (1.0, 0.0)
        };
        Self {
            axis,
            along: along.max(f32::EPSILON),
        }
    }

    /// The unit direction of the stretch.
    pub fn axis(self) -> (f32, f32) {
        self.axis
    }

    /// The scale along the axis.
    pub fn along(self) -> f32 {
        self.along
    }

    /// The scale across the axis, the reciprocal of `along`.
    pub fn across(self) -> f32 {
        1.0 / self.along
    }
}

impl GlassMorph {
    /// Shader budget for extra scene shapes.
    pub const MAX_SHAPES: usize = 8;
}

pub(crate) fn glass_morphs_match(a: &GlassMorph, b: &GlassMorph) -> bool {
    let shape_bits =
        |&(x, y, w, h, r): &(f32, f32, f32, f32, f32)| [x, y, w, h, r].map(f32::to_bits);
    let bits = |m: &GlassMorph| {
        let GlassMorph {
            node_size,
            primary,
            shapes: _,
            glue,
            wobble_amplitude,
            wobble_phase,
            bulge_amplitude,
            bulge_direction,
            ellipse_blend,
            capsule_smoothing_dp,
            deformation,
            zoom_anchor,
        } = m;
        (
            [
                node_size.0,
                node_size.1,
                *glue,
                *wobble_amplitude,
                *wobble_phase,
                *bulge_amplitude,
                *bulge_direction,
                *ellipse_blend,
                *capsule_smoothing_dp,
                zoom_anchor.0,
                zoom_anchor.1,
            ]
            .map(f32::to_bits),
            shape_bits(primary),
            deformation
                .map(|GlassDeformation { axis, along }| [axis.0, axis.1, along].map(f32::to_bits)),
        )
    };
    bits(a) == bits(b)
        && a.shapes
            .iter()
            .map(shape_bits)
            .eq(b.shapes.iter().map(shape_bits))
}

pub(crate) fn morph_output_support(
    morph: &GlassMorph,
    activity: f32,
    morph_pad: f32,
    shadow_reach: f32,
    shadow_offset: f32,
) -> Rect {
    let (px, py, pw, ph, _) = morph.primary;
    let (along, (ax, ay)) = morph.deformation.map_or((1.0, (1.0, 0.0)), |deformation| {
        (
            1.0 + (deformation.along() - 1.0) * activity,
            deformation.axis(),
        )
    });
    let across = 1.0 / along;
    let smallest_strain = along.min(across);
    let reach = (morph_pad + shadow_reach + 4.0) / smallest_strain;
    let rows = [
        (
            (along * ax * ax + across * ay * ay).abs(),
            ((along - across) * ax * ay).abs(),
        ),
        (
            ((along - across) * ax * ay).abs(),
            (along * ay * ay + across * ax * ax).abs(),
        ),
    ];
    let half = (pw * 0.5 + reach, ph * 0.5 + reach);
    let extent_x = rows[0].0 * half.0 + rows[0].1 * half.1;
    let extent_y = rows[1].0 * half.0 + rows[1].1 * half.1;
    let (mut left, mut top) = (px - extent_x, py - extent_y);
    let (mut right, mut bottom) = (px + extent_x, py + extent_y);
    for (sx, sy, sw, sh, _) in &morph.shapes {
        left = left.min(sx - sw * 0.5 - reach);
        top = top.min(sy - sh * 0.5 - reach);
        right = right.max(sx + sw * 0.5 + reach);
        bottom = bottom.max(sy + sh * 0.5 + reach);
    }
    Rect {
        x: left,
        y: top - shadow_offset,
        width: right - left,
        height: bottom - top + 2.0 * shadow_offset,
    }
}

pub(crate) fn set_morph_uniforms(shader: &mut RuntimeShader, morph: &GlassMorph, activity: f32) {
    let (node_w, node_h) = morph.node_size;
    let (cx, cy, w, h, radius) = morph.primary;
    shader.set_float2(0, node_w.max(1.0), node_h.max(1.0));
    shader.set_float2(2, cx, cy);
    shader.set_float2(4, w, h);
    shader.set_float(6, radius);
    let count = morph.shapes.len().min(GlassMorph::MAX_SHAPES);
    shader.set_float(30, count as f32);
    for (index, (sx, sy, sw, sh, sr)) in morph.shapes.iter().take(count).enumerate() {
        let base = 36 + index * 5;
        shader.set_float(base, *sx);
        shader.set_float(base + 1, *sy);
        shader.set_float(base + 2, *sw);
        shader.set_float(base + 3, *sh);
        shader.set_float(base + 4, *sr);
    }
    shader.set_float(31, morph.glue);
    shader.set_float(32, morph.wobble_amplitude * activity);
    shader.set_float(33, morph.wobble_phase);
    shader.set_float(26, morph.bulge_amplitude * activity);
    shader.set_float(27, morph.bulge_direction);
    shader.set_float(110, morph.ellipse_blend.clamp(0.0, 1.0) * activity);
    shader.set_float(
        cranpose_ui_graphics::liquid_glass::GLASS_CAPSULE_SMOOTHING_UNIFORM,
        if morph.capsule_smoothing_dp.is_finite() {
            morph.capsule_smoothing_dp.max(0.0) * activity
        } else {
            0.0
        },
    );
    if let Some(deformation) = morph.deformation {
        let axis = deformation.axis();
        let along = 1.0 + (deformation.along() - 1.0) * activity;
        shader.set_float2(106, axis.0, axis.1);
        shader.set_float(108, along);
        shader.set_float(109, 1.0 / along);
    } else {
        shader.set_float2(106, 1.0, 0.0);
        shader.set_float2(108, 1.0, 1.0);
    }
}

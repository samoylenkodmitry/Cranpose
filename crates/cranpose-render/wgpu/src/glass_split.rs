use cranpose_ui_graphics::{
    GLASS_EFFECT_DENSITY_UNIFORM, GLASS_FOLD_DEPTH_UNIFORM,
    GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM, GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM,
    RuntimeShader,
};

use crate::debug_toggles::DebugToggle;

static NO_GLASS_SPLIT_SCISSORS: DebugToggle = DebugToggle::new("CRANPOSE_NO_GLASS_SPLIT_SCISSORS");

const PLAIN_SDF_FLAGS: [&str; 3] = [
    "GLASS_SCENE_SHAPES_OFF",
    "GLASS_WOBBLE_OFF",
    "GLASS_STRAIN_OFF",
];
const PHYSICAL_REFRACTION_OFF_FLAG: &str = "GLASS_PHYSICAL_REFRACTION_OFF";
const CONTAINER_UNIFORM: usize = 0;
const CENTER_UNIFORM: usize = 2;
const SIZE_UNIFORM: usize = 4;
const CORNER_RADIUS_UNIFORM: usize = 6;
const REFRACTION_DEPTH_UNIFORM: usize = 9;
const GRADIENT_EXTENT_DP: f32 = 1.333_333_4;
const EDGE_EXTENT_DP: f32 = 0.333_333_34;
const MIN_BAND_WIDTH_PX: f32 = 1.0;
const MIN_LINE_WIDTH_PX: f32 = 1.4;
const LOWER_BOUND_SLACK: f32 = 0.98;
const PIXEL_MARGIN: f32 = 1.0;
/// How far past the rim's reach the hole's corner must sit, as a share of
/// the corner radius left after that reach: the rounded rect inset by the
/// reach keeps a corner of `corner - rim_high`, and the largest axis-aligned
/// rectangle inside it touches that arc at 45°, `1 - 1/sqrt(2)` of the
/// radius in from each edge.
const CORNER_TANGENT: f32 = 1.0 - std::f32::consts::FRAC_1_SQRT_2;

pub(crate) type Scissor = (u32, u32, u32, u32);

pub(crate) struct SplitScissors {
    pub(crate) interior: Option<Scissor>,
    pub(crate) rim: [Option<Scissor>; 4],
}

struct Reach {
    inner_x: f32,
    inner_y: f32,
    width: f32,
    height: f32,
    corner: f32,
    interior_inset: f32,
    rim_high: f32,
    outer_outset: f32,
}

fn uniform(shader: &RuntimeShader, slot: usize) -> f32 {
    shader.uniforms().get(slot).copied().unwrap_or(0.0)
}

fn raised(shader: &RuntimeShader, flag: &str) -> bool {
    shader
        .overrides()
        .iter()
        .any(|(name, value)| *name == flag && *value != 0.0)
}

fn reach(shader: &RuntimeShader, origin: (f32, f32), layer_pixel_rect: [f32; 4]) -> Reach {
    let [left, top, rect_width, rect_height] = layer_pixel_rect;
    let left = origin.0 + left;
    let top = origin.1 + top;
    let container = (
        uniform(shader, CONTAINER_UNIFORM),
        uniform(shader, CONTAINER_UNIFORM + 1),
    );
    let cover = container.0 <= 0.0 || container.1 <= 0.0;
    let (scale, center, size) = if cover {
        (
            uniform(shader, GLASS_EFFECT_DENSITY_UNIFORM).max(1.0),
            (rect_width * 0.5, rect_height * 0.5),
            (rect_width, rect_height),
        )
    } else {
        let dp = (
            rect_width / container.0.max(1.0),
            rect_height / container.1.max(1.0),
        );
        (
            dp.0.min(dp.1),
            (
                uniform(shader, CENTER_UNIFORM) * dp.0,
                uniform(shader, CENTER_UNIFORM + 1) * dp.1,
            ),
            (
                uniform(shader, SIZE_UNIFORM) * dp.0,
                uniform(shader, SIZE_UNIFORM + 1) * dp.1,
            ),
        )
    };
    let corner = uniform(shader, CORNER_RADIUS_UNIFORM) * scale;
    let corner = if corner < 0.0 {
        0.5 * size.0.min(size.1)
    } else {
        corner
    };
    let inradius = (size.0 * 0.5).min(size.1 * 0.5).max(1.0);
    let depth_lens = inradius * uniform(shader, REFRACTION_DEPTH_UNIFORM).max(0.0);
    let physical_lens = uniform(shader, GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM).max(0.0) * scale;
    let physical = uniform(shader, GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM) > 0.5
        && !raised(shader, PHYSICAL_REFRACTION_OFF_FLAG);
    let lens = if physical { physical_lens } else { depth_lens }.max(0.001);
    let lens_high = depth_lens.max(physical_lens).max(0.001);
    let gradient = GRADIENT_EXTENT_DP * scale;
    let edge = EDGE_EXTENT_DP * scale;
    let fold = uniform(shader, GLASS_FOLD_DEPTH_UNIFORM).max(0.0) * scale;
    let rim_low = if raised(shader, "GLASS_RIM_STYLE_OFF") {
        gradient.max(MIN_BAND_WIDTH_PX) + 1.0
    } else {
        1.5 * gradient + (0.25 * lens).max(MIN_BAND_WIDTH_PX) + 1.0
    };
    let surface = raised(shader, "GLASS_RIM_STYLE_OFF");
    let meniscus_high = if surface {
        0.0
    } else {
        1.5 * gradient + (0.25 * lens_high).max(MIN_BAND_WIDTH_PX)
    };
    let border_divisor = if surface { 16.0 } else { 8.0 };
    let rim_high = meniscus_high
        .max(gradient.max(MIN_BAND_WIDTH_PX))
        .max(edge.max(MIN_LINE_WIDTH_PX) + (lens_high / border_divisor).max(MIN_LINE_WIDTH_PX))
        .max(fold)
        + 1.0
        + PIXEL_MARGIN;
    Reach {
        inner_x: left + center.0 - size.0 * 0.5,
        inner_y: top + center.1 - size.1 * 0.5,
        width: size.0,
        height: size.1,
        corner: corner.max(0.0),
        interior_inset: rim_low * LOWER_BOUND_SLACK,
        rim_high,
        outer_outset: gradient + PIXEL_MARGIN,
    }
}

fn intersect(a: Scissor, b: Scissor) -> Option<Scissor> {
    let x0 = a.0.max(b.0);
    let y0 = a.1.max(b.1);
    let width = (a.0 + a.2).min(b.0 + b.2).checked_sub(x0)?;
    let height = (a.1 + a.3).min(b.1 + b.3).checked_sub(y0)?;
    (width > 0 && height > 0).then_some((x0, y0, width, height))
}

fn pixel_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Scissor> {
    let x0 = x0.max(0.0);
    let y0 = y0.max(0.0);
    (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

pub(crate) fn split_scissors(
    shader: &RuntimeShader,
    origin: (f32, f32),
    layer_pixel_rect: [f32; 4],
    bounds: Scissor,
) -> Option<SplitScissors> {
    if NO_GLASS_SPLIT_SCISSORS.equals("1")
        || PLAIN_SDF_FLAGS.iter().any(|flag| !raised(shader, flag))
    {
        return None;
    }
    let reach = reach(shader, origin, layer_pixel_rect);
    let bounds = if raised(shader, "GLASS_SHADOW_OFF") && raised(shader, "GLASS_ELLIPSE_BLEND_OFF")
    {
        let visible = pixel_rect(
            (reach.inner_x - reach.outer_outset).floor(),
            (reach.inner_y - reach.outer_outset).floor(),
            (reach.inner_x + reach.width + reach.outer_outset).ceil(),
            (reach.inner_y + reach.height + reach.outer_outset).ceil(),
        )
        .and_then(|rect| intersect(rect, bounds));
        let Some(visible) = visible else {
            return Some(SplitScissors {
                interior: None,
                rim: [None; 4],
            });
        };
        visible
    } else {
        bounds
    };
    let rim_inset = reach.rim_high + (reach.corner - reach.rim_high).max(0.0) * CORNER_TANGENT;
    let interior = pixel_rect(
        (reach.inner_x + reach.interior_inset).floor() - PIXEL_MARGIN,
        (reach.inner_y + reach.interior_inset).floor() - PIXEL_MARGIN,
        (reach.inner_x + reach.width - reach.interior_inset).ceil() + PIXEL_MARGIN,
        (reach.inner_y + reach.height - reach.interior_inset).ceil() + PIXEL_MARGIN,
    )
    .and_then(|rect| intersect(rect, bounds));
    let hole = pixel_rect(
        (reach.inner_x + rim_inset).ceil() + PIXEL_MARGIN,
        (reach.inner_y + rim_inset).ceil() + PIXEL_MARGIN,
        (reach.inner_x + reach.width - rim_inset).floor() - PIXEL_MARGIN,
        (reach.inner_y + reach.height - rim_inset).floor() - PIXEL_MARGIN,
    )
    .and_then(|rect| intersect(rect, bounds));
    let rim = match hole {
        None => [Some(bounds), None, None, None],
        Some((hx, hy, hw, hh)) => {
            let (bx, by, bw, bh) = bounds;
            let right = bx + bw;
            let bottom = by + bh;
            [
                intersect((bx, by, bw, hy.saturating_sub(by)), bounds),
                intersect((bx, hy + hh, bw, bottom.saturating_sub(hy + hh)), bounds),
                intersect((bx, hy, hx.saturating_sub(bx), hh), bounds),
                intersect((hx + hw, hy, right.saturating_sub(hx + hw), hh), bounds),
            ]
        }
    };
    Some(SplitScissors { interior, rim })
}

#[cfg(test)]
#[path = "tests/glass_split_tests.rs"]
mod tests;

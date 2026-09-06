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
    let rim_low = 1.5 * gradient + (0.25 * lens).max(MIN_BAND_WIDTH_PX) + 1.0;
    let rim_high = 1.5 * gradient
        + (0.25 * lens_high).max(MIN_BAND_WIDTH_PX)
        + gradient.max(MIN_BAND_WIDTH_PX)
        + edge.max(MIN_LINE_WIDTH_PX)
        + (lens_high / 8.0).max(MIN_LINE_WIDTH_PX)
        + fold
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
    let rim_inset = reach.rim_high + reach.corner;
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
mod tests {
    use super::*;

    fn plain_card() -> RuntimeShader {
        let mut shader = RuntimeShader::new("fn main() {}");
        shader.set_float2(CONTAINER_UNIFORM, 300.0, 120.0);
        shader.set_float2(CENTER_UNIFORM, 150.0, 60.0);
        shader.set_float2(SIZE_UNIFORM, 300.0, 120.0);
        shader.set_float(CORNER_RADIUS_UNIFORM, 20.0);
        shader.set_float(REFRACTION_DEPTH_UNIFORM, 0.58);
        for flag in PLAIN_SDF_FLAGS {
            shader.set_override(flag, 1.0);
        }
        shader
    }

    #[test]
    fn a_plain_card_splits_into_an_inset_interior_and_four_rim_bands_around_a_hole() {
        let shader = plain_card();
        let rect = [10.0, 20.0, 600.0, 240.0];
        let bounds = (0, 0, 720, 480);
        let split = split_scissors(&shader, (30.0, 40.0), rect, bounds)
            .expect("a plain rounded rect splits");
        let interior = split.interior.expect("the card has an interior");
        assert!(
            interior.0 > 40 && interior.1 > 60,
            "the interior is inset from the card"
        );
        assert!(
            interior.0 + interior.2 < 640 && interior.1 + interior.3 < 300,
            "the interior stays inside the card: {interior:?}"
        );
        let bands: Vec<Scissor> = split.rim.iter().flatten().copied().collect();
        assert_eq!(bands.len(), 4, "the rim is four bands around the hole");
        let hole_x = bands[2].0 + bands[2].2;
        let hole_y = bands[0].1 + bands[0].3;
        assert!(
            hole_x > interior.0 && hole_y > interior.1,
            "the hole is inset further than the interior by the corner radius"
        );
        let covered: u32 = bands.iter().map(|(_, _, w, h)| w * h).sum();
        let hole_area = bands[3].0.saturating_sub(hole_x) * bands[1].1.saturating_sub(hole_y);
        assert_eq!(
            covered + hole_area,
            720 * 480,
            "bands and hole tile the bounds exactly"
        );
    }

    fn rounded_rect_distance(p: (f32, f32), half: (f32, f32), radius: f32) -> f32 {
        let q = (p.0.abs() - (half.0 - radius), p.1.abs() - (half.1 - radius));
        let outside = (q.0.max(0.0).powi(2) + q.1.max(0.0).powi(2)).sqrt();
        outside + q.0.max(q.1).min(0.0) - radius
    }

    #[test]
    fn the_rim_hole_lies_deeper_than_the_rim_reach_even_for_a_wide_corner() {
        let mut shader = plain_card();
        shader.set_float2(CONTAINER_UNIFORM, 300.0, 200.0);
        shader.set_float2(CENTER_UNIFORM, 150.0, 100.0);
        shader.set_float2(SIZE_UNIFORM, 300.0, 200.0);
        shader.set_float(CORNER_RADIUS_UNIFORM, 60.0);
        let rect = [0.0, 0.0, 600.0, 400.0];
        let reach = reach(&shader, (0.0, 0.0), rect);
        let split = split_scissors(&shader, (0.0, 0.0), rect, (0, 0, 600, 400))
            .expect("a wide-cornered card splits");
        let bands: Vec<Scissor> = split.rim.iter().flatten().copied().collect();
        assert_eq!(bands.len(), 4);
        let (hx0, hy0) = (bands[2].0 + bands[2].2, bands[0].1 + bands[0].3);
        let (hx1, hy1) = (bands[3].0, bands[1].1);
        let half = (reach.width * 0.5, reach.height * 0.5);
        let center = (reach.inner_x + half.0, reach.inner_y + half.1);
        for (x, y) in [
            (hx0, hy0),
            (hx1 - 1, hy0),
            (hx0, hy1 - 1),
            (hx1 - 1, hy1 - 1),
        ] {
            let p = (x as f32 + 0.5 - center.0, y as f32 + 0.5 - center.1);
            let d = rounded_rect_distance(p, half, reach.corner);
            assert!(
                d <= -reach.rim_high,
                "hole corner ({x}, {y}) sits at d = {d:.1}, within the rim's reach of \
                 {:.1}: the hole must exclude only fragments the rim draw discards",
                reach.rim_high
            );
        }
    }

    #[test]
    fn a_material_with_scene_shapes_or_wobble_or_strain_keeps_whole_quads() {
        for flag in PLAIN_SDF_FLAGS {
            let mut shader = plain_card();
            shader.clear_override(flag);
            assert!(
                split_scissors(
                    &shader,
                    (0.0, 0.0),
                    [0.0, 0.0, 100.0, 50.0],
                    (0, 0, 100, 50)
                )
                .is_none(),
                "{flag} lowered means the SDF is not the plain rounded rect"
            );
        }
    }

    #[test]
    fn a_tiny_card_has_no_hole_and_the_rim_is_the_whole_bounds() {
        let shader = plain_card();
        let split = split_scissors(&shader, (0.0, 0.0), [0.0, 0.0, 30.0, 12.0], (0, 0, 30, 12))
            .expect("a plain card splits");
        assert_eq!(split.rim, [Some((0, 0, 30, 12)), None, None, None]);
    }

    #[test]
    fn a_card_whose_hole_and_interior_lie_outside_the_bounds_keeps_the_rim_over_the_bounds() {
        let shader = plain_card();
        let bounds = (0, 0, 720, 70);
        let split = split_scissors(&shader, (30.0, 40.0), [10.0, 20.0, 600.0, 240.0], bounds)
            .expect("a plain card splits");
        assert_eq!(
            split.interior, None,
            "no interior pixel is inside the bounds"
        );
        assert_eq!(split.rim, [Some(bounds), None, None, None]);
    }
}

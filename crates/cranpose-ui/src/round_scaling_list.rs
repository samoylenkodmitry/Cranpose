//! Placing list rows the way a round watch scales them.
//!
//! A scaling list shrinks and fades its rows towards the top and bottom of the
//! display so the content follows the bezel. The surprising part, and the part
//! that is wrong in every from-scratch implementation, is that **the list does
//! not re-measure a scaled row**: the column underneath stacks rows at their
//! FULL height, and each is then drawn through a graphics layer whose
//! `transformOrigin` sits on the edge facing the centre line. A row's position
//! therefore depends only on the rows above it, never on how much any of them
//! shrank. Scale first and stack the scaled heights and the list drifts further
//! out of place with every row.
//!
//! Derived from `androidx.wear.compose.foundation.lazy`
//! (`ScalingLazyColumnItemWrapper`, `calculateScaleAndAlpha`, and
//! `convertToCenterOffset`), then checked against where Compose puts rows on
//! 454x454 and 384x384 displays.
//!
//! This is pure geometry: it answers where a row goes and takes no view of how
//! it is drawn.

/// How far a row at the very edge is shrunk and faded.
pub const EDGE_SCALE: f32 = 0.7;
pub const EDGE_ALPHA: f32 = 0.5;
/// The row-height range, as a share of the viewport, over which the transition
/// band grows from [`MIN_TRANSITION_AREA`] to [`MAX_TRANSITION_AREA`]. A taller
/// row starts shrinking further from the edge than a short one.
pub const MIN_ELEMENT_HEIGHT: f32 = 0.2;
pub const MAX_ELEMENT_HEIGHT: f32 = 0.6;
pub const MIN_TRANSITION_AREA: f32 = 0.35;
pub const MAX_TRANSITION_AREA: f32 = 0.55;

/// How much a row is shrunk and faded at a given position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleAlpha {
    pub scale: f32,
    pub alpha: f32,
}

impl ScaleAlpha {
    /// A row sitting fully inside the untransformed middle of the list.
    pub const UNCHANGED: Self = Self {
        scale: 1.0,
        alpha: 1.0,
    };
}

/// Wear's `calculateScaleAndAlpha`, for a row spanning `top..bottom` in a
/// viewport of `viewport`.
///
/// All three are in one unit — device pixels if you want to match Compose
/// exactly, since it does this arithmetic on integers.
/// Returns `None` when the geometry is non-finite or the row has negative
/// height.
pub fn scale_and_alpha(viewport: f32, top: f32, bottom: f32) -> Option<ScaleAlpha> {
    if !viewport.is_finite() || !top.is_finite() || !bottom.is_finite() || bottom < top {
        return None;
    }
    if viewport <= 0.0 {
        return Some(ScaleAlpha::UNCHANGED);
    }
    // Distance to whichever edge this row is nearer, as a share of the viewport.
    let edge = (viewport - top).min(bottom) / viewport;
    let size_ratio = inverse_lerp(
        MIN_ELEMENT_HEIGHT,
        MAX_ELEMENT_HEIGHT,
        (bottom - top) / viewport,
    );
    let line = MIN_TRANSITION_AREA + (MAX_TRANSITION_AREA - MIN_TRANSITION_AREA) * size_ratio;
    if edge >= line || line <= 0.0 {
        return Some(ScaleAlpha::UNCHANGED);
    }
    let progress = ease(1.0 - edge / line);
    Some(ScaleAlpha {
        scale: 1.0 + (EDGE_SCALE - 1.0) * progress,
        alpha: 1.0 + (EDGE_ALPHA - 1.0) * progress,
    })
}

/// Where a row ends up once the list has scaled it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedRow {
    /// Top edge after the transform, in the unit `top` was given in.
    pub top: f32,
    /// Height after the transform.
    pub height: f32,
    pub scale: f32,
    pub alpha: f32,
}

/// Places a row the way a scaling list places one.
///
/// `top` is where the row would sit with nothing scaled — the running total of
/// the FULL heights of the rows above it — and `height` is its full height.
/// `density` is device pixels per unit; pass `0.0` to skip the pixel rounding
/// and work in continuous coordinates.
///
/// Compose does this on integers, and two details of that survive into the
/// result. The scaled height is rounded to a whole pixel before the row is
/// pinned, and `convertToCenterOffset` halves a size with integer division
/// while the offset it is compared against halves in floating point — so an odd
/// pixel height carries exactly half a pixel that a float-only implementation
/// loses.
///
/// Returns `None` for non-finite geometry or a negative height.
pub fn place_row(viewport: f32, top: f32, height: f32, density: f32) -> Option<PlacedRow> {
    if !height.is_finite() || height < 0.0 || !density.is_finite() {
        return None;
    }
    if density <= 0.0 {
        let transform = scale_and_alpha(viewport, top, top + height)?;
        return Some(PlacedRow {
            top,
            height: height * transform.scale,
            scale: transform.scale,
            alpha: transform.alpha,
        });
    }
    let viewport_px = (viewport * density).round();
    let top_px = (top * density).round();
    let height_px = (height * density).round();
    let transform = scale_and_alpha(viewport_px, top_px, top_px + height_px)?;
    let scaled_px = (height_px * transform.scale).round();
    // Wear's `isAboveLine`, on the same integers it uses: a row above the
    // centre line keeps its BOTTOM edge, one below keeps its top.
    let above = top_px + top_px + height_px < viewport_px;
    let pinned = if above {
        top_px + height_px - scaled_px
    } else {
        top_px
    };
    Some(PlacedRow {
        top: (pinned + odd_pixel(height_px) - odd_pixel(scaled_px)) / density,
        height: height_px * transform.scale / density,
        scale: transform.scale,
        alpha: transform.alpha,
    })
}

/// Half a pixel when a pixel height is odd, nothing when it is even — what
/// Compose's integer halving leaves behind beside its floating-point one.
fn odd_pixel(pixels: f32) -> f32 {
    let half = pixels * 0.5;
    half - half.floor()
}

fn inverse_lerp(start: f32, stop: f32, value: f32) -> f32 {
    ((value - start) / (stop - start)).clamp(0.0, 1.0)
}

/// Wear's transition easing, `CubicBezierEasing(0.3, 0.0, 0.7, 1.0)`.
///
/// A Compose easing curve is parametric, so the answer is the curve's y at the
/// parameter whose x is `fraction`. Twelve bisections put the result inside
/// 1/4096, which is far below a pixel at any watch size.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let mut low = 0.0f32;
    let mut high = 1.0f32;
    let mut t = x;
    for _ in 0..12 {
        let value = bezier(t, 0.3, 0.7);
        if value < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) * 0.5;
    }
    bezier(t, 0.0, 1.0)
}

fn bezier(t: f32, first: f32, second: f32) -> f32 {
    let inverse = 1.0 - t;
    3.0 * inverse * inverse * t * first + 3.0 * inverse * t * t * second + t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: f32 = 227.0;

    #[test]
    fn a_row_in_the_middle_is_left_alone() {
        let middle = scale_and_alpha(VIEWPORT, VIEWPORT * 0.45, VIEWPORT * 0.55).unwrap();
        assert_eq!(middle, ScaleAlpha::UNCHANGED);
    }

    #[test]
    fn a_row_at_the_edge_is_shrunk_and_faded_together() {
        let edge = scale_and_alpha(VIEWPORT, 0.0, 20.0).unwrap();
        assert!(edge.scale < 1.0 && edge.scale >= EDGE_SCALE, "{edge:?}");
        assert!(edge.alpha < 1.0 && edge.alpha >= EDGE_ALPHA, "{edge:?}");
        // Both run to their limits together, so a row never fades without
        // shrinking or the reverse.
        let top = scale_and_alpha(VIEWPORT, 0.0, 0.0).unwrap();
        assert!((top.scale - EDGE_SCALE).abs() < 1e-3, "{top:?}");
        assert!((top.alpha - EDGE_ALPHA).abs() < 1e-3, "{top:?}");
    }

    #[test]
    fn the_two_edges_treat_a_row_the_same() {
        let height = 40.0;
        let near_top = scale_and_alpha(VIEWPORT, 8.0, 8.0 + height).unwrap();
        let near_bottom =
            scale_and_alpha(VIEWPORT, VIEWPORT - 8.0 - height, VIEWPORT - 8.0).unwrap();
        assert!((near_top.scale - near_bottom.scale).abs() < 1e-5);
        assert!((near_top.alpha - near_bottom.alpha).abs() < 1e-5);
    }

    #[test]
    fn a_taller_row_starts_shrinking_further_from_the_edge() {
        // The transition band grows with row height. Isolating that needs two
        // rows at the SAME distance from an edge: anchor both near the bottom,
        // where `edge` is measured from the top edge and so does not move when
        // the height does. The taller row has the wider band, so the same
        // distance is a larger fraction of it and it shrinks more.
        let top = VIEWPORT - 10.0;
        let short = scale_and_alpha(VIEWPORT, top, top + VIEWPORT * 0.2).unwrap();
        let tall = scale_and_alpha(VIEWPORT, top, top + VIEWPORT * 0.62).unwrap();
        assert!(tall.scale < short.scale, "short {short:?} tall {tall:?}");
    }

    #[test]
    fn a_row_is_placed_from_the_full_heights_above_it_not_the_scaled_ones() {
        // Two rows of the same full height at the same unscaled offsets must
        // land where those offsets say, however much the first one shrank.
        let first = place_row(VIEWPORT, 0.0, 50.0, 2.0).unwrap();
        let second = place_row(VIEWPORT, 50.0, 50.0, 2.0).unwrap();
        assert!(first.scale < 1.0, "the first row is at the edge: {first:?}");
        // The second row's position is not pushed up by the first row shrinking.
        assert!(second.top >= 49.0, "{second:?}");
    }

    #[test]
    fn a_row_above_the_centre_line_keeps_its_bottom_edge() {
        // Above the line the transform origin is the bottom, so shrinking pulls
        // the top down; below the line the top is pinned and the bottom rises.
        let above = place_row(VIEWPORT, 4.0, 50.0, 2.0).unwrap();
        assert!(above.scale < 1.0, "{above:?}");
        assert!(
            above.top > 4.0,
            "shrinking should pull the top down: {above:?}"
        );

        let below = place_row(VIEWPORT, VIEWPORT - 54.0, 50.0, 2.0).unwrap();
        assert!(below.scale < 1.0, "{below:?}");
        assert!(
            (below.top - (VIEWPORT - 54.0)).abs() < 0.6,
            "the top is pinned below the line: {below:?}"
        );
    }

    #[test]
    fn an_odd_pixel_height_carries_the_half_pixel_composes_integer_halving_leaves() {
        // 25 units at density 2 is a 50px row — even. 25.5 is 51px — odd, and
        // that half pixel is exactly what a float-only implementation drops.
        assert_eq!(odd_pixel(50.0), 0.0);
        assert_eq!(odd_pixel(51.0), 0.5);
        let odd = place_row(VIEWPORT, 3.0, 25.5, 2.0).unwrap();
        assert!(odd.scale < 1.0, "needs to be in the scaled band: {odd:?}");
    }

    #[test]
    fn a_density_of_zero_falls_back_to_continuous_placement_instead_of_dividing_by_it() {
        let placed = place_row(VIEWPORT, 10.0, 50.0, 0.0).unwrap();
        assert!(
            placed.top.is_finite() && placed.height.is_finite(),
            "{placed:?}"
        );
        assert_eq!(placed.top, 10.0);
        let negative = place_row(VIEWPORT, 10.0, 50.0, -2.0).unwrap();
        assert_eq!(negative, placed, "a nonsense density is not a crash");
    }

    #[test]
    fn an_empty_viewport_leaves_everything_alone_rather_than_dividing_by_it() {
        assert_eq!(scale_and_alpha(0.0, 0.0, 10.0), Some(ScaleAlpha::UNCHANGED));
        assert_eq!(
            scale_and_alpha(-5.0, 0.0, 10.0),
            Some(ScaleAlpha::UNCHANGED)
        );
    }

    #[test]
    fn invalid_geometry_is_rejected_instead_of_producing_nan() {
        assert_eq!(scale_and_alpha(f32::NAN, 0.0, 10.0), None);
        assert_eq!(scale_and_alpha(VIEWPORT, 10.0, 9.0), None);
        assert_eq!(place_row(VIEWPORT, 0.0, -1.0, 2.0), None);
        assert_eq!(place_row(VIEWPORT, 0.0, 10.0, f32::INFINITY), None);
    }

    #[test]
    fn the_easing_is_monotonic_and_spans_the_whole_range() {
        assert!((ease(0.0) - 0.0).abs() < 1e-3, "{}", ease(0.0));
        assert!((ease(1.0) - 1.0).abs() < 1e-3, "{}", ease(1.0));
        let mut previous = -1.0;
        for step in 0..=20 {
            let value = ease(step as f32 / 20.0);
            assert!(value >= previous - 1e-4, "not monotonic at {step}");
            previous = value;
        }
    }

    #[test]
    fn the_easing_matches_the_current_wear_compose_curve() {
        assert!((ease(0.25) - 0.166_779).abs() < 1e-3, "{}", ease(0.25));
    }
}

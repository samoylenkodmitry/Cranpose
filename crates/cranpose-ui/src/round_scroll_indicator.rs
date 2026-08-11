//! The curved scroll indicator a round watch puts at 3 o'clock.
//!
//! Every round-screen app needs this and none of it is guessable: the track is
//! described by a height in dp rather than an angle, the thumb is a separate
//! segment with a gap at each end rather than paint over a continuous rail, and
//! a segment shorter than its own stroke turns into a shrinking, fading dot
//! instead of a stubby arc.
//!
//! The numbers and the arithmetic here were read out of
//! `androidx.wear.compose.material3` 1.6.2 with `javap -c` and then checked
//! against where the shipping Compose build actually puts pixels on 454x454 and
//! 384x384 displays. The sources are named per item so the next person can
//! re-derive them rather than trust this comment.
//!
//! This module is deliberately pure geometry. It returns the segments to draw
//! and takes no view of how they are drawn, so it costs nothing to a platform
//! that never shows it and can be tested without a GPU.

use std::f32::consts::FRAC_PI_2;

/// `ScrollIndicatorDefaults.indicatorHeight` — how far the track reaches up and
/// down from 3 o'clock, as a straight-line height rather than an arc length.
pub const INDICATOR_HEIGHT_DP: f32 = 50.0;
/// `ScrollIndicatorDefaults.indicatorWidth`, whose two values are chosen by
/// screen size.
pub const INDICATOR_WIDTH_DP: f32 = 6.0;
pub const INDICATOR_NARROW_WIDTH_DP: f32 = 5.0;
/// Wear's own breakpoint: a display at least this wide gets the wider stroke.
pub const INDICATOR_LARGE_SCREEN_DP: f32 = 225.0;
/// `PaddingDefaults.edgePadding` — how far the track's outer edge stays off the
/// display edge.
pub const INDICATOR_EDGE_PADDING_DP: f32 = 2.0;
/// `ScrollIndicatorDefaults.gapHeight` — the blank left between the thumb and
/// each end of the track.
pub const INDICATOR_GAP_DP: f32 = 3.0;
/// `ScrollIndicatorDefaults.minSizeFraction` / `maxSizeFraction` — the thumb's
/// share of the track is clamped to this range however long the list is.
pub const INDICATOR_MIN_THUMB: f32 = 0.3;
pub const INDICATOR_MAX_THUMB: f32 = 0.7;

/// The stroke width Wear would use on a display this wide.
pub fn indicator_width_dp(display_dp: f32) -> f32 {
    if display_dp.is_finite() && display_dp >= INDICATOR_LARGE_SCREEN_DP {
        INDICATOR_WIDTH_DP
    } else {
        INDICATOR_NARROW_WIDTH_DP
    }
}

/// Where the track sits on a display of the given radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorArc {
    /// Radius of the stroke's centreline, in the same unit as the radius given.
    centreline: f32,
    /// Stroke width, same unit.
    width: f32,
    /// Half the angle the whole track covers, in radians.
    half_sweep: f32,
    /// Angular amount removed from every segment before round caps are drawn.
    /// Wear derives this from the stroke width plus the visible gap.
    segment_inset: f32,
}

impl IndicatorArc {
    /// Radius of the stroke's centreline.
    pub fn centreline(self) -> f32 {
        self.centreline
    }

    /// Stroke width.
    pub fn width(self) -> f32 {
        self.width
    }

    /// Angular amount removed from each segment before its round caps draw.
    pub fn segment_inset(self) -> f32 {
        self.segment_inset
    }

    /// The angle at which the track starts, measured the way a canvas measures
    /// it: `0` at 3 o'clock, increasing clockwise.
    pub fn start_angle(self) -> f32 {
        -self.half_sweep
    }

    /// The whole track's sweep in radians.
    pub fn sweep(self) -> f32 {
        self.half_sweep * 2.0
    }

    /// How much angle a round cap adds beyond the nominal arc at each end.
    ///
    /// Wear draws each segment inset by half a cap at the start and a whole cap
    /// shorter, so the round caps put the ink back exactly on the nominal
    /// bounds. A caller that draws with a butt cap wants this to be zero.
    pub fn cap_sweep(self) -> f32 {
        if self.centreline > 0.0 {
            self.width / self.centreline
        } else {
            0.0
        }
    }
}

fn height_to_sweep(height: f32, radius: f32) -> f32 {
    if radius <= 0.0 || !radius.is_finite() {
        return 0.0;
    }
    (height * 0.5 / radius).clamp(-1.0, 1.0).asin() * 2.0
}

/// Where the track's centreline sits and how far it sweeps.
///
/// Wear describes the track by a height in dp, so the angle it covers depends
/// on the radius it is drawn at — deriving it here rather than storing an angle
/// keeps the indicator the same size in millimetres on every watch.
///
/// The centreline is `radius - edgePadding - strokeWidth / 2`. Wear converts
/// both the track height and `(strokeWidth + gapHeight)` to angles using the
/// padded radius, then adds the latter inset to the total sweep before each
/// segment removes it again. The round caps restore the stroke-width share,
/// leaving the requested visible gap.
pub fn indicator_arc(radius: f32) -> IndicatorArc {
    let width = indicator_width_dp(radius * 2.0);
    let usable_radius = radius - INDICATOR_EDGE_PADDING_DP;
    let centreline = usable_radius - width * 0.5;
    if centreline <= 0.0 || !centreline.is_finite() {
        return IndicatorArc {
            centreline: 0.0,
            width,
            half_sweep: 0.0,
            segment_inset: 0.0,
        };
    }
    let segment_inset = height_to_sweep(width + INDICATOR_GAP_DP, usable_radius);
    let half_sweep = ((height_to_sweep(INDICATOR_HEIGHT_DP, usable_radius) + segment_inset) * 0.5)
        .min(FRAC_PI_2);
    IndicatorArc {
        centreline,
        width,
        half_sweep,
        segment_inset,
    }
}

/// Where the thumb sits inside the track and how long it is, both as fractions
/// of the whole track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorGeometry {
    /// Thumb length as a share of the track, clamped to Wear's range.
    pub thumb: f32,
    /// The thumb's leading edge: `0.0` at the top, `1.0 - thumb` at the bottom.
    pub offset: f32,
}

/// Works out the thumb for a list, or `None` when everything fits on screen and
/// Wear shows nothing at all.
///
/// `content` and `viewport` are lengths in any one unit; `scrolled` is how far
/// the content has travelled, in the same unit.
pub fn indicator_geometry(content: f32, viewport: f32, scrolled: f32) -> Option<IndicatorGeometry> {
    if !(content.is_finite() && viewport.is_finite() && scrolled.is_finite()) {
        return None;
    }
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    let thumb = (viewport / content).clamp(INDICATOR_MIN_THUMB, INDICATOR_MAX_THUMB);
    let travel = content - viewport;
    let progress = (scrolled / travel).clamp(0.0, 1.0);
    Some(IndicatorGeometry {
        thumb,
        offset: progress * (1.0 - thumb),
    })
}

/// One piece of the indicator, ready to draw.
///
/// A segment shorter than its own stroke cannot be drawn as an arc without
/// looking like a blob, so Wear swaps it for a circle that shrinks and fades
/// out together. Callers draw whichever variant they are handed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IndicatorSegment {
    /// A stroked arc with a round cap, already inset so the caps land on the
    /// nominal bounds. `start` and `sweep` are radians, `0` at 3 o'clock.
    Arc { start: f32, sweep: f32, alpha: f32 },
    /// A filled circle standing in for an arc too short to draw.
    Dot {
        /// Angle of the dot's centre, radians.
        angle: f32,
        /// Radius, in the same unit as the arc's stroke width.
        radius: f32,
        alpha: f32,
    },
}

/// Which part of the indicator a segment belongs to, so a caller can colour the
/// thumb and the track differently without re-deriving the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndicatorPart {
    Track,
    Thumb,
}

/// The whole indicator as a list of drawable pieces: track, thumb, track.
///
/// It is three separate segments with a gap at each end of the thumb, not a
/// thumb painted over a continuous rail — drawing a full-length track under a
/// thumb gives a visibly different picture where the gaps should be.
///
/// `alpha` scales every piece, which is how the indicator fades out after the
/// list has been still.
pub fn indicator_segments(
    arc: IndicatorArc,
    geometry: IndicatorGeometry,
    alpha: f32,
) -> [(IndicatorPart, IndicatorSegment); 3] {
    let alpha = if alpha.is_finite() {
        alpha.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb = if geometry.thumb.is_finite() {
        geometry.thumb.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let offset = if geometry.offset.is_finite() {
        geometry.offset.clamp(0.0, 1.0 - thumb)
    } else {
        0.0
    };
    let sweep = arc.sweep();
    let top = arc.start_angle();
    let thumb_start = top + sweep * offset;
    let thumb_sweep = sweep * thumb;
    let below_start = thumb_start + thumb_sweep;
    [
        (
            IndicatorPart::Track,
            segment(top, thumb_start - top, arc.width, arc.segment_inset, alpha),
        ),
        (
            IndicatorPart::Thumb,
            segment(
                thumb_start,
                thumb_sweep,
                arc.width,
                arc.segment_inset,
                alpha,
            ),
        ),
        (
            IndicatorPart::Track,
            segment(
                below_start,
                top + sweep - below_start,
                arc.width,
                arc.segment_inset,
                alpha,
            ),
        ),
    ]
}

/// One segment, with Wear's cap inset applied and its too-short case handled.
fn segment(start: f32, sweep: f32, width: f32, inset: f32, alpha: f32) -> IndicatorSegment {
    if sweep <= 0.0 || inset <= 0.0 {
        return IndicatorSegment::Arc {
            start,
            sweep: 0.0,
            alpha: 0.0,
        };
    }
    if sweep < inset {
        // Below one stroke width Wear stops drawing an arc and draws a circle
        // that shrinks and fades on the same fraction, so a segment leaves the
        // screen smoothly instead of collapsing into a dash.
        let fill = sweep / inset;
        return IndicatorSegment::Dot {
            angle: start + sweep * 0.5,
            radius: width * 0.5 * fill,
            alpha: alpha * fill,
        };
    }
    // `drawCurvedIndicatorSegment` starts half an inset in and runs a whole
    // inset shorter; round caps restore the stroke share and leave the gap.
    IndicatorSegment::Arc {
        start: start + inset * 0.5,
        sweep: sweep - inset,
        alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two displays Google Play requires a Wear app to support, in dp.
    const LARGE_RADIUS_DP: f32 = 113.5; // 454px at density 2
    const SMALL_RADIUS_DP: f32 = 96.0; // 384px at density 2

    #[test]
    fn stroke_width_switches_at_the_wear_large_screen_breakpoint() {
        assert_eq!(indicator_width_dp(224.99), INDICATOR_NARROW_WIDTH_DP);
        assert_eq!(indicator_width_dp(225.0), INDICATOR_WIDTH_DP);
        assert_eq!(indicator_width_dp(f32::NAN), INDICATOR_NARROW_WIDTH_DP);
    }

    #[test]
    fn the_track_lands_where_the_shipping_compose_build_draws_it() {
        // Measured off the Compose build itself: the stroke's centreline sits
        // at 108.5dp on a 454px display and 91.5dp on a 384px one, and the
        // stroke is 6dp on the first and 5dp on the second.
        let large = indicator_arc(LARGE_RADIUS_DP);
        assert!((large.centreline() - 108.5).abs() < 0.01, "{large:?}");
        assert!((large.width() - 6.0).abs() < 0.01, "{large:?}");

        let small = indicator_arc(SMALL_RADIUS_DP);
        assert!((small.centreline() - 91.5).abs() < 0.01, "{small:?}");
        assert!((small.width() - 5.0).abs() < 0.01, "{small:?}");
    }

    #[test]
    fn the_sweep_is_a_height_in_dp_not_a_fixed_angle() {
        // The same 50dp track covers a wider angle on a smaller watch, which is
        // the whole point of storing a height rather than an angle.
        let large = indicator_arc(LARGE_RADIUS_DP).sweep().to_degrees();
        let small = indicator_arc(SMALL_RADIUS_DP).sweep().to_degrees();
        assert!((large - 30.54).abs() < 0.05, "{large}");
        assert!((small - 35.73).abs() < 0.05, "{small}");
        assert!(small > large);
    }

    #[test]
    fn a_list_that_fits_on_screen_shows_no_indicator_at_all() {
        assert_eq!(indicator_geometry(100.0, 100.0, 0.0), None);
        assert_eq!(indicator_geometry(80.0, 100.0, 0.0), None);
        assert_eq!(indicator_geometry(f32::NAN, 100.0, 0.0), None);
        assert_eq!(indicator_geometry(200.0, 0.0, 0.0), None);
    }

    #[test]
    fn the_thumb_is_the_viewport_share_clamped_at_both_ends() {
        // Half the content visible is half the track...
        let half = indicator_geometry(200.0, 100.0, 0.0).unwrap();
        assert!((half.thumb - 0.5).abs() < 1e-6, "{half:?}");
        // ...but a very long list never shrinks it past the floor, and a barely
        // scrolling one never grows it past the ceiling.
        let long = indicator_geometry(10_000.0, 100.0, 0.0).unwrap();
        assert!((long.thumb - INDICATOR_MIN_THUMB).abs() < 1e-6, "{long:?}");
        let short = indicator_geometry(105.0, 100.0, 0.0).unwrap();
        assert!(
            (short.thumb - INDICATOR_MAX_THUMB).abs() < 1e-6,
            "{short:?}"
        );
    }

    #[test]
    fn the_thumb_reaches_the_bottom_of_the_track_and_no_further() {
        let bottom = indicator_geometry(200.0, 100.0, 100.0).unwrap();
        assert!(
            (bottom.offset + bottom.thumb - 1.0).abs() < 1e-6,
            "{bottom:?}"
        );
        // Overscrolling past the end must not push it off the track.
        let past = indicator_geometry(200.0, 100.0, 500.0).unwrap();
        assert_eq!(past, bottom);
    }

    #[test]
    fn the_indicator_is_three_segments_with_a_gap_either_side_of_the_thumb() {
        let arc = indicator_arc(LARGE_RADIUS_DP);
        let geometry = IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        };
        let parts = indicator_segments(arc, geometry, 1.0);
        assert_eq!(parts[0].0, IndicatorPart::Track);
        assert_eq!(parts[1].0, IndicatorPart::Thumb);
        assert_eq!(parts[2].0, IndicatorPart::Track);

        // Every piece is an arc at this size, and the ink they cover — the
        // nominal bounds, once the round caps undo the inset — must stay inside
        // the track with the gaps left blank.
        let ink_bounds = |segment: IndicatorSegment| match segment {
            IndicatorSegment::Arc { start, sweep, .. } => {
                (start - arc.cap_sweep() * 0.5, sweep + arc.cap_sweep())
            }
            other => panic!("expected an arc, got {other:?}"),
        };
        let (above_start, above_sweep) = ink_bounds(parts[0].1);
        let (thumb_start, thumb_sweep) = ink_bounds(parts[1].1);
        let (below_start, below_sweep) = ink_bounds(parts[2].1);
        let gap = arc.segment_inset() - arc.cap_sweep();

        assert!((above_start - arc.start_angle() - gap * 0.5).abs() < 1e-4);
        assert!((thumb_start - (above_start + above_sweep) - gap).abs() < 1e-4);
        assert!((below_start - (thumb_start + thumb_sweep) - gap).abs() < 1e-4);
        assert!(
            (below_start + below_sweep + gap * 0.5 - (arc.start_angle() + arc.sweep())).abs()
                < 1e-4,
            "the track has to end where it should"
        );
    }

    #[test]
    fn a_segment_shorter_than_its_stroke_becomes_a_shrinking_dot() {
        let arc = indicator_arc(LARGE_RADIUS_DP);
        // Thumb hard against the top: the track above it has almost no room.
        let parts = indicator_segments(
            arc,
            IndicatorGeometry {
                thumb: 0.7,
                offset: 0.0,
            },
            1.0,
        );
        match parts[0].1 {
            IndicatorSegment::Dot { radius, alpha, .. } => {
                assert!(
                    radius <= arc.width() * 0.5,
                    "a dot never exceeds the stroke"
                );
                assert!(alpha < 1.0, "it fades on the same fraction as it shrinks");
            }
            IndicatorSegment::Arc { sweep, .. } => {
                assert!(sweep <= 0.0, "an arc this short should have been a dot");
            }
        }
    }

    #[test]
    fn fading_the_indicator_fades_every_piece_of_it() {
        let arc = indicator_arc(LARGE_RADIUS_DP);
        let geometry = IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        };
        for (_, segment) in indicator_segments(arc, geometry, 0.25) {
            let alpha = match segment {
                IndicatorSegment::Arc { alpha, .. } => alpha,
                IndicatorSegment::Dot { alpha, .. } => alpha,
            };
            assert!(alpha <= 0.25 + 1e-6, "{segment:?}");
        }
    }

    #[test]
    fn a_display_too_small_to_hold_the_track_degrades_instead_of_panicking() {
        let tiny = indicator_arc(1.0);
        assert_eq!(tiny.centreline(), 0.0);
        assert_eq!(tiny.sweep(), 0.0);
        assert_eq!(tiny.cap_sweep(), 0.0);
        // And asking for its segments must not divide by that zero.
        let parts = indicator_segments(
            tiny,
            IndicatorGeometry {
                thumb: 0.4,
                offset: 0.3,
            },
            1.0,
        );
        for (_, segment) in parts {
            assert!(matches!(segment, IndicatorSegment::Arc { sweep: 0.0, .. }));
        }
    }

    #[test]
    fn invalid_public_inputs_never_emit_non_finite_draw_values() {
        for radius in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            let arc = indicator_arc(radius);
            assert_eq!(arc.sweep(), 0.0);
            assert_eq!(arc.segment_inset(), 0.0);
        }

        let parts = indicator_segments(
            indicator_arc(LARGE_RADIUS_DP),
            IndicatorGeometry {
                thumb: f32::NAN,
                offset: f32::INFINITY,
            },
            f32::NAN,
        );
        for (_, part) in parts {
            match part {
                IndicatorSegment::Arc {
                    start,
                    sweep,
                    alpha,
                } => assert!(start.is_finite() && sweep.is_finite() && alpha == 0.0),
                IndicatorSegment::Dot {
                    angle,
                    radius,
                    alpha,
                } => assert!(angle.is_finite() && radius.is_finite() && alpha == 0.0),
            }
        }
    }
}

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

use crate::round_scaling_list::ScalingParams;
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
///
/// This is the generic, flat-list model: the thumb is the share of the content
/// on screen and it moves with the pixels. A `ScalingLazyColumn` does **not**
/// work this way — see [`scaling_list_geometry`], which is the rule Wear's own
/// indicator uses for one. Reach for this one when a caller genuinely scrolls
/// pixels, and for that one when it is a Wear list.
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

/// One row of a `ScalingLazyColumn`, as `ScalingLazyListItemInfo` reports it.
///
/// **Device pixels.** Wear's adapter reads a layout that has already been
/// resolved onto the pixel grid — item heights are whole pixels, the viewport's
/// centre line is an integer halving — and it divides by those integers. Doing
/// the same arithmetic in points quietly loses the halves, and the halves are
/// what decide which item index the thumb's ends land on.
///
/// [`scaling_list_items`] builds these from a laid-out list; a caller that
/// already holds a real `ScalingLazyListLayoutInfo` can fill them in directly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorItem {
    /// The row's index in the whole list.
    pub index: usize,
    /// `ScalingLazyListItemInfo.startOffset(ItemCenter)`: the row's top edge
    /// measured from the viewport's centre line, after scaling.
    pub start_offset: f32,
    /// `ScalingLazyListItemInfo.size`: the row's height after scaling, rounded
    /// to a whole pixel. Not the height the row is *drawn* at — the graphics
    /// layer scales by the unrounded scale — but this rounded one is what the
    /// layout info reports and therefore what the indicator divides by.
    pub size: f32,
}

/// A scaling list as `ScalingLazyColumnStateAdapter` sees it. Device pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalingList<'a> {
    /// The rows on screen, in order. Only the first and last are read, but the
    /// whole window is taken because that is what the adapter is handed and
    /// because a caller that trims it to two has to get the window right
    /// itself.
    pub visible: &'a [IndicatorItem],
    /// `totalItemsCount` — every row, on screen or not. This is the
    /// denominator the thumb's length is a share of.
    pub total: usize,
    /// `viewportSize.height`.
    pub viewport: f32,
    /// `beforeContentPadding + beforeAutoCenteringPadding`, the blank the list
    /// keeps above its first row. It counts only while the first row is on
    /// screen, which is the adapter's own rule and not an optimisation.
    pub before_padding: f32,
    /// `afterContentPadding + afterAutoCenteringPadding`, likewise below the
    /// last row.
    pub after_padding: f32,
}

/// Where the first visible row sits, as a fractional item index.
///
/// `androidx.wear.compose.material3.ScalingLazyColumnStateAdapter`. The whole
/// part is the row's index and the fraction is how much of it has gone off the
/// top, so a list that has scrolled half of item 3 away reads 3.5 — **an
/// item-space position, not a pixel one**.
pub fn decimal_first_item_index(list: ScalingList<'_>) -> f32 {
    let Some(first) = list.visible.first() else {
        return 0.0;
    };
    let offset_from_start = if first.index == 0 {
        list.before_padding
    } else {
        0.0
    };
    let start = first.start_offset - offset_from_start;
    let top = -(list.viewport / 2.0);
    let fraction = ((top - start) / (first.size + offset_from_start).max(1.0)).max(0.0);
    finite(first.index as f32 + fraction)
}

/// Where the last visible row sits, as a fractional item index.
///
/// The mirror of [`decimal_first_item_index`]: the fraction is how much of the
/// row is on screen, so a list showing the top third of item 6 reads 6.33.
pub fn decimal_last_item_index(list: ScalingList<'_>) -> f32 {
    let Some(last) = list.visible.last() else {
        return 0.0;
    };
    let span = last.size
        + if last.index + 1 == list.total {
            list.after_padding
        } else {
            0.0
        };
    let end = last.start_offset + span;
    let bottom = list.viewport / 2.0;
    let fraction = (1.0 - (end - bottom) / span.max(1.0)).min(1.0);
    finite(last.index as f32 + fraction)
}

/// How far down the track the thumb's leading edge sits, before the thumb's own
/// length is taken out of the travel. `0.0` at the top, `1.0` at the bottom.
///
/// The denominator is the number of items that are *not* on screen — how far
/// the list can still travel, counted in items — which is why this is not the
/// same number as a pixel scroll's progress on a list whose rows differ in
/// height.
pub fn position_fraction(list: ScalingList<'_>) -> f32 {
    if list.visible.is_empty() {
        return 0.0;
    }
    let first = decimal_first_item_index(list);
    let remaining = list.total as f32 - decimal_last_item_index(list);
    if first + remaining == 0.0 {
        0.0
    } else {
        finite(first / (first + remaining))
    }
}

/// The thumb's length, and the fact that Wear only measures it once.
///
/// `ScalingLazyColumnStateAdapter` holds `currentSizeFraction` and recomputes
/// it **only when `totalItemsCount` changes**, guarded by `previousItemsCount`.
/// That is not a cache in the sense of an optimisation, it is the behaviour:
/// the thumb keeps the length it was given by the list's first layout and does
/// not breathe as rows of different heights scroll past. Recomputing it every
/// frame gives a thumb that grows and shrinks while you turn the crown, which
/// the shipping build does not do.
///
/// One of these belongs to one list. Give a screen its own, and drop it (or
/// call [`ThumbLength::forget`]) when the screen goes away, the way Wear drops
/// the adapter with the `ScreenScaffold` that made it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThumbLength {
    fraction: f32,
    items: usize,
}

impl ThumbLength {
    /// `getSizeFraction`: the share of the track the thumb covers.
    pub fn of(&mut self, list: ScalingList<'_>) -> f32 {
        if list.visible.is_empty() {
            return 0.0;
        }
        if self.items != list.total {
            self.items = list.total;
            let span = decimal_last_item_index(list) - decimal_first_item_index(list);
            let share = span / list.total.max(1) as f32;
            self.fraction = if share.is_finite() {
                share.clamp(INDICATOR_MIN_THUMB, INDICATOR_MAX_THUMB)
            } else {
                INDICATOR_MIN_THUMB
            };
        }
        self.fraction
    }

    /// Forget the measured length, so the next list measures itself again.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

/// The thumb for a `ScalingLazyColumn`, in the item-index space Wear uses.
///
/// This is the second of the two models in this module and the one a Wear list
/// wants. [`indicator_geometry`] answers "what share of the content is on
/// screen, and how far have the pixels travelled"; Wear asks "what share of the
/// *items* is on screen, and how many items are left". The two agree only when
/// every row is the same height and the list is as tall as its content — which
/// is why a port built on the pixel model can look right on one display size
/// and put the thumb in the wrong place on another.
///
/// Returns `None` when there is nothing on screen to describe. It does not
/// decide whether the list is scrollable at all: Wear leaves that to
/// `ScreenScaffold`, and so does this.
pub fn scaling_list_geometry(
    thumb: &mut ThumbLength,
    list: ScalingList<'_>,
) -> Option<IndicatorGeometry> {
    if list.visible.is_empty() || list.total == 0 || !list.viewport.is_finite() {
        return None;
    }
    let size = thumb.of(list);
    let position = position_fraction(list).clamp(0.0, 1.0);
    Some(IndicatorGeometry {
        thumb: size,
        offset: position * (1.0 - size),
    })
}

/// The rows of a laid-out scaling list that are on screen, as the adapter reads
/// them, for a list scaled by Wear's own ramp.
///
/// `rows` are `(top, height)` pairs — the walk's cursor and the row's full
/// height, the same geometry [`crate::round_scaling_list::place_row`] takes —
/// already moved to where the list sits on screen, and in whatever unit
/// `viewport` is given in. `density` converts that unit to device pixels;
/// [`IndicatorItem`] is always in pixels, because that is the space Wear does
/// this arithmetic in.
///
/// The window is the contiguous run of rows whose scaled rectangle still meets
/// the viewport, which is what Wear's own walk out from the centre item
/// produces: it stops the first time the running edge leaves the display.
///
/// `out` is cleared first, so one buffer can be reused frame to frame.
pub fn scaling_list_items<I>(viewport: f32, density: f32, rows: I, out: &mut Vec<IndicatorItem>)
where
    I: IntoIterator<Item = (f32, f32)>,
{
    scaling_list_items_with(ScalingParams::WEAR, viewport, density, rows, out)
}

/// [`scaling_list_items`] for a list whose ramp is not the default one.
///
/// A row's reported size is its full height times the scale the ramp gave it,
/// so a list built with different [`ScalingParams`] reports different sizes and
/// its thumb sits somewhere else. Every list Cranpose ships uses
/// [`ScalingParams::WEAR`] and cannot tell the two apart; a list under
/// `LocalReduceMotion` uses [`ScalingParams::reduced_motion`], where every row
/// reports its full height, and can.
pub fn scaling_list_items_with<I>(
    params: ScalingParams,
    viewport: f32,
    density: f32,
    rows: I,
    out: &mut Vec<IndicatorItem>,
) where
    I: IntoIterator<Item = (f32, f32)>,
{
    out.clear();
    if !viewport.is_finite() || !density.is_finite() {
        return;
    }
    // A caller working in continuous coordinates gets the same rule with the
    // integer steps taken out, which is what `place_row` does with the same
    // argument.
    let pixels = density > 0.0;
    let to_px = |value: f32| if pixels { value * density } else { value };
    let round_px = |value: f32| if pixels { value.round() } else { value };
    let viewport_px = round_px(to_px(viewport));
    // `viewportCenterLinePx()`: half the viewport rounded DOWN, so an odd
    // viewport gives its spare pixel to the half below the line.
    let centre_line = if pixels {
        (viewport_px * 0.5).floor()
    } else {
        viewport_px * 0.5
    };
    for (index, (top, height)) in rows.into_iter().enumerate() {
        let Some(placed) =
            crate::round_scaling_list::place_row_with(params, viewport, top, height, density)
        else {
            continue;
        };
        let height_px = round_px(to_px(height));
        let size = round_px(height_px * placed.scale);
        // `place_row` answers where the row is DRAWN, and Compose's drawn
        // position carries half a pixel that the reported offset does not: the
        // graphics layer's `translationY` is
        // `startOffset - unadjustedStartOffset`, and each of those halves an
        // integer height twice — once with integer division inside
        // `convertToCenterOffset`, once in floating point inside `startOffset`
        // — so the unadjusted row's half survives into the drawing and cancels
        // out of the report. Undoing it here is what keeps the two coordinate
        // systems from sitting half a pixel apart per row.
        let drawn_top = to_px(placed.top);
        let carried = if pixels { odd_pixel(height_px) } else { 0.0 };
        let stacked_top = drawn_top - carried + if pixels { odd_pixel(size) } else { 0.0 };
        if stacked_top > viewport_px || stacked_top + size < 0.0 {
            if out.is_empty() {
                continue;
            }
            break;
        }
        out.push(IndicatorItem {
            index,
            start_offset: drawn_top - carried - centre_line,
            size,
        });
    }
}

/// Half a pixel when a pixel height is odd, nothing when it is even.
fn odd_pixel(pixels: f32) -> f32 {
    let half = pixels * 0.5;
    half - half.floor()
}

fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
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

    /// A synthetic list to hang the adapter's arithmetic on: a 400px viewport,
    /// so the centre line is 200 and `startOffset` is a row's top edge measured
    /// from there, and ten rows of 100px.
    const VIEWPORT: f32 = 400.0;

    fn list<'a>(visible: &'a [IndicatorItem]) -> ScalingList<'a> {
        ScalingList {
            visible,
            total: 10,
            viewport: VIEWPORT,
            before_padding: 0.0,
            after_padding: 0.0,
        }
    }

    fn row(index: usize, start_offset: f32) -> IndicatorItem {
        IndicatorItem {
            index,
            start_offset,
            size: 100.0,
        }
    }

    #[test]
    fn a_row_flush_with_the_top_of_the_screen_is_a_whole_index() {
        // Its top edge is 200px above the centre line, which is the top of the
        // display, so none of it has scrolled away.
        let rows = [row(3, -200.0), row(6, 100.0)];
        assert_eq!(decimal_first_item_index(list(&rows)), 3.0);
    }

    #[test]
    fn a_row_half_off_the_top_reads_half_an_index() {
        // 250 above the centre line is 50 above the display, half of a 100px
        // row -- and the answer is in ITEM units, which is the whole point of
        // this model: 3.5 means "three and a half items have gone past".
        let rows = [row(3, -250.0), row(6, 100.0)];
        assert_eq!(decimal_first_item_index(list(&rows)), 3.5);
    }

    #[test]
    fn the_last_index_counts_how_much_of_the_row_is_on_screen() {
        // Bottom half of the display is 0..200 below the centre line; a row
        // starting at 150 has 50 of its 100 showing.
        let rows = [row(3, -200.0), row(6, 150.0)];
        assert_eq!(decimal_last_item_index(list(&rows)), 6.5);
    }

    #[test]
    fn the_padding_outside_the_list_counts_only_at_the_end_it_belongs_to() {
        let rows = [row(0, -250.0), row(9, 150.0)];
        let padded = ScalingList {
            before_padding: 80.0,
            after_padding: 60.0,
            ..list(&rows)
        };
        // The first row's own span grows by the blank above it, and so does the
        // distance it has travelled: (200 - 250 + 80) / (100 + 80).
        assert!((decimal_first_item_index(padded) - 130.0 / 180.0).abs() < 1e-6);
        // The last row's span grows by the blank below it: 1 - (310 - 200)/160.
        assert!((decimal_last_item_index(padded) - (9.0 + 0.3125)).abs() < 1e-6);

        // The same geometry in the middle of the list ignores both.
        let inner = [row(3, -250.0), row(6, 150.0)];
        let inner = ScalingList {
            before_padding: 80.0,
            after_padding: 60.0,
            ..list(&inner)
        };
        assert_eq!(decimal_first_item_index(inner), 3.5);
        assert_eq!(decimal_last_item_index(inner), 6.5);
    }

    #[test]
    fn the_thumb_is_the_share_of_the_items_on_screen_not_of_the_pixels() {
        // Five of ten items on screen is half the track, whatever those items
        // are worth in pixels.
        let rows = [row(3, -250.0), row(8, 150.0)];
        let mut thumb = ThumbLength::default();
        assert!((thumb.of(list(&rows)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_thumb_is_clamped_at_both_ends_however_long_the_list_is() {
        let rows = [row(3, -250.0), row(4, 150.0)];
        let mut short = ThumbLength::default();
        assert_eq!(short.of(list(&rows)), INDICATOR_MIN_THUMB);

        let rows = [row(0, -250.0), row(9, 150.0)];
        let mut long = ThumbLength::default();
        assert_eq!(long.of(list(&rows)), INDICATOR_MAX_THUMB);
    }

    #[test]
    fn the_thumb_is_measured_once_and_then_only_when_the_list_changes_length() {
        // `previousItemsCount` in the adapter. Not an optimisation: a thumb
        // remeasured every frame breathes as rows of different heights scroll
        // past, and the shipping build's does not move at all.
        let mut thumb = ThumbLength::default();
        let five = [row(3, -250.0), row(8, 150.0)];
        assert!((thumb.of(list(&five)) - 0.5).abs() < 1e-6);

        let three = [row(3, -250.0), row(6, 150.0)];
        assert!(
            (thumb.of(list(&three)) - 0.5).abs() < 1e-6,
            "the window shrank but the list did not, so the thumb holds"
        );

        let longer = ScalingList {
            total: 20,
            ..list(&three)
        };
        assert_eq!(thumb.of(longer), INDICATOR_MIN_THUMB);

        thumb.forget();
        assert!((thumb.of(list(&five)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_position_is_how_many_items_are_left_not_how_far_the_pixels_went() {
        // Three and a half items above the window, three and a half below it.
        let rows = [row(3, -250.0), row(6, 150.0)];
        assert!((position_fraction(list(&rows)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_list_at_the_top_puts_the_thumb_at_the_top_and_one_at_the_end_at_the_end() {
        let mut thumb = ThumbLength::default();
        let top = [row(0, -200.0), row(3, 150.0)];
        let geometry = scaling_list_geometry(&mut thumb, list(&top)).unwrap();
        assert_eq!(geometry.offset, 0.0);

        // The last row measured fully in leaves nothing after it, so the thumb
        // is flush with the end of the track.
        let mut thumb = ThumbLength::default();
        let end = [row(6, -250.0), row(9, 100.0)];
        let geometry = scaling_list_geometry(&mut thumb, list(&end)).unwrap();
        assert_eq!(decimal_last_item_index(list(&end)), 10.0);
        assert!((geometry.offset + geometry.thumb - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_list_with_nothing_on_screen_has_no_indicator() {
        let mut thumb = ThumbLength::default();
        assert_eq!(scaling_list_geometry(&mut thumb, list(&[])), None);
        let rows = [row(3, -250.0)];
        let empty = ScalingList {
            total: 0,
            ..list(&rows)
        };
        assert_eq!(scaling_list_geometry(&mut thumb, empty), None);
    }

    #[test]
    fn the_two_models_disagree_the_moment_the_rows_are_not_all_the_same_height() {
        // This is the whole reason the second entry point exists. One tall row
        // and nine short ones: the pixel model says the thumb is the share of
        // the CONTENT on screen, the adapter says it is the share of the ITEMS,
        // and the tall row counts for one item and for six rows' worth of
        // pixels. A caller that reaches for the flat model on a Wear list gets
        // an answer that happens to look right on a list of uniform rows.
        let heights: Vec<f32> = std::iter::once(600.0).chain([100.0; 9]).collect();
        let content: f32 = heights.iter().sum();
        let pixel = indicator_geometry(content, VIEWPORT, 0.0).unwrap();

        let rows = [row(0, -200.0), row(3, 100.0)];
        let mut thumb = ThumbLength::default();
        let wear = scaling_list_geometry(&mut thumb, list(&rows)).unwrap();

        assert!((pixel.thumb - INDICATOR_MIN_THUMB).abs() < 1e-6);
        assert!((wear.thumb - 0.4).abs() < 1e-6, "{wear:?}");
    }

    #[test]
    fn a_reported_row_is_not_the_row_as_it_is_drawn() {
        // `place_row` answers where a row is DRAWN and the adapter reads what
        // the layout REPORTS, and Compose's two halvings of an odd pixel height
        // put half a pixel between them. 103px is odd; 104px is not.
        let mut out = Vec::new();
        let density = 2.0;
        let viewport = 227.0;
        for (height, carried) in [(51.5, 0.5), (52.0, 0.0)] {
            scaling_list_items(viewport, density, [(20.0, height)], &mut out);
            let drawn = crate::round_scaling_list::place_row(viewport, 20.0, height, density)
                .expect("placed");
            let item = out.first().expect("on screen");
            assert!(
                (item.start_offset - (drawn.top * density - carried - 227.0)).abs() < 1e-4,
                "{height}dp: reported {} against drawn {}",
                item.start_offset,
                drawn.top * density
            );
            // And the size it reports is the drawn height rounded to a pixel,
            // which is not the height the graphics layer scales to.
            assert_eq!(item.size, (drawn.height * density).round());
        }
    }

    #[test]
    fn a_list_that_does_not_scale_its_rows_reports_them_at_full_height() {
        // `scaling_list_items` baked in `ScalingParams::WEAR`, so a list under
        // `LocalReduceMotion` — where the ramp is off and every row keeps its
        // size — was described to the indicator as though its edge rows had
        // shrunk. Identical for every list Cranpose ships and wrong for that
        // one, which is the shape of defect a `_with` variant exists to stop.
        let mut wear = Vec::new();
        let mut still = Vec::new();
        let rows = [(4.0, 52.0), (60.0, 52.0), (116.0, 52.0)];
        scaling_list_items(227.0, 2.0, rows, &mut wear);
        scaling_list_items_with(
            ScalingParams::WEAR.reduced_motion(),
            227.0,
            2.0,
            rows,
            &mut still,
        );
        assert_eq!(wear.len(), still.len());
        assert!(
            wear[0].size < still[0].size,
            "the top row shrinks under the Wear ramp and not under a stilled \
             one: {} vs {}",
            wear[0].size,
            still[0].size
        );
        assert_eq!(still[0].size, 104.0, "52dp at density 2, unscaled");
        // And the default entry point is still the Wear ramp.
        let mut default = Vec::new();
        scaling_list_items_with(ScalingParams::WEAR, 227.0, 2.0, rows, &mut default);
        assert_eq!(default, wear);
    }

    #[test]
    fn the_window_is_the_rows_that_still_meet_the_display() {
        // Ten 40dp rows down a 227dp screen, the list scrolled so row 0 starts
        // 100dp above the top. Wear walks out from the centre item and stops at
        // the first row whose edge has left the viewport, which is the same
        // contiguous run.
        let mut out = Vec::new();
        let rows: Vec<(f32, f32)> = (0..10)
            .map(|index| (index as f32 * 40.0 - 100.0, 40.0))
            .collect();
        scaling_list_items(227.0, 2.0, rows.iter().copied(), &mut out);
        let indices: Vec<usize> = out.iter().map(|item| item.index).collect();
        assert_eq!(indices, vec![2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn invalid_scaling_list_input_never_produces_a_non_finite_thumb() {
        let mut out = Vec::new();
        scaling_list_items(f32::NAN, 2.0, [(0.0, 40.0)], &mut out);
        assert!(out.is_empty());
        scaling_list_items(227.0, f32::NAN, [(0.0, 40.0)], &mut out);
        assert!(out.is_empty());

        let rows = [
            IndicatorItem {
                index: 0,
                start_offset: f32::NAN,
                size: 0.0,
            },
            IndicatorItem {
                index: 3,
                start_offset: f32::INFINITY,
                size: -1.0,
            },
        ];
        let mut thumb = ThumbLength::default();
        let geometry = scaling_list_geometry(&mut thumb, list(&rows)).expect("a geometry");
        assert!(
            geometry.thumb.is_finite() && geometry.offset.is_finite(),
            "{geometry:?}"
        );
        assert!(geometry.thumb >= INDICATOR_MIN_THUMB && geometry.thumb <= INDICATOR_MAX_THUMB);
        assert!(geometry.offset >= 0.0 && geometry.offset <= 1.0);
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

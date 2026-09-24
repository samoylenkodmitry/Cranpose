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

use crate::{
    round_scaling_list::ScalingParams,
    scrollbar::{ThumbBounds, thumb_geometry},
};

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
    centreline: f32,
    width: f32,
    half_sweep: f32,
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
    thumb_geometry(
        content,
        viewport,
        scrolled,
        ThumbBounds::new(INDICATOR_MIN_THUMB, INDICATOR_MAX_THUMB),
    )
    .map(|geometry| IndicatorGeometry {
        thumb: geometry.length,
        offset: geometry.offset,
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
    scaling_list_items_with(ScalingParams::WEAR, viewport, density, rows, out);
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
    let pixels = density > 0.0;
    let to_px = |value: f32| if pixels { value * density } else { value };
    let round_px = |value: f32| if pixels { value.round() } else { value };
    let viewport_px = round_px(to_px(viewport));
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

fn odd_pixel(pixels: f32) -> f32 {
    let half = pixels * 0.5;
    half - half.floor()
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
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

fn segment(start: f32, sweep: f32, width: f32, inset: f32, alpha: f32) -> IndicatorSegment {
    if sweep <= 0.0 || inset <= 0.0 {
        return IndicatorSegment::Arc {
            start,
            sweep: 0.0,
            alpha: 0.0,
        };
    }
    if sweep < inset {
        let fill = sweep / inset;
        return IndicatorSegment::Dot {
            angle: start + sweep * 0.5,
            radius: width * 0.5 * fill,
            alpha: alpha * fill,
        };
    }
    IndicatorSegment::Arc {
        start: start + inset * 0.5,
        sweep: sweep - inset,
        alpha,
    }
}

#[cfg(test)]
#[path = "tests/round_scroll_indicator_tests.rs"]
mod tests;

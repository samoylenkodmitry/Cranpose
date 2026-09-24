//! Placing list rows the way a round watch scales them.
//!
//! A scaling list shrinks and fades its rows towards the top and bottom of the
//! display so the content follows the bezel. The surprising part, and the part
//! that is wrong in every from-scratch implementation, is that **the list does
//! not re-measure a scaled row**: the `LazyColumn` underneath stacks rows at
//! their FULL height, measures and scrolls in that space, and each row is then
//! moved and shrunk by a graphics layer over the top of it.
//!
//! # What that layer is told, and what it is not
//!
//! It is tempting to conclude that a row's drawn position therefore depends
//! only on the rows above it and never on how much any of them shrank. That is
//! true of the `LazyColumn`, and false of what you see.
//! `ScalingLazyColumnItemWrapper` sets
//!
//! ```text
//! translationY = startOffset(item, anchorType) - unadjustedStartOffset(item, anchorType)
//! ```
//!
//! and both halves come out of `ScalingLazyListState.layoutInfo`, which builds
//! its window by walking **outward from the centre item** with a cursor that
//! advances by each row's `ScalingLazyListItemInfo.size` — the scaled size,
//! `roundToInt(size * scale)` — plus the gap. Downward the next row starts at
//! that cursor; upward the cursor is the next row's bottom. So the drawn boxes
//! are stacked edge to edge **at their scaled sizes**, and the Nth row out does
//! depend on how much the N-1 rows between it and the centre shrank.
//!
//! The two accounts agree exactly for the centre row (scale 1, so its scaled
//! size is its full one) and for its immediate neighbours, and separate from
//! the second row out. How far they separate is the whole of what the rows in
//! between shrank, so it depends on the list: on six 52pt rows down a 454pt
//! watch the third row out is 8.5pt higher under this rule and comes fully on
//! screen where the full-height stack ran it off the bottom, while on the real
//! Settings list it is a device pixel of the bottom row's sliver at 192dp and
//! nothing at all at 227dp. The **shape** of the error is the part worth
//! keeping in mind: under the full-height stack the drawn boxes drift apart as
//! they shrink, and under this one they stay exactly one gap apart however
//! small they get.
//!
//! [`place_row`] therefore takes the **cursor**, not a slot in the unscaled
//! stack; [`PlacedRow::reported_height`] is what advances it; and
//! [`place_rows`] is the walk, because a per-row call cannot state a rule about
//! the row after it.
//!
//! The ramp itself is still stated on the row's FULL height at that cursor —
//! `calculateItemInfo` passes `itemStart .. itemStart + item.size` — so a row
//! is scaled by where its unshrunk box would fall and then pinned by whichever
//! edge faces the centre line.
//!
//! Derived from `androidx.wear.compose.foundation.lazy`
//! (`ScalingLazyListState.layoutInfo`, `ScalingLazyColumnItemWrapper`,
//! `calculateItemInfo`, `calculateScaleAndAlpha` and `convertToCenterOffset`,
//! disassembled out of compose-foundation 1.6.2), then checked against where
//! Compose puts rows on 454x454 and 384x384 displays.
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

/// AOSP's `ScalingParams`, as a value rather than six constants.
///
/// `ScalingLazyColumn` takes these as a parameter; the module-level constants
/// above are the defaults it supplies. Holding them in a struct is what lets a
/// caller turn scaling off (see [`ScalingParams::reduced_motion`]) without a
/// second code path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScalingParams {
    pub edge_scale: f32,
    pub edge_alpha: f32,
    pub min_element_height: f32,
    pub max_element_height: f32,
    pub min_transition_area: f32,
    pub max_transition_area: f32,
}

impl Default for ScalingParams {
    fn default() -> Self {
        Self::WEAR
    }
}

impl ScalingParams {
    /// `ScalingLazyColumnDefaults.scalingParams()`.
    pub const WEAR: Self = Self {
        edge_scale: EDGE_SCALE,
        edge_alpha: EDGE_ALPHA,
        min_element_height: MIN_ELEMENT_HEIGHT,
        max_element_height: MAX_ELEMENT_HEIGHT,
        min_transition_area: MIN_TRANSITION_AREA,
        max_transition_area: MAX_TRANSITION_AREA,
    };

    /// What Wear uses under `LocalReduceMotion`: both edge values forced to
    /// `1.0`, which disables scaling and fading entirely rather than damping
    /// them.
    pub const fn reduced_motion(self) -> Self {
        Self {
            edge_scale: 1.0,
            edge_alpha: 1.0,
            min_element_height: self.min_element_height,
            max_element_height: self.max_element_height,
            min_transition_area: self.min_transition_area,
            max_transition_area: self.max_transition_area,
        }
    }
}

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
    scale_and_alpha_with(ScalingParams::WEAR, viewport, top, bottom)
}

/// [`scale_and_alpha`] with the ramp's six knobs supplied.
pub fn scale_and_alpha_with(
    params: ScalingParams,
    viewport: f32,
    top: f32,
    bottom: f32,
) -> Option<ScaleAlpha> {
    if !viewport.is_finite() || !top.is_finite() || !bottom.is_finite() || bottom < top {
        return None;
    }
    if viewport <= 0.0 {
        return Some(ScaleAlpha::UNCHANGED);
    }
    let edge = (viewport - top).min(bottom) / viewport;
    let size_ratio = inverse_lerp(
        params.min_element_height,
        params.max_element_height,
        (bottom - top) / viewport,
    );
    let line = params.min_transition_area
        + (params.max_transition_area - params.min_transition_area) * size_ratio;
    if edge >= line || line <= 0.0 {
        return Some(ScaleAlpha::UNCHANGED);
    }
    let progress = ease(1.0 - edge / line);
    Some(ScaleAlpha {
        scale: 1.0 + (params.edge_scale - 1.0) * progress,
        alpha: 1.0 + (params.edge_alpha - 1.0) * progress,
    })
}

/// Where a row ends up once the list has scaled it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedRow {
    /// Top edge after the transform, in the unit `top` was given in.
    pub top: f32,
    /// Height after the transform.
    pub height: f32,
    /// The height the layout **reports** for this row:
    /// `ScalingLazyListItemInfo.size`, which is `roundToInt(size * scale)`.
    ///
    /// It is not [`Self::height`]. The graphics layer scales by the unrounded
    /// factor, so what is drawn is a fraction of a pixel different from what is
    /// reported — and it is the reported one that Wear stacks the next row
    /// against and that the scroll indicator divides by. Advance an outward
    /// walk by this plus the gap; see the module docs for why the walk stacks
    /// scaled sizes at all.
    pub reported_height: f32,
    pub scale: f32,
    pub alpha: f32,
}

/// Places a row the way a scaling list places one.
///
/// `top` is the outward walk's cursor for this row — the drawn bottom edge of
/// the row between it and the centre, plus the gap — and `height` is its full,
/// unscaled height. It is **not** the row's slot in a stack of full heights;
/// the two agree only out to the centre row's immediate neighbours. See the
/// module docs.
///
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
    place_row_with(ScalingParams::WEAR, viewport, top, height, density)
}

/// [`place_row`] with the ramp's six knobs supplied.
pub fn place_row_with(
    params: ScalingParams,
    viewport: f32,
    top: f32,
    height: f32,
    density: f32,
) -> Option<PlacedRow> {
    if !height.is_finite() || height < 0.0 || !density.is_finite() {
        return None;
    }
    if density <= 0.0 {
        let transform = scale_and_alpha_with(params, viewport, top, top + height)?;
        let scaled = height * transform.scale;
        return Some(PlacedRow {
            top,
            height: scaled,
            reported_height: scaled,
            scale: transform.scale,
            alpha: transform.alpha,
        });
    }
    let viewport_px = (viewport * density).round();
    let top_px = (top * density).round();
    let height_px = (height * density).round();
    let transform = scale_and_alpha_with(params, viewport_px, top_px, top_px + height_px)?;
    let scaled_px = (height_px * transform.scale).round();
    let above = top_px + top_px + height_px < viewport_px;
    let pinned = if above {
        top_px + height_px - scaled_px
    } else {
        top_px
    };
    Some(PlacedRow {
        top: (pinned + odd_pixel(height_px) - odd_pixel(scaled_px)) / density,
        height: height_px * transform.scale / density,
        reported_height: scaled_px / density,
        scale: transform.scale,
        alpha: transform.alpha,
    })
}

/// Everything about a scaling list that is the same for all of its rows.
///
/// Held together rather than passed one by one because [`place_rows_with`]
/// walks a run and every one of these is a property of the run, not of a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowRun {
    /// The list's full height, which is what the ramp is stated against.
    pub viewport: f32,
    /// Which row the walk starts from — `ScalingLazyListState.centerItemIndex`.
    pub anchor: usize,
    /// Where the anchored row's own box starts. This is the one position the
    /// unscaled stack and the walk always agree on: the anchored row is never
    /// scaled, so its cursor and its slot are the same number.
    pub anchor_top: f32,
    /// `Arrangement.spacedBy`, between every pair of drawn boxes.
    pub gap: f32,
    /// Device pixels per unit; `0.0` works in continuous coordinates.
    pub density: f32,
}

/// Places a whole run of rows the way a scaling list places one, walking
/// **outward from the anchored row**.
///
/// This is the shape the rule actually has. [`place_row`] answers for one row
/// given its cursor, and the cursor for the row after it is
/// `PlacedRow::reported_height + gap` further out — never the full height — so
/// a per-row call cannot state the rule on its own and a caller that stacks
/// full heights gets a list that drifts. See the module docs.
///
/// `out` is cleared first and comes back one entry per height, in list order.
pub fn place_rows_with(
    params: ScalingParams,
    run: RowRun,
    heights: &[f32],
    out: &mut Vec<PlacedRow>,
) {
    out.clear();
    if heights.is_empty() {
        return;
    }
    let anchor = run.anchor.min(heights.len() - 1);
    let unscaled = |top: f32, height: f32| PlacedRow {
        top,
        height,
        reported_height: height,
        scale: 1.0,
        alpha: 1.0,
    };
    out.resize(heights.len(), unscaled(0.0, 0.0));
    let place = |top: f32, height: f32| {
        place_row_with(params, run.viewport, top, height, run.density)
            .unwrap_or_else(|| unscaled(top, height))
    };

    let mut cursor = run.anchor_top;
    for (index, &height) in heights.iter().enumerate().skip(anchor) {
        let row = place(cursor, height);
        cursor += height + run.gap;
        out[index] = row;
    }
    let mut bottom = run.anchor_top;
    for index in (0..anchor).rev() {
        let height = heights[index];
        bottom -= run.gap;
        let row = place(bottom - height, height);
        bottom -= height;
        out[index] = row;
    }
}

/// [`place_rows_with`] under Wear's own ramp.
pub fn place_rows(run: RowRun, heights: &[f32], out: &mut Vec<PlacedRow>) {
    place_rows_with(ScalingParams::WEAR, run, heights, out);
}

/// A row's unscaled place in the column: where it would sit and how tall it is
/// with nothing scaled.
///
/// This is the coordinate space the whole module works in. [`place_row`] turns
/// a slot into the transformed rectangle that is actually drawn; the slot
/// itself never moves because a row shrank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub top: f32,
    pub height: f32,
}

impl Slot {
    pub fn centre(self) -> f32 {
        self.top + self.height * 0.5
    }

    pub fn bottom(self) -> f32 {
        self.top + self.height
    }
}

/// Stacks row heights into slots, `gap` apart, starting at zero.
///
/// The stack is of FULL heights — that is the invariant the whole scaling model
/// rests on, and stacking scaled heights instead is the mistake this module
/// exists to prevent.
pub fn stack_into(heights: impl IntoIterator<Item = f32>, gap: f32, out: &mut Vec<Slot>) {
    out.clear();
    let mut cursor = 0.0;
    for height in heights {
        out.push(Slot {
            top: cursor,
            height,
        });
        cursor += height + gap;
    }
}

/// Which item the list holds on its centre line, and by how much it is offset.
///
/// This is `ScalingLazyListState`'s coordinate pair — `centerItemIndex` plus
/// `centerItemScrollOffset` — under the default `ScalingLazyListAnchorType.ItemCenter`,
/// where the anchored point is the item's centre rather than its top edge.
/// A positive `offset` scrolls the content up, the same sign as a scroll
/// position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CentreAnchor {
    pub index: usize,
    pub offset: f32,
}

impl Default for CentreAnchor {
    fn default() -> Self {
        Self {
            index: 1,
            offset: 0.0,
        }
    }
}

/// A length moved onto the whole device pixel Compose would give it.
///
/// Compose's layout is integral — `Dp.roundToPx()` runs before anything is
/// measured and children are placed at an `IntOffset` — and Kotlin's
/// `roundToInt` sends an exact half **up**, not away from zero. Rust's
/// `f32::round` disagrees on exactly the negative halves, which is the case a
/// scroll offset reaches.
pub fn round_to_px(value: f32, density: f32) -> f32 {
    if density <= 0.0 || !density.is_finite() || !value.is_finite() {
        return value;
    }
    (value * density + 0.5).floor() / density
}

/// How far the whole column must move so the anchored item sits on the centre
/// line — Wear's `autoCentering`, as one shift rather than two spacers.
///
/// Wear expresses this by injecting a `Spacer` before and after the content
/// (see [`auto_centring_spacers`]), which is the same arithmetic seen from the
/// other side: with the leading spacer un-clamped, the content offset it
/// produces is exactly this shift. Returning the shift lets a caller place rows
/// directly instead of measuring two phantom items.
///
/// The result is rounded to a whole device pixel, because the `LazyColumn`
/// underneath holds its scroll position as a whole number of pixels: a float
/// delta is rounded before it is applied and the remainder carried, so every
/// item top stays integral. Rounding once here does that for the whole column.
/// Pass `density <= 0.0` to work in continuous coordinates.
pub fn centre_offset(slots: &[Slot], viewport: f32, anchor: CentreAnchor, density: f32) -> f32 {
    let Some(slot) = slots.get(anchor.index).or_else(|| slots.last()) else {
        return 0.0;
    };
    round_to_px(viewport * 0.5 - slot.centre() - anchor.offset, density)
}

/// [`centre_offset`] for a caller that holds its scroll position as a
/// fractional item index rather than an index and a pixel offset.
///
/// `scroll` of `2.5` centres the point halfway between the third and fourth
/// items' centres. This is the shape an app that scrolls by whole rows wants,
/// and it interpolates between item *centres* rather than tops so a tall row
/// next to a short one does not accelerate through the middle.
pub fn centre_offset_at(slots: &[Slot], viewport: f32, scroll: f32, density: f32) -> f32 {
    if slots.is_empty() {
        return 0.0;
    }
    let scroll = if scroll.is_finite() { scroll } else { 0.0 };
    let whole = (scroll.floor().max(0.0) as usize).min(slots.len() - 1);
    let fraction = (scroll - whole as f32).clamp(0.0, 1.0);
    let mut anchor = slots[whole].centre();
    if let Some(next) = slots.get(whole + 1) {
        anchor += (next.centre() - anchor) * fraction;
    }
    round_to_px(viewport * 0.5 - anchor, density)
}

/// Moves every slot by `offset`.
pub fn shift(slots: &mut [Slot], offset: f32) {
    for slot in slots.iter_mut() {
        slot.top += offset;
    }
}

/// The two spacer heights Wear's `autoCentering` injects around the content.
///
/// Wear does not shift the column; it inserts a `Spacer` item before all
/// content and another after it, which is why `totalItemsCount` is two less
/// than the `LazyColumn`'s and every public index is one higher. Both are
/// reproduced here because the numbers differ from the plain shift in two
/// places that show on screen:
///
/// - the leading spacer is clamped at zero, so the anchored item cannot be
///   pushed *below* the centre line by a short list;
/// - the centre line is `floor(viewport / 2)` on an integer pixel grid, so an
///   odd viewport gives its spare pixel to the trailing spacer.
///
/// `viewport` and the slot geometry are in device pixels here, not points —
/// that is the space Wear does this arithmetic in.
pub fn auto_centring_spacers(slots: &[Slot], viewport_px: f32, anchor: CentreAnchor) -> (f32, f32) {
    let leading = slots
        .get(anchor.index)
        .or_else(|| slots.last())
        .map_or(0.0, |slot| {
            leading_auto_centring_spacer(viewport_px, slot.centre(), anchor.offset)
        });
    let trailing = slots.last().map_or(0.0, |slot| {
        trailing_auto_centring_spacer(viewport_px, slot.height)
    });
    (leading, trailing)
}

/// The leading `autoCentering` spacer, for a caller holding the anchored row
/// rather than the stack it came from.
///
/// `anchor_centre_px` is that row's centre measured from the top of the
/// content, which is where [`stack_into`] puts it. See
/// [`auto_centring_spacers`] for what the two spacers are and why the clamp
/// and the floored centre line matter.
pub fn leading_auto_centring_spacer(
    viewport_px: f32,
    anchor_centre_px: f32,
    anchor_offset: f32,
) -> f32 {
    ((viewport_px * 0.5).floor() - anchor_offset - anchor_centre_px).max(0.0)
}

/// The trailing `autoCentering` spacer, which depends only on the last row's
/// height: `unadjustedSizeBelowOffsetPoint` under `ItemCenter` is half of it.
pub fn trailing_auto_centring_spacer(viewport_px: f32, last_height_px: f32) -> f32 {
    (viewport_px - (viewport_px * 0.5).floor() - last_height_px * 0.5).max(0.0)
}

/// The stretch of content a scaling list can hold on its centre line.
///
/// Both ends are content coordinates — the same space [`stack_into`] stacks
/// slots in — and the pair is what a scroll position has to be kept inside.
///
/// **It is not "the anchored row centred" to "the last row centred".** The
/// `LazyColumn` underneath takes its `contentPadding` OUTSIDE both auto-centring
/// spacers, so the padding is scroll the list can spend at each end. At the
/// bottom that means the last row settles `after_padding` **above** the centre
/// line rather than on it; at the top the anchored row can be pulled
/// `before_padding` **below** it. Wear says the same thing from the other side
/// in `ScalingLazyListState.scrollToItem`, which scrolls the `LazyColumn` to
/// `beforeContentPaddingPx - viewportCenterLinePx` to put a row on the line —
/// so the list is already `before_padding` in from its own top when it opens,
/// and a port that stops at the two centred rows cannot reach either end.
///
/// Clamping at the two centred rows costs behaviour and not only pixels: it is
/// the difference between a user reaching the last row of a settings list and
/// not reaching it.
///
/// Both spacers are clamped at zero in Wear (see [`auto_centring_spacers`]), and
/// this states the travel for a list where neither clamp bit — a list long
/// enough to scroll with a leading spacer left. On one clamped at either end the
/// true travel is shorter at that end.
pub fn anchor_travel(
    anchor_centre: f32,
    last_centre: f32,
    before_padding: f32,
    after_padding: f32,
) -> (f32, f32) {
    let end = last_centre + after_padding;
    let start = anchor_centre - before_padding;
    (start.min(end), end)
}

fn odd_pixel(pixels: f32) -> f32 {
    let half = pixels * 0.5;
    half - half.floor()
}

fn inverse_lerp(start: f32, stop: f32, value: f32) -> f32 {
    ((value - start) / (stop - start)).clamp(0.0, 1.0)
}

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
#[path = "tests/round_scaling_list_tests.rs"]
mod tests;

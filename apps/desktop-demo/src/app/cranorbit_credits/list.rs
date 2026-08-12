use super::draw::Painter;
use super::font::{draw_text, text_width, Align, Face, Typography};
use super::layout::Slot;
use super::rows::{RowKind, SettingsRow};
use super::theme::{ColorScheme, GamePalette};
use cranpose_ui_graphics::{Color, StrokeCap};

pub const ITEM_GAP_DP: f32 = 4.0;
pub const HEADER_MIN_DP: f32 = 48.0;
pub const HEADER_TOP_DP: f32 = 16.0;
pub const HEADER_BOTTOM_DP: f32 = 12.0;
pub const HEADER_SIDE_DP: f32 = 14.0;
pub const BUTTON_MIN_DP: f32 = 52.0;
pub const BUTTON_SIDE_DP: f32 = 14.0;
pub const BUTTON_PAD_Y_DP: f32 = 6.0;
pub const SWITCH_PAD_Y_DP: f32 = 8.0;
pub const LABEL_SPACER_DP: f32 = 1.0;
pub const CORNER_DP: f32 = 26.0;
pub const SWITCH_WIDTH_DP: f32 = 32.0;
pub const SWITCH_HEIGHT_DP: f32 = 22.0;
pub const SWITCH_SPACING_DP: f32 = 6.0;
pub const SWITCH_TRACK_DP: f32 = 2.0;
pub const THUMB_UNCHECKED_DP: f32 = 6.0;
pub const THUMB_CHECKED_DP: f32 = 9.0;
pub const TICK_STROKE_DP: f32 = 2.0;
pub const TICK_BASE: [f32; 4] = [-4.6, 1.0, -2.1, 3.5];
pub const TICK_STICK: [f32; 4] = [-1.5, 3.1, 4.5, -2.9];

pub const EDGE_SCALE: f32 = 0.7;
pub const EDGE_ALPHA: f32 = 0.5;
pub const MIN_ELEMENT_HEIGHT: f32 = 0.2;
pub const MAX_ELEMENT_HEIGHT: f32 = 0.6;
pub const MIN_TRANSITION_AREA: f32 = 0.35;
pub const MAX_TRANSITION_AREA: f32 = 0.55;
pub const CENTRED_ITEM: usize = 1;

pub const MAX_WRAP_LINES: usize = 8;

// Wear Material 3's ScrollIndicator, in its own numbers. Read out of
// `androidx.wear.compose.material3` 1.6.2 with `javap -c`
// (`ScrollIndicatorDefaults`, `PaddingDefaults`) and each one checked against
// where the shipping build actually puts pixels on a 454px round display.

/// How far the track reaches up and down from 3 o'clock, as a straight-line
/// height rather than an arc length — Wear's `indicatorHeight`.
pub const INDICATOR_HEIGHT_DP: f32 = 50.0;
/// Stroke width — Wear's `indicatorWidth`, which is 6dp on a large screen and
/// 5dp otherwise. Wear's own breakpoint is a screen at least 225dp across.
pub const INDICATOR_WIDTH_DP: f32 = 6.0;
pub const INDICATOR_NARROW_WIDTH_DP: f32 = 5.0;
pub const INDICATOR_LARGE_SCREEN_DP: f32 = 225.0;
/// How far the track's outer edge stays off the display edge — Wear's
/// `PaddingDefaults.edgePadding`.
pub const INDICATOR_EDGE_PADDING_DP: f32 = 2.0;
/// The blank Wear leaves between the thumb and each end of the track —
/// `ScrollIndicatorDefaults.gapHeight`. The indicator is three separate
/// segments, not a thumb painted over a continuous track.
pub const INDICATOR_GAP_DP: f32 = 3.0;
/// The thumb's share of the track is clamped to this range — Wear's
/// `minSizeFraction` and `maxSizeFraction`.
pub const INDICATOR_MIN_THUMB: f32 = 0.3;
pub const INDICATOR_MAX_THUMB: f32 = 0.7;
/// L\* for the thumb and for the track behind it. Wear takes one colour —
/// `onBackground` — to two lightnesses rather than laying alpha over the
/// background.
pub const INDICATOR_THUMB_LUMINANCE: f32 = 80.0;
pub const INDICATOR_TRACK_LUMINANCE: f32 = 20.0;

/// The stroke width Wear would use on a display this size.
pub fn indicator_width_dp(display_dp: f32) -> f32 {
    if display_dp >= INDICATOR_LARGE_SCREEN_DP {
        INDICATOR_WIDTH_DP
    } else {
        INDICATOR_NARROW_WIDTH_DP
    }
}

/// Where the track's centreline sits and how far it sweeps, given the display
/// radius in dp.
///
/// Wear describes the track by its height in dp, so the angle it covers
/// depends on the radius it is drawn at; deriving it here rather than storing
/// an angle keeps the indicator the same size in millimetres on every watch.
pub fn indicator_arc(radius_dp: f32) -> (f32, f32, f32) {
    let width = indicator_width_dp(radius_dp * 2.0);
    let centreline = radius_dp - INDICATOR_EDGE_PADDING_DP - width * 0.5;
    let half_height = INDICATOR_HEIGHT_DP * 0.5;
    let half_sweep = (half_height / centreline).clamp(-1.0, 1.0).asin();
    (centreline / radius_dp, width / radius_dp, half_sweep)
}

/// Where the thumb sits inside the track, and how long it is — both as
/// fractions of the track, matching what Wear Material's ScrollIndicator
/// derives from a list's scroll state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorGeometry {
    /// Thumb length as a share of the whole track, never below
    /// [`INDICATOR_MIN_THUMB`].
    pub thumb: f32,
    /// Position of the thumb's leading edge, `0.0` at the top of the track and
    /// `1.0 - thumb` at the bottom.
    pub offset: f32,
}

/// Works out the indicator for a laid-out list, or `None` when everything fits
/// on screen and Wear would show nothing at all.
///
/// `slots` are the laid-out rows in screen coordinates and `viewport` is the
/// display height, so this reads the same geometry the rows were drawn from
/// rather than a second, drifting copy of the scroll position.
pub fn indicator_geometry(slots: &[Slot], viewport: f32, gap: f32) -> Option<IndicatorGeometry> {
    let first = slots.first()?;
    let last = slots.last()?;
    // The scaling list centres the first and last rows rather than aligning
    // them with the display edges, so the distance the content can travel is
    // measured between those two centres — that is exactly the range the
    // scroll position moves over.
    let travel = last.centre() - first.centre();
    if travel <= gap {
        return None;
    }
    let content = last.top + last.height - first.top;
    if viewport >= content {
        return None;
    }
    let thumb = (viewport / content).clamp(INDICATOR_MIN_THUMB, INDICATOR_MAX_THUMB);
    let scrolled = (viewport * 0.5 - first.centre()) / travel;
    let offset = scrolled.clamp(0.0, 1.0) * (1.0 - thumb);
    Some(IndicatorGeometry { thumb, offset })
}

/// Draws the Wear scroll indicator on the right edge of the display.
pub fn draw_scroll_indicator(
    painter: &mut Painter,
    geometry: IndicatorGeometry,
    alpha: f32,
    scheme: &ColorScheme,
) {
    if alpha <= 0.002 {
        return;
    }
    let (unit_radius, unit_width, half_sweep) = indicator_arc(painter.radius);
    let sweep = half_sweep * 2.0;
    let top = -half_sweep;
    let track = super::theme::with_luminance(scheme.on_background, INDICATOR_TRACK_LUMINANCE);
    let thumb = super::theme::with_luminance(scheme.on_background, INDICATOR_THUMB_LUMINANCE);
    // Wear's `drawCurvedIndicator` lays down three segments -- track, thumb,
    // track -- rather than a thumb over a full-length track, and leaves
    // `gapHeight` of nothing on either side of the thumb. On a list scrolled
    // near an end the short segment is what gives the indicator its little
    // detached dot.
    let gap = INDICATOR_GAP_DP / (unit_radius * painter.radius);
    let thumb_start = top + sweep * geometry.offset;
    let thumb_sweep = sweep * geometry.thumb;
    let above = thumb_start - gap - top;
    let below_start = thumb_start + thumb_sweep + gap;
    let below = top + sweep - below_start;
    let cap = unit_width / unit_radius;
    indicator_segment(
        painter,
        unit_radius,
        top,
        above,
        unit_width,
        cap,
        track,
        alpha,
    );
    indicator_segment(
        painter,
        unit_radius,
        thumb_start,
        thumb_sweep,
        unit_width,
        cap,
        thumb,
        alpha,
    );
    indicator_segment(
        painter,
        unit_radius,
        below_start,
        below,
        unit_width,
        cap,
        track,
        alpha,
    );
}

/// One piece of the scroll indicator.
///
/// Wear's `drawIndicatorSegment` does not draw an arc shorter than its own
/// stroke: below that it swaps to a circle whose radius and alpha both scale
/// with how much sweep is left, so a segment shrinking to nothing dwindles to a
/// dot and fades out instead of stopping abruptly at one round cap's width.
#[allow(clippy::too_many_arguments)]
fn indicator_segment(
    painter: &mut Painter,
    unit_radius: f32,
    start: f32,
    sweep: f32,
    unit_width: f32,
    cap_sweep: f32,
    color: Color,
    alpha: f32,
) {
    if sweep <= 0.0 || cap_sweep <= 0.0 {
        return;
    }
    if sweep < cap_sweep {
        let fill = sweep / cap_sweep;
        let middle = start + sweep * 0.5;
        painter.dot(
            unit_radius * middle.cos(),
            unit_radius * middle.sin(),
            unit_width * 0.5 * fill,
            color.with_alpha(alpha * fill),
        );
        return;
    }
    // Wear draws the arc a half-cap short at each end and lets the round caps
    // fill it back out, so a segment covers exactly `start ..= start + sweep`.
    // Drawing the full sweep instead grows every segment by a whole stroke
    // width, which is more than the gap Wear leaves and closes it up.
    painter.arc(
        unit_radius,
        start + cap_sweep * 0.5,
        sweep - cap_sweep,
        unit_width,
        StrokeCap::Round,
        color.with_alpha(alpha),
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleAlpha {
    pub scale: f32,
    pub alpha: f32,
}

/// Where a row is drawn, and how faded, once the scaling list has had its say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedRow {
    pub top: f32,
    pub height: f32,
    pub scale: f32,
    pub alpha: f32,
}

/// Places a row the way `ScalingLazyColumn` places one.
///
/// Wear's list does not re-measure a scaled item. The `LazyColumn` underneath
/// stacks items at their full height, and `ScalingLazyColumnItemWrapper` draws
/// each through a graphics layer with `transformOrigin` on the item's top edge
/// and a `translationY` that pins whichever edge faces the centre line — the
/// bottom for an item above the line, the top for one below it. So a row's
/// position depends only on the rows above it, not on how much they shrank.
///
/// What that translation is built from is integral, and that is the part worth
/// keeping. `ScalingLazyColumnMeasureKt` rounds the scaled height to a whole
/// pixel (`roundToInt(size * scale)`), and it reaches the drawn offset through
/// `convertToCenterOffset`, which halves a size with *integer* division, while
/// `startOffset` halves the same size in floating point — so an item whose
/// pixel height is odd carries exactly half a pixel. Do the same arithmetic in
/// dp and a row edge lands mid-pixel, where the rasterizer antialiases a
/// boundary the Compose build draws crisp: a whole scanline of near-miss
/// pixels for every edge, on every scaled row.
///
/// `top` and `height` are in dp; the answer is too.
pub fn place_row(style: &ListStyle, top: f32, height: f32) -> PlacedRow {
    let density = style.typography.density();
    if density <= 0.0 {
        let transform = scale_and_alpha(style.height, top, top + height);
        return PlacedRow {
            top,
            height: height * transform.scale,
            scale: transform.scale,
            alpha: transform.alpha,
        };
    }
    let viewport_px = (style.height * density).round();
    let top_px = (top * density).round();
    let height_px = (height * density).round();
    let transform = scale_and_alpha(viewport_px, top_px, top_px + height_px);
    let scaled_px = (height_px * transform.scale).round();
    // Wear's `isAboveLine`, on the same integers it uses.
    let above = top_px + top_px + height_px < viewport_px;
    let pinned = if above {
        top_px + height_px - scaled_px
    } else {
        top_px
    };
    PlacedRow {
        top: (pinned + odd_pixel(height_px) - odd_pixel(scaled_px)) / density,
        height: height_px * transform.scale / density,
        scale: transform.scale,
        alpha: transform.alpha,
    }
}

/// Half a pixel when a pixel height is odd, and nothing when it is even —
/// what Compose's integer halving leaves behind next to its floating-point one.
fn odd_pixel(pixels: f32) -> f32 {
    let half = pixels * 0.5;
    half - half.floor()
}

pub fn scale_and_alpha(viewport: f32, top: f32, bottom: f32) -> ScaleAlpha {
    if viewport <= 0.0 {
        return ScaleAlpha {
            scale: 1.0,
            alpha: 1.0,
        };
    }
    let edge = (viewport - top).min(bottom) / viewport;
    let size_ratio = inverse_lerp(
        MIN_ELEMENT_HEIGHT,
        MAX_ELEMENT_HEIGHT,
        (bottom - top) / viewport,
    );
    let line = MIN_TRANSITION_AREA + (MAX_TRANSITION_AREA - MIN_TRANSITION_AREA) * size_ratio;
    if edge >= line || line <= 0.0 {
        return ScaleAlpha {
            scale: 1.0,
            alpha: 1.0,
        };
    }
    let progress = ease(1.0 - edge / line);
    ScaleAlpha {
        scale: 1.0 + (EDGE_SCALE - 1.0) * progress,
        alpha: 1.0 + (EDGE_ALPHA - 1.0) * progress,
    }
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

/// A Compose `CubicBezierEasing(a, b, c, d)` evaluated at `fraction`.
///
/// Compose's easing curves are parametric, so the answer is the curve's y at
/// the parameter whose x is `fraction` — found by bisection, the same way the
/// scaling-list easing above does it.
pub fn cubic_bezier_easing(fraction: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    let x = fraction.clamp(0.0, 1.0);
    let mut low = 0.0f32;
    let mut high = 1.0f32;
    let mut t = x;
    for _ in 0..12 {
        if bezier(t, a, c) < x {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) * 0.5;
    }
    bezier(t, b, d)
}

pub struct ListStyle {
    pub typography: Typography,
    pub side_padding: f32,
    pub width: f32,
    pub height: f32,
}

impl ListStyle {
    pub fn new(typography: Typography, side_dp: f32, width: f32, height: f32) -> Self {
        Self {
            typography,
            side_padding: typography.dp(side_dp),
            width,
            height,
        }
    }

    pub fn item_width(&self) -> f32 {
        self.width - self.side_padding * 2.0
    }

    pub fn item_left(&self) -> f32 {
        self.side_padding
    }

    pub fn gap(&self) -> f32 {
        self.typography.dp_px(ITEM_GAP_DP)
    }
}

pub fn row_height(painter: &Painter, style: &ListStyle, row: &SettingsRow) -> f32 {
    let typography = style.typography;
    match row.kind {
        RowKind::Header => {
            let block = typography.list_header().block_height();
            (typography.dp_px(HEADER_TOP_DP) + block + typography.dp_px(HEADER_BOTTOM_DP))
                .max(typography.dp_px(HEADER_MIN_DP))
        }
        RowKind::Label => {
            let face = typography.list_body();
            wrapped_lines(painter, &row.label, face, style.item_width()) as f32
                * face.block_height()
        }
        RowKind::Toggle | RowKind::Action => {
            let pad = if row.kind == RowKind::Toggle {
                typography.dp_px(SWITCH_PAD_Y_DP)
            } else {
                typography.dp_px(BUTTON_PAD_Y_DP)
            };
            let label = typography.button_label();
            let secondary = typography.button_secondary();
            let available = label_width(style, row.kind);
            let mut content =
                wrapped_lines(painter, &row.label, label, available) as f32 * label.block_height();
            if !row.value.is_empty() {
                content += typography.dp_px(LABEL_SPACER_DP)
                    + wrapped_lines(painter, &row.value, secondary, available) as f32
                        * secondary.block_height();
            }
            (content + pad * 2.0).max(typography.dp_px(BUTTON_MIN_DP))
        }
    }
}

fn label_width(style: &ListStyle, kind: RowKind) -> f32 {
    let typography = style.typography;
    let side = typography.dp(BUTTON_SIDE_DP) * 2.0;
    if kind == RowKind::Toggle {
        style.item_width()
            - side
            - typography.dp(SWITCH_WIDTH_DP)
            - typography.dp(SWITCH_SPACING_DP)
    } else {
        style.item_width() - side
    }
}

pub fn wrapped_lines(painter: &Painter, text: &str, face: Face, width: f32) -> usize {
    if text.is_empty() {
        return 1;
    }
    let mut lines = 1usize;
    let mut start = 0usize;
    let mut end = 0usize;
    for (index, word) in text.split(' ').enumerate() {
        let word_end = if index == 0 {
            word.len()
        } else {
            end + 1 + word.len()
        };
        if index > 0 && text_width(painter, &text[start..word_end], face) > width {
            lines += 1;
            start = end + 1;
        }
        end = word_end;
        if lines >= MAX_WRAP_LINES {
            break;
        }
    }
    lines
}

/// The character Compose appends when `TextOverflow.Ellipsis` cuts a line.
pub const ELLIPSIS: &str = "…";

/// A block laid out the way Compose lays one out with `maxLines` and
/// `TextOverflow.Ellipsis`.
///
/// The port draws its own text, so nothing here is inherited: without this a
/// caption that did not fit was simply cut mid-word, while the Kotlin build
/// showed the same text with a trailing ellipsis. At large font settings —
/// which is where `maxLines` starts to bite — that is a visible difference on
/// most screens.
pub struct WrappedBlock<'a> {
    /// Lines to draw, in order. The last one is `overflowed` when set.
    pub lines: [&'a str; MAX_WRAP_LINES],
    /// How many of `lines` are used.
    pub count: usize,
    /// Replacement for the last line when the text did not fit, ending in the
    /// ellipsis. Owned because it is not a slice of the input.
    pub overflowed: Option<String>,
}

impl<'a> WrappedBlock<'a> {
    /// The line at `index` as it should be drawn.
    pub fn line(&self, index: usize) -> &str {
        match (&self.overflowed, index + 1 == self.count) {
            (Some(text), true) => text.as_str(),
            _ => self.lines[index],
        }
    }
}

/// Wraps `text` to `width`, keeps at most `max_lines`, and ellipsizes the last
/// line when anything was left over — Compose's `maxLines` +
/// `TextOverflow.Ellipsis` pair.
pub fn wrap_ellipsized<'a>(
    painter: &Painter,
    text: &'a str,
    face: Face,
    width: f32,
    max_lines: usize,
) -> WrappedBlock<'a> {
    let mut lines: [&str; MAX_WRAP_LINES] = [""; MAX_WRAP_LINES];
    let produced = wrap_into(painter, text, face, width, &mut lines);
    let count = produced.min(max_lines.max(1));
    let dropped = produced > count;
    let last = lines[count - 1];
    // A single word longer than the column does not wrap, so overflow has to be
    // measured as well as counted.
    let too_wide = text_width(painter, last, face) > width;
    if !dropped && !too_wide {
        return WrappedBlock {
            lines,
            count,
            overflowed: None,
        };
    }
    // Compose fills the last line as far as it can and puts the ellipsis at the
    // end, so the tail is measured against the remaining text, not against the
    // wrapped line alone.
    let start = last.as_ptr() as usize - text.as_ptr() as usize;
    let remainder = &text[start..];
    let mut kept = 0usize;
    for (index, _) in remainder.char_indices().skip(1) {
        let candidate = format!("{}{ELLIPSIS}", &remainder[..index]);
        if text_width(painter, &candidate, face) > width {
            break;
        }
        kept = index;
    }
    let trimmed = remainder[..kept].trim_end();
    WrappedBlock {
        lines,
        count,
        overflowed: Some(format!("{trimmed}{ELLIPSIS}")),
    }
}

pub fn wrap_into<'a>(
    painter: &Painter,
    text: &'a str,
    face: Face,
    width: f32,
    out: &mut [&'a str; MAX_WRAP_LINES],
) -> usize {
    if text.is_empty() {
        out[0] = text;
        return 1;
    }
    let mut count = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    for (index, word) in text.split(' ').enumerate() {
        let word_end = if index == 0 {
            word.len()
        } else {
            end + 1 + word.len()
        };
        if index > 0 && text_width(painter, &text[start..word_end], face) > width {
            if count + 1 >= MAX_WRAP_LINES {
                break;
            }
            out[count] = &text[start..end];
            count += 1;
            start = end + 1;
        }
        end = word_end;
    }
    out[count] = &text[start..end];
    count + 1
}

pub fn layout_into(
    painter: &Painter,
    rows: &[SettingsRow],
    style: &ListStyle,
    scroll: f32,
    slots: &mut Vec<Slot>,
) {
    slots.clear();
    if rows.is_empty() {
        return;
    }
    let gap = style.gap();
    let mut cursor = 0.0;
    for row in rows.iter() {
        let height = row_height(painter, style, row);
        slots.push(Slot {
            top: cursor,
            height,
        });
        cursor += height + gap;
    }
    let whole = (scroll.floor().max(0.0) as usize).min(rows.len() - 1);
    let fraction = (scroll - whole as f32).clamp(0.0, 1.0);
    let mut anchor = slots[whole].centre();
    if let Some(next) = slots.get(whole + 1) {
        anchor += (next.centre() - anchor) * fraction;
    }
    // The `LazyColumn` under Wear's list holds its scroll position as a whole
    // number of pixels — a float delta is rounded before it is applied and the
    // remainder is carried, so every item top is integral. Rounding the shift
    // once does the same thing for the whole column, and keeps a row edge off
    // the half-pixel where it would be antialiased against a Compose build
    // that draws it crisp.
    let shift = style.typography.dp_px(style.height * 0.5 - anchor);
    for slot in slots.iter_mut() {
        slot.top += shift;
    }
}

pub fn draw_list(
    painter: &mut Painter,
    rows: &[SettingsRow],
    style: &ListStyle,
    slots: &[Slot],
    palette: &GamePalette,
) {
    let scheme = ColorScheme::of(palette);
    painter.fill_screen(palette.background);
    for (row, slot) in rows.iter().zip(slots.iter()) {
        if slot.top < style.height && slot.top + slot.height > 0.0 {
            draw_row(painter, row, style, slot.top, slot.height, &scheme);
        }
    }
}

fn draw_row(
    painter: &mut Painter,
    row: &SettingsRow,
    style: &ListStyle,
    top: f32,
    height: f32,
    scheme: &ColorScheme,
) {
    let placed = place_row(style, top, height);
    draw_row_at(
        painter,
        row,
        style,
        placed.top,
        placed.height,
        placed.scale,
        placed.alpha,
        scheme,
    );
}

/// One row at its natural size, with nothing scaled and nothing faded.
///
/// This is what a row draws when the scale and the alpha live on a graphics
/// layer instead of in the drawing. Wear's `ScalingLazyColumnItemWrapper`
/// renders each item once at full size and lets the layer carry both, which is
/// why its glyphs are rasterized at a single size wherever the row sits — and
/// why its scaled rows are visibly softer than a row redrawn at a smaller font.
///
/// The row occupies the full list width, exactly as it does on screen, so the
/// content still insets itself by the style's side padding. `top` is the local
/// origin, so a caller drawing into a row-sized layer passes `0.0`.
pub fn draw_row_natural(
    painter: &mut Painter,
    row: &SettingsRow,
    style: &ListStyle,
    top: f32,
    height: f32,
    scheme: &ColorScheme,
) {
    draw_row_at(painter, row, style, top, height, 1.0, 1.0, scheme);
}

#[allow(clippy::too_many_arguments)]
fn draw_row_at(
    painter: &mut Painter,
    row: &SettingsRow,
    style: &ListStyle,
    drawn_top: f32,
    drawn_height: f32,
    scale: f32,
    alpha: f32,
    scheme: &ColorScheme,
) {
    let typography = style.typography;
    let centre_x = style.width * 0.5;
    let left = centre_x - (centre_x - style.item_left()) * scale;
    let width = style.item_width() * scale;

    match row.kind {
        RowKind::Header => {
            let face = typography.list_header().scaled(scale);
            let block = face.block_height();
            let content = typography.dp(HEADER_TOP_DP) * scale
                + block
                + typography.dp(HEADER_BOTTOM_DP) * scale;
            let offset = (drawn_height - content) * 0.5;
            draw_text(
                painter,
                &row.label,
                centre_x,
                drawn_top + offset + typography.dp(HEADER_TOP_DP) * scale,
                face,
                super::theme::faded_by_layer(scheme.on_background, alpha),
                Align::Center,
            );
        }
        RowKind::Label => {
            let face = typography.list_body().scaled(scale);
            let mut lines: [&str; MAX_WRAP_LINES] = [""; MAX_WRAP_LINES];
            let count = wrap_into(painter, &row.label, face, width, &mut lines);
            let mut y = drawn_top;
            for line in lines.iter().take(count) {
                draw_text(
                    painter,
                    line,
                    centre_x,
                    y,
                    face,
                    super::theme::faded_by_layer(scheme.on_surface, alpha),
                    Align::Center,
                );
                y += face.block_height();
            }
        }
        RowKind::Toggle => {
            let container = if row.checked {
                scheme.primary_container
            } else {
                scheme.surface_container
            };
            let content = if row.checked {
                scheme.on_primary_container
            } else {
                scheme.on_surface
            };
            plate(
                painter,
                left,
                drawn_top,
                width,
                drawn_height,
                typography,
                scale,
                super::theme::faded_by_layer(container, alpha),
            );
            let available = label_width(style, RowKind::Toggle) * scale;
            labels(
                painter,
                row,
                style,
                left,
                drawn_top,
                drawn_height,
                typography.dp(SWITCH_PAD_Y_DP) * scale,
                available,
                scale,
                super::theme::faded_by_layer(content, alpha),
                super::theme::faded_by_layer(scheme.on_surface_variant, alpha),
            );
            switch(
                painter,
                style,
                left + width
                    - typography.dp(BUTTON_SIDE_DP) * scale
                    - typography.dp(SWITCH_WIDTH_DP) * scale,
                drawn_top + drawn_height * 0.5,
                scale,
                alpha,
                row.checked,
                scheme,
            );
        }
        RowKind::Action => {
            plate(
                painter,
                left,
                drawn_top,
                width,
                drawn_height,
                typography,
                scale,
                super::theme::faded_by_layer(scheme.primary, alpha),
            );
            let available = label_width(style, RowKind::Action) * scale;
            labels(
                painter,
                row,
                style,
                left,
                drawn_top,
                drawn_height,
                typography.dp(BUTTON_PAD_Y_DP) * scale,
                available,
                scale,
                super::theme::faded_by_layer(scheme.on_primary, alpha),
                super::theme::faded_by_layer(scheme.on_primary, alpha * 0.8),
            );
        }
    }
}

fn plate(
    painter: &mut Painter,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    typography: Typography,
    scale: f32,
    color: Color,
) {
    painter.round_rect(
        left,
        top,
        width,
        height,
        typography.dp(CORNER_DP) * scale,
        color,
    );
}

#[allow(clippy::too_many_arguments)]
fn labels(
    painter: &mut Painter,
    row: &SettingsRow,
    style: &ListStyle,
    left: f32,
    top: f32,
    height: f32,
    pad_y: f32,
    available: f32,
    scale: f32,
    label_color: Color,
    value_color: Color,
) {
    let typography = style.typography;
    let label = typography.button_label().scaled(scale);
    let secondary = typography.button_secondary().scaled(scale);
    let mut lines: [&str; MAX_WRAP_LINES] = [""; MAX_WRAP_LINES];
    let count = wrap_into(painter, &row.label, label, available, &mut lines);
    let mut content = count as f32 * label.block_height();
    let mut value: [&str; MAX_WRAP_LINES] = [""; MAX_WRAP_LINES];
    let mut value_count = 0usize;
    if !row.value.is_empty() {
        value_count = wrap_into(painter, &row.value, secondary, available, &mut value);
        content +=
            typography.dp(LABEL_SPACER_DP) * scale + value_count as f32 * secondary.block_height();
    }
    let text_left = left + typography.dp(BUTTON_SIDE_DP) * scale;
    let mut y = top + (height - content) * 0.5;
    let _ = pad_y;
    for line in lines.iter().take(count) {
        draw_text(
            painter,
            line,
            text_left,
            y,
            label,
            label_color,
            Align::Start,
        );
        y += label.block_height();
    }
    if value_count > 0 {
        y += typography.dp(LABEL_SPACER_DP) * scale;
        for line in value.iter().take(value_count) {
            draw_text(
                painter,
                line,
                text_left,
                y,
                secondary,
                value_color,
                Align::Start,
            );
            y += secondary.block_height();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn switch(
    painter: &mut Painter,
    style: &ListStyle,
    left: f32,
    centre_y: f32,
    scale: f32,
    alpha: f32,
    checked: bool,
    scheme: &ColorScheme,
) {
    let typography = style.typography;
    let width = typography.dp(SWITCH_WIDTH_DP) * scale;
    let height = typography.dp(SWITCH_HEIGHT_DP) * scale;
    let track = if checked {
        scheme.primary
    } else {
        scheme.surface_container
    };
    painter.round_rect(
        left,
        centre_y - height * 0.5,
        width,
        height,
        height * 0.5,
        super::theme::faded_by_layer(track, alpha),
    );
    if !checked {
        let stroke = typography.dp(SWITCH_TRACK_DP) * scale;
        painter.round_rect_stroked(
            left + stroke * 0.5,
            centre_y - height * 0.5 + stroke * 0.5,
            width - stroke,
            height - stroke,
            (height - stroke) * 0.5,
            stroke,
            super::theme::faded_by_layer(scheme.outline, alpha),
        );
    }
    let thumb_radius = typography.dp(if checked {
        THUMB_CHECKED_DP
    } else {
        THUMB_UNCHECKED_DP
    }) * scale;
    let padding = height * 0.5 - thumb_radius;
    let thumb_x = if checked {
        left + width - thumb_radius - padding
    } else {
        left + thumb_radius + padding
    };
    let thumb = if checked {
        scheme.primary_container
    } else {
        scheme.outline
    };
    painter.dot_px(
        thumb_x,
        centre_y,
        thumb_radius,
        super::theme::faded_by_layer(thumb, alpha),
    );
    if checked {
        let unit = typography.dp(1.0) * scale;
        let stroke = typography.dp(TICK_STROKE_DP) * scale;
        for segment in [TICK_BASE, TICK_STICK] {
            painter.line_px(
                thumb_x + segment[0] * unit,
                centre_y + segment[1] * unit,
                thumb_x + segment[2] * unit,
                centre_y + segment[3] * unit,
                stroke,
                StrokeCap::Round,
                super::theme::faded_by_layer(scheme.primary, alpha),
            );
        }
    }
}

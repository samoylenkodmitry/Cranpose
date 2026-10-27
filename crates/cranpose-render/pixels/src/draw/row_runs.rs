//! Which pixels of a row a shape can cover: the runs a fill, a stroke or an
//! arc band paints, before the coverage of each pixel in them.

use cranpose_ui_graphics::{
    ArcGeometry, CornerRadii, LineFrame, LineGeometry, Point, Rect, StrokeJoin, whole,
};

/// How far from a band's edge a pixel centre still takes ink: the
/// antialiasing reaches half a pixel, and the rest is room for the rounding
/// in the distance functions.
pub(super) const INK_REACH: f32 = 0.5 + 1.0 / 64.0;

/// The pixels of one row worth testing, as two runs `start..end`: the band on
/// either side of a hole. A run with `start >= end` is empty.
pub(super) type RowRuns = [(i32, i32); 2];

pub(super) const NO_RUN: (i32, i32) = (0, 0);

/// The pixels whose centres lie within `half` of `centre`, cut to
/// `min..max`.
fn run_within(centre: f32, half: f32, min: i32, max: i32) -> (i32, i32) {
    let start = (whole::ceil(centre - half - 0.5) as i32).max(min);
    let end = (whole::floor(centre + half - 0.5) as i32 + 1).min(max);
    (start, end)
}

/// `run` without the pixels whose centres lie in `from..=to`.
fn runs_around(run: (i32, i32), from: f32, to: f32) -> RowRuns {
    let first = whole::ceil(from - 0.5) as i32;
    let last = whole::floor(to - 0.5) as i32;
    if first > last {
        return [run, NO_RUN];
    }
    [(run.0, run.1.min(first)), (run.0.max(last + 1), run.1)]
}

// The row of a stroked segment that can take ink: the pixels whose centres
// lie within the half width of the segment's line, and within its half
// length and cap of its midpoint along it, each grown by `INK_REACH`.
pub(super) fn line_row_runs(
    line: &LineGeometry,
    frame: &LineFrame,
    y: f32,
    min_x: i32,
    max_x: i32,
) -> RowRuns {
    let dy = y - frame.center.y;
    let Point {
        x: along_x,
        y: along_y,
    } = frame.direction;
    // A pixel centre `dx` right of the midpoint lies `dx * along_y - dy *
    // along_x` across the line and `dx * along_x + dy * along_y` along it.
    let across = offsets_within(along_y, -dy * along_x, line.half_width + INK_REACH);
    let along = offsets_within(
        along_x,
        dy * along_y,
        frame.half_length + line.cap_reach() + INK_REACH,
    );
    let (Some(across), Some(along)) = (across, along) else {
        return [NO_RUN, NO_RUN];
    };
    let (low, high) = (across.0.max(along.0), across.1.min(along.1));
    if low.is_nan() || high.is_nan() || low > high {
        return [NO_RUN, NO_RUN];
    }
    let run = run_within(
        frame.center.x + (low + high) * 0.5,
        (high - low) * 0.5,
        min_x,
        max_x,
    );
    [run, NO_RUN]
}

// The offsets `dx` for which `|dx * slope + offset|` is at most `reach`:
// every one when the slope is zero and the offset within reach, none when
// it is not.
fn offsets_within(slope: f32, offset: f32, reach: f32) -> Option<(f32, f32)> {
    if slope == 0.0 {
        return (offset.abs() <= reach).then_some((f32::NEG_INFINITY, f32::INFINITY));
    }
    let (first, second) = ((-reach - offset) / slope, (reach - offset) / slope);
    Some((first.min(second), first.max(second)))
}

/// The row of an arc that can take ink. Every point of the band, caps
/// included, lies between the inner and the outer radius, so the row stops at
/// the outer circle and skips the inner one.
pub(super) fn arc_row_runs(arc: &ArcGeometry, y: f32, min_x: i32, max_x: i32) -> RowRuns {
    let dy = y - arc.center.y;
    let outer = arc.outer_radius + INK_REACH;
    let reach = outer * outer - dy * dy;
    if reach.is_nan() || reach <= 0.0 {
        return [NO_RUN, NO_RUN];
    }
    let run = run_within(arc.center.x, reach.sqrt(), min_x, max_x);
    let inner = arc.inner_radius - INK_REACH;
    let hole = inner * inner - dy * dy;
    if inner <= 0.0 || hole.is_nan() || hole <= 0.0 {
        return [run, NO_RUN];
    }
    let hole = hole.sqrt();
    runs_around(run, arc.center.x - hole, arc.center.x + hole)
}

/// The row of a stroked rounded rect that can take ink: inside the outer edge
/// grown by `INK_REACH`, and outside the part of the hole that lies at least
/// `INK_REACH` inside the inner edge. Both edges are the rounded boxes
/// `shape_sdf::sdf_stroked_rounded_rect` measures, taken on each side of the
/// centre with that side's corner radius.
pub(super) fn stroke_row_runs(
    rect: Rect,
    radii: Option<CornerRadii>,
    half_width: f32,
    join: StrokeJoin,
    y: f32,
    min_x: i32,
    max_x: i32,
) -> RowRuns {
    let half_width = half_width.max(0.0);
    let half_size = (rect.width * 0.5, rect.height * 0.5);
    let geometry = (
        (half_size.0 - half_width).max(0.0),
        (half_size.1 - half_width).max(0.0),
    );
    let outer = (geometry.0 + half_width, geometry.1 + half_width);
    let inner = (
        (geometry.0 - half_width).max(0.0),
        (geometry.1 - half_width).max(0.0),
    );
    let centre_x = rect.x + half_size.0;
    let local_y = (y - (rect.y + half_size.1)).abs();
    if !(local_y.is_finite() && centre_x.is_finite()) {
        return [(min_x, max_x), NO_RUN];
    }
    let radii = radii.unwrap_or_default();
    let (left, right) = if y - (rect.y + half_size.1) > 0.0 {
        (radii.bottom_left, radii.bottom_right)
    } else {
        (radii.top_left, radii.top_right)
    };
    // How far from the centre the grown outer edge reaches on one side;
    // `None` when the row passes outside it.
    let ink_reach = |radius: f32| {
        let radius = if join != StrokeJoin::Round && radius < 0.0001 {
            0.0
        } else {
            radius + half_width
        };
        let qy = local_y - outer.1 + radius;
        let grown = radius + INK_REACH;
        if qy.is_nan() || qy >= grown {
            return None;
        }
        Some(if qy <= 0.0 {
            outer.0 + INK_REACH
        } else {
            outer.0 - radius + (grown * grown - qy * qy).max(0.0).sqrt()
        })
    };
    // How far from the centre the hole is empty on one side, if it is.
    let empty_reach = |radius: f32| {
        let radius = (radius - half_width).max(0.0);
        let qy = local_y - inner.1 + radius;
        if qy.is_nan() || qy > radius - INK_REACH {
            return None;
        }
        let reach = if qy <= 0.0 {
            inner.0 - INK_REACH
        } else {
            let span = (radius - INK_REACH) * (radius - INK_REACH) - qy * qy;
            inner.0 - radius + span.max(0.0).sqrt()
        };
        (reach >= 0.0).then_some(reach)
    };
    // A side the row misses still keeps the centre's own column, which
    // belongs to the left side.
    let run = match (ink_reach(left), ink_reach(right)) {
        (None, None) => return [NO_RUN, NO_RUN],
        (left, right) => {
            let from = centre_x - left.unwrap_or(0.0);
            let to = centre_x + right.unwrap_or(0.0);
            let start = (whole::ceil(from - 0.5) as i32).max(min_x);
            let end = (whole::floor(to - 0.5) as i32 + 1).min(max_x);
            (start, end)
        }
    };
    match (empty_reach(left), empty_reach(right)) {
        (Some(left), Some(right)) => runs_around(run, centre_x - left, centre_x + right),
        (Some(left), None) => runs_around(run, centre_x - left, centre_x),
        // The right side starts past the centre: the centre's own column
        // belongs to the left one.
        (None, Some(right)) => runs_around(run, centre_x + 1.0 / 64.0, centre_x + right),
        (None, None) => [run, NO_RUN],
    }
}

/// A rounded rect to fill, with what every row of it shares worked out once.
pub(super) struct RoundedFill {
    rect: Rect,
    /// A pixel whose centre lies in `flat_left..=flat_right`, between the
    /// corners' widest reach on either side, is inside on every row of the
    /// rect: an end there needs no test.
    flat_left: f32,
    flat_right: f32,
    top_left: Corner,
    top_right: Corner,
    bottom_right: Corner,
    bottom_left: Corner,
}

/// A rounded corner: a point in its square, the one between the circle's
/// centre and the rect's corner, lies outside when it lies outside the
/// circle. A corner without a radius has its centre at infinity on the side
/// away from its square, so no point lies in the square.
#[derive(Clone, Copy)]
struct Corner {
    centre_x: f32,
    centre_y: f32,
    /// The square of the radius.
    reach: f32,
}

// The side of a corner's centre on which its square lies.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

impl Corner {
    /// `square` names the side of its centre on which the corner's square lies.
    fn new(radius: f32, centre_x: f32, centre_y: f32, square: Side) -> Self {
        if radius > 0.0 {
            return Self {
                centre_x,
                centre_y,
                reach: radius.powi(2),
            };
        }
        Self {
            centre_x: if square == Side::Left {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            },
            centre_y,
            reach: f32::INFINITY,
        }
    }

    fn excludes(self, x: f32, y: f32) -> bool {
        (x - self.centre_x).powi(2) + (y - self.centre_y).powi(2) > self.reach
    }
}

impl RoundedFill {
    pub(super) fn new(rect: Rect, radii: &CornerRadii) -> Self {
        let left = rect.x;
        let right = rect.x + rect.width;
        let top = rect.y;
        let bottom = rect.y + rect.height;
        Self {
            rect,
            flat_left: rect.x + radii.top_left.max(radii.bottom_left),
            flat_right: rect.x + rect.width - radii.top_right.max(radii.bottom_right),
            top_left: Corner::new(
                radii.top_left,
                left + radii.top_left,
                top + radii.top_left,
                Side::Left,
            ),
            top_right: Corner::new(
                radii.top_right,
                right - radii.top_right,
                top + radii.top_right,
                Side::Right,
            ),
            bottom_right: Corner::new(
                radii.bottom_right,
                right - radii.bottom_right,
                bottom - radii.bottom_right,
                Side::Right,
            ),
            bottom_left: Corner::new(
                radii.bottom_left,
                left + radii.bottom_left,
                bottom - radii.bottom_left,
                Side::Left,
            ),
        }
    }

    /// Whether a point of a row within the rect's height passes the left
    /// half of [`point_in_resolved_rounded_rect`]: the left side and the
    /// left corners, tested as that function tests them. Moving right, a
    /// point that passes keeps passing.
    fn clear_of_left(&self, x: f32, y: f32) -> bool {
        let (top_left, bottom_left) = (self.top_left, self.bottom_left);
        x >= self.rect.x
            && !(x < top_left.centre_x && y < top_left.centre_y && top_left.excludes(x, y))
            && !(x < bottom_left.centre_x && y > bottom_left.centre_y && bottom_left.excludes(x, y))
    }

    /// The right half of the test, as [`Self::clear_of_left`]: moving
    /// left, a point that passes keeps passing.
    fn clear_of_right(&self, x: f32, y: f32) -> bool {
        let (top_right, bottom_right) = (self.top_right, self.bottom_right);
        x <= self.rect.x + self.rect.width
            && !(x > top_right.centre_x && y < top_right.centre_y && top_right.excludes(x, y))
            && !(x > bottom_right.centre_x
                && y > bottom_right.centre_y
                && bottom_right.excludes(x, y))
    }

    /// The pixels of the row centred at `y` whose centres lie in the rounded
    /// rect, within `min_x..max_x`. The shape is convex, so they form one
    /// run: it starts at the first pixel clear of the left half and ends
    /// after the last one clear of the right half, and a pixel between
    /// passes both. When the first pixel clear of the left fails the right
    /// half, so does every pixel after it, and the run is empty. Out of
    /// line, so the row loop around a fill stays small enough to keep
    /// inline.
    #[inline(never)]
    pub(super) fn run(&self, y: f32, min_x: i32, max_x: i32) -> (i32, i32) {
        let rect = self.rect;
        if !(y >= rect.y && y <= rect.y + rect.height) {
            return NO_RUN;
        }
        let flat = |x: i32| (self.flat_left..=self.flat_right).contains(&(x as f32 + 0.5));
        let mut start = min_x;
        if !(start < max_x && flat(start)) {
            while start < max_x && !self.clear_of_left(start as f32 + 0.5, y) {
                start += 1;
            }
        }
        let mut end = max_x;
        if !(end > start && flat(end - 1)) {
            while end > start && !self.clear_of_right((end - 1) as f32 + 0.5, y) {
                end -= 1;
            }
        }
        (start, end)
    }
}

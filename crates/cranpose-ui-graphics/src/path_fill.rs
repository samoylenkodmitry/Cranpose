//! Path fills cut into vertical slices the shape stage draws on the GPU.
//!
//! A slice is the part of a fill between two of the path's edges over a run
//! of columns that no vertex and no crossing of edges falls inside, so both
//! of its edges are straight across it: a trapezoid with vertical sides.
//! Slices that meet side by side tile the fill. Each owns the pixels whose
//! centres lie in its columns, so only the fill's outline is anti-aliased
//! and no seam shows between slices.

use crate::{PathFillRule, Point, Rect};

/// One vertical slice of a filled path: the area between a top and a bottom
/// edge, each straight, over the columns from `left` to `right`.
///
/// A side that the next slice of the same fill continues past is shared:
/// a pixel there belongs to the slice whose columns hold its centre, and
/// the side is drawn hard. An open side is part of the fill's outline, and
/// its pixels take the share of them it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trapezoid {
    pub left: f32,
    pub right: f32,
    /// The top edge's y at `left` and at `right`.
    pub top: [f32; 2],
    /// The bottom edge's y at `left` and at `right`.
    pub bottom: [f32; 2],
    pub open_left: bool,
    pub open_right: bool,
}

impl Trapezoid {
    /// The box of its corners.
    pub fn bounds(&self) -> Rect {
        let top = self.top[0].min(self.top[1]);
        let bottom = self.bottom[0].max(self.bottom[1]);
        Rect {
            x: self.left,
            y: top,
            width: self.right - self.left,
            height: bottom - top,
        }
    }

    /// The slice moved by `dx`, `dy`.
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        Self {
            left: self.left + dx,
            right: self.right + dx,
            top: self.top.map(|y| y + dy),
            bottom: self.bottom.map(|y| y + dy),
            ..self
        }
    }

    /// The share of the pixel centred at `point` the slice covers, as the
    /// shape stage takes it: half a pixel each way across each edge and
    /// each open side. A shared side owns the centres in its columns whole.
    pub fn coverage(&self, point: Point) -> f32 {
        let owned =
            (self.open_left || point.x >= self.left) && (self.open_right || point.x < self.right);
        if self.right <= self.left || !owned {
            return 0.0;
        }
        let corner = |x: f32, y: f32| Point::new(x, y);
        let below_top = edge_distance(
            point,
            corner(self.left, self.top[0]),
            corner(self.right, self.top[1]),
            1.0,
        );
        let above_bottom = edge_distance(
            point,
            corner(self.left, self.bottom[0]),
            corner(self.right, self.bottom[1]),
            -1.0,
        );
        let band = half_pixel(below_top) + half_pixel(above_bottom) - 1.0;
        let left = if self.open_left {
            half_pixel(point.x - self.left)
        } else {
            1.0
        };
        let right = if self.open_right {
            half_pixel(self.right - point.x)
        } else {
            1.0
        };
        band.max(0.0) * (left + right - 1.0).clamp(0.0, 1.0)
    }
}

/// How far `point` lies inside the edge from `a` to `b`, `side` 1 for a top
/// edge and -1 for a bottom one: across the edge beside it, and from the
/// nearer end past either end, so a steep edge does not shade the column
/// past its end. The shape stage's `trapezoid_edge_distance` is the same.
fn edge_distance(point: Point, a: Point, b: Point, side: f32) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let extent = (dx * dx + dy * dy).sqrt();
    let (along_x, along_y) = (dx / extent, dy / extent);
    let (offset_x, offset_y) = (point.x - a.x, point.y - a.y);
    let across = (offset_y * along_x - offset_x * along_y) * side;
    let t = offset_x * along_x + offset_y * along_y;
    if (0.0..=extent).contains(&t) {
        return across;
    }
    let end = if t < 0.0 { a } else { b };
    let distance = ((point.x - end.x).powi(2) + (point.y - end.y).powi(2)).sqrt();
    if across < 0.0 {
        -distance
    } else if across > 0.0 {
        distance
    } else {
        0.0
    }
}

/// The share of a pixel inside an edge whose distance from its centre is
/// `distance`, positive inside.
fn half_pixel(distance: f32) -> f32 {
    (distance + 0.5).clamp(0.0, 1.0)
}

/// The most edges a fill is sliced with; a larger path keeps its mask.
const MAX_EDGES: usize = 1024;
/// The most slices one fill is drawn as.
const MAX_SLICES: usize = 4096;
/// How near two values count as equal, in the path's units.
const TOLERANCE: f32 = 1.0e-3;

/// An edge of a path that is not upright, from its left end to its right,
/// with the winding direction it ran in.
#[derive(Clone, Copy, Debug)]
struct Edge {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    winding: i32,
}

impl Edge {
    /// Its y at `x`, exact at its ends, so slices that meet at a vertex
    /// agree where.
    fn y_at(&self, x: f32) -> f32 {
        if x <= self.x0 {
            self.y0
        } else if x >= self.x1 {
            self.y1
        } else {
            self.y0 + (x - self.x0) * (self.y1 - self.y0) / (self.x1 - self.x0)
        }
    }
}

/// An edge across one slice: its y at the slice's left and right sides.
#[derive(Clone, Copy, Debug)]
struct Crossing {
    edge: Edge,
    start: f32,
    end: f32,
}

/// Cuts path fills into [`Trapezoid`]s, reusing its buffers from one fill
/// to the next.
#[derive(Debug, Default)]
pub(crate) struct PathSlicer {
    edges: Vec<Edge>,
    events: Vec<f32>,
    active: Vec<Edge>,
    crossings: Vec<Crossing>,
    slices: Vec<Trapezoid>,
}

impl PathSlicer {
    /// The fill of `contours`, each closed, under `fill_rule`, as slices
    /// from left to right. `None` when a slice's side meets only part of
    /// its neighbour's, as an upright edge inside the fill makes it, when a
    /// coordinate is not finite, or when the path has too many edges.
    pub(crate) fn slice<'a, I>(
        &mut self,
        contours: I,
        fill_rule: PathFillRule,
    ) -> Option<&[Trapezoid]>
    where
        I: IntoIterator<Item = &'a [Point]>,
        I::IntoIter: Clone,
    {
        let filled = contours.into_iter().filter(|points| points.len() >= 3);
        if let (Some(points), None) = (filled.clone().next(), filled.clone().nth(1)) {
            match self.slice_monotone(points) {
                ChainSlicing::Sliced => return Some(&self.slices),
                ChainSlicing::Unsliceable => return None,
                ChainSlicing::NotMonotone => {}
            }
        }
        self.collect_edges(filled)?;
        self.slices.clear();
        self.active.clear();
        self.events.clear();
        self.events
            .extend(self.edges.iter().flat_map(|edge| [edge.x0, edge.x1]));
        // A contour's edges come in runs already ordered, as a chart's do
        // from its left end to its right and back; a stable sort merges the
        // runs instead of sorting the whole.
        self.events.sort_by(f32::total_cmp);
        self.events.dedup();
        self.edges.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let mut next_edge = 0;
        let mut previous = 0..0;
        let mut event = 0;
        let mut left = self.events.first().copied().unwrap_or_default();
        while let Some(&next_event) = self.events.get(event + 1) {
            while let Some(&edge) = self.edges.get(next_edge).filter(|edge| edge.x0 <= left) {
                self.active.push(edge);
                next_edge += 1;
            }
            self.active.retain(|edge| edge.x1 > left);
            let cut = self.order_crossings(left, next_event);
            let right = cut.unwrap_or(next_event);
            let first = self.slices.len();
            self.push_spans(left, right, fill_rule);
            let (before, after) = self.slices.split_at_mut(first);
            share_sides(&mut before[previous], after)?;
            if self.slices.len() > MAX_SLICES {
                return None;
            }
            previous = first..self.slices.len();
            left = right;
            if cut.is_none() {
                event += 1;
            }
        }
        Some(&self.slices)
    }

    /// Slices a contour that each upright line crosses at most twice, as a
    /// chart's area or a convex shape: its two chains from the leftmost
    /// vertex to the rightmost, walked together, give the slices from left
    /// to right with no sorting. They are the slices the sweep in
    /// [`Self::slice`] makes of the contour.
    fn slice_monotone(&mut self, points: &[Point]) -> ChainSlicing {
        if points
            .iter()
            .any(|point| !(point.x.is_finite() && point.y.is_finite()))
        {
            return ChainSlicing::Unsliceable;
        }
        let Some((leftmost, rightmost)) = horizontal_extremes(points) else {
            return ChainSlicing::NotMonotone;
        };
        if !chain_edges(points, leftmost, rightmost, 1, &mut self.edges)
            || !chain_edges(points, leftmost, rightmost, -1, &mut self.active)
        {
            return ChainSlicing::NotMonotone;
        }
        if self.edges.len() + self.active.len() > MAX_EDGES {
            return ChainSlicing::Unsliceable;
        }
        self.slices.clear();
        let (mut first, mut second) = (0, 0);
        let mut left = self.edges.first().map_or(0.0, |edge| edge.x0);
        let mut previous_sliced = false;
        while let (Some(&a), Some(&b)) = (self.edges.get(first), self.active.get(second)) {
            let next = a.x1.min(b.x1);
            let (top, bottom, cut) = ordered_pair(a, b, left, next);
            let right = cut.unwrap_or(next);
            let sliced = top.start != bottom.start || top.end != bottom.end;
            if sliced {
                if previous_sliced && let Some(previous) = self.slices.last_mut() {
                    previous.open_right = false;
                }
                self.slices.push(Trapezoid {
                    left,
                    right,
                    top: [top.start, top.end],
                    bottom: [bottom.start, bottom.end],
                    open_left: !previous_sliced,
                    open_right: true,
                });
                if self.slices.len() > MAX_SLICES {
                    return ChainSlicing::Unsliceable;
                }
            }
            previous_sliced = sliced;
            left = right;
            if cut.is_none() {
                first += usize::from(a.x1 <= left);
                second += usize::from(b.x1 <= left);
            }
        }
        ChainSlicing::Sliced
    }

    fn collect_edges<'a>(&mut self, contours: impl IntoIterator<Item = &'a [Point]>) -> Option<()> {
        self.edges.clear();
        for points in contours {
            let closing = points.iter().skip(1).chain(points.first());
            for (a, b) in points.iter().zip(closing) {
                if !(a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()) {
                    return None;
                }
                let edge = |from: &Point, to: &Point, winding| Edge {
                    x0: from.x,
                    y0: from.y,
                    x1: to.x,
                    y1: to.y,
                    winding,
                };
                if a.x < b.x {
                    self.edges.push(edge(a, b, 1));
                } else if a.x > b.x {
                    self.edges.push(edge(b, a, -1));
                }
            }
            if self.edges.len() > MAX_EDGES {
                return None;
            }
        }
        Some(())
    }

    /// Orders the active edges from top to bottom across the slice from
    /// `left` to `right`, and returns where the first two of them cross
    /// inside it, which ends the slice early.
    fn order_crossings(&mut self, left: f32, right: f32) -> Option<f32> {
        self.crossings.clear();
        self.crossings
            .extend(self.active.iter().map(|&edge| Crossing {
                edge,
                start: edge.y_at(left),
                end: edge.y_at(right),
            }));
        // The active edges keep the previous slice's order, which changes
        // only where edges cross or enter, so the crossings come nearly in
        // order and a pass of insertion settles them. Edges that meet at the
        // left side within the tolerance then go in the order they leave it.
        insertion_sort(&mut self.crossings, |upper, lower| {
            upper
                .start
                .total_cmp(&lower.start)
                .then_with(|| upper.end.total_cmp(&lower.end))
                .is_gt()
        });
        insertion_sort(&mut self.crossings, |upper, lower| {
            (lower.start - upper.start).abs() <= TOLERANCE && upper.end > lower.end
        });
        self.active.clear();
        self.active
            .extend(self.crossings.iter().map(|crossing| crossing.edge));
        let cut = self
            .crossings
            .windows(2)
            .filter_map(|pair| {
                let gap_start = pair[1].start - pair[0].start;
                let overlap_end = pair[0].end - pair[1].end;
                (gap_start > TOLERANCE && overlap_end > TOLERANCE)
                    .then(|| left + (right - left) * gap_start / (gap_start + overlap_end))
            })
            .filter(|&x| x > left && x < right)
            .min_by(f32::total_cmp)?;
        for crossing in &mut self.crossings {
            crossing.end = crossing.edge.y_at(cut);
        }
        Some(cut)
    }

    /// Pushes the slices between `left` and `right`: the runs between the
    /// ordered edges where the winding fills, each side open until
    /// [`share_sides`] finds its neighbour.
    fn push_spans(&mut self, left: f32, right: f32, fill_rule: PathFillRule) {
        let mut winding = 0;
        let mut top = None;
        for crossing in &self.crossings {
            let was_inside = fill_rule.contains(winding);
            winding += crossing.edge.winding;
            match (was_inside, fill_rule.contains(winding)) {
                (false, true) => top = Some(crossing),
                (true, false) => {
                    let Some(top) = top.take() else {
                        continue;
                    };
                    if top.start == crossing.start && top.end == crossing.end {
                        continue;
                    }
                    self.slices.push(Trapezoid {
                        left,
                        right,
                        top: [top.start, top.end],
                        bottom: [crossing.start, crossing.end],
                        open_left: true,
                        open_right: true,
                    });
                }
                _ => {}
            }
        }
    }
}

/// What slicing a contour as one that each upright line crosses at most
/// twice came to.
enum ChainSlicing {
    Sliced,
    /// The contour is monotone, but the sweep would not slice it either.
    Unsliceable,
    NotMonotone,
}

/// The first of the leftmost vertices and the first of the rightmost ones.
/// `None` when every vertex lies on one upright line.
fn horizontal_extremes(points: &[Point]) -> Option<(usize, usize)> {
    let mut leftmost = 0;
    let mut rightmost = 0;
    for (index, point) in points.iter().enumerate() {
        if point.x < points[leftmost].x {
            leftmost = index;
        }
        if point.x > points[rightmost].x {
            rightmost = index;
        }
    }
    (leftmost != rightmost).then_some((leftmost, rightmost))
}

/// The edges from vertex `from` to vertex `to`, stepping `step` through
/// the contour, as edges from left to right with the winding the contour
/// runs them in. False when the chain turns back left, or when an upright
/// edge stands inside it rather than at its ends.
fn chain_edges(
    points: &[Point],
    from: usize,
    to: usize,
    step: isize,
    edges: &mut Vec<Edge>,
) -> bool {
    edges.clear();
    let count = points.len();
    let mut index = from;
    let mut upright_after_edge = false;
    while index != to {
        let next = index.wrapping_add_signed(step).wrapping_add(count) % count;
        let (a, b) = (points[index], points[next]);
        if a.x > b.x {
            return false;
        }
        if a.x < b.x {
            if upright_after_edge {
                return false;
            }
            let winding = if step > 0 { 1 } else { -1 };
            edges.push(Edge {
                x0: a.x,
                y0: a.y,
                x1: b.x,
                y1: b.y,
                winding,
            });
        } else if a.y != b.y && !edges.is_empty() {
            upright_after_edge = true;
        }
        index = next;
    }
    true
}

/// The crossings of edges `a` and `b` across the slice from `left` to
/// `right`, upper first, ordered as [`PathSlicer::order_crossings`] orders
/// them, and where they cross inside the slice, which ends it early.
fn ordered_pair(a: Edge, b: Edge, left: f32, right: f32) -> (Crossing, Crossing, Option<f32>) {
    let crossing = |edge: Edge| Crossing {
        edge,
        start: edge.y_at(left),
        end: edge.y_at(right),
    };
    let (mut upper, mut lower) = (crossing(a), crossing(b));
    if upper
        .start
        .total_cmp(&lower.start)
        .then_with(|| upper.end.total_cmp(&lower.end))
        .is_gt()
    {
        std::mem::swap(&mut upper, &mut lower);
    }
    if (lower.start - upper.start).abs() <= TOLERANCE && upper.end > lower.end {
        std::mem::swap(&mut upper, &mut lower);
    }
    let gap_start = lower.start - upper.start;
    let overlap_end = upper.end - lower.end;
    let cut = (gap_start > TOLERANCE && overlap_end > TOLERANCE)
        .then(|| left + (right - left) * gap_start / (gap_start + overlap_end))
        .filter(|&x| x > left && x < right);
    if let Some(cut) = cut {
        upper.end = upper.edge.y_at(cut);
        lower.end = lower.edge.y_at(cut);
    }
    (upper, lower, cut)
}

/// Moves each item of `items` back past those before it that `misplaced`
/// says belong after it.
fn insertion_sort<T: Copy>(items: &mut [T], misplaced: impl Fn(&T, &T) -> bool) {
    for index in 1..items.len() {
        let mut at = index;
        while at > 0 && misplaced(&items[at - 1], &items[at]) {
            items.swap(at - 1, at);
            at -= 1;
        }
    }
}

/// Marks the sides where `before`'s slices meet `after`'s as shared. `None`
/// when a side meets only part of the other column's fill.
fn share_sides(before: &mut [Trapezoid], after: &mut [Trapezoid]) -> Option<()> {
    for slice in before.iter_mut() {
        let side = [slice.top[1], slice.bottom[1]];
        slice.open_right =
            !side_shared(side, after.iter().map(|next| [next.top[0], next.bottom[0]]))?;
    }
    for slice in after.iter_mut() {
        let side = [slice.top[0], slice.bottom[0]];
        slice.open_left = !side_shared(
            side,
            before
                .iter()
                .map(|previous| [previous.top[1], previous.bottom[1]]),
        )?;
    }
    Some(())
}

/// Whether `side` lies within the fill `others` make on the same line, the
/// runs of them that touch counted as one: `Some(false)` when it meets none
/// of them, `None` when it meets only part of one.
fn side_shared(side: [f32; 2], others: impl Iterator<Item = [f32; 2]>) -> Option<bool> {
    let mut runs = others.peekable();
    while let Some([start, mut end]) = runs.next() {
        while let Some(&[next_start, next_end]) = runs.peek() {
            if next_start > end + TOLERANCE {
                break;
            }
            end = end.max(next_end);
            runs.next();
        }
        if side[0] >= start - TOLERANCE && side[1] <= end + TOLERANCE {
            return Some(true);
        }
        if side[1].min(end) - side[0].max(start) > TOLERANCE {
            return None;
        }
    }
    Some(false)
}

impl PathFillRule {
    /// Whether a point the path winds around `winding` times is filled.
    pub(crate) fn contains(self, winding: i32) -> bool {
        match self {
            Self::NonZero => winding != 0,
            Self::EvenOdd => winding % 2 != 0,
        }
    }
}

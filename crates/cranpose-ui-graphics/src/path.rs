//! Paths an app builds from lines and curves, and the styles a draw scope
//! fills or strokes them with: Compose's `Path`, `DrawStyle` and
//! `PathEffect.dashPathEffect`.
//!
//! Curves are flattened as they are added, within the tolerance vector icons
//! use, so a path holds polylines only. A fill rasterizes them as
//! [`crate::VectorPath`] does; a stroke draws each flattened edge as a
//! [`crate::DrawPrimitive::Line`] on the GPU.

use crate::{
    LineGeometry, PathFillRule, Point, Rect, Stroke, StrokeCap, StrokeJoin, VectorPath,
    vector_path::{flatten_cubic_into, quad_as_cubic},
};

/// One contour of a [`Path`]: its flattened points and whether it closes
/// back to its first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathContour {
    pub points: Vec<Point>,
    pub closed: bool,
}

impl PathContour {
    /// Points a contour holds before it first grows: room for a chart's
    /// few dozen points in a few growths instead of one per doubling from
    /// one.
    const INITIAL_POINTS: usize = 16;

    fn starting_at(point: Point) -> Self {
        let mut points = Vec::with_capacity(Self::INITIAL_POINTS);
        points.push(point);
        Self {
            points,
            closed: false,
        }
    }
}

/// A path built from lines and curves: Compose's `Path`.
///
/// Example: a sparkline, stroked by [`crate::DrawScope::draw_path`]:
///
/// ```
/// use cranpose_ui_graphics::{Path, Point};
///
/// let mut path = Path::new();
/// path.move_to(Point::new(0.0, 10.0));
/// path.line_to(Point::new(10.0, 4.0));
/// path.quadratic_to(Point::new(15.0, 0.0), Point::new(20.0, 6.0));
/// assert_eq!(path.contours().len(), 1);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    contours: Vec<PathContour>,
    pen: Point,
}

impl Path {
    /// An empty path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new contour at `point`.
    pub fn move_to(&mut self, point: Point) {
        self.contours.push(PathContour::starting_at(point));
        self.pen = point;
    }

    /// Adds a straight edge from the pen to `point`.
    pub fn line_to(&mut self, point: Point) {
        self.open_contour().push(point);
        self.pen = point;
    }

    /// Adds the quadratic curve from the pen through `control` to `end`:
    /// Compose's `quadraticTo`.
    pub fn quadratic_to(&mut self, control: Point, end: Point) {
        let (first, second) = quad_as_cubic(self.pen, control, end);
        self.cubic_to(first, second, end);
    }

    /// Adds the cubic curve from the pen through `first` and `second` to
    /// `end`: Compose's `cubicTo`.
    pub fn cubic_to(&mut self, first: Point, second: Point, end: Point) {
        let start = self.pen;
        flatten_cubic_into(self.open_contour(), start, first, second, end);
        self.pen = end;
    }

    /// Closes the current contour back to its first point; the next edge
    /// starts a new contour there.
    pub fn close(&mut self) {
        if let Some(contour) = self.contours.last_mut().filter(|contour| !contour.closed) {
            contour.closed = true;
            if let Some(&first) = contour.points.first() {
                self.pen = first;
            }
        }
    }

    /// Removes every contour.
    pub fn reset(&mut self) {
        self.contours.clear();
        self.pen = Point::ZERO;
    }

    /// The contours, each flattened to points.
    pub fn contours(&self) -> &[PathContour] {
        &self.contours
    }

    /// Whether the path has no edge.
    pub fn is_empty(&self) -> bool {
        !self
            .contours
            .iter()
            .any(|contour| contour.points.len() >= 2)
    }

    /// The box of every point, or an empty rect at the origin for an
    /// empty path.
    pub fn bounds(&self) -> Rect {
        let mut points = self.contours.iter().flat_map(|contour| &contour.points);
        let Some(&first) = points.next() else {
            return Rect::EMPTY;
        };
        let (min, max) = points.fold((first, first), |(min, max), point| {
            (
                Point::new(min.x.min(point.x), min.y.min(point.y)),
                Point::new(max.x.max(point.x), max.y.max(point.y)),
            )
        });
        Rect {
            x: min.x,
            y: min.y,
            width: max.x - min.x,
            height: max.y - min.y,
        }
    }

    /// The path as a fill: every contour closed, under `fill_rule`.
    pub fn to_vector_path(&self, fill_rule: PathFillRule) -> VectorPath {
        VectorPath::from_subpaths(
            self.contours
                .iter()
                .filter(|contour| contour.points.len() >= 2)
                .map(|contour| contour.points.clone())
                .collect(),
            fill_rule,
        )
    }

    /// The contour an edge extends: the last one when it is still open,
    /// otherwise a new one starting at the pen.
    fn open_contour(&mut self) -> &mut Vec<Point> {
        if self.contours.last().is_none_or(|contour| contour.closed) {
            self.contours.push(PathContour::starting_at(self.pen));
        }
        let last = self.contours.len() - 1;
        &mut self.contours[last].points
    }
}

/// Dashes along a stroke: on for the first interval, off for the second,
/// and so on, starting `phase` into the pattern. Compose's
/// `PathEffect.dashPathEffect(intervals, phase)`. Up to
/// [`DashPathEffect::MAX_INTERVALS`] intervals; a pattern with an odd
/// count or a non-positive sum draws the stroke whole, as Skia does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DashPathEffect {
    intervals: [f32; DashPathEffect::MAX_INTERVALS],
    count: usize,
    phase: f32,
}

impl DashPathEffect {
    /// The most intervals a pattern holds.
    pub const MAX_INTERVALS: usize = 8;

    /// A dash pattern of `intervals`, the extras past
    /// [`Self::MAX_INTERVALS`] dropped, starting `phase` into it.
    pub fn new(intervals: &[f32], phase: f32) -> Self {
        let mut stored = [0.0; Self::MAX_INTERVALS];
        let count = intervals.len().min(Self::MAX_INTERVALS);
        stored[..count].copy_from_slice(&intervals[..count]);
        Self {
            intervals: stored,
            count,
            phase,
        }
    }

    /// The intervals, on then off in turn.
    pub fn intervals(&self) -> &[f32] {
        &self.intervals[..self.count]
    }

    fn period(&self) -> Option<f32> {
        let period: f32 = self
            .intervals()
            .iter()
            .map(|interval| interval.max(0.0))
            .sum();
        (self.count >= 2 && self.count.is_multiple_of(2) && period > 0.0 && period.is_finite())
            .then_some(period)
    }

    /// Hands `dash` each dash of the polyline through `points` in turn, as
    /// a polyline of its own; the whole polyline when the pattern draws it
    /// whole.
    pub fn for_each_dash(&self, points: &[Point], mut dash: impl FnMut(&[Point])) {
        let Some(period) = self.period() else {
            dash(points);
            return;
        };
        let interval = |index: usize| self.intervals[index].max(0.0);
        let mut current: Vec<Point> = Vec::new();
        // Where in the pattern the walk is, and which interval it is in.
        let mut into = self.phase.rem_euclid(period);
        let mut index = 0;
        while into >= interval(index) {
            into -= interval(index);
            index = (index + 1) % self.count;
        }
        for pair in points.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let length = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
            let at = |distance: f32| {
                let t = if length > 0.0 { distance / length } else { 0.0 };
                Point::new(
                    start.x + (end.x - start.x) * t,
                    start.y + (end.y - start.y) * t,
                )
            };
            let mut walked = 0.0;
            while walked < length {
                let on = index.is_multiple_of(2);
                let step = (interval(index) - into).min(length - walked);
                if on {
                    if current.is_empty() {
                        current.push(at(walked));
                    }
                    current.push(at(walked + step));
                }
                walked += step;
                into += step;
                if into >= interval(index) {
                    into = 0.0;
                    if on && current.len() >= 2 {
                        dash(&current);
                    }
                    current.clear();
                    index = (index + 1) % self.count;
                }
            }
        }
        if current.len() >= 2 {
            dash(&current);
        }
    }
}

/// How [`crate::DrawScope::draw_path`] paints a path: Compose's `Fill` and
/// `Stroke` draw styles, the stroke dashed when it carries a path effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawStyle {
    /// Fills the path's contours, each closed, under the non-zero rule.
    Fill,
    /// Strokes the path's edges with the stroke's width, cap and join.
    Stroke(Stroke),
    /// Strokes the dashes the effect cuts the path's edges into, each dash
    /// capped by the stroke's cap.
    DashedStroke(Stroke, DashPathEffect),
}

/// Hands `line` the segments a stroke draws for the polyline through
/// `points`, closed back to its first point when `closed`. An open
/// polyline's two ends take the stroke's cap; every corner takes the cap
/// closest to the stroke's join, round for round and bevel joins, square
/// for a miter, which is exact at a right angle. A segment whose two ends
/// want the same cap draws with it; one whose ends differ draws butt ends,
/// grown by half the width at a square end, and a dot of the stroke at a
/// round one.
pub fn for_each_stroke_line(
    points: &[Point],
    closed: bool,
    stroke: Stroke,
    mut line: impl FnMut(LineGeometry),
) {
    let corner = match stroke.join {
        StrokeJoin::Miter => StrokeCap::Square,
        StrokeJoin::Round | StrokeJoin::Bevel => StrokeCap::Round,
    };
    let closing = closed
        .then(|| points.first().zip(points.last()))
        .flatten()
        .filter(|(first, last)| first != last)
        .map(|(&first, &last)| (last, first));
    let edges = points
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .chain(closing)
        .filter(|(start, end)| start != end);
    let count = edges.clone().count();
    for (index, (start, end)) in edges.enumerate() {
        let start_cap = if closed || index > 0 {
            corner
        } else {
            stroke.cap
        };
        let end_cap = if closed || index + 1 < count {
            corner
        } else {
            stroke.cap
        };
        let with_cap = |cap| Stroke { cap, ..stroke };
        if start_cap == end_cap {
            line(LineGeometry::new(start, end, with_cap(start_cap)));
            continue;
        }
        let half_width = stroke.half_width();
        let length = ((end.x - start.x).powi(2) + (end.y - start.y).powi(2)).sqrt();
        let grow = |cap: StrokeCap| {
            if cap == StrokeCap::Square {
                half_width / length
            } else {
                0.0
            }
        };
        let (back, ahead) = (grow(start_cap), grow(end_cap));
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        line(LineGeometry::new(
            Point::new(start.x - dx * back, start.y - dy * back),
            Point::new(end.x + dx * ahead, end.y + dy * ahead),
            with_cap(StrokeCap::Butt),
        ));
        for (point, cap) in [(start, start_cap), (end, end_cap)] {
            if cap == StrokeCap::Round {
                line(LineGeometry::new(point, point, with_cap(StrokeCap::Round)));
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/path_tests.rs"]
mod tests;

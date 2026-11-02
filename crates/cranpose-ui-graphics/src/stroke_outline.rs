// The outline of what a stroke's lines cover, as closed contours that all
// turn one way: a non-zero fill of them covers each point once, so a
// translucent stroke drawn as that fill shows no darker overlap where its
// lines meet.

use std::f32::consts::{PI, TAU};

use crate::{
    PathContour, Point, StrokeCap, stroke::LineGeometry, vector_path::FLATTEN_TOLERANCE,
    whole::to_i32,
};

// The most points a disc at a round end takes. Discs take an even count, so
// no edge of one stands upright, which a fill's slices cannot follow.
const MAX_DISC_POINTS: usize = 64;

#[derive(Debug, Default)]
pub(crate) struct StrokeOutline {
    // The contours, the first `used` of them this outline's: the rest keep
    // their room for the next.
    contours: Vec<PathContour>,
    used: usize,
    // The center of the last disc: the lines on both sides of a round join
    // each end at its disc.
    last_disc: Option<Point>,
}

impl StrokeOutline {
    pub(crate) const fn new() -> Self {
        Self {
            contours: Vec::new(),
            used: 0,
            last_disc: None,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.used = 0;
        self.last_disc = None;
    }

    pub(crate) fn contours(&self) -> &[PathContour] {
        &self.contours[..self.used]
    }

    // Adds what `line` covers as `LineRun::push` draws it: its body, pushed
    // out half the width past each square end, and a disc at each round end.
    pub(crate) fn add_line(&mut self, line: LineGeometry) {
        if line.is_degenerate() {
            return;
        }
        let half = line.half_width;
        if line.start != line.end || line.cap == StrokeCap::Square {
            let along = line.frame().direction;
            let reach = if line.cap == StrokeCap::Square {
                half
            } else {
                0.0
            };
            let (across_x, across_y) = (-along.y * half, along.x * half);
            let start = Point::new(
                line.start.x - along.x * reach,
                line.start.y - along.y * reach,
            );
            let end = Point::new(line.end.x + along.x * reach, line.end.y + along.y * reach);
            self.next_contour().extend([
                Point::new(start.x + across_x, start.y + across_y),
                Point::new(end.x + across_x, end.y + across_y),
                Point::new(end.x - across_x, end.y - across_y),
                Point::new(start.x - across_x, start.y - across_y),
            ]);
        }
        if line.cap == StrokeCap::Round {
            self.add_disc(line.start, half);
            self.add_disc(line.end, half);
        }
    }

    // A disc's polygon, within the tolerance curves flatten to, turning the
    // way a line's body does.
    fn add_disc(&mut self, center: Point, radius: f32) {
        if self.last_disc == Some(center) {
            return;
        }
        self.last_disc = Some(center);
        let count = disc_points(radius);
        let (sin, cos) = (TAU / count as f32).sin_cos();
        let (mut x, mut y) = (radius, 0.0_f32);
        let points = self.next_contour();
        for _ in 0..count {
            points.push(Point::new(center.x + x, center.y + y));
            (x, y) = (x * cos + y * sin, y * cos - x * sin);
        }
    }

    // The points of a new closed contour, empty, in the room an earlier
    // outline left.
    fn next_contour(&mut self) -> &mut Vec<Point> {
        if self.used == self.contours.len() {
            self.contours.push(PathContour {
                points: Vec::new(),
                closed: true,
            });
        }
        self.used += 1;
        let points = &mut self.contours[self.used - 1].points;
        points.clear();
        points
    }
}

// The even count of points that keeps a disc of `radius` within the
// flattening tolerance of its circle.
fn disc_points(radius: f32) -> usize {
    let cos = 1.0 - FLATTEN_TOLERANCE / radius;
    let needed = if cos > -1.0 {
        to_i32((PI / cos.acos()).ceil())
    } else {
        0
    };
    (usize::try_from(needed).map_or(4, |count| count.clamp(4, MAX_DISC_POINTS)) + 1) & !1
}

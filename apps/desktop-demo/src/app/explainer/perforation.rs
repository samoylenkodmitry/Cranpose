// The outline of perforated paper: a rectangle whose edges are bitten by
// round holes at an even pitch, a hole on every corner.

use std::f32::consts::{FRAC_PI_2, TAU};

use cranpose_ui::{Path, Point, Rect};

// The way a perforated outline runs. The holes bite into the paper on the
// right of the way: a clockwise outline is a stamp's own edge; a
// counter-clockwise one inside a larger outline is the edge of the hole a
// block of stamps leaves in its sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Winding {
    Clockwise,
    CounterClockwise,
}

// Traces `rect` into `path` with `holes` holes of `radius` along its width
// and its height, besides those on the corners.
pub(super) fn perforated(
    path: &mut Path,
    rect: Rect,
    (across, down): (u32, u32),
    radius: f32,
    winding: Winding,
) {
    let (left, top) = (rect.x, rect.y);
    let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
    let corners = match winding {
        Winding::Clockwise => [(left, top), (right, top), (right, bottom), (left, bottom)],
        Winding::CounterClockwise => [(left, top), (left, bottom), (right, bottom), (right, top)],
    };
    let mut started = false;
    for side in 0..4 {
        let (from, to) = (corners[side], corners[(side + 1) % 4]);
        let next = corners[(side + 2) % 4];
        let holes = if from.1 == to.1 { across } else { down }.max(1);
        let along = Point::new(
            (to.0 - from.0) / holes as f32,
            (to.1 - from.1) / holes as f32,
        );
        let step = (along.x * along.x + along.y * along.y).sqrt().max(1.0e-3);
        let way = Point::new(along.x / step, along.y / step);
        if !started {
            path.move_to(Point::new(from.0 + way.x * radius, from.1 + way.y * radius));
            started = true;
        }
        for hole in 1..holes {
            let centre = Point::new(
                from.0 + along.x * hole as f32,
                from.1 + along.y * hole as f32,
            );
            path.line_to(Point::new(
                centre.x - way.x * radius,
                centre.y - way.y * radius,
            ));
            bite(path, centre, radius, (way, way));
        }
        path.line_to(Point::new(to.0 - way.x * radius, to.1 - way.y * radius));
        let out = Point::new(next.0 - to.0, next.1 - to.1);
        let out_length = (out.x * out.x + out.y * out.y).sqrt().max(1.0e-3);
        bite(
            path,
            Point::new(to.0, to.1),
            radius,
            (way, Point::new(out.x / out_length, out.y / out_length)),
        );
    }
    path.close();
}

// The arc of a hole at `centre` from where the outline comes in along
// `incoming` to where it leaves along `outgoing`, round the right side.
fn bite(path: &mut Path, centre: Point, radius: f32, (incoming, outgoing): (Point, Point)) {
    let start = (-incoming.y).atan2(-incoming.x);
    let mut end = outgoing.y.atan2(outgoing.x);
    while end >= start {
        end -= TAU;
    }
    let pieces = ((start - end) / FRAC_PI_2).ceil().max(1.0) as u32;
    let sweep = (end - start) / pieces as f32;
    let handle = 4.0 / 3.0 * (sweep / 4.0).tan() * radius;
    let at = |angle: f32| {
        Point::new(
            centre.x + radius * angle.cos(),
            centre.y + radius * angle.sin(),
        )
    };
    for piece in 0..pieces {
        let a = start + sweep * piece as f32;
        let b = a + sweep;
        let (from, to) = (at(a), at(b));
        path.cubic_to(
            Point::new(from.x - handle * a.sin(), from.y + handle * a.cos()),
            Point::new(to.x + handle * b.sin(), to.y - handle * b.cos()),
            to,
        );
    }
}

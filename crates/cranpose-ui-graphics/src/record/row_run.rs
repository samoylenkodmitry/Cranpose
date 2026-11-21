use super::{BrushShading, OPEN_LEFT_BIT, OPEN_RIGHT_BIT, ShapeRecorder, rect_row};
use crate::{
    BlendMode, LineFrame, LineGeometry, Rect, ShapeRecordBody, ShapeRecordCurve, Trapezoid,
};

// The rows of one run, a stroke's lines or a fill's slices, which share a
// brush, a blend, a shading and a fragment kind with no interior, and so a
// segment key. The first row takes `push_shaded`, which opens or joins a
// segment. A later row only extends that segment while it holds no band
// class; in an arc's segment it takes the full check, which can cut a new
// segment, after which every row extends that one. The run counts the rows
// it appends and adds them to the segment once, when it closes.
pub(super) struct RowRun {
    blend: BlendMode,
    shading: BrushShading,
    plain: bool,
    appended: u32,
}

impl RowRun {
    pub(super) fn new(blend: BlendMode, shading: BrushShading) -> Self {
        Self {
            blend,
            shading,
            plain: false,
            appended: 0,
        }
    }

    // Records the row `body` and `curve` make. They make it in its columns
    // when it extends the segment.
    #[inline]
    pub(super) fn push(
        &mut self,
        recorder: &mut ShapeRecorder,
        body: impl FnOnce() -> ShapeRecordBody,
        curve: impl FnOnce() -> ShapeRecordCurve,
    ) {
        if self.plain {
            let flag = self.shading.flag();
            recorder.tables.shapes.push_row_with(
                || {
                    let mut body = body();
                    body.flags |= flag;
                    body
                },
                curve,
            );
            self.appended += 1;
            return;
        }
        recorder.push_shaded(body(), curve(), [0.0; 4], (self.blend, self.shading), None);
        self.plain = recorder.extends_plainly();
    }

    pub(super) fn close(self, recorder: &mut ShapeRecorder) {
        if self.appended == 0 {
            return;
        }
        if let Some(segment) = recorder.tables_mut().segments.last_mut() {
            segment.count += self.appended;
        }
    }
}

// The body of the segment `line`, whose ends' box is `ends`, painted by the
// interned `brush`: the ends ride in the arc columns. A line reads back
// nothing else, so it keeps no source arguments, which an arc keeps for its
// radius and angles.
#[inline]
pub(super) fn line_body(
    line: &LineGeometry,
    ends: Rect,
    (brush, color): (u32, [f32; 4]),
    stroke_width: f32,
    flags: u32,
) -> ShapeRecordBody {
    ShapeRecordBody {
        rect: rect_row(ends),
        color,
        stroke_width,
        flags,
        brush,
        placement: 0,
        arc_geometry: [line.start.x, line.start.y, line.end.x, line.end.y],
    }
}

// The curve of a segment whose frame is `frame`: the unit direction and
// half length the vertex stage orients its quad by.
#[inline]
pub(super) fn line_curve(frame: &LineFrame) -> ShapeRecordCurve {
    ShapeRecordCurve {
        radii: [0.0; 4],
        arc_normalized: [frame.direction.x, frame.direction.y, frame.half_length, 0.0],
    }
}

// The body of `trapezoid`, whose box is `bounds`, painted by the interned
// `brush`; see `ShapeRecorder::push_trapezoids`.
#[inline]
pub(super) fn slice_body(
    trapezoid: &Trapezoid,
    bounds: Rect,
    (brush, color): (u32, [f32; 4]),
    flags: u32,
) -> ShapeRecordBody {
    let open = (u32::from(trapezoid.open_left) * OPEN_LEFT_BIT)
        | (u32::from(trapezoid.open_right) * OPEN_RIGHT_BIT);
    ShapeRecordBody {
        rect: rect_row(bounds),
        color,
        stroke_width: 0.0,
        flags: flags | open,
        brush,
        placement: 0,
        arc_geometry: [
            trapezoid.top[0],
            trapezoid.top[1],
            trapezoid.bottom[0],
            trapezoid.bottom[1],
        ],
    }
}

// The curve of `trapezoid` against the brush rect `radii` holds.
#[inline]
pub(super) fn slice_curve(trapezoid: &Trapezoid, radii: [f32; 4]) -> ShapeRecordCurve {
    ShapeRecordCurve {
        radii,
        arc_normalized: [
            trapezoid.left,
            trapezoid.right,
            trapezoid.right - trapezoid.left,
            0.0,
        ],
    }
}

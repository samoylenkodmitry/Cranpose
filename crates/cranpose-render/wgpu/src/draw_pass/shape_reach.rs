use cranpose_ui_graphics::Rect;

use super::SegmentRun;
use crate::{
    geometry::{DEVICE_SNAP_SUBPIXEL_STEPS, canonicalize_device_coordinate, snap_delta_for_anchor},
    render::{TargetRect, scissor_rect_for_rect},
    scene::RunDraw,
    scissor::target_rect_between,
};

// The coverage under which the shape stage discards a fragment
// (`alpha < 0.001` in `shape.wgsl`): a pixel given less keeps its colour.
const LEAST_COVERAGE: f32 = 0.001;

// How many units in the last place of the largest magnitude on the way to a
// coordinate the shape stage's arithmetic, in any order of its adds, may put
// it from the one worked out here.
const ROUNDING_ULPS: f32 = 4.0;

// How far past a clip's edge, in device pixels, its scissor or the shape
// stage's test of it may let a record draw: the scissor rounds the clip to
// the 1/16 pixel grid, which an unsnapped placement's shape stage does not.
const CLIP_SLACK: f32 = 1.0 / 16.0;

// The reach of one run, worked out at the first of the pass's tests that
// asks for it: whether the run ends the held draws, and the scissor of its
// chunk in a slot.
pub(super) struct RunReach<'a, 's, 'r> {
    pub(super) draw: &'a RunDraw,
    pub(super) run: &'a SegmentRun<'s, 'r>,
    bounds: Rect,
    reach: Option<Option<TargetRect>>,
}

impl<'a, 's, 'r> RunReach<'a, 's, 'r> {
    pub(super) fn new(draw: &'a RunDraw, bounds: Rect, run: &'a SegmentRun<'s, 'r>) -> Self {
        Self {
            draw,
            run,
            bounds,
            reach: None,
        }
    }

    pub(super) fn get(&mut self) -> Option<TargetRect> {
        let Self {
            draw,
            run,
            bounds,
            reach,
        } = self;
        *reach.get_or_insert_with(|| shape_reach(draw, *bounds, run))
    }
}

// The target pixels the records of `draw` can colour, drawn as `run` places
// them; `bounds` is the run's snapped logical rect within its clip.
//
// The shape stage gives a rect or a rounded rect, filled or stroked,
// coverage only at a pixel whose centre lies less than half a pixel past
// the record's device rect, and discards a fragment whose coverage is under
// `LEAST_COVERAGE`. Its device rect is the one worked out here, snapped to
// the 1/16 pixel grid under a snapped placement (to the outer neighbour
// near a tie), within rounding: a pixel beside an edge is reached when that
// rounding could give it the least coverage. A line's, a path slice's and
// an arc's slanted edges reach further along an axis (a butt end's corner
// 0.71 pixel), so a run that holds one, like a turned segment's run, takes
// a pixel more on each side. A run whose edges do not stay finite reaches
// the whole target.
fn shape_reach(draw: &RunDraw, bounds: Rect, run: &SegmentRun<'_, '_>) -> Option<TargetRect> {
    let scale = run.segment.scale;
    let viewport = run.viewport;
    if !viewport.transform.is_identity() || !draw.holds_only_rects() {
        let pixel = 1.0 / scale;
        let wide = Rect {
            x: bounds.x - pixel,
            y: bounds.y - pixel,
            width: bounds.width + 2.0 * pixel,
            height: bounds.height + 2.0 * pixel,
        };
        return scissor_rect_for_rect(wide, scale, viewport);
    }
    let placement = &draw.placement;
    let snapped = placement.snap_anchor.is_some();
    let delta = placement
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default();
    let shapes = draw.bounds;
    let logical = [
        shapes.x + delta.x,
        shapes.y + delta.y,
        shapes.x + shapes.width + delta.x,
        shapes.y + shapes.height + delta.y,
    ];
    let offset = viewport.offset;
    // The shape stage works a rect's centre and half size out of both its
    // edges, so the larger one bounds the rounding of each.
    let moved = [
        placement.offset.x.abs() + delta.x.abs(),
        placement.offset.y.abs() + delta.y.abs(),
    ];
    let errors = [0, 1].map(|axis| {
        let largest = logical[axis].abs().max(logical[axis + 2].abs()) + moved[axis];
        ROUNDING_ULPS * ulp(largest * scale + 2.0 * offset[axis].abs())
    });
    let mut edges = [0.0; 4];
    for (side, value) in logical.into_iter().enumerate() {
        let axis = side % 2;
        let error = errors[axis];
        let edge = if side < 2 { Edge::Low } else { Edge::High };
        let device = value * scale;
        let device = if snapped {
            on_grid(device, error, edge)
        } else {
            device
        };
        let target = device - offset[axis];
        edges[side] = match edge {
            Edge::Low => (target - 1.0 + LEAST_COVERAGE - error).ceil(),
            Edge::High => (target + error - LEAST_COVERAGE).floor() + 1.0,
        };
    }
    if !edges.iter().all(|edge| edge.is_finite()) {
        return Some((0, 0, viewport.width, viewport.height));
    }
    if let Some(clip) = placement.clip {
        let cut =
            [clip.x, clip.y, clip.x + clip.width, clip.y + clip.height].map(|edge| edge * scale);
        let slack = [0, 1].map(|axis| {
            let largest = cut[axis].abs().max(cut[axis + 2].abs()) + offset[axis].abs();
            CLIP_SLACK + ROUNDING_ULPS * ulp(largest)
        });
        // `max` and `min` pass over a NaN, so a clip edge the arithmetic
        // loses cuts nothing.
        let cut = [
            (cut[0] - offset[0] - slack[0]).floor(),
            (cut[1] - offset[1] - slack[1]).floor(),
            (cut[2] - offset[0] + slack[0]).ceil(),
            (cut[3] - offset[1] + slack[1]).ceil(),
        ];
        edges = [
            edges[0].max(cut[0]),
            edges[1].max(cut[1]),
            edges[2].min(cut[2]),
            edges[3].min(cut[3]),
        ];
    }
    target_rect_between(edges, (viewport.width, viewport.height))
}

// Which edge of a rect a coordinate is: outward is down for the low edge
// and up for the high one.
#[derive(Clone, Copy)]
enum Edge {
    Low,
    High,
}

// `value` on the 1/16 pixel grid as a snapped placement's shape stage
// rounds it; within `error` of a tie, the neighbour outward past `edge`.
fn on_grid(value: f32, error: f32, edge: Edge) -> f32 {
    let steps = DEVICE_SNAP_SUBPIXEL_STEPS;
    let scaled = value * steps;
    if (scaled - scaled.floor() - 0.5).abs() > error * steps {
        return canonicalize_device_coordinate(value);
    }
    match edge {
        Edge::Low => scaled.floor() / steps,
        Edge::High => scaled.ceil() / steps,
    }
}

// The distance from `value` to the next larger `f32` in magnitude. Its bits
// work it out without the NaN and infinity tests of `f32::next_up`: a value
// that is not finite gives NaN, so an edge reaches the whole target and a
// clip edge cuts nothing.
fn ulp(value: f32) -> f32 {
    let magnitude = value.abs();
    f32::from_bits(magnitude.to_bits() + 1) - magnitude
}

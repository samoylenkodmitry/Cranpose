use cranpose_ui_graphics::Rect;

use super::SegmentRun;
use crate::{
    geometry::{DEVICE_SNAP_SUBPIXEL_STEPS, snap_delta_for_anchor},
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

// The target pixels the records of `draw` can colour, drawn as `run` places
// them; `bounds` is the run's snapped logical rect within its clip.
//
// The shape stage gives a pixel coverage only where its centre lies less
// than half a pixel past a record's device rect, and discards a fragment
// whose coverage is under `LEAST_COVERAGE`. Its device rect is the one
// worked out here, snapped to the 1/16 pixel grid under a snapped placement
// (to the outer neighbour near a tie), within rounding: a pixel beside an
// edge is reached when that rounding could give it the least coverage.
// A turned segment's records take a pixel more on each side, and a run
// whose edges are not all finite reaches the whole target.
pub(super) fn shape_reach(
    draw: &RunDraw,
    bounds: Rect,
    run: &SegmentRun<'_, '_>,
) -> Option<TargetRect> {
    let scale = run.segment.scale;
    let viewport = run.viewport;
    if !viewport.transform.is_identity() {
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
    if !logical.iter().chain(&offset).all(|value| value.is_finite()) {
        return Some((0, 0, viewport.width, viewport.height));
    }
    let moved = [
        placement.offset.x.abs() + delta.x.abs(),
        placement.offset.y.abs() + delta.y.abs(),
    ];
    let mut rows = [0.0; 4];
    for (side, value) in logical.into_iter().enumerate() {
        let axis = side % 2;
        let magnitude = (value.abs() + moved[axis]) * scale + 2.0 * offset[axis].abs();
        let error = ROUNDING_ULPS * ulp(magnitude);
        let edge = if side < 2 { Edge::Low } else { Edge::High };
        let device = value * scale;
        let device = if snapped {
            on_grid(device, error, edge)
        } else {
            device
        };
        let target = device - offset[axis];
        rows[side] = match edge {
            Edge::Low => (target - 1.0 + LEAST_COVERAGE - error).ceil(),
            Edge::High => (target + error - LEAST_COVERAGE).floor() + 1.0,
        };
    }
    if let Some(clip) = placement.clip {
        let cut = [
            (clip.x * scale - offset[0] - CLIP_SLACK).floor(),
            (clip.y * scale - offset[1] - CLIP_SLACK).floor(),
            ((clip.x + clip.width) * scale - offset[0] + CLIP_SLACK).ceil(),
            ((clip.y + clip.height) * scale - offset[1] + CLIP_SLACK).ceil(),
        ];
        rows = [
            rows[0].max(cut[0]),
            rows[1].max(cut[1]),
            rows[2].min(cut[2]),
            rows[3].min(cut[3]),
        ];
    }
    target_rect_between(rows, (viewport.width, viewport.height))
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
    let near_tie = (scaled - scaled.floor() - 0.5).abs() <= error * steps;
    let rounded = match (near_tie, edge) {
        (true, Edge::Low) => scaled.floor(),
        (true, Edge::High) => scaled.ceil(),
        (false, _) => scaled.round(),
    };
    rounded / steps
}

// The distance from `value`, a finite magnitude, to the next larger `f32`.
fn ulp(value: f32) -> f32 {
    let next = f32::from_bits(value.abs().to_bits() + 1);
    next - value.abs()
}

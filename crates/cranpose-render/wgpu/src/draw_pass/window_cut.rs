use std::ops::Range;

use cranpose_ui_graphics::Rect;

use super::{PassSegment, PassTarget, op_draw_bounds, segment_viewport_rect};
use crate::{scene::DrawOp, scissor::intersect_scissors};

// The segments of a pass into a page drawn in several parts: each segment
// cut to each part, with the ops that reach into that part. A pass works
// out each op's bounds once and tests them against each part, so a part
// walks only its own ops. The lists live across frames.
#[derive(Default)]
pub(crate) struct WindowCut {
    ops: Vec<DrawOp>,
    bounds: Vec<Option<Rect>>,
    cuts: Vec<Cut>,
}

// A segment cut to one part: the segment it came from, the scissor of the
// part, the run of the cut's ops, and whether that run starts with the
// segment's first op.
struct Cut {
    source: usize,
    scissor: (u32, u32, u32, u32),
    ops: Range<usize>,
    first: bool,
}

impl WindowCut {
    // Cuts `segments` to each of `parts` in turn, in their order: each
    // segment's part in the first, then in the next. The parts share no
    // pixel, so no pixel draws twice. A single part only narrows each
    // segment's scissor: the pass tests each op against it once anyway.
    pub(crate) fn cut<'a>(
        &'a mut self,
        segments: &mut Vec<PassSegment<'a>>,
        parts: impl Iterator<Item = (u32, u32, u32, u32)> + Clone,
        target: PassTarget<'_>,
    ) {
        let mut first_two = parts.clone();
        if let (Some(only), None) = (first_two.next(), first_two.next()) {
            segments.retain_mut(
                |segment| match intersect_scissors(segment.scissor, Some(only)) {
                    Some(scissor) => {
                        segment.scissor = scissor;
                        true
                    }
                    None => false,
                },
            );
            return;
        }
        self.ops.clear();
        self.cuts.clear();
        for (source, segment) in segments.iter().enumerate() {
            self.bounds.clear();
            self.bounds.extend(
                segment
                    .ops
                    .iter()
                    .map(|op| op_draw_bounds(segment.scene, op, segment.scale)),
            );
            for part in parts.clone() {
                let Some(Some(scissor)) = intersect_scissors(segment.scissor, Some(part)) else {
                    continue;
                };
                let cut = PassSegment {
                    scissor: Some(scissor),
                    ..segment.clone()
                };
                let rect = segment_viewport_rect(target, &cut);
                let meets = |bounds: &Option<Rect>| {
                    bounds.is_some_and(|bounds| bounds.intersect(rect).is_some())
                };
                let start = self.ops.len();
                self.ops.extend(
                    segment
                        .ops
                        .iter()
                        .zip(&self.bounds)
                        .filter(|(_, bounds)| meets(bounds))
                        .map(|(op, _)| *op),
                );
                self.cuts.push(Cut {
                    source,
                    scissor,
                    ops: start..self.ops.len(),
                    first: self.bounds.first().is_some_and(meets),
                });
            }
        }
        let this: &'a Self = self;
        let sources = segments.len();
        for cut in &this.cuts {
            let segment = segments[cut.source].clone();
            segments.push(PassSegment {
                ops: &this.ops[cut.ops.clone()],
                scissor: Some(cut.scissor),
                first_run_window: segment.first_run_window.filter(|_| cut.first),
                ..segment
            });
        }
        segments.drain(..sources);
    }
}

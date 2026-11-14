use super::ResolvedComposite;
use crate::{
    geometry::SegmentTransform,
    scene::{CompositorScene, DrawOp},
};

/// One scene's contribution to a pass: its ops in z order, the composites
/// resolved for it, where its device space origin sits in the target's
/// scene space, the target pixels it may touch (the whole target when
/// `None`), the transform its device space is drawn under: the identity,
/// except for a layer drawn in place, whose segments carry no composites,
/// and the scale from its scene's logical space to device pixels.
#[derive(Clone)]
pub(crate) struct PassSegment<'a> {
    pub(crate) scene: &'a CompositorScene,
    pub(crate) ops: &'a [DrawOp],
    pub(crate) composites: &'a [ResolvedComposite],
    pub(crate) offset: [f32; 2],
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
    pub(crate) first_run_window: Option<std::ops::Range<u32>>,
    pub(crate) transform: SegmentTransform,
    pub(crate) scale: f32,
}

impl<'a> PassSegment<'a> {
    // All of `scene`'s ops, with no composites or scissor, untransformed.
    pub(crate) fn plain(scene: &'a CompositorScene, offset: [f32; 2], scale: f32) -> Self {
        Self {
            scene,
            ops: &scene.draw_ops,
            composites: &[],
            offset,
            scissor: None,
            first_run_window: None,
            transform: SegmentTransform::IDENTITY,
            scale,
        }
    }
}

/// How the segments of a pass lie: in order, each over the ones before it,
/// or side by side, as the members of an atlas lie, each within its
/// scissor, which no other segment's touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentLayout {
    Stacked,
    SideBySide,
}

/// Whether side-by-side segments `a` and `b`, next to each other, share one
/// binding as members (see [`super::PassPrep::members`]).
pub(super) fn joins_members(a: &PassSegment<'_>, b: &PassSegment<'_>) -> bool {
    is_member(a) && is_member(b)
}

/// Whether a side-by-side segment can share a binding with others: it is
/// untransformed, without composites and within its scissor, and its offset
/// is whole pixels, so its records snapped to the device grid stay on it in
/// the target's space.
fn is_member(segment: &PassSegment<'_>) -> bool {
    segment.transform.is_identity()
        && segment.composites.is_empty()
        && segment.scissor.is_some()
        && segment.offset[0].fract() == 0.0
        && segment.offset[1].fract() == 0.0
}

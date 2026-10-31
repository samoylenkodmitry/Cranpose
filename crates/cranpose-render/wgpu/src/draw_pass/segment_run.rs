use super::PassSegment;
use crate::render::{DrawSpace, TargetRect, ViewportUniformParams};

/// One segment's viewport, turn included, and what its shapes and glyphs
/// bind while its items are batched.
pub(super) struct SegmentRun<'s, 'a> {
    pub(super) segment: &'a PassSegment<'s>,
    pub(super) viewport: ViewportUniformParams,
    pub(super) binding: SegmentBinding,
    /// What moves the segment's device space into the binding's: whole
    /// pixels for a member of side-by-side segments, else zero.
    pub(super) shift: [f32; 2],
    /// A member's scissor, which its binding leaves to each draw that may
    /// reach past it.
    pub(super) slot: Option<TargetRect>,
}

/// How an arena run's clip applies: see [`PassPrep::arena_clip`].
pub(super) struct ArenaClip {
    pub(super) paint: Option<TargetRect>,
    pub(super) tested: crate::render::SegmentClip,
    pub(super) chunk: Option<TargetRect>,
}

impl SegmentRun<'_, '_> {
    pub(super) fn space(&self) -> DrawSpace {
        DrawSpace {
            viewport: self.viewport,
            scale: self.segment.scale,
        }
    }

    /// The viewport the segment's images draw under: a member's are moved
    /// into its binding's space.
    pub(super) fn image_viewport(&self) -> ViewportUniformParams {
        if self.slot.is_some() {
            self.binding.bound
        } else {
            self.viewport
        }
    }
}

/// What a segment's shape and glyph batches bind: its viewport without its
/// turn, which its records and glyphs carry, the slot claimed for that and
/// its scissor. Members side by side bind the target's own space without a
/// scissor: their records and glyphs carry their offsets.
#[derive(Clone, Copy)]
pub(super) struct SegmentBinding {
    pub(super) uniform_slot: usize,
    pub(super) bound: ViewportUniformParams,
    pub(super) scissor: Option<(u32, u32, u32, u32)>,
}

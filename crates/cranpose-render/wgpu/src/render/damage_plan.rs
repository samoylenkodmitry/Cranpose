// What a frame changes against the frame its target kept, and what it draws
// for that.

use super::FrameRoot;
use crate::{
    frame::{Damage, Held, PresentedImage, Repaint},
    frame_packet::FramePacket,
    target_state::TargetDrawn,
};

/// The kept frame a frame's damage is measured against.
#[derive(Clone, Copy)]
pub(super) enum Keeper {
    /// The renderer's composition target.
    Composition,
    /// A swapchain image.
    Image(PresentedImage),
    /// The frame drawn last into an image that comes cleared; `keeping`
    /// when the composition target takes a copy of this frame to keep.
    Cleared { keeping: bool },
}

/// What a frame changes against the frame its target kept, what it draws
/// for that, and which target kept it.
#[derive(Clone, Copy)]
pub(super) struct FrameDamagePlan {
    pub(super) damage: Damage,
    pub(super) repaint: Repaint,
    pub(super) keeper: Keeper,
    /// The frame as its target holds it once drawn.
    pub(super) held: Held,
    /// Whether the frame draws whole as the first of its target's frames
    /// whose frame before is known.
    pub(super) baseline: bool,
}

impl FrameDamagePlan {
    // How `packet` went into the target the plan draws it into, through
    // `root`.
    pub(super) fn drawn(self, packet: &FramePacket, root: &FrameRoot) -> TargetDrawn {
        TargetDrawn {
            damage: self.damage,
            skipped: packet.skipped,
            knew_before: !self.baseline,
            cleared: matches!(self.keeper, Keeper::Cleared { .. })
                || matches!(
                    root,
                    FrameRoot::Composition {
                        cleared: Some(_),
                        ..
                    }
                ),
        }
    }
}

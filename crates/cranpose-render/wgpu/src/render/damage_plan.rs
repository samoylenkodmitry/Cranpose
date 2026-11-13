// What a frame changes against the frame its target kept, and what it draws
// for that.

use super::FrameRoot;
use crate::{
    frame::{ClearedImages, Damage, Held, PresentedImage, Repaint, SwapchainHistory},
    frame_packet::{FramePacket, HeldWindow},
    gpu_stats::{FrameDamage, FrameStats},
    target_state::TargetDrawn,
};

// The targets a window frame keeps its pixels in.
pub(super) struct Keepers<'a> {
    // The frame the composition target holds, when the renderer has one.
    pub(super) composition: Option<&'a mut Option<Held>>,
    pub(super) swapchain: &'a mut SwapchainHistory,
    pub(super) cleared: &'a mut ClearedImages,
}

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
    // Whether the target held no frame before this one.
    pub(super) unheld: bool,
}

impl FrameDamagePlan {
    // The frame as its target holds it once drawn; while it may show
    // `stand_ins`, with the parts it drew marked for the next frame to draw
    // again, the whole window when it drew whole.
    pub(super) fn held_with(self, stand_ins: bool) -> Held {
        match self.repaint {
            Repaint::Window(rects) if stand_ins => self.held.with_stand_ins(Damage::Part(rects)),
            Repaint::Whole if stand_ins => self.held.with_stand_ins(Damage::Full),
            _ => self.held,
        }
    }

    // Keeps the frame of the plan in its target as `held`, none when it did
    // not draw there; returns the oldest packet the window's next frame
    // finds in its target with the parts it draws again, `None` when a
    // target holds none known.
    pub(super) fn keep(self, held: Option<Held>, keepers: Keepers<'_>) -> Option<HeldWindow> {
        let Keepers {
            composition,
            swapchain,
            cleared,
        } = keepers;
        match self.keeper {
            Keeper::Composition => {
                let kept = composition?;
                *kept = held;
                held.map(Held::window)
            }
            Keeper::Image(image) => {
                swapchain.record(image, held);
                swapchain.held()
            }
            Keeper::Cleared { keeping } => {
                cleared.kept = held;
                let kept = composition?;
                if keeping {
                    cleared.hand_over(kept);
                }
                cleared.copied.then_some(*kept).flatten().map(Held::window)
            }
        }
    }

    // How `packet` went into the target the plan draws it into, through
    // `root`.
    pub(super) fn drawn(self, packet: &FramePacket, root: &FrameRoot) -> TargetDrawn {
        TargetDrawn {
            damage: self.damage,
            skipped: packet.skipped,
            knew_before: !self.baseline,
            unheld: self.unheld,
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

// Whether `plan` draws only where `packet` holds every draw.
pub(super) fn admits(plan: Option<&FrameDamagePlan>, packet: &FramePacket) -> bool {
    packet
        .culled
        .is_none_or(|region| plan.is_some_and(|plan| plan.repaint.within(&region)))
}

// Counts what a frame of `plan` changes and draws of a page of `viewport`
// in `stats`, and returns what it draws.
pub(super) fn count_damage(
    plan: Option<FrameDamagePlan>,
    viewport: (u32, u32),
    stats: &FrameStats,
) -> Repaint {
    stats
        .damage
        .set(plan.map_or(FrameDamage::Untracked, |plan| plan.damage.stats()));
    let repaint = plan.map_or(Repaint::Whole, |plan| plan.repaint);
    stats.repainted_pixels.set(repaint.pixels(viewport));
    repaint
}

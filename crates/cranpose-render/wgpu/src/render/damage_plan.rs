// What a frame changes against the frame its target kept, and what it draws
// for that.

use std::rc::Rc;

use super::{FrameOutput, FrameRoot, GpuRenderer, OutputMode, surface_is_direct_root};
use crate::{
    frame::{
        ClearedImages, Damage, FrameWindow, Held, ImageContent, OwnSake, PresentedImage, Repaint,
        Since, SwapchainHistory,
    },
    frame_packet::{FramePacket, HeldWindow},
    gpu_stats::{FrameDamage, FrameStats},
    offscreen::OffscreenTarget,
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
    // Why the frame draws whole for the target's sake.
    pub(super) own_sake: Option<OwnSake>,
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

    // How `packet` went into the target the plan draws it into.
    pub(super) fn drawn(self, packet: &FramePacket) -> TargetDrawn {
        TargetDrawn {
            damage: self.damage,
            skipped: packet.skipped,
            knew_before: !self.baseline,
            own_sake: self.own_sake,
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

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the window frame plan in damage_plan"
)]
impl GpuRenderer {
    /// Where the frame of `packet` renders: straight into an image that can
    /// take it, unless the image comes cleared and the composition target
    /// keeps the frame for it; else into the composition target.
    pub(super) fn frame_root(
        &mut self,
        output: FrameOutput<'_>,
        viewport: (u32, u32),
        packet: &FramePacket,
    ) -> FrameRoot {
        if let (OutputMode::Display, Some(view), Some(texture)) =
            (output.mode, output.view, output.texture)
            && surface_is_direct_root(
                texture,
                self.pipelines.composition_format,
                viewport,
                packet.reads_page(),
            )
        {
            let image = Rc::new(OffscreenTarget::from_surface(texture.clone(), view.clone()));
            if output.image == ImageContent::Cleared && self.cleared.copied {
                return FrameRoot::Composition {
                    composition: self.take_composition_target(viewport.0, viewport.1),
                    cleared: Some(image),
                };
            }
            return FrameRoot::Surface { image, keep: None };
        }
        FrameRoot::Composition {
            composition: self.take_composition_target(viewport.0.max(1), viewport.1.max(1)),
            cleared: None,
        }
    }

    /// What `packet` changes against the frame `frame_root` kept: a frame
    /// displayed through the composition target, or into a swapchain `image`
    /// that holds an earlier frame, draws only that, while a screenshot is
    /// drawn whole and so is a frame into any other presentable image. An
    /// image that comes cleared takes the frame from the composition target
    /// while the frames change a small part of a heavy window.
    pub(super) fn damage_of(
        &mut self,
        packet: &FramePacket,
        frame_root: &mut FrameRoot,
        output_mode: OutputMode,
        viewport: (u32, u32),
        (surface_epoch, image): (u64, ImageContent),
    ) -> Option<FrameDamagePlan> {
        let frame = self.frame_window(packet, viewport);
        let held = frame.held();
        let plan = |since: Since, keeper| FrameDamagePlan {
            damage: since.damage,
            repaint: since.damage.repaint(),
            keeper,
            held,
            baseline: since.baseline,
            own_sake: since.own_sake,
        };
        match frame_root {
            FrameRoot::Composition { composition, .. }
                if matches!(output_mode, OutputMode::Screenshot) =>
            {
                composition.kept = None;
                None
            }
            FrameRoot::Composition {
                composition,
                cleared: Some(cleared),
            } => {
                let damage =
                    self.cleared
                        .copied_damage(&self.frame_damages, composition.kept, &frame);
                if matches!(damage.damage, Damage::Part(_)) {
                    return Some(plan(damage, Keeper::Composition));
                }
                // The frame changes most of the window, or costs less drawn
                // whole than copied: it draws straight into the image, and
                // the composition target lets the frame go.
                self.cleared.copied = false;
                *frame_root = FrameRoot::Surface {
                    image: Rc::clone(cleared),
                    keep: None,
                };
                Some(FrameDamagePlan {
                    repaint: Repaint::Whole,
                    ..plan(damage, Keeper::Cleared { keeping: false })
                })
            }
            FrameRoot::Composition { composition, .. } => {
                // The swapchain image the frame is copied into holds a frame
                // the history does not follow.
                self.swapchain.forget();
                let damage = self.frame_damages.since(composition.kept, &frame);
                Some(plan(damage, Keeper::Composition))
            }
            FrameRoot::Surface { image: target, .. } => {
                let image = match image {
                    ImageContent::Untracked => return None,
                    ImageContent::Numbered(image) => image,
                    ImageContent::Cleared => {
                        let (damage, keeping) = self.cleared.damage(&self.frame_damages, &frame);
                        if keeping {
                            *frame_root = FrameRoot::Surface {
                                image: Rc::clone(target),
                                keep: Some(self.take_composition_target(viewport.0, viewport.1)),
                            };
                        }
                        return Some(FrameDamagePlan {
                            repaint: Repaint::Whole,
                            ..plan(damage, Keeper::Cleared { keeping })
                        });
                    }
                };
                self.swapchain.begin(surface_epoch);
                // The frame changes what the frame presented last showed by
                // its damage; it draws over what its image holds by the
                // damages since that image's frame, and whole into an image
                // that comes without the frame it held.
                let damage = self.swapchain.presented_damage(&frame, &self.frame_damages);
                let held = self
                    .swapchain
                    .changed_since(image, &frame, &self.frame_damages);
                Some(FrameDamagePlan {
                    repaint: held.repaint(),
                    ..plan(damage, Keeper::Image(image))
                })
            }
        }
    }

    // Whether `packet` draws what the surface shows, so it takes no image and
    // no present; the frame then closes as drawn for the renderer's warm-up.
    pub(crate) fn shows(&mut self, packet: &FramePacket, viewport: (u32, u32)) -> bool {
        self.frame_damages.note(packet.frame_id, packet.damage);
        let frame = self.frame_window(packet, viewport);
        let shown = self.swapchain.shows(&frame, &self.frame_damages);
        if shown {
            self.history.skip_frame();
        }
        shown
    }

    pub(super) fn frame_window<'p>(
        &self,
        packet: &'p FramePacket,
        viewport: (u32, u32),
    ) -> FrameWindow<'p> {
        FrameWindow {
            packet,
            viewport,
            setting: self.text.frame_setting(packet.clear, packet.debug_toggles),
        }
    }

    // Keeps the frame of `plan` just drawn into its target, with the parts
    // it drew marked while it may show a stand-in. A frame that draws
    // nothing into a presentable image submits nothing, and is drawn.
    pub(super) fn keep_window(
        &mut self,
        plan: Option<FrameDamagePlan>,
        submitted: &Result<bool, String>,
    ) -> Option<HeldWindow> {
        let plan = plan?;
        let drawn = match submitted {
            Ok(submitted) => *submitted || plan.repaint == Repaint::Nothing,
            Err(_) => false,
        };
        let stand_ins = self.frame_stats.drew_stand_in() || self.pipelines.stand_ins_kept.get();
        let keepers = Keepers {
            composition: self
                .composition_target
                .as_mut()
                .map(|target| &mut target.kept),
            swapchain: &mut self.swapchain,
            cleared: &mut self.cleared,
        };
        plan.keep(drawn.then(|| plan.held_with(stand_ins)), keepers)
    }

    /// Lets go of the copy of the last frame the renderer keeps for a
    /// surface whose images come cleared, as a WebGPU canvas's do: for a
    /// runtime whose app holds still, so the copy's window of memory goes.
    /// The next frame draws whole into its image.
    pub(crate) fn release_kept_copy(&mut self) {
        if !self.cleared.copied {
            return;
        }
        match self.composition_target.take() {
            Some(mut composition) => self.cleared.take_back(&mut composition.kept),
            None => self.cleared.copied = false,
        }
    }
}

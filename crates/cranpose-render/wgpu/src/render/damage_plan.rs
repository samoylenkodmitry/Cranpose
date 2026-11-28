// What a frame changes against the frame its target kept, and what it draws
// for that.

use std::rc::Rc;

use super::{FrameOutput, FrameRoot, GpuRenderer, OutputMode, surface_is_direct_root};
use crate::{
    frame::{
        ClearedImages, Damage, FrameWindow, Held, ImageContent, OwnSake, PresentedImage, Repaint,
        Since, StandInPixels, SwapchainHistory, keeps_copy, whole_when_lighter,
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
    // The renderer's composition target, copied whole into `image` when it
    // takes the frame for a swapchain image.
    Composition {
        image: Option<PresentedImage>,
    },
    /// A swapchain image.
    Image(PresentedImage),
    // The frame drawn last into an image whose pixels the renderer does not
    // know: one that comes cleared, or `image` of a swapchain; `keeping`
    // when the composition target takes a copy of this frame to keep.
    Cleared {
        keeping: bool,
        image: Option<PresentedImage>,
    },
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
    // The parts of the target's frame drawn with stand-ins that the frame
    // leaves as they are until a pipeline lands.
    pub(super) stand_ins: Damage,
}

impl FrameDamagePlan {
    // A frame that changes `since` in the target `keeper` names, which
    // holds `held` once the frame is drawn: it draws what it changes.
    pub(super) fn new(since: Since, keeper: Keeper, held: Held) -> Self {
        Self {
            damage: since.damage,
            repaint: since.damage.repaint(),
            keeper,
            held,
            baseline: since.baseline,
            own_sake: since.own_sake,
            stand_ins: kept_stand_ins(since),
        }
    }

    // The same frame drawn whole, whatever it changes.
    pub(super) fn whole(self) -> Self {
        Self {
            repaint: Repaint::Whole,
            stand_ins: Damage::NONE,
            ..self
        }
    }

    // The same frame drawn over a swapchain image, which changes `over`
    // there.
    fn over(self, over: Since) -> Self {
        Self {
            repaint: over.damage.repaint(),
            stand_ins: kept_stand_ins(over),
            ..self
        }
    }

    // Whether the frame leaves parts of its target drawn with stand-ins.
    pub(super) fn keeps_stand_ins(&self) -> bool {
        !self.stand_ins.is_none()
    }

    // The frame as its target holds it once drawn: with the stand-ins it
    // kept and, while it may show `stand_ins`, the parts it drew, the whole
    // window when it drew whole.
    pub(super) fn held_with(self, stand_ins: bool) -> Held {
        let drawn = match self.repaint {
            Repaint::Window(rects) if stand_ins => Damage::Part(rects),
            Repaint::Whole if stand_ins => Damage::Full,
            _ => Damage::NONE,
        };
        self.held
            .with_stand_ins(self.stand_ins)
            .with_stand_ins(drawn)
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
        let (window, image) = match self.keeper {
            Keeper::Image(image) => {
                swapchain.record(image, held);
                return swapchain.held();
            }
            Keeper::Composition { image } => {
                let window = composition.and_then(|kept| {
                    *kept = held;
                    held.map(Held::window)
                });
                (window, image)
            }
            Keeper::Cleared { keeping, image } => {
                cleared.kept = held;
                let window = composition.and_then(|kept| {
                    if keeping {
                        cleared.hand_over(kept);
                    }
                    cleared.copied.then_some(*kept).flatten().map(Held::window)
                });
                (window, image)
            }
        };
        let Some(image) = image else {
            return window;
        };
        swapchain.record(image, held);
        // Once no image can come holding no frame, the next frame draws
        // straight into its image, over the oldest frame they hold.
        if swapchain.may_hand_out_unknown() {
            window
        } else {
            swapchain.held()
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

// The stand-ins a frame that changes `since` keeps: none when it draws
// whole.
fn kept_stand_ins(since: Since) -> Damage {
    match since.damage {
        Damage::Part(_) => since.stand_ins,
        Damage::Full | Damage::Skipped => Damage::NONE,
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
    stats
        .kept_stand_in_pixels
        .set(plan.map_or(0, |plan| plan.stand_ins.repaint().pixels(viewport)));
    repaint
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the window frame plan in damage_plan"
)]
impl GpuRenderer {
    // Where the frame of `packet` renders, into `image` of the surface of
    // `surface_epoch`: straight into an image that can take it, unless the
    // image's pixels may not be known to the renderer and the composition
    // target keeps the frame for it; else into the composition target.
    pub(super) fn frame_root(
        &mut self,
        output: FrameOutput<'_>,
        viewport: (u32, u32),
        packet: &FramePacket,
        surface_epoch: u64,
    ) -> FrameRoot {
        if let ImageContent::Numbered(_) = output.image {
            self.swapchain.begin(surface_epoch);
        }
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
            if self.cleared.copied {
                if self.copy_serves(output.image) {
                    return FrameRoot::Composition {
                        composition: self.take_composition_target(viewport.0, viewport.1),
                        cleared: Some(image),
                    };
                }
                // Every image of the swapchain holds a frame it follows:
                // frames draw straight into their images again.
                if matches!(output.image, ImageContent::Numbered(_)) {
                    self.release_kept_copy();
                }
            }
            return FrameRoot::Surface { image, keep: None };
        }
        FrameRoot::Composition {
            composition: self.take_composition_target(viewport.0.max(1), viewport.1.max(1)),
            cleared: None,
        }
    }

    // Whether a frame into an image of `content` takes the frame from the
    // composition target's copy while the target keeps one: an image that
    // comes cleared, or any image of a swapchain that may still hand out
    // one holding no frame, so the copy stays the frame presented last.
    fn copy_serves(&self, content: ImageContent) -> bool {
        match content {
            ImageContent::Cleared => true,
            ImageContent::Numbered(_) => self.swapchain.may_hand_out_unknown(),
            ImageContent::Untracked => false,
        }
    }

    // What `packet` changes against the frame `frame_root` kept: a frame
    // displayed through the composition target, or into a swapchain `image`
    // that holds an earlier frame, draws only that, while a screenshot is
    // drawn whole and so is a frame into any other presentable image. An
    // image whose pixels the renderer does not know, one that comes cleared
    // or one a swapchain hands out before it holds a frame, takes the frame
    // from the composition target while the frames change a small part of a
    // heavy window. A frame whose scenes leave their depth with the host
    // draws whole.
    pub(super) fn damage_of(
        &mut self,
        packet: &FramePacket,
        frame_root: &mut FrameRoot,
        output_mode: OutputMode,
        viewport: (u32, u32),
        image: ImageContent,
    ) -> Option<FrameDamagePlan> {
        let plan = self.kept_damage(packet, frame_root, output_mode, viewport, image)?;
        Some(self.depth_targets.host_plan(plan))
    }

    // What `packet` changes against the frame `frame_root` kept, as
    // `damage_of` says, whatever the host's depth.
    fn kept_damage(
        &mut self,
        packet: &FramePacket,
        frame_root: &mut FrameRoot,
        output_mode: OutputMode,
        viewport: (u32, u32),
        image: ImageContent,
    ) -> Option<FrameDamagePlan> {
        let frame = self.frame_window(packet, viewport);
        let held = frame.held();
        let swapchain_image = match image {
            ImageContent::Numbered(image) => Some(image),
            ImageContent::Cleared | ImageContent::Untracked => None,
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
                // A culled packet holds the draws of a part of the window:
                // one that needs the whole window goes back unchanged, and
                // the whole packet decides.
                if keeps_copy(damage, &frame) || packet.culled.is_some() {
                    let keeper = Keeper::Composition {
                        image: swapchain_image,
                    };
                    return Some(FrameDamagePlan::new(damage, keeper, held));
                }
                // The frame changes most of the window, or costs less drawn
                // whole than copied: it draws straight into the image, and
                // the composition target lets the frame go.
                self.cleared.copied = false;
                *frame_root = FrameRoot::Surface {
                    image: Rc::clone(cleared),
                    keep: None,
                };
                let keeper = Keeper::Cleared {
                    keeping: false,
                    image: swapchain_image,
                };
                Some(FrameDamagePlan::new(damage, keeper, held).whole())
            }
            FrameRoot::Composition { composition, .. } => {
                // The swapchain image the frame is copied into holds a frame
                // the history does not follow.
                self.swapchain.forget();
                let damage = self.frame_damages.since(composition.kept, &frame);
                let keeper = Keeper::Composition { image: None };
                Some(FrameDamagePlan::new(damage, keeper, held))
            }
            FrameRoot::Surface { keep, .. } => {
                let plan = self.straight_damage(&frame, image)?;
                if let Keeper::Cleared { keeping: true, .. } = plan.keeper {
                    *keep = Some(self.take_composition_target(viewport.0, viewport.1));
                }
                Some(plan)
            }
        }
    }

    // What `frame`, drawn straight into `image`, changes there: over the
    // frame a swapchain image holds, or whole into an image whose pixels the
    // renderer does not know, which the composition target may keep a copy
    // of for the images that follow.
    fn straight_damage(
        &mut self,
        frame: &FrameWindow<'_>,
        image: ImageContent,
    ) -> Option<FrameDamagePlan> {
        let held = frame.held();
        let (since, keeping, image) = match image {
            ImageContent::Untracked => return None,
            ImageContent::Numbered(image) => {
                // The frame changes what the frame presented last showed by
                // its damage; it draws over what its image holds by the
                // damages since that image's frame.
                let damage = self.swapchain.presented_damage(frame, &self.frame_damages);
                if !(self.swapchain.holds_none(image) && self.swapchain.may_hand_out_unknown()) {
                    let over = self
                        .swapchain
                        .changed_since(image, frame, &self.frame_damages);
                    return Some(
                        FrameDamagePlan::new(damage, Keeper::Image(image), held).over(over),
                    );
                }
                // An image that comes without a frame while more such
                // images may follow takes the frame whole.
                let keeping = keeps_copy(whole_when_lighter(damage, frame), frame);
                (damage, keeping, Some(image))
            }
            ImageContent::Cleared => {
                let (damage, keeping) = self.cleared.damage(&self.frame_damages, frame);
                (damage, keeping, None)
            }
        };
        let keeper = Keeper::Cleared { keeping, image };
        Some(FrameDamagePlan::new(since, keeper, held).whole())
    }

    // Keeps what `packet`, of a window of `viewport`, changes, for the
    // targets that hold an earlier frame.
    pub(super) fn note_damage(&mut self, packet: &FramePacket, viewport: (u32, u32)) {
        self.frame_damages.note(packet.frame_id, &packet.damage);
        self.swapchain.fold(
            packet.frame_id,
            &packet.damage,
            viewport,
            &self.frame_damages,
        );
    }

    // Whether `packet` draws what the surface shows, so it takes no image and
    // no present; the frame then closes as drawn for the renderer's warm-up.
    pub(crate) fn shows(&mut self, packet: &FramePacket, viewport: (u32, u32)) -> bool {
        self.note_damage(packet, viewport);
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
            landings: self.pipelines.landings(),
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
        if plan.keeps_stand_ins() {
            self.pipelines.await_landing_over_stand_ins();
        }
        let drawn = match submitted {
            Ok(submitted) => *submitted || plan.repaint == Repaint::Nothing,
            Err(_) => false,
        };
        let stand_ins = self.frame_stats.drew_stand_in() || self.pipelines.stand_ins.kept();
        // Without a placeholder, fallback triangles or an instant draw kept,
        // the frame's stand-ins are general or universal pipelines, which
        // draw the bytes of their specializations.
        let pixels = if self.pipelines.stand_ins.kept() {
            StandInPixels::Interim
        } else {
            StandInPixels::Final
        };
        let keepers = Keepers {
            composition: self
                .composition_target
                .as_mut()
                .map(|target| &mut target.kept),
            swapchain: &mut self.swapchain,
            cleared: &mut self.cleared,
        };
        let held = drawn.then(|| plan.held_with(stand_ins).with_stand_in_pixels(pixels));
        plan.keep(held, keepers)
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

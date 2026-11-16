use super::{
    Capture, Damage, FrameDamages, FrameWindow, Held, PresentedImage, Since,
    captures::{collect, widen},
    settle,
};
use crate::frame_packet::HeldWindow;

/// The images of a swapchain the history follows.
const IMAGES: usize = 8;

/// The frames a swapchain's images hold: a frame drawn into an image that
/// holds an older frame draws the union of the damages since that frame.
#[derive(Default)]
pub(crate) struct SwapchainHistory {
    surface_epoch: u64,
    /// By image index, the frame the image holds.
    held: [Option<Held>; IMAGES],
    // By image index, a bit for each image the surface handed out.
    seen: u8,
    // How many images the surface hands out, and whether it handed one out
    // a second time: until it handed out that many, or one twice, the next
    // image may be one it never handed out, which holds no frame.
    images: u32,
    cycled: bool,
    // The frames since the surface handed an image out a second time.
    reused: u32,
    /// The frame presented last, when it went into an image the history
    /// follows and holds.
    presented: Option<Held>,
    // What the backdrops of the frame in progress read and write.
    captures: Vec<Capture>,
}

impl SwapchainHistory {
    /// Starts a frame on the surface of `surface_epoch`: a new surface's
    /// images hold none of the frames before.
    pub(crate) fn begin(&mut self, surface_epoch: u64) {
        if surface_epoch != self.surface_epoch {
            self.surface_epoch = surface_epoch;
            self.forget();
        }
    }

    // Forgets what the images hold.
    pub(crate) fn forget(&mut self) {
        self.held = [None; IMAGES];
        self.seen = 0;
        self.cycled = false;
        self.reused = 0;
        self.presented = None;
    }

    // Whether the surface may still hand out an image that holds no frame
    // the history follows: one it never handed out, or one that holds none.
    // A surface may hand an image out again before it handed out all of its
    // images, as a Metal layer that makes its drawables as frames need them
    // does: the history waits for the rest for as many frames as the
    // surface holds images.
    pub(crate) fn may_hand_out_unknown(&self) -> bool {
        let images = self.images.min(u8::BITS);
        let unseen = images == 0 || self.seen.count_ones() < images;
        let unknown = self
            .held
            .iter()
            .enumerate()
            .any(|(index, held)| self.seen & (1 << index) != 0 && held.is_none());
        (unseen || unknown) && self.reused < images.max(1)
    }

    // Whether `image` comes holding no frame the history follows.
    pub(crate) fn holds_none(&self, image: PresentedImage) -> bool {
        !image.kept
            || self
                .held
                .get(image.index as usize)
                .copied()
                .flatten()
                .is_none()
    }

    // The oldest frame the images the surface handed out hold, and the
    // parts of their frames drawn with stand-ins; `None` while one of them
    // holds none known, or while the next image may be one the surface never
    // handed out.
    pub(crate) fn held(&self) -> Option<HeldWindow> {
        if !self.cycled && self.seen.count_ones() < self.images.min(u8::BITS) {
            return None;
        }
        let mut window: Option<HeldWindow> = None;
        for (index, held) in self.held.iter().enumerate() {
            if self.seen & (1 << index) == 0 {
                continue;
            }
            let image = held.as_ref()?.window();
            match &mut window {
                Some(window) => {
                    window.oldest = window.oldest.min(image.oldest);
                    if !image.stand_ins.is_none() {
                        if window.stand_ins.is_none() {
                            window.landings = image.landings;
                        } else if window.landings != image.landings {
                            window.landings = None;
                        }
                    }
                    match (&mut window.stand_ins, image.stand_ins) {
                        (Damage::Part(parts), Damage::Part(more)) => parts.add_all(&more),
                        (stand_ins, _) => *stand_ins = Damage::Full,
                    }
                }
                None => window = Some(image),
            }
        }
        window
    }

    fn see(&mut self, image: PresentedImage) {
        self.images = image.images;
        if let Some(bit) = 1u8.checked_shl(image.index) {
            self.cycled |= self.seen & bit != 0;
            self.seen |= bit;
        }
        if self.cycled {
            self.reused = self.reused.saturating_add(1);
        }
    }

    // Whether `frame` draws what the surface shows: the frame presented last
    // on the frame's surface, into an image the history follows, with
    // nothing changed since. Such a frame needs no image and no present.
    pub(crate) fn shows(&self, frame: &FrameWindow<'_>, damages: &FrameDamages) -> bool {
        frame.packet.surface_epoch == self.surface_epoch
            && matches!(
                damages.since(self.presented, frame).damage,
                Damage::Part(changed) if changed.is_empty()
            )
    }

    /// What `frame` changes against the frame presented last.
    pub(crate) fn presented_damage(
        &self,
        frame: &FrameWindow<'_>,
        damages: &FrameDamages,
    ) -> Since {
        damages.since(self.presented, frame)
    }

    /// What `frame` changes in `image`: the union of the damages since the
    /// frame the image holds, grown by what the frame's backdrops read and
    /// write, as a backdrop over a pixel an earlier frame changed changes
    /// what it writes; with the parts of that frame whose stand-ins it keeps.
    pub(crate) fn changed_since(
        &mut self,
        image: PresentedImage,
        frame: &FrameWindow<'_>,
        damages: &FrameDamages,
    ) -> Since {
        let held = self.held.get(image.index as usize).copied().flatten();
        let since = damages.since(held.filter(|_| image.kept), frame);
        let Damage::Part(mut rects) = since.damage else {
            return since;
        };
        if rects.is_empty() || !frame.packet.root.contains_backdrop() {
            return since;
        }
        self.captures.clear();
        let place = (frame.packet.root_scale, frame.viewport);
        let damage = if collect(&frame.packet.root, place, &mut self.captures) {
            widen(&self.captures, &mut rects);
            settle(rects, frame.viewport)
        } else {
            Damage::Full
        };
        Since { damage, ..since }
    }

    /// Ends the frame drawn into `image`, which holds `held`: none when the
    /// frame did not draw there whole.
    pub(crate) fn record(&mut self, image: PresentedImage, held: Option<Held>) {
        self.see(image);
        if let Some(slot) = self.held.get_mut(image.index as usize) {
            *slot = held;
        }
        self.presented = held;
    }
}

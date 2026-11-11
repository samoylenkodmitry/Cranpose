use super::{
    Capture, Damage, FrameDamages, FrameWindow, Held, PresentedImage, Since,
    captures::{collect, widen},
    settle,
};

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
        self.presented = None;
    }

    // The oldest frame the images the surface handed out hold; `None`
    // while one of them holds none known.
    pub(crate) fn oldest(&self) -> Option<u64> {
        let mut oldest = None;
        for (index, held) in self.held.iter().enumerate() {
            if self.seen & (1 << index) != 0 {
                let frame = held.as_ref()?.frame();
                oldest = Some(oldest.map_or(frame, |oldest: u64| oldest.min(frame)));
            }
        }
        oldest
    }

    fn see(&mut self, image: PresentedImage) {
        if let Some(bit) = 1u8.checked_shl(image.index) {
            self.seen |= bit;
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
    /// what it writes.
    pub(crate) fn changed_since(
        &mut self,
        image: PresentedImage,
        frame: &FrameWindow<'_>,
        damages: &FrameDamages,
    ) -> Damage {
        let held = self.held.get(image.index as usize).copied().flatten();
        let damage = damages.since(held.filter(|_| image.kept), frame).damage;
        let Damage::Part(mut rects) = damage else {
            return damage;
        };
        if rects.is_empty() || !frame.packet.root.contains_backdrop() {
            return damage;
        }
        self.captures.clear();
        let place = (frame.packet.root_scale, frame.viewport);
        if !collect(&frame.packet.root, place, &mut self.captures) {
            return Damage::Full;
        }
        widen(&self.captures, &mut rects);
        settle(rects, frame.viewport)
    }

    /// Ends the frame `held` drawn into `image`: the image holds it when it
    /// was `drawn` whole.
    pub(crate) fn record(&mut self, image: PresentedImage, held: Held, drawn: bool) {
        self.see(image);
        let held = drawn.then_some(held);
        if let Some(slot) = self.held.get_mut(image.index as usize) {
            *slot = held;
        }
        self.presented = held;
    }
}

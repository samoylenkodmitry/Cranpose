use super::{Damage, DamageRects, KeptWindow, PresentedImage};

/// The frames back a kept swapchain image can be drawn again from, and the
/// images the history follows.
const IMAGE_HISTORY: usize = 8;

/// The frames a swapchain's images hold and each frame's damage against the
/// frame before: a frame drawn into an image that holds an older frame draws
/// the union of the damages since that frame.
#[derive(Default)]
pub(crate) struct SwapchainHistory {
    surface_epoch: u64,
    frame: u64,
    /// By image index, the frame the image holds.
    held: [Option<u64>; IMAGE_HISTORY],
    /// By frame modulo [`IMAGE_HISTORY`], the frame and its damage.
    damages: [Option<(u64, Damage)>; IMAGE_HISTORY],
    /// The summary of the frame presented last.
    pub(crate) kept: KeptWindow,
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

    /// Forgets what the images hold.
    pub(crate) fn forget(&mut self) {
        self.held = [None; IMAGE_HISTORY];
        self.kept.forget();
    }

    /// Forgets what `image` holds: a frame the history does not follow
    /// draws into it.
    pub(crate) fn drop_image(&mut self, image: PresentedImage) {
        if let Some(held) = self.held.get_mut(image.index as usize) {
            *held = None;
        }
    }

    /// The pixels that differ between the frame the kept `image` holds and a
    /// frame of `damage`, none when none does; `None` unless the image holds
    /// a frame whose damages since are all known.
    pub(crate) fn changed_since(
        &self,
        image: PresentedImage,
        damage: Damage,
    ) -> Option<DamageRects> {
        let held = self.held.get(image.index as usize).copied().flatten()?;
        let Damage::Part(mut union) = damage else {
            return None;
        };
        for frame in held + 1..=self.frame {
            let entry = self.damages[(frame % IMAGE_HISTORY as u64) as usize];
            let (_, Damage::Part(part)) = entry.filter(|(at, _)| *at == frame)? else {
                return None;
            };
            for rect in part.as_slice() {
                union.add(*rect);
            }
        }
        Some(union)
    }

    /// Ends a frame of `damage`: `image` holds it when it was `drawn` whole.
    pub(crate) fn record(&mut self, image: PresentedImage, damage: Damage, drawn: bool) {
        self.frame += 1;
        self.damages[(self.frame % IMAGE_HISTORY as u64) as usize] = Some((self.frame, damage));
        if let Some(held) = self.held.get_mut(image.index as usize) {
            *held = drawn.then_some(self.frame);
        }
    }
}

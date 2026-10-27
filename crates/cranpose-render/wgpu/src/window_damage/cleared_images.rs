use super::{COPY_AFTER, COPY_HOLD_OFF, Damage, FrameWindow, KeptWindow, Summaries, WindowDamage};

/// The frames drawn into presentable images that come cleared. While a page
/// whose items cover the window more than twice changes a small part of it,
/// the composition target keeps the frame, draws only what changes, and is
/// copied whole into each image; the first frame that changes most of the
/// window draws straight into its image again and lets the target go. The
/// images summarize first after [`COPY_HOLD_OFF`] frames.
#[derive(Default)]
pub(crate) struct ClearedImages {
    /// The summary of the frame drawn straight into an image last.
    pub(crate) kept: KeptWindow,
    /// The frames in a row drawn straight into an image that changed a
    /// small part of the window.
    small_frames: u32,
    /// Whether these images took a frame yet.
    started: bool,
    /// Whether the composition target keeps the frame for these images.
    pub(crate) copied: bool,
}

impl ClearedImages {
    /// What `frame`, drawn straight into an image, changes against the frame
    /// before, summarized by `summaries`; and whether the composition target
    /// is to keep a copy of it.
    pub(crate) fn damage(
        &mut self,
        summaries: &mut WindowDamage,
        frame: &FrameWindow<'_>,
    ) -> (Damage, bool) {
        if !self.started {
            self.started = true;
            summaries.hold_off(COPY_HOLD_OFF);
        }
        let damage = summaries.begin_as(frame, &mut self.kept, Summaries::COPIED);
        if !matches!(damage, Damage::Part(_)) {
            self.small_frames = 0;
            return (damage, false);
        }
        self.small_frames += 1;
        let keeps = self.small_frames >= COPY_AFTER;
        if keeps {
            self.small_frames = 0;
        }
        (damage, keeps)
    }

    /// Hands the summary of the frame just copied into the composition
    /// target to the target's `kept`. A frame that drew a stand-in leaves no
    /// summary, so the next frame draws whole and the target goes.
    pub(crate) fn hand_over(&mut self, target: &mut KeptWindow) {
        std::mem::swap(&mut self.kept, target);
        self.kept.forget();
        self.copied = true;
    }

    /// Takes back the summary of the frame the composition target kept,
    /// which goes: frames draw straight into their images again.
    pub(crate) fn take_back(&mut self, target: &mut KeptWindow) {
        std::mem::swap(&mut self.kept, target);
        self.copied = false;
        self.small_frames = 0;
    }
}

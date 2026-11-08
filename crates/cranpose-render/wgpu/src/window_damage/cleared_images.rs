use super::{
    COPY_AFTER, COPY_COVERAGE, COPY_HOLD_OFF, Damage, DeviceRect, FrameDamages, FrameWindow, Held,
    Since,
};
use crate::{collect::LayerScene, draw_pass::op_draw_bounds};

/// The frames drawn into presentable images that come cleared. While a page
/// whose draws cover the window more than twice changes a small part of it,
/// the composition target keeps the frame, draws only what changes, and is
/// copied whole into each image; the first frame that changes most of the
/// window draws straight into its image again and lets the target go. A
/// copy starts at the earliest after [`COPY_HOLD_OFF`] frames.
#[derive(Default)]
pub(crate) struct ClearedImages {
    /// The frame drawn straight into an image last.
    pub(crate) kept: Option<Held>,
    /// The frames in a row drawn straight into an image that changed a
    /// small part of the window.
    small_frames: u32,
    /// The frames these images took, up to past [`COPY_HOLD_OFF`].
    frames: u32,
    /// Whether the composition target keeps the frame for these images.
    pub(crate) copied: bool,
}

impl ClearedImages {
    /// What `frame`, drawn straight into an image, changes against the frame
    /// before; and whether the composition target is to keep a copy of it.
    pub(crate) fn damage(
        &mut self,
        damages: &FrameDamages,
        frame: &FrameWindow<'_>,
    ) -> (Since, bool) {
        // The images hold off for [`COPY_HOLD_OFF`] frames; the frame after
        // draws whole, as the one the next compares with.
        if self.frames <= COPY_HOLD_OFF {
            self.frames += 1;
            let baseline = self.frames > COPY_HOLD_OFF;
            let damage = if baseline {
                Damage::Full
            } else {
                Damage::Skipped
            };
            return (Since { damage, baseline }, false);
        }
        let since = self.copied_damage(damages, self.kept, frame);
        if !matches!(since.damage, Damage::Part(_)) {
            self.small_frames = 0;
            return (since, false);
        }
        self.small_frames += 1;
        let keeps = self.small_frames >= COPY_AFTER;
        if keeps {
            self.small_frames = 0;
        }
        (since, keeps)
    }

    /// What `frame`, drawn into the composition target that keeps the frame
    /// before for these images, changes there; a frame whose draws cover
    /// less than [`COPY_COVERAGE`] windows costs less drawn whole.
    pub(crate) fn copied_damage(
        &self,
        damages: &FrameDamages,
        kept: Option<Held>,
        frame: &FrameWindow<'_>,
    ) -> Since {
        let since = damages.since(kept, frame);
        match since.damage {
            Damage::Part(_) if covers_less(frame) => Since {
                damage: Damage::Full,
                ..since
            },
            _ => since,
        }
    }

    /// Hands the frame just copied into the composition target to the
    /// target's `kept`. A frame that drew a stand-in leaves none, so the
    /// next frame draws whole and the target goes.
    pub(crate) fn hand_over(&mut self, target: &mut Option<Held>) {
        *target = self.kept.take();
        self.copied = true;
    }

    /// Takes back the frame the composition target kept, which goes: frames
    /// draw straight into their images again.
    pub(crate) fn take_back(&mut self, target: &mut Option<Held>) {
        self.kept = target.take();
        self.copied = false;
        self.small_frames = 0;
    }
}

/// Whether the draws of `frame` cover fewer than [`COPY_COVERAGE`] windows
/// of pixels, a pixel counted once for each draw over it.
fn covers_less(frame: &FrameWindow<'_>) -> bool {
    let (width, height) = frame.viewport;
    let window = DeviceRect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    };
    let scale = frame.packet.root_scale;
    let mut covered = coverage(&frame.packet.root, scale, window);
    if let Some(overlay) = frame.packet.overlay.as_deref() {
        covered += coverage(overlay, scale, window);
    }
    covered < COPY_COVERAGE * window.width * window.height
}

// The window pixels the root-level draws and composites of `layer` cover.
fn coverage(layer: &LayerScene, scale: f32, window: DeviceRect) -> f32 {
    let area = |rect: cranpose_ui_graphics::Rect| {
        DeviceRect::from_logical(rect, scale)
            .intersect(window)
            .map_or(0.0, |rect| rect.width * rect.height)
    };
    let ops: f32 = layer
        .scene
        .draw_ops
        .iter()
        .filter_map(|op| op_draw_bounds(&layer.scene, op, scale))
        .map(area)
        .sum();
    let children: f32 = layer
        .children
        .iter()
        .map(|child| area(child.transform.bounds_for_rect(child.local_bounds)))
        .sum();
    ops + children
}

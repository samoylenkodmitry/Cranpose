use super::{COPY_COVERAGE, Damage, DeviceRect, FrameDamages, FrameWindow, Held, OwnSake, Since};
use crate::{collect::LayerScene, draw_pass::op_draw_bounds};

// The frames drawn into presentable images whose pixels the renderer does
// not know: images that come cleared, and the images a swapchain hands out
// while one may come holding no frame. From the first frame of a page whose
// draws cover the window more than twice, and from the first frame that
// changes a small part of such a page, the composition target keeps the
// frame, draws only what changes, and is copied whole into each image; the
// first frame that changes most of the window draws straight into its image
// again and lets the target go.
#[derive(Default)]
pub(crate) struct ClearedImages {
    /// The frame drawn straight into an image last.
    pub(crate) kept: Option<Held>,
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
        let since = self.copied_damage(damages, self.kept, frame);
        (since, keeps_copy(since, frame))
    }

    /// What `frame`, drawn into the composition target that keeps the frame
    /// before for these images, changes there; a frame whose draws cover
    /// less than [`COPY_COVERAGE`] windows costs less drawn whole, unless its
    /// packet holds the draws of a part of the window only.
    pub(crate) fn copied_damage(
        &self,
        damages: &FrameDamages,
        kept: Option<Held>,
        frame: &FrameWindow<'_>,
    ) -> Since {
        whole_when_lighter(damages.since(kept, frame), frame)
    }

    /// Hands the frame just copied into the composition target to the
    /// target's `kept`. Over a frame drawn whole with a stand-in, the next
    /// frame draws whole and the target goes.
    pub(crate) fn hand_over(&mut self, target: &mut Option<Held>) {
        *target = self.kept.take();
        self.copied = true;
    }

    /// Takes back the frame the composition target kept, which goes: frames
    /// draw straight into their images again.
    pub(crate) fn take_back(&mut self, target: &mut Option<Held>) {
        self.kept = target.take();
        self.copied = false;
    }
}

// `since`, or the whole window for a frame whose draws cover less than
// COPY_COVERAGE windows: it costs less drawn whole than copied, unless its
// packet holds the draws of a part of the window only.
pub(crate) fn whole_when_lighter(since: Since, frame: &FrameWindow<'_>) -> Since {
    match since.damage {
        Damage::Part(_) if frame.packet.culled.is_none() && covers_less(frame) => Since {
            damage: Damage::Full,
            ..since
        },
        _ => since,
    }
}

// Whether the composition target keeps a copy of `frame`, drawn whole
// straight into an image whose pixels the renderer does not know, for such
// images after it, the frame changing `since` against the frame before, as
// `whole_when_lighter` has it: it changes a small part of a page that costs
// more drawn whole than copied, or it is the first frame of such a page. A
// start draws its first frame whole once and copies it into the images that
// follow, where each image would otherwise take the whole window again.
pub(crate) fn keeps_copy(since: Since, frame: &FrameWindow<'_>) -> bool {
    match since.damage {
        Damage::Part(_) => true,
        Damage::Full => since.own_sake == Some(OwnSake::NoFrame) && !covers_less(frame),
        Damage::Skipped => false,
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

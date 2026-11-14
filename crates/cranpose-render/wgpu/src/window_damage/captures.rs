// What a frame's backdrops read and write, in window pixels: a changed pixel
// a backdrop reads changes every pixel it writes, and a backdrop drawn again
// reads the page beneath it again, which must be drawn again then too.

use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_ui_graphics::{Rect, expand_rect};

use super::{
    super::{MAX_RESOLVE_DEPTH, clipped, composites_nothing},
    DamageRects, DeviceRect, snap_delta, window_rect,
};
use crate::collect::{ChildLayer, LayerScene};

/// What one backdrop reads and writes, in window pixels.
#[derive(Clone, Copy)]
pub(crate) struct Capture {
    reads: DeviceRect,
    writes: DeviceRect,
}

// Where a layer's content lands in the window.
#[derive(Clone, Copy)]
struct Place {
    // From the layer's logical space to window pixels.
    to_window: ProjectiveTransform,
    // The window pixels the layer's clips leave.
    clip: DeviceRect,
    // The device pixels per logical unit the layer is drawn at.
    scale: f32,
    depth: usize,
}

impl Place {
    // The window pixels `rect` of the layer's space may touch: a pixel of
    // the layer's raster and a window pixel past it all around, for the
    // filtering and snapping between them.
    fn reach(&self, rect: Rect) -> DeviceRect {
        let bounds = self
            .to_window
            .bounds_for_rect(expand_rect(rect, 1.0 / self.scale));
        DeviceRect {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
        .expand(1.0)
    }

    fn window(&self, rect: Rect) -> Option<DeviceRect> {
        self.reach(rect).intersect(self.clip)
    }
}

/// Puts the captures of the frame drawn from `root` at `scale` into a
/// window of `viewport` into `captures`; false when the frame holds what
/// they cannot describe.
pub(crate) fn collect(
    root: &LayerScene,
    (scale, viewport): (f32, (u32, u32)),
    captures: &mut Vec<Capture>,
) -> bool {
    let place = Place {
        to_window: ProjectiveTransform::uniform_scale(scale),
        clip: window_rect(viewport),
        scale,
        depth: 0,
    };
    layer(root, &place, captures)
}

fn layer(layer: &LayerScene, place: &Place, captures: &mut Vec<Capture>) -> bool {
    if place.depth >= MAX_RESOLVE_DEPTH {
        return false;
    }
    let scene = &layer.scene;
    for backdrop in scene
        .backdrop_layers
        .iter()
        .filter(|layer| layer.alpha != 0.0)
    {
        let snap = snap_delta(backdrop.snap_anchor, place.scale);
        let rect = backdrop.rect.translate(snap.x, snap.y);
        let reach = expand_rect(rect, backdrop.effect.output_padding());
        let clip = backdrop.clip.map(|clip| clip.translate(snap.x, snap.y));
        if let Some(visible) = clipped(reach, clip) {
            let reads = expand_rect(visible, backdrop.effect.input_padding());
            let first = captures.len();
            captures.push(Capture {
                reads: place.reach(reads),
                writes: place.reach(visible),
            });
            // An effect range around the backdrop spreads a change of it
            // over all the effect writes.
            for effect in &scene.effect_layers {
                if (effect.z_start..effect.z_end).contains(&backdrop.z_index) {
                    let snap = snap_delta(effect.snap_anchor, place.scale);
                    let padding = effect
                        .effect
                        .as_ref()
                        .map_or(0.0, cranpose_ui_graphics::RenderEffect::output_padding);
                    let written = expand_rect(effect.rect.translate(snap.x, snap.y), padding);
                    let clip = effect.clip.map(|clip| clip.translate(snap.x, snap.y));
                    if let Some(writes) = clipped(written, clip).and_then(|rect| place.window(rect))
                    {
                        spread(&mut captures[first..], writes);
                    }
                }
            }
        }
    }
    layer
        .children
        .iter()
        .all(|child| self::child(child, place, captures))
}

fn child(child: &ChildLayer, place: &Place, captures: &mut Vec<Capture>) -> bool {
    if !child.reads_backdrop() || composites_nothing(child) {
        return true;
    }
    if child.mesh.is_some() {
        return false;
    }
    let Some(clip) = child.clip.map_or(Some(place.clip), |clip| {
        place.reach(clip).intersect(place.clip)
    }) else {
        return true;
    };
    let inner = Place {
        to_window: child.transform.then(place.to_window),
        clip,
        scale: place.scale * child.surface_scale,
        depth: place.depth + 1,
    };
    let first = captures.len();
    if !layer(&child.content, &inner, captures) {
        return false;
    }
    let output = match child
        .backdrop
        .as_ref()
        .filter(|_| child.backdrop_alpha != 0.0)
    {
        Some(backdrop) => {
            let output = backdrop.output_padding();
            captures.push(Capture {
                reads: inner.reach(expand_rect(
                    child.local_bounds,
                    backdrop.input_padding() + output,
                )),
                writes: inner.reach(expand_rect(child.local_bounds, output)),
            });
            output
        }
        None => 0.0,
    };
    // An effect spreads a change of what lies inside over all the child
    // writes; what its content draws past its bounds is not known here, so
    // that is all the child may write within its clips.
    if child.effect.is_some() {
        let padding = child.effect.as_ref().map_or(0.0, |effect| {
            effect.input_padding() + effect.output_padding()
        });
        let writes = if child.rounded_clip.is_some() {
            inner.window(expand_rect(child.local_bounds, padding + output))
        } else {
            Some(inner.clip)
        };
        if let Some(writes) = writes {
            spread(&mut captures[first..], writes);
        }
    }
    true
}

fn spread(captures: &mut [Capture], writes: DeviceRect) {
    for capture in captures {
        capture.writes = capture.writes.union(writes);
    }
}

/// Grows `damage` by all that each capture touching it reads and writes,
/// until no capture touches more.
pub(crate) fn widen(captures: &[Capture], damage: &mut DamageRects) {
    loop {
        let mut grew = false;
        for capture in captures {
            let touched = damage.meets(capture.reads) || damage.meets(capture.writes);
            let both = capture.reads.union(capture.writes);
            if touched && !damage.holds(both) {
                damage.add(both);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
}

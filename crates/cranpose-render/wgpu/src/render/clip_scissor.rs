use super::{TargetRect, ViewportUniformParams};
use crate::scene::Placement;

/// The target pixels a clipped shape variant keeps of `placement`'s records
/// under `viewport`, as a scissor that stands in for its clip test: every
/// pixel whose centre, moved by the viewport's offset, lies inside the
/// device clip, inclusive of its edges, as the shader compares them. `None`
/// without a clip or under a turn, where the variant tests the clip itself.
/// An empty clip keeps an empty scissor.
fn clip_scissor(
    placement: &Placement,
    root_scale: f32,
    viewport: ViewportUniformParams,
) -> Option<TargetRect> {
    if !viewport.transform.is_identity() {
        return None;
    }
    let [x, y, width, height] = placement.device_clip(root_scale)?;
    let columns = kept_pixels(x, x + width, viewport.offset[0], viewport.width);
    let rows = kept_pixels(y, y + height, viewport.offset[1], viewport.height);
    Some((
        columns.start,
        rows.start,
        columns.end - columns.start,
        rows.end - rows.start,
    ))
}

// The last clip scissor each kind of run worked out, with its key. The arena
// runs of a pass come in rows under one clip, and the stored runs between them
// keep theirs apart.
#[derive(Default)]
pub(crate) struct Last {
    arena: Option<(Key, TargetRect)>,
    stored: Option<(Key, TargetRect)>,
}

impl Last {
    pub(crate) fn arena(
        &mut self,
        placement: &Placement,
        root_scale: f32,
        viewport: &ViewportUniformParams,
    ) -> Option<TargetRect> {
        // Most runs have no clip, and return before the call.
        placement.clip?;
        kept_scissor(&mut self.arena, placement, root_scale, viewport)
    }

    pub(crate) fn stored(
        &mut self,
        placement: &Placement,
        root_scale: f32,
        viewport: &ViewportUniformParams,
    ) -> Option<TargetRect> {
        kept_scissor(&mut self.stored, placement, root_scale, viewport)
    }
}

// The scissor of `placement`'s clip, from `kept` when its key matches, else
// worked out and kept. Out of line: both kinds of run share one copy.
#[inline(never)]
fn kept_scissor(
    kept: &mut Option<(Key, TargetRect)>,
    placement: &Placement,
    root_scale: f32,
    viewport: &ViewportUniformParams,
) -> Option<TargetRect> {
    let key = Key::of(placement, root_scale, viewport)?;
    if let Some((last, scissor)) = kept
        && *last == key
    {
        return Some(*scissor);
    }
    let scissor = clip_scissor(placement, root_scale, *viewport)?;
    *kept = Some((key, scissor));
    Some(scissor)
}

// Every input of a clip's scissor, so equal keys keep equal pixels. The raw
// clip stands in for its device rect, whose snapping costs more than a compare.
// Floats are kept as their bits: equal bits keep equal pixels, and integers
// compare in less code.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    clip: [u32; 4],
    root_scale: u32,
    offset: [u32; 2],
    extent: (u32, u32),
    snapped: bool,
}

impl Key {
    fn of(
        placement: &Placement,
        root_scale: f32,
        viewport: &ViewportUniformParams,
    ) -> Option<Self> {
        if !viewport.transform.is_identity() {
            return None;
        }
        let clip = placement.clip?;
        Some(Self {
            clip: [clip.x, clip.y, clip.width, clip.height].map(f32::to_bits),
            root_scale: root_scale.to_bits(),
            offset: viewport.offset.map(f32::to_bits),
            extent: (viewport.width, viewport.height),
            snapped: placement.snap_anchor.is_some(),
        })
    }
}

/// The pixels along one axis whose centre, moved by `offset`, lies within
/// `low..=high`, within `0..extent`: stepped with the shader's own float
/// sums so the edges fall where its comparisons put them.
fn kept_pixels(low: f32, high: f32, offset: f32, extent: u32) -> std::ops::Range<u32> {
    let start = first_past(low, true, (0, extent), offset);
    start..first_past(high, false, (start, extent), offset)
}

// The first pixel of `from..extent` whose centre, moved by `offset`, lies past
// `edge`, or on it when `on` says so; `extent` when none does. Out of line:
// both edges of both axes share one copy.
#[inline(never)]
fn first_past(edge: f32, on: bool, (from, extent): (u32, u32), offset: f32) -> u32 {
    let centre = |pixel: u32| pixel as f32 + 0.5 + offset;
    let past = |pixel: u32| {
        if on {
            centre(pixel) >= edge
        } else {
            centre(pixel) > edge
        }
    };
    let short = |pixel: u32| {
        if on {
            centre(pixel) < edge
        } else {
            centre(pixel) <= edge
        }
    };
    let guess = (edge - offset - 0.5).ceil().clamp(0.0, extent as f32) as u32;
    let mut pixel = guess.max(from);
    while pixel > from && past(pixel - 1) {
        pixel -= 1;
    }
    while pixel < extent && short(pixel) {
        pixel += 1;
    }
    pixel
}

#[cfg(test)]
#[path = "tests/clip_scissor_tests.rs"]
mod tests;

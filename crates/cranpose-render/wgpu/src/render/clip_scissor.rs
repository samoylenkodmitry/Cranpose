use super::{TargetRect, ViewportUniformParams};
use crate::scene::Placement;

/// The target pixels a clipped shape variant keeps of `placement`'s records
/// under `viewport`, as a scissor that stands in for its clip test: every
/// pixel whose centre, moved by the viewport's offset, lies inside the
/// device clip, inclusive of its edges, as the shader compares them. `None`
/// without a clip or under a turn, where the variant tests the clip itself.
/// An empty clip keeps an empty scissor.
pub(crate) fn clip_scissor(
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

/// The pixels along one axis whose centre, moved by `offset`, lies within
/// `low..=high`, within `0..extent`: stepped with the shader's own float
/// sums so the edges fall where its comparisons put them.
fn kept_pixels(low: f32, high: f32, offset: f32, extent: u32) -> std::ops::Range<u32> {
    let centre = |pixel: u32| pixel as f32 + 0.5 + offset;
    let guess = |edge: f32| (edge - offset - 0.5).ceil().clamp(0.0, extent as f32) as u32;
    let mut start = guess(low);
    while start > 0 && centre(start - 1) >= low {
        start -= 1;
    }
    while start < extent && centre(start) < low {
        start += 1;
    }
    let mut end = guess(high).max(start);
    while end > start && centre(end - 1) > high {
        end -= 1;
    }
    while end < extent && centre(end) <= high {
        end += 1;
    }
    start..end
}

#[cfg(test)]
#[path = "tests/clip_scissor_tests.rs"]
mod tests;

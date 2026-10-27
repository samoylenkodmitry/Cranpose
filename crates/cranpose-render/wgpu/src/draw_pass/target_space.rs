// A composite's geometry moved from its segment's device space into the
// target's, which starts at the segment's offset.

use crate::effect_renderer::RoundedCompositeMask;

pub(super) fn dest_in_target(
    dest: (f32, f32, f32, f32),
    segment_offset: [f32; 2],
) -> (f32, f32, f32, f32) {
    (
        dest.0 - segment_offset[0],
        dest.1 - segment_offset[1],
        dest.2,
        dest.3,
    )
}

pub(super) fn mask_in_target(
    mask: Option<RoundedCompositeMask>,
    segment_offset: [f32; 2],
) -> Option<RoundedCompositeMask> {
    mask.map(|mask| RoundedCompositeMask {
        rect: [
            mask.rect[0] - segment_offset[0],
            mask.rect[1] - segment_offset[1],
            mask.rect[2],
            mask.rect[3],
        ],
        radii: mask.radii,
    })
}

/// Re-bases an inverse (target pixel -> source pixel) matrix onto a target
/// whose origin is `offset` pixels into the space the matrix was built for.
pub(super) fn translate_inverse(inverse: [[f32; 3]; 3], offset: [f32; 2]) -> [[f32; 3]; 3] {
    let mut shifted = inverse;
    for row in &mut shifted {
        row[2] += row[0] * offset[0] + row[1] * offset[1];
    }
    shifted
}

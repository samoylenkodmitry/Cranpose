use super::PassSegment;

/// Changes between flat and turned segments a pass takes before its shapes
/// draw with the pipeline that tells them apart per record: below it, a draw
/// per change costs less than the flag test every fragment pays there.
const MIXED_TURN_CHANGES: usize = 8;

/// Whether the flat and turned segments of a pass alternate at least
/// `MIXED_TURN_CHANGES` times, as a grid of cells tilted by turns does when
/// some cells are not tilted.
pub(super) fn turns_mixed(segments: &[PassSegment<'_>]) -> bool {
    segments
        .windows(2)
        .filter(|pair| pair[0].transform.is_identity() != pair[1].transform.is_identity())
        .count()
        >= MIXED_TURN_CHANGES
}

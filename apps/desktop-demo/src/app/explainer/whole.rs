// The whole numbers the explainer reads off its floats and its counts: how
// many cells a length covers, which cell a point lies in and a count as the
// `u32` a mesh or a hash holds. A float is cut toward zero. A NaN is zero. A
// float past the range of the type stops at its end.

// The cells a length of them covers: none for a negative length.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a length is cut to whole cells, and a negative one covers none"
)]
pub(super) fn count(length: f32) -> usize {
    length as usize
}

// The cell a point lies in, counted from the first cell: negative before it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a point is cut to the whole cell it lies in"
)]
pub(super) fn offset(point: f32) -> isize {
    point as isize
}

// A count or a place in a list as the `u32` that a mesh or a hash holds. The
// explainer's counts lie far below the range of a `u32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a usize on the web is as wide as a u32, so the lint fires on the wider targets only"
)]
pub(super) fn narrow(count: usize) -> u32 {
    count as u32
}

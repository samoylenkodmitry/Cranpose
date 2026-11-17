// The whole numbers the explainer reads off its floats: how many cells a
// length covers and which cell a point lies in. A float is cut toward zero.
// A NaN is zero. A float past the range of the type stops at its end.

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

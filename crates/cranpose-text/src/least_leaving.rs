// The least width that leaves a width once another is subtracted from it,
// in the f32 arithmetic a layout compares widths in.

// The least width that leaves at least `left` once `taken` is subtracted
// from it, as a layout takes an indent off the width it wraps at. A hold
// whose ends are found this way changes exactly where the comparisons of a
// fresh layout change, so a width at either end of it needs no margin.
// Most lines take no indent, so the check for one stays inline and the
// search out of line.
#[inline]
pub(crate) fn least_leaving(left: f32, taken: f32) -> f32 {
    // With nothing taken, a width leaves itself.
    if taken == 0.0 {
        left
    } else {
        searched(left, taken)
    }
}

// The least width that leaves `left` once a `taken` other than none is
// subtracted from it.
#[inline(never)]
fn searched(left: f32, taken: f32) -> f32 {
    let estimate = left + taken;
    if !estimate.is_finite() {
        return estimate;
    }
    let place = |width: f32| turned(width.to_bits().cast_signed());
    let leaves = |place: i32| f32::from_bits(turned(place).cast_unsigned()) - taken >= left;
    // The estimate is the least width or a rounding off it, but a `taken`
    // far wider than the width moves the difference only once in many
    // places: the search steps away from the estimate in doubling steps
    // until the answer flips, then halves the steps back.
    let first = place(estimate);
    let leaving = leaves(first);
    let toward = if leaving { -1 } else { 1 };
    let (mut near, mut step) = (first, 1);
    let far = loop {
        let far = near
            .saturating_add(toward * step)
            .clamp(place(f32::NEG_INFINITY), place(f32::INFINITY));
        if leaves(far) != leaving {
            break far;
        }
        near = far;
        step = step.saturating_mul(2);
    };
    let (mut short, mut enough) = if leaving { (far, near) } else { (near, far) };
    while short.abs_diff(enough) > 1 {
        let middle = short.midpoint(enough);
        if leaves(middle) {
            enough = middle;
        } else {
            short = middle;
        }
    }
    f32::from_bits(turned(enough).cast_unsigned())
}

// The place of a width among all widths, from the least up, from its bits,
// and the bits back from a place: the bits of a negative width count down
// as it grows, so they are turned around, which the same turn undoes.
fn turned(bits: i32) -> i32 {
    bits ^ ((bits >> 31) & i32::MAX)
}

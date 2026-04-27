#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::slot) enum CheckedU32Delta {
    Add(u32),
    Sub(u32),
}

impl CheckedU32Delta {
    #[inline]
    pub(in crate::slot) fn from_i64(delta: i64, field: &'static str) -> Self {
        if delta >= 0 {
            Self::Add(
                u32::try_from(delta).unwrap_or_else(|_| panic_u32_delta_out_of_range(field, delta)),
            )
        } else {
            Self::Sub(
                u32::try_from(delta.unsigned_abs())
                    .unwrap_or_else(|_| panic_u32_delta_out_of_range(field, delta)),
            )
        }
    }

    #[inline]
    fn as_i64(self) -> i64 {
        match self {
            Self::Add(delta) => i64::from(delta),
            Self::Sub(delta) => -i64::from(delta),
        }
    }
}

#[inline]
pub(crate) fn checked_usize_to_u32(value: usize, field: &'static str) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| panic!("{field} exceeds u32 storage limit: {value}"))
}

#[inline]
pub(in crate::slot) fn checked_usize_to_i64(value: usize, field: &'static str) -> i64 {
    i64::try_from(value)
        .unwrap_or_else(|_| panic!("{field} exceeds i64 mutation delta limit: {value}"))
}

#[inline]
pub(in crate::slot) fn checked_u32_delta(
    value: u32,
    delta: CheckedU32Delta,
    min: u32,
    field: &'static str,
) -> u32 {
    let updated = match delta {
        CheckedU32Delta::Add(delta) => value
            .checked_add(delta)
            .unwrap_or_else(|| panic_u32_delta_overflow(field, value, delta)),
        CheckedU32Delta::Sub(delta) => value
            .checked_sub(delta)
            .unwrap_or_else(|| panic_u32_delta_below_min(field, value, -i64::from(delta), min)),
    };
    if updated < min {
        panic_u32_delta_below_min(field, value, delta.as_i64(), min);
    }
    updated
}

#[cold]
#[inline(never)]
fn panic_u32_delta_out_of_range(field: &'static str, delta: i64) -> ! {
    panic!("{field} delta exceeds u32 storage limit: {delta}");
}

#[cold]
#[inline(never)]
fn panic_u32_delta_overflow(field: &'static str, value: u32, delta: u32) -> ! {
    panic!("{field} mutation overflow: {value} + {delta}");
}

#[cold]
#[inline(never)]
fn panic_u32_delta_below_min(
    field: &'static str,
    value: u32,
    delta: impl Into<i64>,
    min: u32,
) -> ! {
    let delta = delta.into();
    panic!("{field} cannot become smaller than {min}: {value} + {delta}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_usize_to_u32_accepts_u32_max() {
        assert_eq!(
            checked_usize_to_u32(u32::MAX as usize, "test field"),
            u32::MAX
        );
    }

    #[test]
    #[should_panic(expected = "test field exceeds u32 storage limit")]
    fn checked_usize_to_u32_rejects_overflow() {
        checked_usize_to_u32(u32::MAX as usize + 1, "test field");
    }

    #[test]
    fn checked_u32_delta_applies_positive_and_negative_deltas() {
        assert_eq!(
            checked_u32_delta(
                10,
                CheckedU32Delta::from_i64(5, "test field"),
                0,
                "test field"
            ),
            15
        );
        assert_eq!(
            checked_u32_delta(
                10,
                CheckedU32Delta::from_i64(-5, "test field"),
                0,
                "test field"
            ),
            5
        );
    }

    #[test]
    #[should_panic(expected = "test field cannot become smaller than 1")]
    fn checked_u32_delta_rejects_values_below_minimum() {
        checked_u32_delta(
            1,
            CheckedU32Delta::from_i64(-1, "test field"),
            1,
            "test field",
        );
    }
}

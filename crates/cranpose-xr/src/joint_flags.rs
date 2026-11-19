//! How the runtime marks a located joint.

// OpenXR's `XrSpaceLocationFlags`.
const ORIENTATION_VALID: u64 = 0x1;
const POSITION_VALID: u64 = 0x2;
const ORIENTATION_TRACKED: u64 = 0x4;
const POSITION_TRACKED: u64 = 0x8;

// Whether a joint's pose is usable, and whether the runtime measures it now. A pose is usable when
// both its position and its turn are valid, and measured when both are tracked. A joint that is
// valid and not tracked holds the runtime's estimate.
pub(crate) const fn joint_flags(location_flags: u64) -> (bool, bool) {
    let valid = POSITION_VALID | ORIENTATION_VALID;
    let tracked = POSITION_TRACKED | ORIENTATION_TRACKED;
    (
        location_flags & valid == valid,
        location_flags & tracked == tracked,
    )
}

#[cfg(test)]
#[path = "tests/joint_flags_tests.rs"]
mod tests;

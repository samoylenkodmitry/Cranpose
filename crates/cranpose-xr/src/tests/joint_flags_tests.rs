use super::*;

#[test]
fn a_joint_is_valid_only_with_position_and_turn_and_tracked_only_with_both_measured() {
    // OpenXR: orientation valid 1, position valid 2, orientation tracked 4, position tracked 8
    assert_eq!(joint_flags(0), (false, false));
    assert_eq!(joint_flags(0x3), (true, false));
    assert_eq!(joint_flags(0x1), (false, false));
    assert_eq!(joint_flags(0x2), (false, false));
    assert_eq!(joint_flags(0xF), (true, true));
    assert_eq!(joint_flags(0xC), (false, true));
    assert_eq!(joint_flags(0x7), (true, false));
}

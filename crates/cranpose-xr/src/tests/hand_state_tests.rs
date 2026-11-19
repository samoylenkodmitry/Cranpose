use super::*;
use crate::{Hand, XrFrame};

#[test]
fn the_joints_sit_in_openxrs_order_with_the_palm_and_the_wrist_first() {
    assert_eq!(HandJointType::Palm.index(), 0);
    assert_eq!(HandJointType::Wrist.index(), 1);
    assert_eq!(HandJointType::ThumbMetacarpal.index(), 2);
    assert_eq!(HandJointType::IndexTip.index(), 10);
    assert_eq!(HandJointType::MiddleMetacarpal.index(), 11);
    assert_eq!(HandJointType::RingMetacarpal.index(), 16);
    assert_eq!(HandJointType::LittleTip.index(), 25);
    assert_eq!(HandJointType::COUNT, 26);
    for (at, joint) in HandJointType::ALL.into_iter().enumerate() {
        assert_eq!(joint.index(), at, "{joint:?}");
    }
}

#[test]
fn a_joint_is_read_by_its_type_and_a_hand_that_is_not_tracked_stays_at_rest() {
    let mut state = HandState::default();
    state.joints[HandJointType::IndexTip.index()].radius = 0.004;
    assert_eq!(state.joint(HandJointType::IndexTip).radius, 0.004);
    assert_eq!(state.joint(HandJointType::Palm).radius, 0.0);
    assert_eq!(state.tracking_state, TrackingState::Stopped);
    assert_eq!(state.aim, None);
    assert!(
        state
            .joints
            .iter()
            .all(|joint| !joint.valid && !joint.tracked)
    );
}

#[test]
fn a_frame_gives_each_hand_by_its_side() {
    let mut frame = XrFrame::default();
    frame.left_hand.tracking_state = TrackingState::Tracking;
    assert_eq!(
        frame.hand(Hand::Left).tracking_state,
        TrackingState::Tracking
    );
    assert_eq!(
        frame.hand(Hand::Right).tracking_state,
        TrackingState::Stopped
    );
}

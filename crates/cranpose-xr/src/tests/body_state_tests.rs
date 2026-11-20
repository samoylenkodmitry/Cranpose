use super::*;

#[test]
fn the_body_gives_the_joints_the_runtime_counted_and_none_beyond() {
    assert_eq!(BodyJointType::COUNT, 84);
    assert_eq!(BodyJointType::LeftHandPalm.index(), 18);
    assert_eq!(BodyJointType::RightHandLittleTip.index(), 69);
    assert_eq!(BodyJointType::LeftUpperLeg.index(), 70);
    assert_eq!(BodyJointType::RightFootBall.index(), 83);
    let mut body = BodyState::default();
    assert!(body.tracked_joints().is_empty());
    assert_eq!(body.joint(BodyJointType::Hips), None);
    body.joint_count = 70;
    assert_eq!(body.tracked_joints().len(), 70);
    assert!(body.joint(BodyJointType::Head).is_some());
    assert!(body.joint(BodyJointType::LeftUpperLeg).is_none());
    body.joint_count = 500;
    assert_eq!(body.tracked_joints().len(), 84);
}

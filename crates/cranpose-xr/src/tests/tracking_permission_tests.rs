use super::{
    PermissionAnswer::{Blocked, Granted, NotAsked, Refused},
    *,
};

#[test]
fn a_feature_with_every_group_granted_or_no_group_is_ready() {
    assert_eq!(permission_step(&[]), PermissionStep::Ready);
    assert_eq!(permission_step(&[Granted]), PermissionStep::Ready);
    assert_eq!(permission_step(&[Granted, Granted]), PermissionStep::Ready);
}

#[test]
fn a_feature_waits_while_any_group_has_no_answer() {
    assert_eq!(permission_step(&[NotAsked]), PermissionStep::Wait);
    // The microphone is granted and the face is not: a face from audio would stay empty
    assert_eq!(permission_step(&[NotAsked, Granted]), PermissionStep::Wait);
    assert_eq!(permission_step(&[Granted, NotAsked]), PermissionStep::Wait);
}

#[test]
fn one_refused_group_stops_the_feature_whatever_the_others_say() {
    assert_eq!(permission_step(&[Refused]), PermissionStep::Refused);
    assert_eq!(permission_step(&[Blocked]), PermissionStep::Refused);
    assert_eq!(
        permission_step(&[Granted, Refused]),
        PermissionStep::Refused
    );
    assert_eq!(
        permission_step(&[NotAsked, Blocked]),
        PermissionStep::Refused
    );
}

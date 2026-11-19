use super::{
    PermissionAnswer::{Blocked, Granted, NotAsked, Refused},
    Prompt::{NotShown, Shown},
    *,
};

#[test]
fn a_feature_with_every_group_granted_or_no_group_is_ready() {
    assert_eq!(permission_step(&[], NotShown), PermissionStep::Ready);
    assert_eq!(permission_step(&[Granted], NotShown), PermissionStep::Ready);
    assert_eq!(
        permission_step(&[Granted, Granted], Shown),
        PermissionStep::Ready
    );
}

#[test]
fn the_prompt_shows_once_and_then_the_backend_waits_for_the_answer() {
    assert_eq!(
        permission_step(&[NotAsked], NotShown),
        PermissionStep::Request
    );
    assert_eq!(permission_step(&[NotAsked], Shown), PermissionStep::Wait);
    // The microphone is granted and the face is not: the face's prompt is still due
    assert_eq!(
        permission_step(&[NotAsked, Granted], NotShown),
        PermissionStep::Request
    );
}

#[test]
fn one_refused_group_stops_the_feature_and_no_prompt_shows_again() {
    assert_eq!(
        permission_step(&[Refused], NotShown),
        PermissionStep::Refused
    );
    assert_eq!(permission_step(&[Blocked], Shown), PermissionStep::Refused);
    assert_eq!(
        permission_step(&[Granted, Refused], NotShown),
        PermissionStep::Refused
    );
    assert_eq!(
        permission_step(&[NotAsked, Blocked], NotShown),
        PermissionStep::Refused
    );
}

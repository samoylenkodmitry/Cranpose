//! The permissions that tracking asks of the user.

// What the user answered for one group of alternative permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, target_os = "android")),
    expect(
        dead_code,
        reason = "only the Android host gets answers other than a grant"
    )
)]
pub(crate) enum PermissionAnswer {
    NotAsked,
    Granted,
    Refused,
    Blocked,
}

// Where a feature stands, given the answers for each of its permission groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermissionStep {
    // Every group is granted, or the feature needs none.
    Ready,
    // A group has no answer: the prompt has not shown or the user has not answered it.
    Wait,
    // The user refused a group. Android shows one more prompt at most, and only the user's
    // settings allow the permission for good, so the backend does not ask again.
    Refused,
}

pub(crate) fn permission_step(answers: &[PermissionAnswer]) -> PermissionStep {
    if answers.iter().any(|answer| {
        matches!(
            answer,
            PermissionAnswer::Refused | PermissionAnswer::Blocked
        )
    }) {
        PermissionStep::Refused
    } else if answers
        .iter()
        .all(|answer| matches!(answer, PermissionAnswer::Granted))
    {
        PermissionStep::Ready
    } else {
        PermissionStep::Wait
    }
}

#[cfg(test)]
#[path = "tests/tracking_permission_tests.rs"]
mod tests;

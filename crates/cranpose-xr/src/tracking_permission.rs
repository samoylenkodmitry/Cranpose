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

// Whether the backend showed the system's prompt for a feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prompt {
    NotShown,
    Shown,
}

// What the backend does next for a feature, given the answers for each of its permission groups
// and whether it already showed the prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PermissionStep {
    // Every group is granted, or the feature needs none.
    Ready,
    // A group has no answer and the prompt has not shown: show it.
    Request,
    // The prompt shows, or the runtime has not told its answer yet.
    Wait,
    // The user refused a group. The backend does not ask again: Android allows one more prompt at
    // most, and only the user's settings allow it for good.
    Refused,
}

pub(crate) fn permission_step(answers: &[PermissionAnswer], prompt: Prompt) -> PermissionStep {
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
    } else if prompt == Prompt::Shown {
        PermissionStep::Wait
    } else {
        PermissionStep::Request
    }
}

// Whether the backend may try to make the feature's tracker now. It may when every group is
// granted. It may also while nothing is granted or refused: a runtime that defines no permission for
// the feature never answers and needs none, and one that does refuses the tracker until the user
// grants it. It may not while one group is granted and another waits, because the runtime would
// make a tracker that the missing permission leaves empty, as a face from audio before the
// microphone is granted, and the backend never makes it again.
pub(crate) fn may_try(answers: &[PermissionAnswer], step: PermissionStep) -> bool {
    match step {
        PermissionStep::Ready => true,
        PermissionStep::Refused => false,
        PermissionStep::Request | PermissionStep::Wait => answers
            .iter()
            .all(|answer| *answer == PermissionAnswer::NotAsked),
    }
}

#[cfg(test)]
#[path = "tests/tracking_permission_tests.rs"]
mod tests;

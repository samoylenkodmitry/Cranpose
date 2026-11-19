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

#[cfg(test)]
#[path = "tests/tracking_permission_tests.rs"]
mod tests;

//! A subspace that places its content relative to the user.

use cranpose::prelude::*;
use cranpose_core::{CompositionLocalProvider, rememberKeyed};

use crate::{
    FollowTarget, Subspace,
    space::{Origin, local_space, local_subspace_origin},
};

/// A [`Subspace`] whose origin follows a target, where an ordinary subspace's origin is in front
/// of where the head was when the app started. Jetpack XR's `Subspace` with a `follow`
/// parameter, which replaced `FollowingSubspace`. Rust has no overloads, so this is a composable
/// of its own.
///
/// `follow` says what the subspace follows and how. With
/// `FollowTarget::View(FollowMode::Snap(dimensions))` the panels stand in front of the head when
/// the subspace first composes, and again after the runtime recenters the room. Until the headset
/// has tracked the head, which can take a few frames after the app starts, the panels stand where
/// a `Subspace` puts them. With `FollowMode::Tight` the origin follows the target in every frame.
/// [`FollowTarget::Controller`] follows the controller in a hand, for example to put a small ball
/// above it or a menu at the hand. [`FollowTarget::Anchor`] follows an [`Anchor`](crate::Anchor),
/// which holds content on a table or a wall and keeps it there after the app restarts, with
/// [`Anchor::persist`](crate::Anchor::persist).
///
/// The origin has the target's pose in the dimensions that the mode tracks
/// ([`TrackedDimensions`](crate::TrackedDimensions)). With `TrackedDimensions::All` the panels
/// turn with the target about every axis: a menu at a controller holds its place against the
/// controller as the hand tilts and rolls. With `TrackedDimensions::TranslationAndYaw` the origin
/// has the target's place and its turn about the vertical axis, so the panels stand upright. A
/// panel that the user carried keeps its place relative to the origin, so it moves with the
/// origin: a carry of a panel at a controller sets its offset from the controller. A panel
/// that another following subspace takes over, as when the app names a new anchor, is placed by its
/// modifier again. A panel's
/// [`SpatialMoveEvent`](crate::SpatialMoveEvent) poses are relative to this origin.
///
/// A following subspace moves its panels in the headset's frame loop. A moving target does not
/// compose the subspace's content again.
///
/// Outside a headset it is a `Subspace`.
#[composable]
pub fn FollowingSubspace(follow: FollowTarget, content: impl FnMut() + 'static) {
    let Some(space) = local_space().current() else {
        Subspace(content);
        return;
    };
    let id = rememberKeyed(follow, |target| space.new_follow_origin(target.clone()));
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove_follow_origin(id))
    });
    let mut content = content;
    Subspace(move || {
        CompositionLocalProvider(
            [local_subspace_origin().provides(Origin::following(id))],
            &mut content,
        );
    });
}

#[cfg(test)]
#[path = "tests/following_subspace_tests.rs"]
mod tests;

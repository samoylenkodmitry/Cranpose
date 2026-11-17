//! A subspace that places its content relative to the user.

use cranpose::prelude::*;
use cranpose_core::CompositionLocalProvider;

use crate::{
    LockingBehavior, Subspace,
    space::{SUBSPACE_ORIGIN, local_space, local_subspace_origin},
};

/// A [`Subspace`] whose origin is in front of the user's head, where an ordinary subspace's origin
/// is in front of where the head was when the app started. Jetpack XR's `UserSubspace`.
///
/// `locking_behavior` tells how the origin follows the head. With [`LockingBehavior::Static`]
/// the panels stand in front of the head when the subspace first composes, and again after the
/// runtime recenters the room. Until the headset has tracked the head, which can take a few
/// frames after the app starts, the panels stand where a `Subspace` puts them. A panel that
/// the user carried keeps its place relative to the origin, so it moves with the origin when the
/// room is recentered. A panel's [`SpatialMoveEvent`](crate::SpatialMoveEvent) poses are
/// relative to this origin.
///
/// Outside a headset it is a `Subspace`.
#[composable]
pub fn UserSubspace(locking_behavior: LockingBehavior, content: impl FnMut() + 'static) {
    let Some(space) = local_space().current() else {
        Subspace(content);
        return;
    };
    match locking_behavior {
        LockingBehavior::Static => {}
    }
    let origin = remember({
        let space = space.clone();
        move || space.new_user_origin()
    })
    .with(|origin| *origin);
    let id = origin.id;
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove_user_origin(id))
    });
    // The state is read here, so a new origin composes the subspace's panels again
    let pose = origin.pose.get().unwrap_or(SUBSPACE_ORIGIN);
    let mut content = content;
    Subspace(move || {
        CompositionLocalProvider([local_subspace_origin().provides(pose)], &mut content);
    });
}

#[cfg(test)]
#[path = "tests/user_subspace_tests.rs"]
mod tests;

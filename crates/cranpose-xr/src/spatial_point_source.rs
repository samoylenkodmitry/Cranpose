//! A sound at a pose in a subspace.

use cranpose::prelude::*;

use crate::{
    PointSource, SubspaceModifier,
    space::{local_space, local_subspace_origin},
};

/// Plays `source` from the pose that `modifier` gives in its subspace, for as long as this is in
/// the composition. The sound follows the head of the listener. The level and the time of arrival
/// in each ear come from where the pose lies relative to the head in each frame. Jetpack XR has no
/// composable for it. It plays a sound at an entity with `SpatialSoundPool.play`.
///
/// The [`offset`](SubspaceModifier::offset) and the [`rotate`](SubspaceModifier::rotate) of the
/// modifier place the source. Its size has no effect. A source that stands at a panel or a volume
/// belongs on the modifier of that panel or volume. [`SubspaceModifier::point_source`] makes the
/// sound follow the content when the user carries it.
///
/// A headset plays the sound through the audio player that the app installed:
/// `cranpose::install_audio`, or the `audio` feature on Android.
/// [`SpatialCapabilities::is_spatial_audio_enabled`](crate::SpatialCapabilities::is_spatial_audio_enabled)
/// tells whether the headset does. Outside a headset the composable plays nothing.
#[composable]
pub fn SpatialPointSource(modifier: SubspaceModifier, source: PointSource) {
    let Some(space) = local_space().current() else {
        return;
    };
    let id = remember({
        let space = space.clone();
        move || space.new_sound_id()
    })
    .with(|id| *id);
    let origin = local_subspace_origin().current();
    space.place_point_source(id, source, origin, origin.then(&modifier.pose()));
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove_point_source(id))
    });
}

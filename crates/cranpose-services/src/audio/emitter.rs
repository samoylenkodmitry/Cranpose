//! A point that sound comes from.

use std::sync::Arc;

use super::{distance_attenuation::DistanceAttenuation, spatial_scene::SpatialScene};

/// Names an [`Emitter`] on the audio thread. A voice that plays from an emitter carries it in
/// [`PlaybackParams::emitter`](crate::PlaybackParams::emitter).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EmitterId {
    pub(crate) slot: u8,
    // The generation of the slot's use. A slot that another emitter took has a new one.
    pub(crate) generation: u32,
}

/// A point in space that voices play from. The handle moves it. Dropping the handle ends the
/// emitter. The voices that play from it stop at the next render of the audio thread.
///
/// [`place`](Self::place) writes the place for the audio thread to read. It makes no call into the
/// player, takes no lock and waits for nothing. A frame loop calls it for each emitter in each
/// frame.
pub struct Emitter {
    scene: Arc<SpatialScene>,
    id: EmitterId,
    attenuation: DistanceAttenuation,
}

impl Emitter {
    pub(crate) fn new(
        scene: Arc<SpatialScene>,
        id: EmitterId,
        attenuation: DistanceAttenuation,
    ) -> Self {
        Self {
            scene,
            id,
            attenuation,
        }
    }

    /// The name that a voice uses to play from this emitter.
    pub const fn id(&self) -> EmitterId {
        self.id
    }

    /// Moves the emitter to `position`, in meters, in the same frame as the listener's pose.
    pub fn place(&self, position: [f32; 3]) {
        self.scene
            .place_emitter(self.id, position, self.attenuation);
    }
}

impl Drop for Emitter {
    fn drop(&mut self) {
        self.scene.release_emitter(self.id);
    }
}

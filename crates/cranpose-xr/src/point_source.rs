//! The sound that a point in a subspace plays.

use cranpose::{PlaybackParams, SoundId};

use crate::PointSourceParams;

/// A sound that plays from a point in a subspace: which sound, how it fades with the distance and
/// how it plays. A [`SpatialPointSource`](crate::SpatialPointSource) puts it at a pose, and
/// [`SubspaceModifier::point_source`](crate::SubspaceModifier::point_source) attaches it to a panel
/// or a volume.
///
/// Jetpack XR plays a sound with `SpatialSoundPool.play(session, soundPool, soundID, params,
/// entity, volume, priority, loop, rate)`. Here `sound` is the id that the app's audio player
/// gave it, `params` are the [`PointSourceParams`], the volume and the rate are
/// [`playback`](Self::playback) of [`PlaybackParams`] and the entity is where the source is put.
/// The priority has no counterpart: the player takes the oldest voice of a one-shot cue when
/// it runs out of voices. A source plays for as long as it is in the composition. One that does
/// not [`loop`](Self::looping) plays once, when it enters the composition.
///
/// ```ignore
/// let sound = cranpose::default_audio().load(include_bytes!("hum.wav"))?;
/// SpatialPointSource(
///     SubspaceModifier::new().offset(600.0, 0.0, 0.0),
///     PointSource::new(sound).looping(),
/// );
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointSource {
    /// The sound, as the app's audio player named it when it loaded the clip.
    pub sound: SoundId,
    /// How the source sounds in space.
    pub params: PointSourceParams,
    /// How the sound plays: its volume, its rate and its bus. The pan has no effect, because the
    /// place of the source sets the sides.
    pub playback: PlaybackParams,
    /// Whether the sound plays again from its start each time that it ends.
    pub looping: bool,
}

impl PointSource {
    /// A source that plays `sound` once, at the volume and the rate that it was recorded.
    pub const fn new(sound: SoundId) -> Self {
        Self {
            sound,
            params: PointSourceParams::new(),
            playback: PlaybackParams::DEFAULT,
            looping: false,
        }
    }

    /// Sets how the source sounds in space.
    pub const fn params(mut self, params: PointSourceParams) -> Self {
        self.params = params;
        self
    }

    /// Sets the volume, the rate and the bus of the sound.
    pub const fn playback(mut self, playback: PlaybackParams) -> Self {
        self.playback = playback;
        self
    }

    /// Makes the sound play again from its start each time that it ends. Jetpack XR's `loop` of
    /// -1.
    pub const fn looping(mut self) -> Self {
        self.looping = true;
        self
    }
}

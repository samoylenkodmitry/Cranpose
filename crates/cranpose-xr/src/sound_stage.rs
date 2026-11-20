//! The sound of a headset app: the head's pose, the places of the point sources and the app's
//! lifecycle, which the frame loop hands to the audio player in each frame.
//!
//! The player's mixer reads them on its own thread. The stage writes the head's pose and each
//! emitter's place into cells that the audio thread reads without a lock, so a frame calls into
//! the player only when a source starts.

use std::sync::Arc;

use cranpose::{AudioListener, AudioPlayerRef, Emitter, LifecycleState, SpatialScene};

use crate::{PointSource, Pose, sound_sources::SoundPlace, space::Space};

// A source that plays: the values it started with and the emitter that its voice plays from.
struct Playing {
    id: u64,
    source: PointSource,
    emitter: Emitter,
}

pub(crate) struct SoundStage {
    player: AudioPlayerRef,
    scene: Arc<SpatialScene>,
    playing: Vec<Playing>,
    // The sources of the frame, read anew at each frame into the same storage
    places: Vec<SoundPlace>,
    // Whether the scene's emitters ran out, which is logged once
    full: bool,
    #[cfg(feature = "audio-tap")]
    tap: TapLog,
}

impl SoundStage {
    // The stage of `player`, or `None` when the player has no spatial sound: the no-op player
    // that an app gets when it installed none.
    pub(crate) fn new(player: AudioPlayerRef) -> Option<Self> {
        let scene = player.spatial_scene()?;
        // The app starts paused. The loop resumes it when the runtime gives the app its focus.
        scene.set_paused(true);
        #[cfg(feature = "audio-tap")]
        scene.tap().set_enabled(true);
        Some(Self {
            player,
            scene,
            playing: Vec::new(),
            places: Vec::new(),
            full: false,
            #[cfg(feature = "audio-tap")]
            tap: TapLog::default(),
        })
    }

    // Sets the listener at `head`. Keeps the emitter of each source where the source stands. Starts
    // or ends voices for the sources that entered or left the space. The head is `None` while the
    // runtime does not track it. The listener then stays where it was.
    pub(crate) fn update(&mut self, head: Option<Pose>, space: &Space) {
        if let Some(head) = head {
            self.scene.set_listener(AudioListener {
                position: head.position,
                orientation: head.orientation,
            });
        }
        space.sounds_into(&mut self.places);
        // A source that left, or that now plays something else, ends with its emitter
        let places = &self.places;
        self.playing.retain(|playing| {
            places
                .iter()
                .any(|place| place.id == playing.id && place.source == playing.source)
        });
        for index in 0..self.places.len() {
            let place = self.places[index];
            // A source whose following subspace has no place stays where it was
            let Some(pose) = place.pose else {
                continue;
            };
            match self.playing.iter().find(|playing| playing.id == place.id) {
                Some(playing) => playing.emitter.place(pose.position),
                None => self.start(&place, &pose),
            }
        }
        #[cfg(feature = "audio-tap")]
        self.tap.log(&self.scene, &self.places);
    }

    // Plays the source `place` from a new emitter at `pose`.
    fn start(&mut self, place: &SoundPlace, pose: &Pose) {
        let source = place.source;
        let Some(emitter) = self
            .scene
            .add_emitter(pose.position, source.params.distance_attenuation)
        else {
            if !self.full {
                log::warn!("cranpose xr: more point sources than the audio player's emitters");
                self.full = true;
            }
            return;
        };
        let params = source.playback.emitter(emitter.id());
        if source.looping {
            self.player.play_loop(source.sound, params);
        } else {
            self.player.play(source.sound, params);
        }
        self.playing.push(Playing {
            id: place.id,
            source,
            emitter,
        });
    }

    // Tells the stage what the lifecycle of the app is. The sources sound while the app has input
    // focus. They stand silent in every other state: under a system panel, while the runtime shows
    // none of the app and once the app ends.
    pub(crate) fn lifecycle(&self, state: LifecycleState) {
        self.scene
            .set_paused(!state.is_at_least(LifecycleState::Resumed));
    }
}

// The line that the debug tap of the mixed output logs once a second.
#[cfg(feature = "audio-tap")]
#[derive(Default)]
struct TapLog {
    window: u32,
}

#[cfg(feature = "audio-tap")]
impl TapLog {
    // Logs the levels of each ear of the second that the mixer finished last, with where the first
    // source stood relative to the head in that frame: x to the right, y up and z behind.
    fn log(&mut self, scene: &SpatialScene, places: &[SoundPlace]) {
        let Some(levels) = scene.tap().levels() else {
            return;
        };
        if levels.window == self.window {
            return;
        }
        self.window = levels.window;
        let head = scene.listener().unwrap_or_default();
        let source = places.iter().find_map(|place| place.pose);
        let [x, y, z] = source.map_or([0.0; 3], |pose| head.local(pose.position));
        log::info!(
            "cranpose xr: audio tap, second {}: left {:.4} right {:.4} (rms), paused {}, the source is at x {x:+.2} y {y:+.2} z {z:+.2} m of the head",
            levels.window,
            levels.left,
            levels.right,
            scene.is_paused(),
        );
    }
}

#[cfg(test)]
#[path = "tests/sound_stage_tests.rs"]
mod tests;

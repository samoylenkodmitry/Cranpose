// The demo's sound: a hum of harmonics that a spatial source loops. It is made in code, so the app
// carries no sound file. The torus plays it, and the `XR_DEMO_SOUND` builds play it from a place
// that stays at the head's side.

use std::f32::consts::TAU;

use cranpose::{AudioClip, AudioError, PlaybackParams, SoundId, default_audio, prelude::*};
use cranpose_xr::{PointSource, local_spatial_capabilities};

const SAMPLE_RATE: u32 = 48_000;
// The length of the loop in seconds. Every tone and the pulse have a whole number of cycles in it,
// so the loop has no seam.
const SECONDS: u32 = 2;
// The lowest tone in Hz and how many of its harmonics sound. The high ones give the head's shadow
// something to cut.
const FUNDAMENTAL: f32 = 110.0;
const HARMONICS: u32 = 24;
// The loudest sample of the clip, as a fraction of full scale.
const PEAK: f32 = 0.5;
// The torus plays the hum at this volume.
const VOLUME: f32 = 0.6;

// One loop of the hum: the harmonics of the fundamental, each one weaker than the one below, under
// a pulse that rises and falls once.
fn clip() -> Result<AudioClip, AudioError> {
    let frames = SAMPLE_RATE * SECONDS;
    let mut samples: Vec<f32> = (0..frames)
        .map(|frame| {
            let seconds = frame as f32 / SAMPLE_RATE as f32;
            let tone: f32 = (1..=HARMONICS)
                .map(|harmonic| {
                    let harmonic = harmonic as f32;
                    (TAU * FUNDAMENTAL * harmonic * seconds).sin() / harmonic.sqrt()
                })
                .sum();
            let pulse = 0.6 + 0.4 * (TAU * seconds / SECONDS as f32).sin();
            tone * pulse
        })
        .collect();
    let loudest = samples
        .iter()
        .fold(f32::MIN_POSITIVE, |peak, sample| peak.max(sample.abs()));
    for sample in &mut samples {
        *sample *= PEAK / loudest;
    }
    AudioClip::from_samples(samples, 1, SAMPLE_RATE)
}

// The id of the hum in the app's audio player, or `None` where the headset has no spatial audio. The
// clip loads once and leaves the player with the composition.
#[composable]
pub(crate) fn rememberHum() -> Option<SoundId> {
    if !local_spatial_capabilities()
        .current()
        .is_spatial_audio_enabled()
    {
        return None;
    }
    let sound = remember(|| {
        clip()
            .and_then(|clip| default_audio().load_clip(clip))
            .map_err(|error| log::error!("xr demo: no hum: {error}"))
            .ok()
    })
    .with(|sound| *sound);
    if let Some(sound) = sound {
        DisposableEffect(sound, move |_| {
            DisposableEffectResult::new(move || default_audio().unload(sound))
        });
    }
    sound
}

// The hum as a looping source at the volume that the torus plays it.
pub(crate) fn source(sound: SoundId) -> PointSource {
    PointSource::new(sound)
        .looping()
        .playback(PlaybackParams::new().volume(VOLUME))
}

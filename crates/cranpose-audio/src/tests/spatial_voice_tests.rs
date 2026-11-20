use cranpose_services::{AudioListener, DistanceAttenuation, Emitter, PlaybackParams};

use super::*;
use crate::mixer::{
    Command,
    tests::{Harness, clip, harness},
};

const RATE: f32 = 48_000.0;
const RATE_HZ: u32 = 48_000;
// The frame of the clip where its step rises from 0 to 1
const STEP_AT: usize = 64;
const NATURAL: DistanceAttenuation = DistanceAttenuation::Natural { min_distance: 1.0 };

// What each ear of the output heard, one list of samples for each.
struct Heard {
    ears: [Vec<f32>; 2],
}

impl Heard {
    fn of(out: &[f32]) -> Self {
        Self {
            ears: [0, 1].map(|ear| out.iter().skip(ear).step_by(2).copied().collect()),
        }
    }

    // The first frame where the ear hears more than one percent of full scale
    fn onset(&self, ear: usize) -> usize {
        self.ears[ear]
            .iter()
            .position(|sample| sample.abs() > 0.01)
            .expect("the ear heard the sound")
    }

    // The mean of the last 256 frames, where the step has settled
    fn level(&self, ear: usize) -> f32 {
        let tail = &self.ears[ear][self.ears[ear].len() - 256..];
        tail.iter().sum::<f32>() / 256.0
    }
}

// A mono clip that is silent for `STEP_AT` frames and 1 from there.
fn step_clip(frames: usize) -> ClipData {
    clip(
        (0..frames)
            .map(|frame| if frame < STEP_AT { 0.0 } else { 1.0 })
            .collect(),
        1,
        RATE_HZ,
    )
}

fn dc_clip(frames: usize) -> ClipData {
    clip(vec![1.0; frames], 1, RATE_HZ)
}

// A mixer with the listener at `listener`.
fn rig(listener: AudioListener) -> Harness {
    let h = harness(RATE, 2);
    h.scene.set_listener(listener);
    h
}

fn emitter_at(h: &Harness, source: [f32; 3], attenuation: DistanceAttenuation) -> Emitter {
    h.scene
        .add_emitter(source, attenuation)
        .expect("a free slot")
}

// Plays `clip` from `emitter` at `volume`.
fn play_from(h: &mut Harness, clip: ClipData, looping: bool, emitter: &Emitter, volume: f32) {
    let (gain_left, gain_right) = PlaybackParams::new().volume(volume).gains();
    for command in [
        Command::LoadClip { slot: 0, clip },
        Command::Play {
            voice: 1,
            slot: 0,
            gain_left,
            gain_right,
            rate: 1.0,
            bus: 0,
            looping,
            emitter: Some(emitter.id()),
        },
    ] {
        assert!(h.commands.push(command).is_ok());
    }
}

fn render(h: &mut Harness, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    h.mixer.render(&mut out);
    out
}

// What a listener hears of a step that plays from `source`, in one render call of 2048 frames.
fn hear(listener: AudioListener, source: [f32; 3], attenuation: DistanceAttenuation) -> Heard {
    let mut h = rig(listener);
    let emitter = emitter_at(&h, source, attenuation);
    play_from(&mut h, step_clip(8_192), false, &emitter, 1.0);
    Heard::of(&render(&mut h, 2_048))
}

fn at_origin() -> AudioListener {
    AudioListener::AT_ORIGIN
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_source_ahead_reaches_both_ears_together_at_full_level() {
    let heard = hear(at_origin(), [0.0, 0.0, -1.0], NATURAL);
    assert_eq!(heard.onset(0), STEP_AT);
    assert_eq!(heard.onset(1), STEP_AT);
    assert!(close(heard.level(0), 1.0) && close(heard.level(1), 1.0));
}

#[test]
fn a_source_to_the_left_reaches_the_right_ear_31_frames_later_and_6_db_lower() {
    let heard = hear(at_origin(), [-1.0, 0.0, 0.0], NATURAL);
    assert_eq!(heard.onset(0), STEP_AT);
    // 656 microseconds at 48 kHz are 31.5 frames
    let late = heard.onset(1) - STEP_AT;
    assert!(
        (31..=32).contains(&late),
        "the right ear came {late} frames late"
    );
    assert!(close(heard.level(0), 1.0));
    assert!(close(heard.level(1), 0.501));
}

#[test]
fn a_source_to_the_right_reaches_the_left_ear_later_and_lower() {
    let heard = hear(at_origin(), [1.0, 0.0, 0.0], NATURAL);
    assert_eq!(heard.onset(1), STEP_AT);
    let late = heard.onset(0) - STEP_AT;
    assert!(
        (31..=32).contains(&late),
        "the left ear came {late} frames late"
    );
    assert!(close(heard.level(1), 1.0));
    assert!(close(heard.level(0), 0.501));
}

#[test]
fn a_source_behind_sounds_as_one_ahead_does() {
    let ahead = hear(at_origin(), [0.0, 0.0, -1.0], NATURAL);
    let behind = hear(at_origin(), [0.0, 0.0, 1.0], NATURAL);
    assert_eq!(ahead.ears, behind.ears);
}

#[test]
fn a_far_source_is_quiet_in_both_ears_and_a_curve_of_none_keeps_it_loud() {
    let far = hear(at_origin(), [0.0, 0.0, -10.0], NATURAL);
    assert_eq!(far.onset(0), far.onset(1));
    assert!(close(far.level(0), 0.1) && close(far.level(1), 0.1));
    let loud = hear(at_origin(), [0.0, 0.0, -10.0], DistanceAttenuation::None);
    assert!(close(loud.level(0), 1.0));
}

#[test]
fn the_head_that_turns_left_hears_a_source_ahead_of_its_start_in_the_right_ear() {
    let (sin, cos) = (std::f32::consts::FRAC_PI_4).sin_cos();
    let turned_left = AudioListener {
        orientation: [0.0, sin, 0.0, cos],
        ..AudioListener::AT_ORIGIN
    };
    let heard = hear(turned_left, [0.0, 0.0, -1.0], NATURAL);
    assert!(heard.level(1) > heard.level(0) + 0.4);
    assert!(heard.onset(0) > heard.onset(1) + 30);
}

#[test]
fn the_volume_of_the_voice_scales_both_ears() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut h, step_clip(8_192), false, &emitter, 0.5);
    let heard = Heard::of(&render(&mut h, 2_048));
    assert!(close(heard.level(0), 0.5) && close(heard.level(1), 0.25));
}

#[test]
fn a_stereo_clip_is_one_source_at_the_mean_of_its_channels() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [0.0, 0.0, -1.0], NATURAL);
    // Left channel 1, right channel 0
    let stereo = clip([1.0, 0.0].repeat(4_096), 2, RATE_HZ);
    play_from(&mut h, stereo, true, &emitter, 1.0);
    let heard = Heard::of(&render(&mut h, 2_048));
    assert!(close(heard.level(0), 0.5) && close(heard.level(1), 0.5));
}

#[test]
fn a_clip_that_loops_wraps_the_late_ear_to_its_end_and_one_that_does_not_starts_it_in_silence() {
    let mut looped = rig(at_origin());
    let emitter = emitter_at(&looped, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut looped, dc_clip(4_800), true, &emitter, 1.0);
    let heard = Heard::of(&render(&mut looped, 512));
    assert_eq!(heard.onset(1), 0);
    let mut once = rig(at_origin());
    let emitter = emitter_at(&once, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut once, dc_clip(4_800), false, &emitter, 1.0);
    let heard = Heard::of(&render(&mut once, 512));
    assert!(heard.onset(1) >= 31);
}

#[test]
fn a_source_that_moves_across_the_head_follows_it_without_a_click() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut h, dc_clip(4_800), true, &emitter, 1.0);
    // The source stands to the left, and the left ear is the louder one
    let before = Heard::of(&render(&mut h, 1_024));
    assert!(before.level(0) > before.level(1) + 0.4);
    // It jumps to the right between two render calls
    emitter.place([1.0, 0.0, 0.0]);
    let crossing = Heard::of(&render(&mut h, 128));
    let after = Heard::of(&render(&mut h, 1_024));
    assert!(after.level(1) > after.level(0) + 0.4);
    // Neither ear's level steps: a gain that changed by 0.5 in one frame would show as a step of
    // 0.5. The ramp spreads it over the call, and the filters' states follow their coefficients.
    for (ear, samples) in crossing.ears.iter().enumerate() {
        let steps = samples.windows(2).map(|pair| (pair[1] - pair[0]).abs());
        let largest = steps.fold(0.0_f32, f32::max);
        assert!(largest < 0.05, "ear {ear} stepped by {largest}");
    }
}

#[test]
fn a_source_that_moves_slowly_past_the_head_passes_through_the_middle() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-2.0, 0.0, -1.0], NATURAL);
    play_from(&mut h, dc_clip(4_800), true, &emitter, 1.0);
    let mut sides = Vec::new();
    for step in 0..=20 {
        emitter.place([-2.0 + 0.2 * step as f32, 0.0, -1.0]);
        let heard = Heard::of(&render(&mut h, 256));
        sides.push(heard.level(0) - heard.level(1));
    }
    // From the left ear louder, through equal, to the right ear louder, and the balance crosses
    // zero once
    assert!(sides[0] > 0.1, "{sides:?}");
    assert!(sides[20] < -0.1, "{sides:?}");
    let crossings = sides
        .windows(2)
        .filter(|pair| pair[0] * pair[1] < 0.0)
        .count();
    assert_eq!(crossings, 1, "{sides:?}");
}

#[test]
fn a_paused_scene_is_silent_after_one_render_call_and_the_voice_goes_on_where_it_stood() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-1.0, 0.0, 0.0], NATURAL);
    // A clip that ends after 3000 frames, with no loop
    play_from(&mut h, dc_clip(3_000), false, &emitter, 1.0);
    let playing = Heard::of(&render(&mut h, 1_000));
    assert!(playing.level(0) > 0.9);
    h.scene.set_paused(true);
    // The voice fades out across this call, and stands silent from the next one on
    let fading = Heard::of(&render(&mut h, 128));
    assert!(
        fading.ears[0]
            .last()
            .is_some_and(|sample| sample.abs() < 0.02)
    );
    // Far longer than the clip lasts, and not a sample of sound
    for _ in 0..100 {
        let out = render(&mut h, 128);
        assert!(out.iter().all(|sample| *sample == 0.0));
    }
    h.scene.set_paused(false);
    // The voice fades in across the first call and plays at full level in the next
    let fading_in = Heard::of(&render(&mut h, 512));
    assert!(fading_in.level(0) > 0.5, "the voice did not come back");
    let resumed = Heard::of(&render(&mut h, 512));
    assert!(resumed.level(0) > 0.9);
}

#[test]
fn a_voice_whose_emitter_ended_stops() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut h, dc_clip(4_800), true, &emitter, 1.0);
    assert!(Heard::of(&render(&mut h, 512)).level(0) > 0.9);
    drop(emitter);
    // The voice fades out of nothing: the first call after the end is already silent
    let out = render(&mut h, 512);
    assert!(out.iter().all(|sample| *sample == 0.0));
    assert_eq!(h.mixer.active_voices(), 0);
}

#[test]
fn the_tap_reports_the_level_of_each_ear_once_a_second_while_it_is_on() {
    let mut h = rig(at_origin());
    let emitter = emitter_at(&h, [-1.0, 0.0, 0.0], NATURAL);
    play_from(&mut h, dc_clip(4_800), true, &emitter, 1.0);
    h.run(48_000);
    assert_eq!(h.scene.tap().levels(), None, "the tap is off");
    h.scene.tap().set_enabled(true);
    h.run(48_000);
    let levels = h.scene.tap().levels().expect("one second went by");
    assert_eq!(levels.window, 1);
    assert!(
        close(levels.left, 1.0) && close(levels.right, 0.501),
        "{levels:?}"
    );
    // The source moves to the right of the head, and the next second shows it
    emitter.place([1.0, 0.0, 0.0]);
    h.run(48_000);
    let levels = h.scene.tap().levels().expect("two seconds went by");
    assert_eq!(levels.window, 2);
    assert!(
        close(levels.right, 1.0) && close(levels.left, 0.501),
        "{levels:?}"
    );
}

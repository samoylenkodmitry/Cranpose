use super::*;
use crate::ring;

struct Harness {
    commands: Producer<Command>,
    retired: Consumer<ClipData>,
    mixer: Mixer,
    leaked: Arc<AtomicU32>,
    streaming: Arc<AtomicBool>,
}

impl Harness {
    fn run(&mut self, frames: usize) -> RenderStatus {
        let burst = 128;
        let channels = self.mixer.device_channels;
        let mut out = vec![0.0f32; burst * channels];
        let mut status = RenderStatus::Continue;
        let mut remaining = frames;
        while remaining > 0 {
            let take = remaining.min(burst);
            status = self.mixer.render(&mut out[..take * channels]);
            remaining -= take;
        }
        status
    }
}

fn harness(sample_rate: f32, channels: usize) -> Harness {
    let (command_tx, command_rx) = ring::channel::<Command>(64);
    let (retired_tx, retired_rx) = ring::channel::<ClipData>(64);
    let leaked = Arc::new(AtomicU32::new(0));
    let streaming = Arc::new(AtomicBool::new(true));
    let seed = MixerSeed {
        commands: command_rx,
        retired: retired_tx,
        leaked_clips: Arc::clone(&leaked),
        underruns: Arc::new(AtomicU32::new(0)),
        streaming: Arc::clone(&streaming),
    };
    Harness {
        commands: command_tx,
        retired: retired_rx,
        mixer: Mixer::new(seed, sample_rate, channels),
        leaked,
        streaming,
    }
}

fn clip(samples: Vec<f32>, channels: u8, sample_rate: u32) -> ClipData {
    ClipData {
        samples: samples.into(),
        channels,
        sample_rate,
    }
}

fn play(voice: u64, slot: u32) -> Command {
    Command::Play {
        voice,
        slot,
        gain_left: 1.0,
        gain_right: 1.0,
        rate: 1.0,
        bus: 0,
        looping: false,
    }
}

#[test]
fn renders_silence_without_voices() {
    let mut h = harness(48_000.0, 2);
    let mut out = vec![1.0f32; 8];
    h.mixer.render(&mut out);
    assert!(out.iter().all(|sample| *sample == 0.0));
}

#[test]
fn plays_a_one_shot_and_frees_the_voice() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0, 1.0], 1, 48_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");

    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 0, "a two-frame clip ends at once");
    assert!(out[0] > 0.0 && out[1] > 0.0);
    assert_eq!(out[6], 0.0, "past the end of the clip is silent");
}

#[test]
fn overlapping_voices_sum() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.25; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");
    h.commands.push(play(2, 0)).expect("queued");
    h.commands.push(play(3, 0)).expect("queued");

    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 3);
    assert!(out[0] > 0.5, "three voices sum, got {}", out[0]);
}

#[test]
fn output_is_clamped_to_the_nominal_range() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    for voice in 1..=8 {
        h.commands.push(play(voice, 0)).expect("queued");
    }
    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
    assert!((out[0] - 1.0).abs() < 1e-6);
}

#[test]
fn rate_shifts_the_read_position() {
    let mut h = harness(48_000.0, 1);
    let ramp: Vec<f32> = (0..64).map(|i| i as f32 / 64.0).collect();
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(ramp, 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 2.0,
            bus: 0,
            looping: false,
        })
        .expect("queued");

    let mut out = vec![0.0f32; 4];
    h.mixer.render(&mut out);
    for (frame, sample) in out.iter().enumerate() {
        let expected = (2 * frame) as f32 / 64.0;
        assert!(
            (sample - expected).abs() < 1e-6,
            "frame {frame}: expected {expected}, got {sample}"
        );
    }
}

#[test]
fn clip_sample_rate_is_resampled_to_the_device_rate() {
    let mut h = harness(48_000.0, 1);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.5; 1024], 1, 24_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");
    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 1);
    for _ in 0..255 {
        h.mixer.render(&mut out);
    }
    assert_eq!(
        h.mixer.active_voices(),
        1,
        "still playing after 2048 frames"
    );
}

#[test]
fn looping_voice_keeps_going_until_stopped() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.5, 0.5], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 9,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 1,
            looping: true,
        })
        .expect("queued");

    let mut out = vec![0.0f32; 64];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 1);
    assert!(out[40] != 0.0, "the loop refills the whole buffer");

    h.commands
        .push(Command::StopVoice { voice: 9 })
        .expect("queued");
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 0);
}

#[test]
fn muting_a_bus_silences_only_that_bus() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 0.5,
            gain_right: 0.5,
            rate: 1.0,
            bus: 1,
            looping: true,
        })
        .expect("queued");
    h.commands
        .push(Command::SetBusEnabled {
            bus: 1,
            enabled: false,
        })
        .expect("queued");

    let mut out = vec![0.0f32; 16];
    h.mixer.render(&mut out);
    assert!(out.iter().all(|sample| *sample == 0.0));
    assert_eq!(h.mixer.active_voices(), 1, "muting does not stop the voice");

    h.commands
        .push(Command::SetBusEnabled {
            bus: 1,
            enabled: true,
        })
        .expect("queued");
    h.mixer.render(&mut out);
    assert!(out[0] > 0.0, "unmuting resumes mid-track");
}

#[test]
fn master_volume_scales_every_bus() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 0.5,
            gain_right: 0.5,
            rate: 1.0,
            bus: 0,
            looping: true,
        })
        .expect("queued");
    h.commands.push(Command::SetMaster(0.0)).expect("queued");
    let mut out = vec![0.0f32; 16];
    h.mixer.render(&mut out);
    assert!(out.iter().all(|sample| *sample == 0.0));

    h.commands.push(Command::SetMaster(1.0)).expect("queued");
    h.mixer.render(&mut out);
    assert!((out[0] - 0.5).abs() < 1e-6);
}

#[test]
fn stop_clip_silences_every_voice_of_that_clip() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::LoadClip {
            slot: 1,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");
    h.commands.push(play(2, 0)).expect("queued");
    h.commands.push(play(3, 1)).expect("queued");
    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 3);

    h.commands
        .push(Command::StopClip { slot: 0 })
        .expect("queued");
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 1);

    h.commands.push(Command::StopAll).expect("queued");
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 0);
}

#[test]
fn voice_stealing_prefers_one_shots_over_loops() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 4096], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 0,
            looping: true,
        })
        .expect("queued");
    let mut out = vec![0.0f32; 8];
    for voice in 2..=(MAX_VOICES as u64) {
        h.commands.push(play(voice, 0)).expect("queued");
    }
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), MAX_VOICES);

    h.commands
        .push(play(MAX_VOICES as u64 + 1, 0))
        .expect("queued");
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), MAX_VOICES);
    assert!(
        h.mixer.voices.iter().any(|voice| voice.id == 1),
        "the looping voice is not stolen while one-shots remain"
    );
}

#[test]
fn unloading_a_clip_returns_it_to_the_ui_thread() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 3,
            clip: clip(vec![1.0; 8], 1, 48_000),
        })
        .expect("queued");
    h.commands.push(play(5, 3)).expect("queued");
    let mut out = vec![0.0f32; 4];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 1);

    h.commands
        .push(Command::UnloadClip { slot: 3 })
        .expect("queued");
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 0);
    assert!(h.retired.pop().is_some(), "the clip came back for dropping");
    assert_eq!(h.leaked.load(Ordering::Relaxed), 0);
}

#[test]
fn replacing_a_slot_returns_the_previous_clip() {
    let mut h = harness(48_000.0, 2);
    for _ in 0..2 {
        h.commands
            .push(Command::LoadClip {
                slot: 1,
                clip: clip(vec![1.0; 8], 1, 48_000),
            })
            .expect("queued");
    }
    let mut out = vec![0.0f32; 4];
    h.mixer.render(&mut out);
    assert!(h.retired.pop().is_some());
    assert!(h.retired.pop().is_none());
}

#[test]
fn out_of_range_slots_are_ignored() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: MAX_CLIPS as u32 + 5,
            clip: clip(vec![1.0; 8], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::UnloadClip {
            slot: MAX_CLIPS as u32 + 5,
        })
        .expect("queued");
    h.commands
        .push(play(1, MAX_CLIPS as u32 + 5))
        .expect("queued");
    let mut out = vec![0.0f32; 4];
    h.mixer.render(&mut out);
    assert_eq!(h.mixer.active_voices(), 0);
    assert!(
        h.retired.pop().is_some(),
        "the rejected clip is handed back"
    );
}

#[test]
fn retune_changes_gain_and_rate_of_a_running_voice() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 4096], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 4,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 0,
            looping: true,
        })
        .expect("queued");
    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    assert!((out[0] - 1.0).abs() < 1e-6);

    h.commands
        .push(Command::RetuneVoice {
            voice: 4,
            gain_left: 0.25,
            gain_right: 0.25,
            rate: 2.0,
        })
        .expect("queued");
    h.mixer.render(&mut out);
    assert!((out[0] - 0.25).abs() < 1e-6);
    let voice = h.mixer.voices.iter().find(|v| v.id == 4).expect("running");
    assert!((voice.step - 2.0).abs() < 1e-9);
}

#[test]
fn device_format_change_keeps_voice_pitch() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 4096], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 0,
            looping: true,
        })
        .expect("queued");
    let mut out = vec![0.0f32; 8];
    h.mixer.render(&mut out);
    h.mixer.set_device_format(24_000.0, 2);
    assert_eq!(h.mixer.device_sample_rate(), 24_000.0);
    let voice = h.mixer.voices.iter().find(|v| v.id == 1).expect("running");
    assert!((voice.step - 2.0).abs() < 1e-9);
}

#[test]
fn nan_gains_and_rates_do_not_wedge_the_mixer() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::SetMaster(f32::NAN))
        .expect("queued");
    h.commands
        .push(Command::SetBusVolume {
            bus: 0,
            volume: f32::INFINITY,
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 0.5,
            gain_right: 0.5,
            rate: f32::NAN,
            bus: 0,
            looping: true,
        })
        .expect("queued");
    let mut out = vec![0.0f32; 16];
    h.mixer.render(&mut out);
    assert!(out.iter().all(|sample| sample.is_finite()));
}

#[test]
fn silence_gives_the_device_up_after_the_grace_period() {
    let mut h = harness(48_000.0, 2);
    let grace = grace_frames(48_000.0) as usize;
    assert_eq!(h.run(grace - 128), RenderStatus::Continue);
    assert!(h.streaming.load(Ordering::SeqCst), "still inside the grace");
    assert_eq!(h.run(128), RenderStatus::Idle);
    assert!(!h.streaming.load(Ordering::SeqCst));
}

#[test]
fn the_grace_period_starts_when_the_last_voice_ends() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.5; 48_000], 1, 48_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");

    let grace = grace_frames(48_000.0) as usize;
    assert_eq!(h.run(48_000 + grace - 128), RenderStatus::Continue);
    assert_eq!(h.run(128), RenderStatus::Idle);
}

#[test]
fn a_looping_voice_holds_the_device_open_indefinitely() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.5; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 0,
            looping: true,
        })
        .expect("queued");

    let grace = grace_frames(48_000.0) as usize;
    assert_eq!(h.run(grace * 2), RenderStatus::Continue);
    assert!(h.streaming.load(Ordering::SeqCst));
}

#[test]
fn a_muted_voice_still_counts_as_a_reason_to_run() {
    let mut h = harness(48_000.0, 2);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![0.5; 64], 1, 48_000),
        })
        .expect("queued");
    h.commands
        .push(Command::Play {
            voice: 1,
            slot: 0,
            gain_left: 1.0,
            gain_right: 1.0,
            rate: 1.0,
            bus: 1,
            looping: true,
        })
        .expect("queued");
    h.commands
        .push(Command::SetBusEnabled {
            bus: 1,
            enabled: false,
        })
        .expect("queued");

    let grace = grace_frames(48_000.0) as usize;
    assert_eq!(h.run(grace + 128), RenderStatus::Continue);
}

#[test]
fn a_command_landing_while_the_stream_stops_keeps_it_alive() {
    let mut h = harness(48_000.0, 2);
    assert_eq!(h.run(grace_frames(48_000.0) as usize), RenderStatus::Idle);
    assert!(!h.streaming.load(Ordering::SeqCst));

    h.commands.push(Command::StopAll).expect("queued");
    assert_eq!(h.mixer.settle(0, 128), RenderStatus::Continue);
    assert!(h.streaming.load(Ordering::SeqCst));
}

#[test]
fn the_grace_period_is_a_duration_not_a_callback_count() {
    let mut h = harness(24_000.0, 2);
    let grace = grace_frames(24_000.0) as usize;
    assert_eq!(grace * 2, grace_frames(48_000.0) as usize);
    assert_eq!(h.run(grace - 128), RenderStatus::Continue);
    assert_eq!(h.run(128), RenderStatus::Idle);
}

#[test]
fn a_device_rate_change_rescales_the_grace_period() {
    let mut h = harness(48_000.0, 2);
    h.mixer.set_device_format(24_000.0, 2);
    assert_eq!(h.run(grace_frames(24_000.0) as usize), RenderStatus::Idle);
}

#[test]
fn mono_device_downmixes_both_channels() {
    let mut h = harness(48_000.0, 1);
    h.commands
        .push(Command::LoadClip {
            slot: 0,
            clip: clip(vec![1.0, -1.0, 1.0, -1.0], 2, 48_000),
        })
        .expect("queued");
    h.commands.push(play(1, 0)).expect("queued");
    let mut out = vec![0.0f32; 2];
    h.mixer.render(&mut out);
    assert!(
        out[0].abs() < 1e-6,
        "opposite channels cancel in the downmix"
    );
}

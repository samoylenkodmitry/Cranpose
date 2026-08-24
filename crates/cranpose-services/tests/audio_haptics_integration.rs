//! The audio and haptics surfaces as an application sees them: through the
//! crate's public re-exports, inside a real composition.
//!
//! The unit tests inside the modules cover behaviour. These cover reachability
//! — that every type an app needs is exported, and that an app with no backend
//! installed composes and runs without panicking.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{location_key, Composition, MemoryApplier};
use cranpose_services::{
    clear_platform_audio, clear_platform_haptics, default_audio, default_haptics, local_audio,
    local_haptics, rememberSoundBank, AudioBus, AudioClip, AudioError, HapticEffect, HapticError,
    HapticFeedback, HapticPattern, PlaybackParams, ProvideAudio, ProvideHaptics, SoundId,
    SoundSpec, VoiceId,
};

fn run_test_composition(build: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
}

/// A one-frame mono WAVE stream, so the bank has something real to decode.
fn tiny_wav() -> Vec<u8> {
    let data = 0i16.to_le_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[test]
fn integration_audio_without_a_backend_never_panics() {
    clear_platform_audio();
    let player = default_audio();
    assert!(!player.is_available());

    let id = player
        .load(&tiny_wav())
        .expect("the no-op player accepts clips");
    assert!(id.is_valid());
    player.play(id, PlaybackParams::default());
    let voice = player.play_loop(id, PlaybackParams::new().pitch_semitones(7.0).pan(-0.5));
    player.set_voice_params(voice, PlaybackParams::new().volume(0.2));
    player.stop_voice(voice);
    player.stop(id);
    player.stop_all();
    player.set_master_volume(0.5);
    player.set_bus_volume(AudioBus::Music, 0.25);
    player.set_bus_enabled(AudioBus::Effects, false);
    assert!(!player.bus_enabled(AudioBus::Effects));
    player.suspend();
    player.resume();
    player.unload(id);

    assert_eq!(
        player.play_loop(SoundId::NONE, PlaybackParams::default()),
        VoiceId::NONE
    );
    assert!(matches!(
        player.load(b"not audio"),
        Err(AudioError::UnsupportedFormat(_))
    ));
    assert!(AudioClip::decode(&tiny_wav()).is_ok());
}

#[test]
fn integration_remember_sound_bank_composes_without_a_backend() {
    clear_platform_audio();
    let observed = Rc::new(RefCell::new(None));
    let wav = tiny_wav();

    {
        let observed = Rc::clone(&observed);
        run_test_composition(move || {
            let observed = Rc::clone(&observed);
            let wav = wav.clone();
            ProvideAudio(move || {
                let specs = [
                    SoundSpec::new("hit", &wav).volume(0.4),
                    SoundSpec::new("theme", &wav).bus(AudioBus::Music),
                ];
                let bank = rememberSoundBank(&specs);
                bank.play(0);
                bank.play_with(1, PlaybackParams::new().rate(1.5));
                bank.play_named("hit", PlaybackParams::default());
                *observed.borrow_mut() = Some((
                    bank.len(),
                    bank.failures().len(),
                    local_audio().current().is_available(),
                ));
            });
        });
    }

    assert_eq!(*observed.borrow(), Some((2, 0, false)));
}

#[test]
fn integration_haptics_waveform_surface_is_reachable() {
    clear_platform_haptics();
    let haptics = default_haptics();

    // The 21-distinct-feels case: short waveforms, each its own shape.
    let feels = [
        HapticPattern::new(&[0, 8], &[0, 60]).expect("light tick"),
        HapticPattern::new(&[0, 12, 8, 12], &[0, 180, 0, 90]).expect("double tap"),
        HapticPattern::repeating(&[0, 30, 30], &[0, 255, 0], 1).expect("alarm"),
    ];
    for feel in &feels {
        haptics.play_pattern(feel);
    }
    haptics.vibrate(25, 200);
    haptics.perform_effect(HapticEffect::HeavyClick);
    haptics.perform(HapticFeedback::Success);
    haptics.cancel();
    assert!(!haptics.has_amplitude_control());

    // Mismatched lengths are an error, not a panic, and never reach a platform.
    assert_eq!(
        HapticPattern::new(&[0, 20, 40], &[0, 255]),
        Err(HapticError::LengthMismatch {
            timings: 3,
            amplitudes: 2
        })
    );
    assert_eq!(HapticPattern::new(&[], &[]), Err(HapticError::Empty));
    assert_eq!(
        HapticPattern::new(&[0, 0], &[0, 255]),
        Err(HapticError::ZeroDuration)
    );
    assert_eq!(
        HapticPattern::repeating(&[0, 20], &[0, 255], 9),
        Err(HapticError::RepeatOutOfRange { index: 9, len: 2 })
    );
}

#[test]
fn integration_provide_haptics_reaches_descendants() {
    clear_platform_haptics();
    let seen = Rc::new(RefCell::new(Vec::new()));

    {
        let seen = Rc::clone(&seen);
        run_test_composition(move || {
            let seen = Rc::clone(&seen);
            ProvideHaptics(move || {
                let haptics = local_haptics().current();
                haptics.play_pattern(&HapticPattern::new(&[0, 40], &[0, 220]).expect("valid"));
                seen.borrow_mut().push(haptics.has_amplitude_control());
            });
        });
    }

    assert_eq!(*seen.borrow(), vec![false]);
}

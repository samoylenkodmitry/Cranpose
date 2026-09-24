use std::cell::Cell;

use parking_lot::Mutex;

use super::*;
use crate::run_test_composition;

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

#[derive(Default)]
struct RecordingPlayer {
    played: Mutex<Vec<(SoundId, PlaybackParams)>>,
    unloaded: Mutex<Vec<SoundId>>,
    next: Mutex<u32>,
}

impl AudioPlayer for RecordingPlayer {
    fn load_clip(&self, _clip: AudioClip) -> Result<SoundId, AudioError> {
        let mut next = self.next.lock();
        *next += 1;
        Ok(SoundId::from_raw(*next))
    }
    fn play(&self, id: SoundId, params: PlaybackParams) {
        self.played.lock().push((id, params));
    }
    fn play_loop(&self, _id: SoundId, _params: PlaybackParams) -> VoiceId {
        VoiceId::from_raw(7)
    }
    fn stop(&self, _id: SoundId) {}
    fn stop_voice(&self, _voice: VoiceId) {}
    fn set_master_volume(&self, _volume: f32) {}
    fn unload(&self, id: SoundId) {
        self.unloaded.lock().push(id);
    }
    fn is_available(&self) -> bool {
        true
    }
}

#[test]
fn playback_params_default_is_neutral() {
    let params = PlaybackParams::default();
    assert_eq!(params.volume, 1.0);
    assert_eq!(params.rate, 1.0);
    assert_eq!(params.pan, 0.0);
    assert_eq!(params.bus, AudioBus::Effects);
    assert_eq!(params, PlaybackParams::new());
    assert_eq!(params, PlaybackParams::DEFAULT);
}

#[test]
fn playback_params_sanitizes_out_of_range_and_nan() {
    let wild = PlaybackParams {
        volume: f32::NAN,
        rate: 1_000.0,
        pan: -9.0,
        bus: AudioBus::Music,
    }
    .sanitized();
    assert_eq!(wild.volume, 1.0);
    assert_eq!(wild.rate, PlaybackParams::MAX_RATE);
    assert_eq!(wild.pan, -1.0);
    assert_eq!(wild.bus, AudioBus::Music);

    let slow = PlaybackParams::new().rate(0.0).sanitized();
    assert_eq!(slow.rate, PlaybackParams::MIN_RATE);
}

#[test]
fn pitch_semitones_maps_octaves_to_rate() {
    let up = PlaybackParams::new().pitch_semitones(12.0);
    assert!((up.rate - 2.0).abs() < 1e-5);
    let down = PlaybackParams::new().pitch_semitones(-12.0);
    assert!((down.rate - 0.5).abs() < 1e-5);
    let broken = PlaybackParams::new().pitch_semitones(f32::NAN);
    assert_eq!(broken.rate, 1.0);
}

#[test]
fn pan_gains_are_constant_power() {
    let (left, right) = PlaybackParams::new().gains();
    assert!((left - right).abs() < 1e-6);
    assert!((left * left + right * right - 1.0).abs() < 1e-5);

    let (left, right) = PlaybackParams::new().pan(-1.0).gains();
    assert!((left - 1.0).abs() < 1e-5);
    assert!(right.abs() < 1e-5);

    let (left, right) = PlaybackParams::new().pan(1.0).gains();
    assert!(left.abs() < 1e-5);
    assert!((right - 1.0).abs() < 1e-5);
}

#[test]
fn audio_bus_indices_round_trip() {
    for bus in AudioBus::ALL {
        assert_eq!(AudioBus::from_index(bus.index()), Some(bus));
    }
    assert_eq!(AudioBus::from_index(2), None);
    assert_eq!(AudioBus::default(), AudioBus::Effects);
}

#[test]
fn noop_player_hands_out_handles_and_keeps_settings() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_audio();
    let player = default_audio();
    assert!(!player.is_available());

    let id = player.load(&tiny_wav()).expect("no-op load succeeds");
    assert!(id.is_valid());
    let second = player.load(&tiny_wav()).expect("no-op load succeeds");
    assert_ne!(id, second);

    player.play(id, PlaybackParams::new());
    let voice = player.play_loop(id, PlaybackParams::new());
    assert!(voice.is_valid());
    player.stop_voice(voice);
    player.stop(id);
    player.stop_all();
    player.set_voice_params(voice, PlaybackParams::new());
    player.unload(id);
    player.suspend();
    player.resume();

    player.set_master_volume(0.25);
    assert_eq!(player.master_volume(), 0.25);
    player.set_master_volume(f32::NAN);
    assert_eq!(player.master_volume(), 1.0);
    player.set_bus_enabled(AudioBus::Music, false);
    assert!(!player.bus_enabled(AudioBus::Music));
    assert!(player.bus_enabled(AudioBus::Effects));
    player.set_bus_volume(AudioBus::Effects, 0.5);
    assert_eq!(player.bus_volume(AudioBus::Effects), 0.5);
}

#[test]
fn noop_player_rejects_invalid_loop_handle() {
    let _guard = crate::registry::test_service_guard();
    let player = NoopAudioPlayer::new();
    assert_eq!(
        player.play_loop(SoundId::NONE, PlaybackParams::new()),
        VoiceId::NONE
    );
}

#[test]
fn registered_player_replaces_the_default() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_audio();
    assert!(!default_audio().is_available());
    let player: AudioPlayerRef = Arc::new(RecordingPlayer::default());
    set_platform_audio(player);
    assert!(default_audio().is_available());
    clear_platform_audio();
    assert!(!default_audio().is_available());
}

#[test]
fn audio_clip_validates_shape() {
    assert!(matches!(
        AudioClip::from_samples(vec![0.0], 0, 44_100),
        Err(AudioError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        AudioClip::from_samples(vec![0.0], 3, 44_100),
        Err(AudioError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        AudioClip::from_samples(vec![0.0], 1, 0),
        Err(AudioError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        AudioClip::from_samples(Vec::new(), 1, 44_100),
        Err(AudioError::Decode(_))
    ));
    assert!(matches!(
        AudioClip::from_samples(vec![0.0, 0.0, 0.0], 2, 44_100),
        Err(AudioError::Decode(_))
    ));

    let clip = AudioClip::from_samples(vec![0.0, 0.5], 2, 44_100).expect("valid clip");
    assert_eq!(clip.frames(), 1);
    assert_eq!(clip.channels(), 2);
    assert!(clip.duration_secs() > 0.0);
    assert_eq!(clip.shared_samples().len(), 2);
    assert!(format!("{clip:?}").contains("AudioClip"));
}

#[test]
fn audio_clip_decode_rejects_unknown_container() {
    assert!(matches!(
        AudioClip::decode(b"OggS not really"),
        Err(AudioError::UnsupportedFormat(_))
    ));
}

#[test]
fn sound_bank_loads_applies_base_volume_and_unloads_on_drop() {
    let player = Arc::new(RecordingPlayer::default());
    let wav = tiny_wav();
    let specs = [
        SoundSpec::new("hit", &wav).volume(0.5),
        SoundSpec::new("music", &wav).bus(AudioBus::Music),
        SoundSpec::new("broken", b"not audio"),
    ];
    let player_ref: AudioPlayerRef = player.clone();
    let bank = SoundBank::load(player_ref, &specs);

    assert_eq!(bank.len(), 3);
    assert!(!bank.is_empty());
    assert_eq!(bank.failures().len(), 1);
    assert_eq!(bank.failures()[0].name, "broken");
    assert!(!bank.id(2).is_valid());
    assert_eq!(bank.find("music"), Some(bank.id(1)));
    assert_eq!(bank.find("absent"), None);
    assert_eq!(bank[0], bank.id(0));
    assert_eq!(bank[99], SoundId::NONE);
    assert!(format!("{bank:?}").contains("SoundBank"));

    bank.play(0);
    bank.play_with(1, PlaybackParams::new().volume(0.5));
    bank.play_named("hit", PlaybackParams::new().pan(1.0));
    bank.play_with(2, PlaybackParams::new());
    bank.play_named("absent", PlaybackParams::new());
    assert_eq!(bank.play_loop(2, PlaybackParams::new()), VoiceId::NONE);
    assert!(bank.play_loop(0, PlaybackParams::new()).is_valid());
    assert_eq!(bank.play_loop(99, PlaybackParams::new()), VoiceId::NONE);
    bank.stop(0);
    bank.stop(2);

    let played = player.played.lock().clone();
    assert_eq!(played.len(), 3);
    assert!((played[0].1.volume - 0.5).abs() < 1e-6);
    assert_eq!(played[0].1.bus, AudioBus::Effects);
    assert!((played[1].1.volume - 0.5).abs() < 1e-6);
    assert_eq!(played[1].1.bus, AudioBus::Music);
    assert!((played[2].1.pan - 1.0).abs() < 1e-6);

    drop(bank);
    assert_eq!(player.unloaded.lock().len(), 3);
}

#[test]
fn sound_bank_key_tracks_names_and_lengths() {
    let a = [1u8, 2, 3];
    let b = [1u8, 2, 3, 4];
    assert_eq!(
        sound_bank_key(&[SoundSpec::new("x", &a)]),
        sound_bank_key(&[SoundSpec::new("x", &a)])
    );
    assert_ne!(
        sound_bank_key(&[SoundSpec::new("x", &a)]),
        sound_bank_key(&[SoundSpec::new("y", &a)])
    );
    assert_ne!(
        sound_bank_key(&[SoundSpec::new("x", &a)]),
        sound_bank_key(&[SoundSpec::new("x", &b)])
    );
    assert_ne!(
        sound_bank_key(&[SoundSpec::new("x", &a)]),
        sound_bank_key(&[SoundSpec::new("x", &a), SoundSpec::new("x", &a)])
    );
}

#[test]
fn provide_audio_publishes_the_platform_player() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_audio();
    let player: AudioPlayerRef = Arc::new(RecordingPlayer::default());
    set_platform_audio(player);

    let captured = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            ProvideAudio(move || {
                *captured.borrow_mut() = Some(local_audio().current().is_available());
            });
        });
    }

    assert_eq!(*captured.borrow(), Some(true));
    clear_platform_audio();
}

#[test]
fn local_audio_defaults_to_the_noop_player() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_audio();
    let captured = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            ProvideAudio(move || {
                *captured.borrow_mut() = Some(local_audio().current().is_available());
            });
        });
    }
    assert_eq!(*captured.borrow(), Some(false));
}

#[test]
fn remember_sound_bank_loads_once_across_recompositions() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_audio();
    let player = Arc::new(RecordingPlayer::default());
    let player_ref: AudioPlayerRef = player.clone();
    set_platform_audio(player_ref);

    let wav = tiny_wav();
    let bank_len = Rc::new(Cell::new(0usize));
    let bank_len_build = Rc::clone(&bank_len);
    let mut build = move || {
        let specs = [SoundSpec::new("a", &wav), SoundSpec::new("b", &wav)];
        let bank = rememberSoundBank(&specs);
        bank_len_build.set(bank.len());
    };

    let key = cranpose_core::location_key(file!(), line!(), column!());
    let mut composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
    composition.render(key, &mut build).expect("first render");
    composition.render(key, &mut build).expect("second render");

    assert_eq!(bank_len.get(), 2);
    assert_eq!(
        *player.next.lock(),
        2,
        "the bank decodes once across renders"
    );
    clear_platform_audio();
}

//! The media contract as a session, rather than as a call at a time.
//!
//! The unit tests in `media.rs` pin each rule. What only shows up across a
//! whole session is here: an interruption arriving mid-track and the framework
//! putting playback back exactly where it was, a lock screen driving a
//! transport the application never wired up, and a screen composed after the
//! host was rebuilt learning what is playing without asking.

use cranpose_services::{
    audio_focus, clear_platform_media_player, current_media_item, media_capabilities, media_volume,
    observe_audio_focus, observe_media_commands, observe_playback_progress, observe_playback_state,
    open_media, pause_media, play_media, playback_progress, playback_state, publish_audio_focus,
    publish_media_command, publish_playback_progress, publish_playback_state,
    set_media_analysis_enabled, set_media_volume, set_platform_media_player, stop_media,
    AudioFocus, MediaCapabilities, MediaCommand, MediaError, MediaItem, MediaMetadata, MediaPlayer,
    PlaybackProgress, PlaybackState, DUCKED_GAIN,
};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// One media session exists per process, so these tests take turns with it.
/// Without this they would each install a backend over the others' and read
/// state a different test published.
fn one_session_at_a_time() -> MutexGuard<'static, ()> {
    static SESSION: Mutex<()> = Mutex::new(());
    SESSION.lock().unwrap_or_else(|error| error.into_inner())
}

/// A backend that behaves like a real one: it publishes what it was asked to
/// do rather than answering questions about it, and it remembers the gain the
/// framework handed it.
struct SessionPlayer {
    capabilities: MediaCapabilities,
    calls: Mutex<Vec<String>>,
    volume: Mutex<f32>,
    position: Mutex<Duration>,
    duration: Mutex<Option<Duration>>,
}

impl SessionPlayer {
    fn install(capabilities: MediaCapabilities) -> Arc<SessionPlayer> {
        clear_platform_media_player();
        let player = Arc::new(SessionPlayer {
            capabilities,
            calls: Mutex::new(Vec::new()),
            volume: Mutex::new(1.0),
            position: Mutex::new(Duration::ZERO),
            duration: Mutex::new(None),
        });
        set_platform_media_player(player.clone());
        player
    }

    fn note(&self, call: &str) {
        self.calls.lock().expect("calls").push(call.to_string());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls").clone()
    }

    fn gain(&self) -> f32 {
        *self.volume.lock().expect("volume")
    }

    /// What a backend's own progress thread does while an item plays.
    fn advance(&self, by: Duration) {
        let position = {
            let mut position = self.position.lock().expect("position");
            *position += by;
            *position
        };
        let duration = *self.duration.lock().expect("duration");
        publish_playback_progress(PlaybackProgress {
            position,
            duration,
            buffered: duration.unwrap_or(position),
        });
    }
}

impl MediaPlayer for SessionPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        self.capabilities
    }

    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.note("prepare");
        *self.position.lock().expect("position") = Duration::ZERO;
        *self.duration.lock().expect("duration") = item.metadata.duration;
        publish_playback_state(PlaybackState::Paused);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        self.note("play");
        publish_playback_state(PlaybackState::Playing);
        Ok(())
    }

    fn pause(&self) {
        self.note("pause");
        publish_playback_state(PlaybackState::Paused);
    }

    fn stop(&self) {
        self.note("stop");
        *self.position.lock().expect("position") = Duration::ZERO;
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        self.note("seek");
        *self.position.lock().expect("position") = position;
        let duration = *self.duration.lock().expect("duration");
        publish_playback_progress(PlaybackProgress {
            position,
            duration,
            buffered: duration.unwrap_or(position),
        });
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        *self.volume.lock().expect("volume") = volume;
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        self.note(&format!("session:{}", metadata.title));
    }
}

fn track(seconds: u64) -> MediaItem {
    MediaItem::new("file:///music/track.flac").with_metadata(
        MediaMetadata::titled("Track")
            .artist("Artist")
            .duration(Duration::from_secs(seconds)),
    )
}

/// A phone call arrives in the middle of a track and ends. Nothing an
/// application wrote decides what happens; the framework's one policy does, and
/// the position it comes back to is the one it left.
#[test]
fn an_interruption_puts_playback_back_where_it_was() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities::TRANSPORT);

    open_media(track(200)).expect("the item opens");
    play_media().expect("the item plays");
    player.advance(Duration::from_secs(30));
    assert_eq!(playback_progress().position, Duration::from_secs(30));

    publish_audio_focus(AudioFocus::LostTransient);
    assert_eq!(playback_state(), PlaybackState::Paused);
    assert_eq!(audio_focus(), AudioFocus::LostTransient);
    // A pause is not a rewind: the position survives the call.
    assert_eq!(playback_progress().position, Duration::from_secs(30));

    publish_audio_focus(AudioFocus::Gained);
    assert!(playback_state().is_playing());
    player.advance(Duration::from_secs(5));
    assert_eq!(playback_progress().position, Duration::from_secs(35));

    assert_eq!(
        player.calls(),
        vec!["prepare", "play", "pause", "play"],
        "the framework paused and resumed; the application asked for neither"
    );
    clear_platform_media_player();
}

/// A navigation prompt talks over the track. The volume the application asks
/// for and the gain the device is given are different numbers, which is what
/// lets a volume slider work while ducked without undoing the duck.
#[test]
fn ducking_lowers_the_device_without_touching_the_applications_volume() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities::TRANSPORT);
    open_media(track(120)).expect("the item opens");
    play_media().expect("the item plays");
    set_media_volume(0.8);
    assert_eq!(player.gain(), 0.8);

    publish_audio_focus(AudioFocus::Ducked);
    assert_eq!(player.gain(), 0.8 * DUCKED_GAIN);
    assert!(playback_state().is_playing(), "ducking is not pausing");

    // The user drags the volume slider while the prompt is speaking.
    set_media_volume(0.4);
    assert_eq!(media_volume(), 0.4);
    assert_eq!(player.gain(), 0.4 * DUCKED_GAIN);

    publish_audio_focus(AudioFocus::Gained);
    assert_eq!(
        player.gain(),
        0.4,
        "the volume the user chose is what comes back"
    );
    clear_platform_media_player();
}

/// The lock screen drives the transport, and the application hears about the
/// one command that needs a playlist it owns.
#[test]
fn a_lock_screen_drives_a_transport_the_application_never_wired_up() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities {
        session: true,
        equalizer: false,
        ..MediaCapabilities::TRANSPORT
    });
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let _commands = observe_media_commands(move |command| {
        recorder.lock().expect("commands").push(command);
    });

    open_media(track(90)).expect("the item opens");
    publish_media_command(MediaCommand::Play);
    assert!(playback_state().is_playing());

    publish_media_command(MediaCommand::SeekTo(Duration::from_secs(45)));
    assert_eq!(playback_progress().position, Duration::from_secs(45));

    publish_media_command(MediaCommand::TogglePlayPause);
    assert_eq!(playback_state(), PlaybackState::Paused);

    publish_media_command(MediaCommand::Next);
    assert_eq!(
        playback_state(),
        PlaybackState::Paused,
        "`Next` needs an order the framework does not have, so it changes nothing here"
    );

    assert_eq!(
        *seen.lock().expect("commands"),
        vec![
            MediaCommand::Play,
            MediaCommand::SeekTo(Duration::from_secs(45)),
            MediaCommand::TogglePlayPause,
            MediaCommand::Next,
        ]
    );
    assert!(player.calls().contains(&"session:Track".to_string()));
    clear_platform_media_player();
}

/// The host tore the surface down and built it again — an Android
/// configuration change, a browser tab restored. The screen that comes back
/// asks nothing: registering is what delivers what is playing.
#[test]
fn a_screen_composed_after_the_host_was_rebuilt_learns_what_is_playing() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities::TRANSPORT);
    open_media(track(300)).expect("the item opens");
    play_media().expect("the item plays");
    player.advance(Duration::from_secs(64));

    // The first screen goes away with the surface.
    {
        let _state = observe_playback_state(|_| {});
        let _progress = observe_playback_progress(|_| {});
    }

    // The rebuilt screen registers and is told at once, without a poll.
    let state = Arc::new(Mutex::new(None));
    let progress = Arc::new(Mutex::new(None));
    let state_sink = Arc::clone(&state);
    let progress_sink = Arc::clone(&progress);
    let _state_observer = observe_playback_state(move |published| {
        *state_sink.lock().expect("state") = Some(published);
    });
    let _progress_observer = observe_playback_progress(move |published| {
        *progress_sink.lock().expect("progress") = Some(published);
    });

    assert_eq!(
        state.lock().expect("state").clone(),
        Some(PlaybackState::Playing)
    );
    assert_eq!(
        progress
            .lock()
            .expect("progress")
            .and_then(|progress| progress.fraction()),
        Some(64.0 / 300.0)
    );
    assert_eq!(
        current_media_item().map(|item| item.display_title().to_string()),
        Some("Track".to_string())
    );
    clear_platform_media_player();
}

/// A screen asks what this device can do before it draws a control for it, and
/// what it is told is what the backend reports — not what the framework hopes.
#[test]
fn a_screen_is_told_which_controls_this_device_will_honour() {
    let _session = one_session_at_a_time();
    SessionPlayer::install(MediaCapabilities {
        seeking: true,
        speed: false,
        looping: true,
        analysis: false,
        session: false,
        equalizer: false,
        probing: false,
    });

    let capabilities = media_capabilities();
    assert!(capabilities.seeking);
    assert!(!capabilities.speed);
    assert!(!capabilities.analysis);
    assert!(
        !set_media_analysis_enabled(true),
        "a visualiser must be told there are no samples rather than drawn silent"
    );

    clear_platform_media_player();
    assert_eq!(
        media_capabilities(),
        MediaCapabilities::default(),
        "with no backend every capability is absent"
    );
}

/// Stopping closes the item. What follows is a fresh session rather than a
/// resumption of the last one.
#[test]
fn stopping_leaves_nothing_of_the_last_session_behind() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities::TRANSPORT);
    open_media(track(60)).expect("the item opens");
    play_media().expect("the item plays");
    player.advance(Duration::from_secs(20));

    stop_media();

    assert_eq!(playback_state(), PlaybackState::Idle);
    assert_eq!(current_media_item(), None);
    assert_eq!(playback_progress(), PlaybackProgress::default());
    assert_eq!(play_media(), Err(MediaError::NothingLoaded));
    assert_eq!(audio_focus(), AudioFocus::Gained);
    clear_platform_media_player();
}

/// Focus changes while nothing is loaded are not a reason to start something.
#[test]
fn regaining_focus_with_nothing_open_starts_nothing() {
    let _session = one_session_at_a_time();
    SessionPlayer::install(MediaCapabilities::TRANSPORT);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let _focus = observe_audio_focus(move |focus| recorder.lock().expect("focus").push(focus));

    publish_audio_focus(AudioFocus::LostTransient);
    publish_audio_focus(AudioFocus::Gained);

    assert_eq!(playback_state(), PlaybackState::Idle);
    assert_eq!(
        *seen.lock().expect("focus"),
        vec![
            AudioFocus::Gained,
            AudioFocus::LostTransient,
            AudioFocus::Gained
        ],
        "the current focus is delivered on registering, then every change"
    );
    clear_platform_media_player();
}

/// A user pause is remembered as a user pause: focus coming back afterwards
/// must not start the track behind their back.
#[test]
fn a_user_pause_survives_an_interruption_that_follows_it() {
    let _session = one_session_at_a_time();
    SessionPlayer::install(MediaCapabilities::TRANSPORT);
    open_media(track(60)).expect("the item opens");
    play_media().expect("the item plays");
    pause_media();

    publish_audio_focus(AudioFocus::LostTransient);
    publish_audio_focus(AudioFocus::Gained);

    assert_eq!(playback_state(), PlaybackState::Paused);
    clear_platform_media_player();
}

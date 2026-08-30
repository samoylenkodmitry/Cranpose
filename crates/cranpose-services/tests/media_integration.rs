use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use cranpose_services::{
    AudioFocus, DUCKED_GAIN, MediaCapabilities, MediaCommand, MediaError, MediaItem, MediaMetadata,
    MediaPlayer, PlaybackProgress, PlaybackState, audio_focus, clear_platform_media_player,
    current_media_item, media_capabilities, media_volume, observe_audio_focus,
    observe_media_commands, observe_playback_progress, observe_playback_state, open_media,
    pause_media, play_media, playback_progress, playback_state, publish_audio_focus,
    publish_media_command, publish_playback_progress, publish_playback_state,
    set_media_analysis_enabled, set_media_volume, set_platform_media_player, stop_media,
};

fn one_session_at_a_time() -> MutexGuard<'static, ()> {
    static SESSION: Mutex<()> = Mutex::new(());
    SESSION.lock().unwrap_or_else(|error| error.into_inner())
}

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

#[test]
fn a_screen_composed_after_the_host_was_rebuilt_learns_what_is_playing() {
    let _session = one_session_at_a_time();
    let player = SessionPlayer::install(MediaCapabilities::TRANSPORT);
    open_media(track(300)).expect("the item opens");
    play_media().expect("the item plays");
    player.advance(Duration::from_secs(64));

    {
        let _state = observe_playback_state(|_| {});
        let _progress = observe_playback_progress(|_| {});
    }

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

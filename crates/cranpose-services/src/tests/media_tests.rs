use super::*;
use crate::registry::test_service_guard;

struct FakePlayer {
    capabilities: MediaCapabilities,
    calls: Mutex<Vec<String>>,
    volume: Mutex<f32>,
    prepare_fails: bool,
}

impl FakePlayer {
    fn new() -> Arc<FakePlayer> {
        Arc::new(FakePlayer {
            capabilities: MediaCapabilities {
                seeking: true,
                speed: true,
                looping: true,
                analysis: true,
                session: true,
                equalizer: true,
                probing: true,
            },
            calls: Mutex::new(Vec::new()),
            volume: Mutex::new(1.0),
            prepare_fails: false,
        })
    }

    fn with(capabilities: MediaCapabilities) -> Arc<FakePlayer> {
        Arc::new(FakePlayer {
            capabilities,
            calls: Mutex::new(Vec::new()),
            volume: Mutex::new(1.0),
            prepare_fails: false,
        })
    }

    fn failing() -> Arc<FakePlayer> {
        Arc::new(FakePlayer {
            capabilities: MediaCapabilities::TRANSPORT,
            calls: Mutex::new(Vec::new()),
            volume: Mutex::new(1.0),
            prepare_fails: true,
        })
    }

    fn note(&self, call: impl Into<String>) {
        self.calls.lock().push(call.into());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }
}

impl MediaPlayer for FakePlayer {
    fn capabilities(&self) -> MediaCapabilities {
        self.capabilities
    }

    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.note(format!("prepare {}", item.uri));
        if self.prepare_fails {
            return Err(MediaError::UnsupportedSource(item.uri.clone()));
        }
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
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        self.note(format!("seek {}", position.as_millis()));
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        *self.volume.lock() = volume;
    }

    fn set_speed(&self, speed: f32) -> bool {
        self.note(format!("speed {speed}"));
        true
    }

    fn set_looping(&self, looping: bool) {
        self.note(format!("looping {looping}"));
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        self.note(format!("analysis {enabled}"));
        true
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        self.note(format!("session {}", metadata.title));
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        vec![
            EqualizerBand::new(60.0, 6.0),
            EqualizerBand::new(1_000.0, 6.0),
            EqualizerBand::new(10_000.0, 6.0),
        ]
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        self.note(format!(
            "equalizer {} preamp {} gains {:?}",
            settings.enabled, settings.preamp_db, settings.gains_db
        ));
    }
}

fn install() -> (crate::registry::TestServiceGuard, Arc<FakePlayer>) {
    let guard = test_service_guard();
    clear_platform_media_player();
    let player = FakePlayer::new();
    set_platform_media_player(player.clone());
    (guard, player)
}

fn track() -> MediaItem {
    MediaItem::new("file:///music/track.flac").with_metadata(
        MediaMetadata::titled("Track")
            .artist("Artist")
            .duration(Duration::from_secs(200)),
    )
}

#[test]
fn metadata_carries_everything_a_lock_screen_shows() {
    let artwork = MediaArtwork {
        bytes: vec![1, 2, 3].into(),
        mime: "image/png".to_string(),
    };
    let metadata = MediaMetadata::titled("Song")
        .artist("Band")
        .album("Record")
        .duration(Duration::from_secs(210))
        .artwork(artwork.clone());

    assert_eq!(metadata.album, "Record");
    assert_eq!(metadata.artwork.as_ref(), Some(&artwork));
    assert!(!metadata.is_empty());
    assert!(!MediaMetadata::default().album("Record").is_empty());
    assert!(MediaMetadata::default().is_empty());
}

#[test]
fn a_band_reports_what_it_can_actually_do() {
    let band = EqualizerBand::new(1_000.0, 6.0);
    assert_eq!(band.clamp_gain(0.0), 0.0);
    assert_eq!(band.clamp_gain(6.0), 6.0);
    assert_eq!(band.clamp_gain(7.5), 6.0);
    assert_eq!(band.clamp_gain(-7.5), -6.0);
}

#[test]
fn an_equalizer_setting_is_clamped_to_the_bands_the_backend_has() {
    let bands = vec![
        EqualizerBand::new(60.0, 6.0),
        EqualizerBand::new(1_000.0, 6.0),
    ];
    let asked = EqualizerSettings {
        enabled: true,
        preamp_db: -3.0,
        gains_db: vec![12.0, -12.0, 4.0],
    };
    let applied = asked.clamped_to(&bands);
    assert_eq!(applied.gains_db, vec![6.0, -6.0]);
    assert_eq!(applied.preamp_db, -3.0);
    assert!(applied.enabled);
}

#[test]
fn a_setting_shorter_than_the_bands_leaves_the_rest_flat() {
    let bands = octave_equalizer_bands(12.0);
    let applied = EqualizerSettings {
        enabled: true,
        preamp_db: 0.0,
        gains_db: vec![3.0],
    }
    .clamped_to(&bands);
    assert_eq!(applied.gains_db.len(), bands.len());
    assert_eq!(applied.gains_db[0], 3.0);
    assert!(applied.gains_db[1..].iter().all(|gain| *gain == 0.0));
}

#[test]
fn a_curve_reaches_the_backend_clamped_to_its_own_bands() {
    let (_guard, player) = install();

    assert_eq!(media_equalizer_bands().len(), 3);
    assert!(set_media_equalizer(EqualizerSettings {
        enabled: true,
        preamp_db: -2.0,
        gains_db: vec![9.0, 0.0, -9.0],
    }));

    assert!(
        player
            .calls()
            .iter()
            .any(|call| call == "equalizer true preamp -2 gains [6.0, 0.0, -6.0]"),
        "the backend was not given the clamped curve: {:?}",
        player.calls()
    );
}

#[test]
fn a_curve_is_remembered_even_where_nothing_can_apply_it() {
    let _guard = test_service_guard();
    clear_platform_media_player();

    let asked = EqualizerSettings {
        enabled: true,
        preamp_db: -1.0,
        gains_db: vec![4.0, -4.0],
    };
    assert!(!set_media_equalizer(asked.clone()));
    assert_eq!(media_equalizer(), asked);
    assert!(media_equalizer_bands().is_empty());
}

#[test]
fn a_backend_without_an_equalizer_says_so_rather_than_pretending() {
    let _guard = test_service_guard();
    clear_platform_media_player();
    set_platform_media_player(FakePlayer::with(MediaCapabilities {
        equalizer: false,
        ..MediaCapabilities::TRANSPORT
    }));

    assert!(media_equalizer_bands().is_empty());
    assert!(!set_media_equalizer(EqualizerSettings::flat(10)));
}

#[test]
fn an_item_falls_back_to_its_file_name_for_a_title() {
    assert_eq!(
        MediaItem::new("file:///music/03 - Song.mp3").display_title(),
        "03 - Song.mp3"
    );
    assert_eq!(
        MediaItem::new("https://host/stream?token=1").display_title(),
        "stream"
    );
    assert_eq!(track().display_title(), "Track");
}

#[test]
fn progress_reports_fractions_only_for_items_that_have_a_length() {
    let known = PlaybackProgress::new(Duration::from_secs(30), Duration::from_secs(120));
    assert_eq!(known.fraction(), Some(0.25));
    assert_eq!(known.buffered_fraction(), Some(1.0));

    let live = PlaybackProgress {
        position: Duration::from_secs(30),
        duration: None,
        buffered: Duration::from_secs(35),
    };
    assert_eq!(live.fraction(), None);
    assert_eq!(live.buffered_fraction(), None);
}

#[test]
fn progress_never_reads_past_the_end_of_the_item() {
    let progress = PlaybackProgress::new(Duration::from_secs(500), Duration::from_secs(120));
    assert_eq!(progress.position, Duration::from_secs(120));
    assert_eq!(progress.fraction(), Some(1.0));
}

#[test]
fn samples_reject_a_layout_that_does_not_describe_the_data() {
    assert!(MediaSamples::new(44_100, 2, 0, vec![0.0; 3]).is_none());
    assert!(MediaSamples::new(0, 2, 0, vec![0.0; 4]).is_none());
    assert!(MediaSamples::new(44_100, 0, 0, vec![0.0; 4]).is_none());

    let block = MediaSamples::new(44_100, 2, 7, vec![0.0; 4410]).expect("well-formed block");
    assert_eq!(block.frames(), 2205);
    assert_eq!(block.span(), Duration::from_millis(50));
    assert_eq!(block.sequence, 7);
}

#[test]
fn without_a_backend_every_call_reports_that_it_is_unsupported() {
    let _guard = test_service_guard();
    clear_platform_media_player();

    assert!(!media_playback_supported());
    assert_eq!(media_capabilities(), MediaCapabilities::default());
    assert_eq!(open_media(track()), Err(MediaError::Unsupported));
    assert_eq!(
        playback_state(),
        PlaybackState::Failed(MediaError::Unsupported)
    );
    assert_eq!(play_media(), Err(MediaError::Unsupported));
    assert_eq!(seek_media(Duration::ZERO), Err(MediaError::Unsupported));
    assert!(!set_media_speed(2.0));
    assert!(!set_media_analysis_enabled(true));
}

#[test]
fn opening_an_item_shows_the_wait_before_the_backend_is_asked() {
    let (_guard, player) = install();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let _observer = observe_playback_state(move |state| recorder.lock().push(state));

    open_media(track()).expect("the fake backend opens anything");

    assert_eq!(
        *seen.lock(),
        vec![
            PlaybackState::Idle,
            PlaybackState::Loading,
            PlaybackState::Paused,
        ]
    );
    assert_eq!(
        player.calls(),
        vec!["session Track", "prepare file:///music/track.flac"]
    );
    assert_eq!(current_media_item().map(|item| item.uri), Some(track().uri));
}

#[test]
fn an_item_that_cannot_be_opened_publishes_the_failure() {
    let _guard = test_service_guard();
    clear_platform_media_player();
    set_platform_media_player(FakePlayer::failing());

    let error = open_media(track()).expect_err("the failing backend refuses");
    assert_eq!(
        error,
        MediaError::UnsupportedSource("file:///music/track.flac".to_string())
    );
    assert_eq!(playback_state().failure(), Some(&error));
}

#[test]
fn the_transport_routes_to_the_backend_and_publishes_what_it_did() {
    let (_guard, player) = install();

    open_media(track()).expect("opens");
    play_media().expect("plays");
    assert!(playback_state().is_playing());

    toggle_media();
    assert_eq!(playback_state(), PlaybackState::Paused);

    toggle_media();
    assert!(playback_state().is_playing());

    stop_media();
    assert_eq!(playback_state(), PlaybackState::Idle);
    assert_eq!(current_media_item(), None);

    assert_eq!(
        player.calls(),
        vec![
            "session Track",
            "prepare file:///music/track.flac",
            "play",
            "pause",
            "play",
            "stop",
        ]
    );
}

#[test]
fn playing_nothing_reports_that_nothing_is_loaded() {
    let (_guard, _player) = install();

    assert_eq!(play_media(), Err(MediaError::NothingLoaded));
    assert_eq!(
        seek_media(Duration::from_secs(1)),
        Err(MediaError::NothingLoaded)
    );
}

#[test]
fn a_seek_is_clamped_to_the_item_rather_than_to_each_backend() {
    let (_guard, player) = install();
    open_media(track()).expect("opens");

    seek_media(Duration::from_secs(1_000)).expect("seeks");

    assert!(player.calls().contains(&"seek 200000".to_string()));
}

#[test]
fn a_seek_bar_fraction_maps_onto_the_item() {
    let (_guard, player) = install();
    open_media(track()).expect("opens");

    seek_media_fraction(0.25).expect("seeks");
    seek_media_fraction(3.0).expect("clamps rather than refusing");

    let calls = player.calls();
    assert!(calls.contains(&"seek 50000".to_string()));
    assert!(calls.contains(&"seek 200000".to_string()));
}

#[test]
fn a_stream_with_no_length_has_no_seek_bar_fraction() {
    let (_guard, _player) = install();
    open_media(MediaItem::new("https://host/live")).expect("opens");

    assert_eq!(seek_media_fraction(0.5), Err(MediaError::NotSeekable));
}

#[test]
fn a_backend_that_cannot_seek_says_so_instead_of_moving_nothing() {
    let _guard = test_service_guard();
    clear_platform_media_player();
    set_platform_media_player(FakePlayer::with(MediaCapabilities {
        seeking: false,
        ..MediaCapabilities::TRANSPORT
    }));

    open_media(track()).expect("opens");
    assert_eq!(
        seek_media(Duration::from_secs(1)),
        Err(MediaError::NotSeekable)
    );
}

#[test]
fn what_reaches_the_device_is_the_volume_combined_with_the_focus_gain() {
    let (_guard, player) = install();
    open_media(track()).expect("opens");

    set_media_volume(0.5);
    assert_eq!(*player.volume.lock(), 0.5);
    assert_eq!(media_volume(), 0.5);

    publish_audio_focus(AudioFocus::Ducked);
    assert_eq!(*player.volume.lock(), 0.5 * DUCKED_GAIN);

    set_media_volume(1.0);
    assert_eq!(*player.volume.lock(), DUCKED_GAIN);

    publish_audio_focus(AudioFocus::Gained);
    assert_eq!(*player.volume.lock(), 1.0);
}

#[test]
fn a_volume_outside_the_range_is_brought_back_into_it() {
    let (_guard, player) = install();

    set_media_volume(4.0);
    assert_eq!(media_volume(), 1.0);
    set_media_volume(-1.0);
    assert_eq!(media_volume(), 0.0);
    assert_eq!(*player.volume.lock(), 0.0);
}

#[test]
fn a_transient_loss_pauses_and_the_next_gain_resumes() {
    let (_guard, _player) = install();
    open_media(track()).expect("opens");
    play_media().expect("plays");

    publish_audio_focus(AudioFocus::LostTransient);
    assert_eq!(playback_state(), PlaybackState::Paused);

    publish_audio_focus(AudioFocus::Gained);
    assert!(playback_state().is_playing());
}

#[test]
fn regaining_focus_does_not_resume_what_the_user_paused() {
    let (_guard, _player) = install();
    open_media(track()).expect("opens");
    play_media().expect("plays");
    pause_media();

    publish_audio_focus(AudioFocus::LostTransient);
    publish_audio_focus(AudioFocus::Gained);

    assert_eq!(playback_state(), PlaybackState::Paused);
}

#[test]
fn focus_lost_for_good_stops_and_does_not_come_back() {
    let (_guard, _player) = install();
    open_media(track()).expect("opens");
    play_media().expect("plays");

    publish_audio_focus(AudioFocus::Lost);
    assert_eq!(playback_state(), PlaybackState::Idle);

    publish_audio_focus(AudioFocus::Gained);
    assert_eq!(playback_state(), PlaybackState::Idle);
}

#[test]
fn session_commands_drive_the_transport_and_still_reach_the_application() {
    let (_guard, player) = install();
    open_media(track()).expect("opens");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let _observer = observe_media_commands(move |command| recorder.lock().push(command));

    publish_media_command(MediaCommand::Play);
    assert!(playback_state().is_playing());
    publish_media_command(MediaCommand::TogglePlayPause);
    assert_eq!(playback_state(), PlaybackState::Paused);
    publish_media_command(MediaCommand::SeekTo(Duration::from_secs(10)));
    publish_media_command(MediaCommand::Next);

    assert_eq!(
        *seen.lock(),
        vec![
            MediaCommand::Play,
            MediaCommand::TogglePlayPause,
            MediaCommand::SeekTo(Duration::from_secs(10)),
            MediaCommand::Next,
        ]
    );
    assert!(player.calls().contains(&"seek 10000".to_string()));
    assert_eq!(playback_state(), PlaybackState::Paused);
}

#[test]
fn next_and_previous_are_the_commands_the_framework_leaves_alone() {
    assert!(MediaCommand::Play.is_transport());
    assert!(MediaCommand::SeekTo(Duration::ZERO).is_transport());
    assert!(!MediaCommand::Next.is_transport());
    assert!(!MediaCommand::Previous.is_transport());
}

#[test]
fn analysis_is_off_until_it_is_asked_for_and_only_where_it_exists() {
    let (_guard, player) = install();
    assert!(set_media_analysis_enabled(true));
    assert!(player.calls().contains(&"analysis true".to_string()));

    clear_platform_media_player();
    set_platform_media_player(FakePlayer::with(MediaCapabilities::TRANSPORT));
    assert!(!set_media_analysis_enabled(true));
}

#[test]
fn the_backend_states_which_audio_formats_it_decodes() {
    let guard = test_service_guard();
    clear_platform_media_player();
    assert!(
        media_audio_extensions().is_empty(),
        "with no backend there is nothing to claim"
    );

    struct Narrow;
    impl MediaPlayer for Narrow {
        fn capabilities(&self) -> MediaCapabilities {
            MediaCapabilities::TRANSPORT
        }
        fn prepare(&self, _item: &MediaItem) -> Result<(), MediaError> {
            Ok(())
        }
        fn play(&self) -> Result<(), MediaError> {
            Ok(())
        }
        fn pause(&self) {}
        fn stop(&self) {}
        fn set_volume(&self, _volume: f32) {}
        fn audio_extensions(&self) -> Vec<&'static str> {
            vec!["mp3", "wav"]
        }
    }

    set_platform_media_player(Arc::new(Narrow));
    assert_eq!(media_audio_extensions(), vec!["mp3", "wav"]);

    clear_platform_media_player();
    set_platform_media_player(FakePlayer::with(MediaCapabilities::TRANSPORT));
    assert!(
        media_audio_extensions().is_empty(),
        "a backend with no opinion says so rather than guessing"
    );
    drop(guard);
}

#[test]
fn the_newest_sample_block_replaces_the_stored_one() {
    let (_guard, _player) = install();
    let first = MediaSamples::new(48_000, 1, 1, vec![0.25; 8]).expect("block");
    let second = MediaSamples::new(48_000, 1, 2, vec![0.5; 8]).expect("block");

    publish_media_samples(first);
    publish_media_samples(second.clone());

    assert_eq!(latest_media_samples(), Some(second));
    record_dropped_media_samples();
    record_dropped_media_samples();
    assert_eq!(dropped_media_samples(), 2);
}

#[test]
fn turning_analysis_off_forgets_the_last_block() {
    let (_guard, _player) = install();
    publish_media_samples(MediaSamples::new(48_000, 1, 1, vec![0.25; 8]).expect("block"));

    assert!(set_media_analysis_enabled(false));

    assert_eq!(latest_media_samples(), None);
}

#[test]
fn observers_stop_being_called_once_they_are_dropped() {
    let (_guard, _player) = install();
    let seen = Arc::new(Mutex::new(0usize));
    let recorder = Arc::clone(&seen);
    let observer = observe_playback_progress(move |_| *recorder.lock() += 1);

    publish_playback_progress(PlaybackProgress::new(
        Duration::from_secs(1),
        Duration::from_secs(10),
    ));
    let delivered = *seen.lock();
    drop(observer);
    publish_playback_progress(PlaybackProgress::new(
        Duration::from_secs(2),
        Duration::from_secs(10),
    ));

    assert_eq!(*seen.lock(), delivered);
}

#[test]
fn published_progress_never_reads_past_the_end() {
    let (_guard, _player) = install();
    publish_playback_progress(PlaybackProgress {
        position: Duration::from_secs(99),
        duration: Some(Duration::from_secs(10)),
        buffered: Duration::from_secs(99),
    });

    let progress = playback_progress();
    assert_eq!(progress.position, Duration::from_secs(10));
    assert_eq!(progress.buffered, Duration::from_secs(10));
}

#[test]
fn playing_holds_the_runtime_awake_and_stopping_lets_it_sleep() {
    let (_guard, _player) = install();
    assert!(!holds_background_work());

    open_media(track()).expect("opens");
    assert!(
        !holds_background_work(),
        "an item that is open but not playing is not work the runtime must keep turning for"
    );

    play_media().expect("plays");
    assert!(holds_background_work());

    pause_media();
    assert!(!holds_background_work());

    play_media().expect("plays");
    assert!(holds_background_work());
    stop_media();
    assert!(!holds_background_work());
}

#[test]
fn a_destroyed_host_stops_playback_but_a_backgrounded_one_does_not() {
    let (_guard, _player) = install();
    open_media(track()).expect("opens");
    play_media().expect("plays");

    on_lifecycle(LifecycleEvent {
        from: LifecycleState::Resumed,
        to: LifecycleState::Stopped,
    });
    assert!(playback_state().is_playing());

    on_lifecycle(LifecycleEvent {
        from: LifecycleState::Stopped,
        to: LifecycleState::Destroyed,
    });
    assert_eq!(playback_state(), PlaybackState::Idle);
}

#[test]
fn metadata_learned_after_playback_started_reaches_the_session() {
    let (_guard, player) = install();
    open_media(MediaItem::new("file:///music/untagged.mp3")).expect("opens");

    set_media_metadata(MediaMetadata::titled("Late Tag").artist("Artist"));

    assert!(player.calls().contains(&"session Late Tag".to_string()));
    assert_eq!(
        current_media_item().map(|item| item.metadata.title),
        Some("Late Tag".to_string())
    );
}

#[test]
fn metadata_with_nothing_in_it_is_metadata_a_lock_screen_can_skip() {
    assert!(MediaMetadata::default().is_empty());
    assert!(!MediaMetadata::titled("Track").is_empty());
}

#[test]
fn a_path_opens_here_and_everything_else_is_the_platforms() {
    struct Opener;
    impl MediaSourceOpener for Opener {
        fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle> {
            let path = crate::test_scratch_dir("media-source-opener").join("document.bin");
            std::fs::write(&path, uri.as_bytes())?;
            Ok(MediaSourceHandle {
                stream: File::open(path)?,
                len: Some(uri.len() as u64),
            })
        }
    }
    fn read(handle: MediaSourceHandle) -> String {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut { handle.stream }, &mut text).expect("read");
        text
    }

    clear_platform_media_source_opener();
    let path = crate::test_scratch_dir("media-source").join("track.bin");
    std::fs::write(&path, b"bytes").expect("write the fixture");
    let file = open_media_source(&uri_for_path(&path)).expect("the file");
    assert_eq!(file.len, Some(5), "a real file states its length");
    assert_eq!(read(file), "bytes");
    let error = open_media_source("content://provider/document/7").expect_err("no opener");
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);

    set_platform_media_source_opener(Arc::new(Opener));
    let opened = open_media_source("content://provider/document/7").expect("the opener");
    clear_platform_media_source_opener();
    assert_eq!(opened.len, Some(29));
    assert_eq!(read(opened), "content://provider/document/7");
}

#[test]
fn a_path_survives_the_round_trip_through_a_uri() {
    let path = PathBuf::from("/music/Sgt. Pepper's #1.mp3");
    let uri = uri_for_path(&path);

    assert_eq!(uri, "file:///music/Sgt.%20Pepper%27s%20%231.mp3");
    assert_eq!(path_from_uri(&uri), Some(path));
}

#[test]
fn a_windows_path_keeps_its_drive_letter() {
    let uri = uri_for_path(Path::new("C:\\Music\\track.mp3"));

    assert_eq!(uri, "file:///C%3A/Music/track.mp3");
    assert_eq!(
        path_from_uri(&uri),
        Some(PathBuf::from("C:/Music/track.mp3"))
    );
}

#[test]
fn a_bare_path_is_accepted_as_itself() {
    assert_eq!(
        path_from_uri("/music/track.mp3"),
        Some(PathBuf::from("/music/track.mp3"))
    );
}

#[test]
fn anything_that_is_not_a_local_file_has_no_path() {
    assert_eq!(path_from_uri("https://host/stream.mp3"), None);
    assert_eq!(path_from_uri("content://media/audio/1"), None);
    assert_eq!(path_from_uri("blob:https://host/abc"), None);
    assert_eq!(path_from_uri("file://host/share/track.mp3"), None);
    assert_eq!(path_from_uri(""), None);
}

#[test]
fn a_truncated_escape_is_not_guessed_at() {
    assert_eq!(path_from_uri("file:///music/track%2"), None);
    assert_eq!(path_from_uri("file:///music/track%zz.mp3"), None);
}

#[test]
fn speed_and_looping_reach_a_backend_that_has_them() {
    let (_guard, player) = install();
    assert!(set_media_speed(1.5));
    set_media_looping(true);

    let calls = player.calls();
    assert!(calls.contains(&"speed 1.5".to_string()));
    assert!(calls.contains(&"looping true".to_string()));
}

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_macros::composable;
use cranpose_services::{
    AbrVariant, AudioFocus, AudioRole, ClippingConfiguration, Cue, CueAlignment, CueGroup,
    DUCKED_GAIN, FrameTiming, HlsRenditionType, LIP_SYNC_TOLERANCE, LoadControl, ManifestError,
    MasterClock, MediaCommand, MediaItem, MediaProperties, PlaybackParameters, Player,
    PlayerBackend, PlayerBackendFactory, PlayerCommand, PlayerError, PlayerEvent, PlayerSink,
    PlayerState, PositionDiscontinuityReason, RepeatMode, SubtitleConfiguration, TrackEntry,
    TrackFormat, TrackGroup, TrackSelectionOverride, TrackSelectionParameters, TrackType, Tracks,
    VideoSize, parse_dash_mpd, parse_hls_master, parse_hls_media, parse_srt, parse_subtitles,
    parse_webvtt, publish_audio_focus, publish_media_command, rememberIsLoading, rememberIsPlaying,
    rememberPlayerError, rememberPlayerState, select_abr_variant,
};

#[derive(Clone, Debug, PartialEq)]
enum BackendCall {
    Open(String, Duration),
    Play,
    Pause,
    Stop,
    SeekTo(Duration),
    Volume(f32),
    Parameters(PlaybackParameters),
    TrackSelection(TrackSelectionParameters),
    Release,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().expect("test mutex")
}

struct FakeBackend {
    calls: Arc<Mutex<Vec<BackendCall>>>,
}

impl PlayerBackend for FakeBackend {
    fn open(&self, item: &MediaItem, start_position: Duration) {
        lock(&self.calls).push(BackendCall::Open(item.uri.clone(), start_position));
    }

    fn play(&self) {
        lock(&self.calls).push(BackendCall::Play);
    }

    fn pause(&self) {
        lock(&self.calls).push(BackendCall::Pause);
    }

    fn stop(&self) {
        lock(&self.calls).push(BackendCall::Stop);
    }

    fn seek_to(&self, position: Duration) {
        lock(&self.calls).push(BackendCall::SeekTo(position));
    }

    fn set_volume(&self, volume: f32) {
        lock(&self.calls).push(BackendCall::Volume(volume));
    }

    fn set_playback_parameters(&self, parameters: PlaybackParameters) {
        lock(&self.calls).push(BackendCall::Parameters(parameters));
    }

    fn set_track_selection(&self, parameters: &TrackSelectionParameters) {
        lock(&self.calls).push(BackendCall::TrackSelection(parameters.clone()));
    }

    fn release(&self) {
        lock(&self.calls).push(BackendCall::Release);
    }
}

struct FakeFactory {
    calls: Arc<Mutex<Vec<BackendCall>>>,
    sink: Arc<Mutex<Option<PlayerSink>>>,
}

impl FakeFactory {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<BackendCall>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let factory = Arc::new(Self {
            calls: Arc::clone(&calls),
            sink: Arc::new(Mutex::new(None)),
        });
        (factory, calls)
    }

    fn sink(&self) -> PlayerSink {
        lock(&self.sink)
            .clone()
            .expect("backend created by PlayerBuilder")
    }
}

impl PlayerBackendFactory for FakeFactory {
    fn create(
        &self,
        sink: PlayerSink,
        _load_control: LoadControl,
        _audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        *lock(&self.sink) = Some(sink);
        Ok(Arc::new(FakeBackend {
            calls: Arc::clone(&self.calls),
        }))
    }
}

#[test]
fn player_lifecycle_commands_and_error_recovery() {
    let (factory, calls) = FakeFactory::new();
    let player = Player::builder()
        .with_load_control(LoadControl::FEED)
        .with_audio_role(AudioRole::Secondary)
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player builds");
    let sink = factory.sink();

    assert_eq!(player.playback_state(), PlayerState::Idle);
    assert!(!player.is_playing());
    assert!(!player.is_command_available(PlayerCommand::PlayPause));
    assert_eq!(player.load_control(), LoadControl::FEED);
    assert_eq!(player.audio_role(), AudioRole::Secondary);

    player.set_media_item(MediaItem::new("https://example.com/clip.mp4"));
    assert!(player.is_command_available(PlayerCommand::Prepare));
    assert!(player.is_command_available(PlayerCommand::PlayPause));

    player.prepare();
    assert_eq!(player.playback_state(), PlayerState::Buffering);
    assert!(lock(&calls).iter().any(|c| matches!(
        c,
        BackendCall::Open(uri, pos) if uri == "https://example.com/clip.mp4" && *pos == Duration::ZERO
    )));

    sink.set_loading(true);
    assert!(player.is_loading());
    sink.set_media_properties(MediaProperties {
        duration: Some(Duration::from_secs(30)),
        is_live: false,
        is_seekable: true,
    });
    assert!(player.is_command_available(PlayerCommand::SeekInCurrentMediaItem));
    assert!(player.is_current_media_item_seekable());
    assert!(!player.is_current_media_item_live());
    assert_eq!(player.duration(), Some(Duration::from_secs(30)));

    sink.set_playback_state(PlayerState::Ready);
    assert_eq!(player.playback_state(), PlayerState::Ready);
    assert!(!player.is_playing());

    player.play();
    assert!(player.play_when_ready());
    assert!(player.is_playing());

    sink.report_error(PlayerError::Network("timeout".into()));
    assert_eq!(player.playback_state(), PlayerState::Idle);
    assert!(!player.is_playing());
    assert_eq!(
        player.player_error(),
        Some(PlayerError::Network("timeout".into()))
    );

    player.prepare();
    assert_eq!(player.player_error(), None);
    assert_eq!(player.playback_state(), PlayerState::Buffering);
}

#[test]
fn player_video_size_tracks_cues_first_frame_and_release() {
    let (factory, calls) = FakeFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player builds");
    let sink = factory.sink();

    let events: Arc<Mutex<Vec<PlayerEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&events);
    let _observer = player.observe_events(move |ev| lock(&sink_events).push(ev));

    let rotated = VideoSize {
        width: 1920,
        height: 1080,
        unapplied_rotation_degrees: 90,
        pixel_width_height_ratio: 1.0,
    };
    sink.set_video_size(rotated);
    assert_eq!(player.video_size(), rotated);
    let aspect = rotated.display_aspect_ratio().expect("non-zero");
    assert!((aspect - (1080.0 / 1920.0)).abs() < 1e-4);

    let tracks = Tracks {
        groups: vec![TrackGroup {
            id: "v0".into(),
            track_type: TrackType::Video,
            tracks: vec![TrackEntry {
                format: TrackFormat {
                    id: "1080p".into(),
                    width: Some(1920),
                    height: Some(1080),
                    bitrate: Some(5_000_000),
                    ..TrackFormat::default()
                },
                supported: true,
                selected: true,
            }],
            adaptive_supported: true,
        }],
    };
    sink.set_tracks(tracks);
    assert!(player.current_tracks().contains_type(TrackType::Video));
    assert!(player.current_tracks().is_type_selected(TrackType::Video));

    let cues = CueGroup {
        cues: vec![Cue {
            text: "Hello".into(),
            line: None,
            position: None,
            size: None,
            text_alignment: CueAlignment::Center,
            start_time: Duration::from_secs(1),
            end_time: Duration::from_secs(3),
        }],
        presentation_time: Duration::from_secs(1),
    };
    sink.set_cues(cues.clone());
    assert_eq!(player.current_cues(), cues);

    sink.notify_rendered_first_frame();
    sink.notify_rendered_first_frame();
    assert!(player.rendered_first_frame());
    let first_frame_count = lock(&events)
        .iter()
        .filter(|ev| matches!(ev, PlayerEvent::RenderedFirstFrame))
        .count();
    assert_eq!(first_frame_count, 1);

    player.release();
    assert_eq!(player.playback_state(), PlayerState::Idle);
    assert!(!player.is_command_available(PlayerCommand::Stop));
    let call_count_after_release = lock(&calls).len();
    assert!(matches!(lock(&calls).last(), Some(BackendCall::Release)));
    player.play();
    player.seek_to(Duration::from_secs(5));
    assert_eq!(lock(&calls).len(), call_count_after_release);
}

#[test]
fn playlist_repeat_shuffle_and_seek_rules() {
    let (factory, calls) = FakeFactory::new();
    let player = Player::builder()
        .with_seek_back_increment(Duration::from_secs(10))
        .with_seek_forward_increment(Duration::from_secs(20))
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player builds");
    let sink = factory.sink();

    let items = vec![
        MediaItem::new("https://example.com/0.mp4"),
        MediaItem::new("https://example.com/1.mp4"),
        MediaItem::new("https://example.com/2.mp4"),
        MediaItem::new("https://example.com/3.mp4"),
    ];
    player.set_media_items(items, 1, Duration::from_secs(5));
    assert_eq!(player.media_item_count(), 4);
    assert_eq!(player.current_media_item_index(), 1);
    assert_eq!(player.current_position(), Duration::from_secs(5));
    assert!(player.has_previous_media_item());
    assert!(player.has_next_media_item());

    player.prepare();
    player.play();
    sink.set_media_properties(MediaProperties {
        duration: Some(Duration::from_secs(25)),
        is_live: false,
        is_seekable: true,
    });
    sink.set_playback_state(PlayerState::Ready);

    // Seek forward clamps at duration even while playing; pause to assert exact zero on seek back.
    player.seek_to(Duration::from_secs(12));
    player.seek_forward();
    assert_eq!(player.current_position(), Duration::from_secs(25));
    player.pause();
    player.seek_to(Duration::from_secs(4));
    player.seek_back();
    assert_eq!(player.current_position(), Duration::ZERO);
    player.play();

    // Past 3 s: seek_to_previous restarts current item instead of jumping back.
    player.pause();
    player.seek_to(Duration::from_secs(8));
    player.seek_to_previous();
    assert_eq!(player.current_media_item_index(), 1);
    assert_eq!(player.current_position(), Duration::ZERO);

    // Within 3 s: seek_to_previous jumps to previous item (0).
    player.seek_to_previous();
    assert_eq!(player.current_media_item_index(), 0);
    player.play();

    // Auto-advance on Ended moves 0 -> 1, and RepeatMode::One loops item 1.
    sink.set_playback_state(PlayerState::Ended);
    assert_eq!(player.current_media_item_index(), 1);
    player.set_repeat_mode(RepeatMode::One);
    sink.set_playback_state(PlayerState::Ended);
    assert_eq!(player.current_media_item_index(), 1);

    // RepeatMode::All wraps from last item back to 0.
    player.set_repeat_mode(RepeatMode::All);
    player.seek_to_next();
    player.seek_to_next();
    assert_eq!(player.current_media_item_index(), 3);
    player.seek_to_next();
    assert_eq!(player.current_media_item_index(), 0);

    // Shuffle keeps the active index first and visits all 4 distinct items.
    player.set_repeat_mode(RepeatMode::Off);
    player.set_shuffle_mode(true);
    let mut visited = vec![player.current_media_item_index()];
    while player.has_next_media_item() {
        player.seek_to_next();
        visited.push(player.current_media_item_index());
    }
    assert_eq!(visited.len(), 4);
    visited.sort_unstable();
    assert_eq!(visited, vec![0, 1, 2, 3]);

    // Add, remove, and clear adjust indices cleanly.
    player.add_media_item(MediaItem::new("https://example.com/4.mp4"));
    assert_eq!(player.media_item_count(), 5);
    player.remove_media_item(0);
    assert_eq!(player.media_item_count(), 4);
    player.clear_media_items();
    assert_eq!(player.media_item_count(), 0);
    assert_eq!(player.playback_state(), PlayerState::Idle);
    assert!(lock(&calls).contains(&BackendCall::Stop));
}

#[test]
fn volume_mute_speed_and_track_selection_propagate_to_backend() {
    let (factory, calls) = FakeFactory::new();
    let player = Player::builder()
        .with_audio_role(AudioRole::Muted)
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player builds");

    assert!(player.is_muted());
    player.set_volume(0.6);
    assert!((player.volume() - 0.6).abs() < 1e-6);
    assert_eq!(lock(&calls).last(), Some(&BackendCall::Volume(0.0)));

    player.set_muted(false);
    assert_eq!(lock(&calls).last(), Some(&BackendCall::Volume(0.6)));

    player.set_playback_parameters(PlaybackParameters::new(f32::NAN, 99.0));
    assert_eq!(
        player.playback_parameters(),
        PlaybackParameters {
            speed: 1.0,
            pitch: 16.0
        }
    );

    let selection = TrackSelectionParameters::default()
        .with_max_video_size(1280, 720)
        .with_preferred_audio_language("en")
        .with_disabled_type(TrackType::Text)
        .with_override(TrackSelectionOverride {
            group_id: "v0".into(),
            track_indices: vec![1],
        });
    player.set_track_selection_parameters(selection.clone());
    assert_eq!(player.track_selection_parameters(), selection);
    assert_eq!(
        lock(&calls).last(),
        Some(&BackendCall::TrackSelection(selection))
    );
}

#[composable]
fn ObservePlayer(
    player: Player,
    state_out: Rc<Cell<PlayerState>>,
    playing_out: Rc<Cell<bool>>,
    loading_out: Rc<Cell<bool>>,
    err_out: Rc<Cell<bool>>,
    recompositions: Rc<Cell<u32>>,
) {
    recompositions.set(recompositions.get() + 1);
    state_out.set(rememberPlayerState(&player).value());
    playing_out.set(rememberIsPlaying(&player).value());
    loading_out.set(rememberIsLoading(&player).value());
    err_out.set(rememberPlayerError(&player).value().is_some());
}

#[test]
fn compose_hooks_recompose_only_on_matching_transitions() {
    let (factory, _) = FakeFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player builds");
    let sink = factory.sink();
    player.set_media_item(MediaItem::new("https://example.com/video.mp4"));

    let mut composition = Composition::new(MemoryApplier::new());
    let observed_state = Rc::new(Cell::new(PlayerState::Idle));
    let observed_playing = Rc::new(Cell::new(false));
    let observed_loading = Rc::new(Cell::new(false));
    let observed_err = Rc::new(Cell::new(false));
    let recompositions = Rc::new(Cell::new(0_u32));

    let p = player.clone();
    let s = Rc::clone(&observed_state);
    let pl = Rc::clone(&observed_playing);
    let l = Rc::clone(&observed_loading);
    let e = Rc::clone(&observed_err);
    let r = Rc::clone(&recompositions);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            ObservePlayer(
                p.clone(),
                Rc::clone(&s),
                Rc::clone(&pl),
                Rc::clone(&l),
                Rc::clone(&e),
                Rc::clone(&r),
            );
        })
        .expect("initial render");
    assert_eq!(recompositions.get(), 1);

    let pump_recompose = |comp: &mut Composition<MemoryApplier>, done: &dyn Fn() -> bool| -> bool {
        let runtime = comp.runtime_handle();
        for _ in 0..50 {
            runtime.drain_ui();
            comp.process_invalid_scopes().expect("recompose");
            if done() {
                return true;
            }
            std::thread::yield_now();
        }
        done()
    };

    player.prepare();
    sink.set_loading(true);
    sink.set_playback_state(PlayerState::Ready);
    player.play();

    assert!(pump_recompose(&mut composition, &|| {
        observed_state.get() == PlayerState::Ready
            && observed_playing.get()
            && observed_loading.get()
    }));

    // Unrelated progress/volume updates do not trigger recomposition of state/playing/loading hooks.
    let count_before_idle = recompositions.get();
    sink.set_progress(Duration::from_secs(2), Duration::from_secs(10));
    player.set_volume(0.5);
    composition.runtime_handle().drain_ui();
    composition.process_invalid_scopes().expect("recompose");
    assert_eq!(recompositions.get(), count_before_idle);

    sink.report_error(PlayerError::Decoder("corrupt".into()));
    assert!(pump_recompose(&mut composition, &|| {
        observed_err.get() && !observed_playing.get()
    }));
}

#[test]
fn master_clock_enforces_40ms_lip_sync_window() {
    let clock = MasterClock::new();
    assert!(!clock.is_running());
    assert_eq!(clock.position(), Duration::ZERO);

    clock.sync_audio(Duration::from_millis(1_000));
    assert_eq!(clock.position(), Duration::from_millis(1_000));

    // Exact match or slightly behind within 40 ms -> PresentNow.
    assert_eq!(
        clock.frame_action(Duration::from_millis(1_000)),
        FrameTiming::PresentNow
    );
    assert_eq!(
        clock.frame_action(Duration::from_millis(1_000) - LIP_SYNC_TOLERANCE),
        FrameTiming::PresentNow
    );

    // Late past 40 ms -> DropLate.
    assert_eq!(
        clock.frame_action(Duration::from_millis(950)),
        FrameTiming::DropLate(Duration::from_millis(50))
    );

    // Early by 100 ms at 2.0x speed -> Wait 50 ms wall clock.
    clock.set_rate(2.0);
    assert_eq!(
        clock.frame_action(Duration::from_millis(1_100)),
        FrameTiming::Wait(Duration::from_millis(50))
    );
}

#[test]
fn srt_and_webvtt_subtitles_parse_and_query_boundaries() {
    let srt = "\u{feff}1\n00:00:01,500 --> 00:00:04,000\n<i>Hello</i> &amp; welcome\nLine two\n\n2\n00:00:03,000 --> 00:00:05,250\nOverlap!\n";
    let srt_track = parse_srt(srt);
    assert_eq!(srt_track.cues().len(), 2);
    assert_eq!(srt_track.cues()[0].text, "Hello & welcome\nLine two");
    assert!(srt_track.cues_at(Duration::from_millis(1_499)).is_empty());
    assert_eq!(
        srt_track.cues_at(Duration::from_millis(1_500)).cues.len(),
        1
    );
    assert_eq!(
        srt_track.cues_at(Duration::from_millis(3_500)).cues.len(),
        2
    );
    assert_eq!(
        srt_track.cues_at(Duration::from_millis(4_000)).cues.len(),
        1
    );
    assert!(srt_track.cues_at(Duration::from_millis(5_250)).is_empty());

    let vtt = "WEBVTT\n\nNOTE ignore this comment\n\nSTYLE\n::cue { color: white; }\n\nintro\n00:02.000 --> 00:05.500 line:85% position:50% size:80% align:start\n<v Speaker>Hi &lt;there&gt;\n";
    let vtt_track = parse_subtitles(Some("text/vtt"), vtt);
    assert_eq!(vtt_track, parse_webvtt(vtt));
    assert_eq!(vtt_track.cues().len(), 1);
    let cue = &vtt_track.cues()[0];
    assert_eq!(cue.text, "Hi <there>");
    assert_eq!(cue.start_time, Duration::from_secs(2));
    assert_eq!(cue.end_time, Duration::from_millis(5_500));
    assert_eq!(cue.text_alignment, CueAlignment::Start);
    assert!((cue.line.expect("line") - 0.85).abs() < 1e-4);
    assert!((cue.position.expect("position") - 0.50).abs() < 1e-4);
    assert!((cue.size.expect("size") - 0.80).abs() < 1e-4);
}

#[test]
fn hls_manifests_parse_variants_renditions_and_segments() {
    let master_src = "#EXTM3U\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aac\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"audio/en.m3u8\"\n\
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English CC\",LANGUAGE=\"en\",URI=\"subs/en.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,AVERAGE-BANDWIDTH=750000,RESOLUTION=640x360,FRAME-RATE=30.0,CODECS=\"avc1.4d401e,mp4a.40.2\",AUDIO=\"aac\",SUBTITLES=\"subs\"\n\
360p/index.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=3500000,RESOLUTION=1920x1080,FRAME-RATE=60.0,CODECS=\"avc1.64002a\"\n\
1080p/index.m3u8\n";
    let master = parse_hls_master("https://cdn.example.com/hls/master.m3u8", master_src)
        .expect("master parses");
    assert_eq!(master.variants.len(), 2);
    assert_eq!(master.renditions.len(), 2);
    assert_eq!(master.renditions[0].kind, HlsRenditionType::Audio);
    assert_eq!(
        master.renditions[0].uri.as_deref(),
        Some("https://cdn.example.com/hls/audio/en.m3u8")
    );
    assert_eq!(
        master.variants[1].uri,
        "https://cdn.example.com/hls/1080p/index.m3u8"
    );
    assert_eq!(master.variants[1].resolution, Some((1920, 1080)));
    assert!(matches!(
        parse_hls_master("https://cdn.example.com/", "not a playlist"),
        Err(ManifestError::InvalidFormat(_))
    ));

    let media_src = "#EXTM3U\n\
#EXT-X-TARGETDURATION:6\n\
#EXT-X-MEDIA-SEQUENCE:42\n\
#EXT-X-MAP:URI=\"init.mp4\"\n\
#EXTINF:6.000,\n\
#EXT-X-BYTERANGE:1000@0\n\
seg0.m4s\n\
#EXT-X-DISCONTINUITY\n\
#EXTINF:4.500,\n\
#EXT-X-BYTERANGE:500\n\
seg1.m4s\n\
#EXT-X-ENDLIST\n";
    let media = parse_hls_media("https://cdn.example.com/hls/1080p/index.m3u8", media_src)
        .expect("media parses");
    assert!(!media.is_live);
    assert_eq!(media.target_duration, Duration::from_secs(6));
    assert_eq!(media.segments.len(), 2);
    assert_eq!(media.segments[0].sequence, 42);
    assert_eq!(media.segments[0].byte_range, Some((1000, 0)));
    assert_eq!(
        media.segments[0].init_uri.as_deref(),
        Some("https://cdn.example.com/hls/1080p/init.mp4")
    );
    assert_eq!(media.segments[1].sequence, 43);
    assert_eq!(media.segments[1].byte_range, Some((500, 1000)));
    assert!(media.segments[1].discontinuity);
    assert_eq!(media.total_duration(), Duration::from_millis(10_500));
}

#[test]
fn dash_manifest_and_abr_selection_work_end_to_end() {
    let dash_xml = r#"<MPD type="static" mediaPresentationDuration="PT1M30.5S">
      <Period>
        <BaseURL>dash/</BaseURL>
        <AdaptationSet contentType="video" mimeType="video/mp4">
          <SegmentTemplate timescale="1000" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/chunk_$Number$_$Time$.m4s" startNumber="1">
            <SegmentTimeline>
              <S t="0" d="4000" r="1"/>
            </SegmentTimeline>
          </SegmentTemplate>
          <Representation id="v720" bandwidth="2000000" width="1280" height="720" frameRate="60000/1001" codecs="avc1.640020"/>
        </AdaptationSet>
        <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en">
          <Representation id="a128" bandwidth="128000" codecs="mp4a.40.2">
            <SegmentList timescale="1000" duration="4000">
              <Initialization sourceURL="a128/init.mp4"/>
              <SegmentURL media="a128/seg1.m4s"/>
            </SegmentList>
          </Representation>
        </AdaptationSet>
      </Period>
    </MPD>"#;
    let dash = parse_dash_mpd("https://cdn.example.com/stream.mpd", dash_xml).expect("dash parses");
    assert!(!dash.is_live);
    assert_eq!(dash.duration, Some(Duration::from_millis(90_500)));
    assert_eq!(dash.adaptation_sets.len(), 2);
    let video_rep = &dash.adaptation_sets[0].representations[0];
    assert_eq!(
        video_rep.init_uri.as_deref(),
        Some("https://cdn.example.com/dash/v720/init.mp4")
    );
    assert_eq!(video_rep.segments.len(), 2);
    assert_eq!(
        video_rep.segments[1].uri,
        "https://cdn.example.com/dash/v720/chunk_2_4000.m4s"
    );
    assert_eq!(dash.adaptation_sets[1].track_type, TrackType::Audio);
    assert_eq!(dash.adaptation_sets[1].language.as_deref(), Some("en"));
    assert_eq!(dash.adaptation_sets[1].representations[0].segments.len(), 1);

    let variants = [
        AbrVariant {
            index: 0,
            bitrate: 800_000,
            width: Some(640),
            height: Some(360),
        },
        AbrVariant {
            index: 1,
            bitrate: 2_000_000,
            width: Some(1280),
            height: Some(720),
        },
        AbrVariant {
            index: 2,
            bitrate: 5_000_000,
            width: Some(1920),
            height: Some(1080),
        },
    ];
    assert_eq!(
        select_abr_variant(&variants, 3_000_000, &TrackSelectionParameters::default()),
        Some(1)
    );
    assert_eq!(
        select_abr_variant(
            &variants,
            10_000_000,
            &TrackSelectionParameters::default().with_max_video_size(1280, 720),
        ),
        Some(1)
    );
    assert_eq!(
        select_abr_variant(&variants, 100_000, &TrackSelectionParameters::default()),
        Some(0)
    );
}

#[test]
fn media_item_builders_carry_headers_clipping_subtitles_and_bytes() {
    let item = MediaItem::new("https://example.com/protected.m3u8")
        .with_mime_type("application/x-mpegURL")
        .with_header("Authorization", "Bearer tok")
        .with_headers([("X-Client", "cranpose")])
        .with_clipping(ClippingConfiguration {
            start_position: Duration::from_secs(5),
            end_position: Some(Duration::from_secs(35)),
        })
        .with_subtitle(
            SubtitleConfiguration::new("https://example.com/en.vtt")
                .with_mime_type("text/vtt")
                .with_language("en")
                .with_label("English"),
        );
    assert_eq!(item.mime_type.as_deref(), Some("application/x-mpegURL"));
    assert_eq!(item.headers.len(), 2);
    assert_eq!(
        item.clipping.expect("clipping").end_position,
        Some(Duration::from_secs(35))
    );
    assert_eq!(
        item.subtitle_configurations[0].language.as_deref(),
        Some("en")
    );

    let mem = MediaItem::from_bytes(vec![0_u8, 1, 2, 3], "video/mp4");
    assert_eq!(mem.bytes.as_deref(), Some(&[0_u8, 1, 2, 3][..]));
    assert_eq!(mem.mime_type.as_deref(), Some("video/mp4"));
}

#[test]
fn repeat_mode_cycles_in_standard_order() {
    assert_eq!(RepeatMode::Off.next(), RepeatMode::One);
    assert_eq!(RepeatMode::One.next(), RepeatMode::All);
    assert_eq!(RepeatMode::All.next(), RepeatMode::Off);
    assert_eq!(
        PositionDiscontinuityReason::AutoTransition,
        PositionDiscontinuityReason::AutoTransition
    );
}

static GLOBAL_MEDIA_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn audio_focus_ducks_pauses_and_resumes_primary_player_only() {
    let _guard = lock(&GLOBAL_MEDIA_LOCK);
    publish_audio_focus(AudioFocus::Gained);
    let (primary_factory, primary_calls) = FakeFactory::new();
    let (secondary_factory, secondary_calls) = FakeFactory::new();
    let primary = Player::builder()
        .with_audio_role(AudioRole::Primary)
        .with_backend_factory(Arc::clone(&primary_factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("primary player");
    let secondary = Player::builder()
        .with_audio_role(AudioRole::Secondary)
        .with_backend_factory(Arc::clone(&secondary_factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("secondary player");

    primary.set_media_item(MediaItem::new("https://example.com/p.mp4"));
    primary.prepare();
    primary.play();
    primary_factory
        .sink()
        .set_playback_state(PlayerState::Ready);
    secondary.set_media_item(MediaItem::new("https://example.com/s.mp4"));
    secondary.prepare();
    secondary.play();
    secondary_factory
        .sink()
        .set_playback_state(PlayerState::Ready);

    publish_audio_focus(AudioFocus::Ducked);
    assert_eq!(
        lock(&primary_calls).last(),
        Some(&BackendCall::Volume(DUCKED_GAIN))
    );
    assert_ne!(
        lock(&secondary_calls).last(),
        Some(&BackendCall::Volume(DUCKED_GAIN))
    );

    publish_audio_focus(AudioFocus::LostTransient);
    assert!(!primary.is_playing());
    assert!(secondary.is_playing());

    publish_audio_focus(AudioFocus::Gained);
    assert!(primary.is_playing());
    assert_eq!(lock(&primary_calls).last(), Some(&BackendCall::Play));

    // Manual pause during focus loss must not auto-resume on subsequent Gained.
    publish_audio_focus(AudioFocus::LostTransient);
    primary.pause();
    publish_audio_focus(AudioFocus::Gained);
    assert!(!primary.is_playing());

    primary.release();
    secondary.release();
}

#[test]
fn os_media_commands_control_active_primary_player() {
    let _guard = lock(&GLOBAL_MEDIA_LOCK);
    publish_audio_focus(AudioFocus::Gained);
    let (factory, _calls) = FakeFactory::new();
    let player = Player::builder()
        .with_audio_role(AudioRole::Primary)
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("primary player");
    player.set_media_items(
        vec![
            MediaItem::new("https://example.com/0.mp4"),
            MediaItem::new("https://example.com/1.mp4"),
        ],
        0,
        Duration::ZERO,
    );
    player.prepare();
    player.play();
    factory.sink().set_media_properties(MediaProperties {
        duration: Some(Duration::from_secs(30)),
        is_live: false,
        is_seekable: true,
    });
    factory.sink().set_playback_state(PlayerState::Ready);
    assert!(player.is_playing());

    publish_media_command(MediaCommand::Pause);
    assert!(!player.is_playing());
    publish_media_command(MediaCommand::TogglePlayPause);
    assert!(player.is_playing());
    publish_media_command(MediaCommand::SeekTo(Duration::from_secs(12)));
    assert!(player.current_position() >= Duration::from_secs(12));
    publish_media_command(MediaCommand::Next);
    assert_eq!(player.current_media_item_index(), 1);
    player.release();
}

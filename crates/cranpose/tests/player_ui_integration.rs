use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose::{
    Cue, CueAlignment, CueGroup, DisplayModeError, HostController, LifecycleState, MediaItem,
    MediaProperties, PlatformDirectories, PlaybackParameters, Player, PlayerBackendFactory,
    PlayerDisplayCapabilities, PlayerDisplayController, PlayerDisplayMode, PlayerLifecycleEffect,
    PlayerLifecyclePolicy, PlayerState, RepeatMode, TrackEntry, TrackFormat, TrackGroup, TrackType,
    Tracks, VideoSize, clear_host_controller, clear_platform_player_display_controller, composable,
    dispatch_lifecycle_state, player_display_mode, publish_player_display_mode, rememberCuesState,
    rememberFullscreenButtonState, rememberMuteButtonState, rememberNextButtonState,
    rememberPictureInPictureButtonState, rememberPlayPauseButtonState, rememberPlaybackSpeedState,
    rememberPlayerWith, rememberPresentationState, rememberPreviousButtonState,
    rememberProgressStateWithTickCount, rememberProgressStateWithTickInterval,
    rememberRepeatButtonState, rememberSeekBackButtonState, rememberSeekForwardButtonState,
    rememberShuffleButtonState, rememberTracksState, set_host_controller,
    set_platform_player_display_controller,
};
use cranpose_core::{Composition, MemoryApplier, location_key};

use crate::player_test_support::TestFactory;

fn serial_global() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn pump(composition: &mut Composition<MemoryApplier>, done: impl Fn() -> bool) -> bool {
    let runtime = composition.runtime_handle();
    for _ in 0..80 {
        runtime.drain_ui();
        composition.process_invalid_scopes().expect("recompose");
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

#[derive(Clone, Default)]
struct BoardProbe {
    recompositions: Rc<Cell<u32>>,
    show_play: Rc<Cell<bool>>,
    muted: Rc<Cell<bool>>,
    speed: Rc<Cell<f32>>,
    repeat: Rc<Cell<RepeatMode>>,
    shuffle: Rc<Cell<bool>>,
    next_enabled: Rc<Cell<bool>>,
    prev_enabled: Rc<Cell<bool>>,
    tracks_count: Rc<Cell<usize>>,
    cue_text: Rc<RefCell<String>>,
}

impl PartialEq for BoardProbe {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.recompositions, &other.recompositions)
    }
}

#[composable]
fn ControlBoard(player: Player, probe: BoardProbe) {
    probe.recompositions.set(probe.recompositions.get() + 1);
    let play_pause = rememberPlayPauseButtonState(&player);
    let mute = rememberMuteButtonState(&player);
    let speed = rememberPlaybackSpeedState(&player);
    let repeat = rememberRepeatButtonState(&player);
    let shuffle = rememberShuffleButtonState(&player);
    let _back = rememberSeekBackButtonState(&player);
    let _fwd = rememberSeekForwardButtonState(&player);
    let next = rememberNextButtonState(&player);
    let prev = rememberPreviousButtonState(&player);
    let tracks = rememberTracksState(&player);
    let cues = rememberCuesState(&player);

    probe.show_play.set(play_pause.show_play());
    probe.muted.set(mute.is_muted());
    probe.speed.set(speed.playback_speed());
    probe.repeat.set(repeat.repeat_mode());
    probe.shuffle.set(shuffle.shuffle_on());
    probe.next_enabled.set(next.is_enabled());
    probe.prev_enabled.set(prev.is_enabled());
    probe.tracks_count.set(tracks.tracks().groups.len());
    *probe.cue_text.borrow_mut() = cues
        .cues()
        .cues
        .first()
        .map_or_else(String::new, |c| c.text.clone());
}

#[test]
fn control_state_holders_react_to_events_and_stay_idle_when_paused() {
    let factory = TestFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player");
    let sink = factory.sink();
    player.set_media_items(
        vec![
            MediaItem::new("https://example.com/a.mp4"),
            MediaItem::new("https://example.com/b.mp4"),
        ],
        0,
        Duration::ZERO,
    );

    let mut composition = Composition::new(MemoryApplier::new());
    let probe = BoardProbe {
        show_play: Rc::new(Cell::new(true)),
        speed: Rc::new(Cell::new(1.0)),
        ..BoardProbe::default()
    };
    {
        let (p, pr) = (player.clone(), probe.clone());
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                ControlBoard(p.clone(), pr.clone());
            })
            .expect("render");
    }
    assert!(probe.show_play.get());
    assert!(probe.next_enabled.get());
    assert!(!probe.prev_enabled.get());

    // Drive state transitions via player & backend sink.
    player.prepare();
    sink.set_playback_state(PlayerState::Ready);
    player.play();
    player.set_muted(true);
    player.set_playback_parameters(PlaybackParameters::with_speed(1.5));
    player.set_repeat_mode(RepeatMode::All);
    player.set_shuffle_mode(true);
    sink.set_tracks(Tracks {
        groups: vec![TrackGroup {
            id: "a0".into(),
            track_type: TrackType::Audio,
            tracks: vec![TrackEntry {
                format: TrackFormat {
                    id: "en".into(),
                    language: Some("en".into()),
                    ..TrackFormat::default()
                },
                supported: true,
                selected: true,
            }],
            adaptive_supported: false,
        }],
    });
    sink.set_cues(CueGroup {
        cues: vec![Cue {
            text: "Caption".into(),
            line: None,
            position: None,
            size: None,
            text_alignment: CueAlignment::Center,
            start_time: Duration::ZERO,
            end_time: Duration::from_secs(2),
        }],
        presentation_time: Duration::ZERO,
    });

    assert!(pump(&mut composition, || {
        !probe.show_play.get()
            && probe.muted.get()
            && (probe.speed.get() - 1.5).abs() < 1e-4
            && probe.repeat.get() == RepeatMode::All
            && probe.shuffle.get()
            && probe.tracks_count.get() == 1
            && probe.cue_text.borrow().as_str() == "Caption"
    }));

    // Pause and verify zero recompositions occur while idle.
    player.pause();
    assert!(pump(&mut composition, || probe.show_play.get()));
    let paused_count = probe.recompositions.get();
    for _ in 0..10 {
        composition.runtime_handle().drain_ui();
        composition.process_invalid_scopes().expect("recompose");
    }
    assert_eq!(probe.recompositions.get(), paused_count);
}

#[composable]
fn ProgressReader(
    player: Player,
    recompositions: Rc<Cell<u32>>,
    position_out: Rc<Cell<Duration>>,
    fraction_out: Rc<Cell<Option<f32>>>,
) {
    recompositions.set(recompositions.get() + 1);
    let progress = rememberProgressStateWithTickInterval(&player, Duration::from_millis(25));
    let _by_count = rememberProgressStateWithTickCount(&player, 100);
    position_out.set(progress.current_position());
    fraction_out.set(progress.fraction());
}

#[test]
fn paused_progress_state_causes_zero_recompositions_and_updates_on_seek() {
    let factory = TestFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player");
    let sink = factory.sink();
    player.set_media_item(MediaItem::new("https://example.com/clip.mp4"));
    player.prepare();
    sink.set_media_properties(MediaProperties {
        duration: Some(Duration::from_secs(100)),
        is_live: false,
        is_seekable: true,
    });
    sink.set_playback_state(PlayerState::Ready);

    let mut composition = Composition::new(MemoryApplier::new());
    let recompositions = Rc::new(Cell::new(0_u32));
    let position = Rc::new(Cell::new(Duration::ZERO));
    let fraction = Rc::new(Cell::new(None));
    {
        let (p, r, pos, frac) = (
            player.clone(),
            Rc::clone(&recompositions),
            Rc::clone(&position),
            Rc::clone(&fraction),
        );
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                ProgressReader(p.clone(), Rc::clone(&r), Rc::clone(&pos), Rc::clone(&frac));
            })
            .expect("render");
    }

    // While paused, no tick timer runs and recompositions stay constant.
    assert!(pump(&mut composition, || true));
    let settled = recompositions.get();
    for _ in 0..10 {
        composition.runtime_handle().drain_ui();
        composition.process_invalid_scopes().expect("recompose");
    }
    assert_eq!(recompositions.get(), settled);

    // Seeking while paused updates immediately via PositionDiscontinuity event.
    player.seek_to(Duration::from_secs(25));
    assert!(pump(&mut composition, || {
        position.get() == Duration::from_secs(25)
    }));
    assert!((fraction.get().expect("fraction") - 0.25).abs() < 1e-4);
}

#[composable]
fn PresentationObserver(
    player: Player,
    cover_out: Rc<Cell<bool>>,
    aspect_out: Rc<Cell<Option<f32>>>,
    shutter_renders: Rc<Cell<u32>>,
) {
    let pres = rememberPresentationState(&player);
    cover_out.set(pres.cover_surface());
    aspect_out.set(pres.aspect_ratio());
    if pres.cover_surface() {
        shutter_renders.set(shutter_renders.get() + 1);
    }
}

#[test]
fn presentation_state_and_content_frame_manage_shutter_across_transitions() {
    let factory = TestFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player");
    let sink = factory.sink();
    player.set_media_items(
        vec![
            MediaItem::new("https://example.com/1.mp4"),
            MediaItem::new("https://example.com/2.mp4"),
        ],
        0,
        Duration::ZERO,
    );

    let mut composition = Composition::new(MemoryApplier::new());
    let cover = Rc::new(Cell::new(true));
    let aspect = Rc::new(Cell::new(None));
    let shutter_renders = Rc::new(Cell::new(0_u32));
    {
        let (p, c, a, sr) = (
            player.clone(),
            Rc::clone(&cover),
            Rc::clone(&aspect),
            Rc::clone(&shutter_renders),
        );
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                PresentationObserver(p.clone(), Rc::clone(&c), Rc::clone(&a), Rc::clone(&sr));
            })
            .expect("render");
    }
    assert!(cover.get());
    assert!(shutter_renders.get() >= 1);

    sink.set_video_size(VideoSize {
        width: 1920,
        height: 1080,
        unapplied_rotation_degrees: 90,
        pixel_width_height_ratio: 1.0,
    });
    sink.notify_rendered_first_frame();
    assert!(pump(&mut composition, || !cover.get() && aspect.get().is_some()));
    assert!((aspect.get().expect("aspect") - (1080.0 / 1920.0)).abs() < 1e-4);

    // Clip transition re-covers the surface until the new clip renders its first frame.
    player.seek_to_next();
    assert!(pump(&mut composition, || cover.get()));
}

#[test]
fn remember_player_releases_backend_when_leaving_composition() {
    let factory = TestFactory::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let f = Arc::clone(&factory);
    composition
        .render(key, move || {
            let _player = rememberPlayerWith(|b| {
                b.with_backend_factory(Arc::clone(&f) as Arc<dyn PlayerBackendFactory>)
            });
        })
        .expect("mount");
    assert!(!factory.released.load(Ordering::Acquire));

    composition.render(key, || {}).expect("unmount");
    assert!(factory.released.load(Ordering::Acquire));
}

struct SpyHost {
    keep_on_calls: AtomicUsize,
    last_keep_on: AtomicBool,
}

impl HostController for SpyHost {
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        None
    }
    fn set_keep_screen_on(&self, enabled: bool) {
        self.keep_on_calls.fetch_add(1, Ordering::AcqRel);
        self.last_keep_on.store(enabled, Ordering::Release);
    }
    fn background(&self) {}
    fn exit(&self) {}
}

#[test]
fn lifecycle_effect_pauses_resumes_and_toggles_keep_screen_on() {
    let _guard = serial_global();
    let spy = Arc::new(SpyHost {
        keep_on_calls: AtomicUsize::new(0),
        last_keep_on: AtomicBool::new(false),
    });
    set_host_controller(Arc::clone(&spy) as Arc<dyn HostController>);
    dispatch_lifecycle_state(LifecycleState::Resumed);

    let factory = TestFactory::new();
    let player = Player::builder()
        .with_backend_factory(Arc::clone(&factory) as Arc<dyn PlayerBackendFactory>)
        .build()
        .expect("player");
    let sink = factory.sink();
    player.set_media_item(MediaItem::new("https://example.com/v.mp4"));
    player.prepare();
    sink.set_playback_state(PlayerState::Ready);
    player.play();

    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    {
        let p = player.clone();
        composition
            .render(key, move || {
                PlayerLifecycleEffect(p.clone(), PlayerLifecyclePolicy::PauseInBackground);
            })
            .expect("render");
    }
    assert!(pump(&mut composition, || spy
        .last_keep_on
        .load(Ordering::Acquire)));

    // Backgrounding pauses playback and releases KeepScreenOn.
    dispatch_lifecycle_state(LifecycleState::Stopped);
    assert!(pump(&mut composition, || {
        !player.play_when_ready() && !spy.last_keep_on.load(Ordering::Acquire)
    }));

    // Returning to foreground resumes playback because lifecycle paused it.
    dispatch_lifecycle_state(LifecycleState::Resumed);
    assert!(pump(&mut composition, || player.play_when_ready()));

    // A user pause is NOT resumed after a background/foreground cycle.
    player.pause();
    assert!(pump(&mut composition, || !player.play_when_ready()));
    dispatch_lifecycle_state(LifecycleState::Stopped);
    assert!(pump(&mut composition, || true));
    dispatch_lifecycle_state(LifecycleState::Resumed);
    assert!(pump(&mut composition, || true));
    assert!(!player.play_when_ready());

    composition.render(key, || {}).expect("unmount");
    clear_host_controller();
}

struct FakeDisplayController {
    caps: PlayerDisplayCapabilities,
    requests: Mutex<Vec<PlayerDisplayMode>>,
}

impl PlayerDisplayController for FakeDisplayController {
    fn capabilities(&self) -> PlayerDisplayCapabilities {
        self.caps
    }
    fn request_mode(&self, mode: PlayerDisplayMode) -> Result<(), DisplayModeError> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(mode);
        Ok(())
    }
}

#[composable]
fn DisplayButtons(
    fs_out: Rc<Cell<bool>>,
    pip_out: Rc<Cell<bool>>,
    on_ready: impl Fn(cranpose::FullscreenButtonState, cranpose::PictureInPictureButtonState) + 'static,
) {
    let fs = rememberFullscreenButtonState();
    let pip = rememberPictureInPictureButtonState();
    fs_out.set(fs.is_fullscreen());
    pip_out.set(pip.is_in_picture_in_picture());
    on_ready(fs, pip);
}

#[test]
fn fullscreen_and_pip_hooks_drive_platform_display_controller() {
    let _guard = serial_global();
    clear_platform_player_display_controller();
    let controller = Arc::new(FakeDisplayController {
        caps: PlayerDisplayCapabilities {
            fullscreen: true,
            picture_in_picture: true,
        },
        requests: Mutex::new(Vec::new()),
    });
    set_platform_player_display_controller(
        Arc::clone(&controller) as Arc<dyn PlayerDisplayController>
    );

    let mut composition = Composition::new(MemoryApplier::new());
    let fs_active = Rc::new(Cell::new(false));
    let pip_active = Rc::new(Cell::new(false));
    let fs_holder = Rc::new(Cell::new(None));
    let pip_holder = Rc::new(Cell::new(None));
    {
        let (f, p, fh, ph) = (
            Rc::clone(&fs_active),
            Rc::clone(&pip_active),
            Rc::clone(&fs_holder),
            Rc::clone(&pip_holder),
        );
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                let (fh, ph) = (Rc::clone(&fh), Rc::clone(&ph));
                DisplayButtons(Rc::clone(&f), Rc::clone(&p), move |fs, pip| {
                    fh.set(Some(fs));
                    ph.set(Some(pip));
                });
            })
            .expect("render");
    }

    fs_holder.get().expect("fs state").on_click();
    assert!(pump(&mut composition, || fs_active.get()));
    assert_eq!(player_display_mode(), PlayerDisplayMode::Fullscreen);

    pip_holder.get().expect("pip state").on_click();
    assert!(pump(&mut composition, || pip_active.get() && !fs_active.get()));

    // Host-initiated exit (e.g. OS PiP close button) updates Compose state.
    publish_player_display_mode(PlayerDisplayMode::Normal);
    assert!(pump(&mut composition, || !pip_active.get()));
    clear_platform_player_display_controller();
}

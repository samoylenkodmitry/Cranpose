#![allow(unsafe_code)]

use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use block2::RcBlock;
use cranpose_services::{
    AudioFocus, MediaCapabilities, MediaCommand, MediaError, MediaItem, MediaMetadata, MediaPlayer,
    PlaybackProgress, PlaybackState, publish_audio_focus, publish_media_command,
    publish_playback_progress, publish_playback_state, set_platform_media_player,
};
use dispatch2::DispatchQueue;
use objc2::{
    AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject},
    sel,
};
use objc2_av_foundation::{
    AVPlayer, AVPlayerItem, AVPlayerItemDidPlayToEndTimeNotification,
    AVPlayerItemFailedToPlayToEndTimeNotification, AVPlayerItemStatus, AVURLAsset,
};
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryPlayback, AVAudioSessionInterruptionNotification,
    AVAudioSessionInterruptionOptionKey, AVAudioSessionInterruptionOptions,
    AVAudioSessionInterruptionType, AVAudioSessionInterruptionTypeKey,
};
use objc2_core_media::CMTime;
use objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol, NSString, NSURL,
};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

const SEEK_TIMESCALE: i32 = 1_000_000;

struct PlayerHolder {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    observer: Retained<ItemObserver>,
    stated_duration: Option<Duration>,
}

unsafe impl Send for PlayerHolder {}

fn player_slot() -> &'static Mutex<Option<PlayerHolder>> {
    static SLOT: OnceLock<Mutex<Option<PlayerHolder>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static VOLUME: Mutex<f32> = Mutex::new(1.0);
static SPEED: Mutex<f32> = Mutex::new(1.0);
static LOOPING: AtomicBool = AtomicBool::new(false);

pub(crate) fn register() {
    configure_audio_session();
    install_interruption_observer();
    install_remote_commands();
    set_platform_media_player(Arc::new(IosMediaPlayer));
}

struct IosMediaPlayer;

fn on_main<R: Send>(action: impl FnOnce(MainThreadMarker) -> R + Send) -> R {
    if let Some(mtm) = MainThreadMarker::new() {
        return action(mtm);
    }
    let mut landed = None;
    DispatchQueue::main().exec_sync(|| {
        let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
        landed = Some(action(mtm));
    });
    landed.expect("the main queue ran the action")
}

fn volume() -> f32 {
    *VOLUME.lock().unwrap_or_else(|error| error.into_inner())
}

fn speed() -> f32 {
    *SPEED.lock().unwrap_or_else(|error| error.into_inner())
}

fn configure_audio_session() {
    unsafe {
        let session = AVAudioSession::sharedInstance();
        if let Some(category) = AVAudioSessionCategoryPlayback
            && let Err(error) = session.setCategory_error(category)
        {
            log::warn!("cranpose: iOS audio session category refused: {error:?}");
        }
    }
}

fn activate_audio_session(active: bool) {
    unsafe {
        if let Err(error) = AVAudioSession::sharedInstance().setActive_error(active) {
            log::warn!("cranpose: iOS audio session activation refused: {error:?}");
        }
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeMediaObserver"]
    #[ivars = ()]
    struct SessionObserver;

    unsafe impl NSObjectProtocol for SessionObserver {}

    impl SessionObserver {
        #[unsafe(method(cranposeAudioSessionInterrupted:))]
        fn interrupted(&self, notification: &NSNotification) {
            let Some(info) = notification.userInfo() else {
                return;
            };
            let Some(kind) = number_for_key(&info, unsafe { AVAudioSessionInterruptionTypeKey })
            else {
                return;
            };
            let kind = AVAudioSessionInterruptionType(kind.unsignedLongValue() as usize);
            if kind == AVAudioSessionInterruptionType::Began {
                publish_audio_focus(AudioFocus::LostTransient);
                return;
            }
            let resume = number_for_key(&info, unsafe { AVAudioSessionInterruptionOptionKey })
                .map(|options| {
                    AVAudioSessionInterruptionOptions(options.unsignedLongValue() as usize)
                        .contains(AVAudioSessionInterruptionOptions::ShouldResume)
                })
                .unwrap_or(false);
            if resume {
                activate_audio_session(true);
                publish_audio_focus(AudioFocus::Gained);
            } else {
                publish_audio_focus(AudioFocus::Lost);
            }
        }
    }
);

impl SessionObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn number_for_key(
    info: &NSDictionary,
    key: Option<&'static NSString>,
) -> Option<Retained<NSNumber>> {
    let value = info.objectForKey(key?)?;
    value.downcast::<NSNumber>().ok()
}

fn install_interruption_observer() {
    static OBSERVER: OnceLock<SendRetained<SessionObserver>> = OnceLock::new();
    let observer = OBSERVER.get_or_init(|| SendRetained(SessionObserver::new()));
    let Some(name) = (unsafe { AVAudioSessionInterruptionNotification }) else {
        return;
    };
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer.0,
            sel!(cranposeAudioSessionInterrupted:),
            Some(name),
            None,
        );
    }
}

struct SendRetained<T: ?Sized>(Retained<T>);

unsafe impl<T: ?Sized> Send for SendRetained<T> {}
unsafe impl<T: ?Sized> Sync for SendRetained<T> {}

fn install_remote_commands() {
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        for (command, action) in [
            (center.playCommand(), MediaCommand::Play),
            (center.pauseCommand(), MediaCommand::Pause),
            (
                center.togglePlayPauseCommand(),
                MediaCommand::TogglePlayPause,
            ),
            (center.stopCommand(), MediaCommand::Stop),
            (center.nextTrackCommand(), MediaCommand::Next),
            (center.previousTrackCommand(), MediaCommand::Previous),
        ] {
            let handler = RcBlock::new(
                move |_event: std::ptr::NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                    publish_media_command(action);
                    MPRemoteCommandHandlerStatus::Success
                },
            );
            command.setEnabled(true);
            let _ = command.addTargetWithHandler(&handler);
        }

        let seek = center.changePlaybackPositionCommand();
        let handler = RcBlock::new(
            move |event: std::ptr::NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let event = event.as_ref();
                let Some(event) = event.downcast_ref::<MPChangePlaybackPositionCommandEvent>()
                else {
                    return MPRemoteCommandHandlerStatus::CommandFailed;
                };
                let seconds = event.positionTime();
                if !seconds.is_finite() || seconds < 0.0 {
                    return MPRemoteCommandHandlerStatus::CommandFailed;
                }
                publish_media_command(MediaCommand::SeekTo(Duration::from_secs_f64(seconds)));
                MPRemoteCommandHandlerStatus::Success
            },
        );
        seek.setEnabled(true);
        let _ = seek.addTargetWithHandler(&handler);
    }
}

fn publish_now_playing(metadata: &MediaMetadata, position: Duration, rate: f32) {
    unsafe {
        let title = NSString::from_str(&metadata.title);
        let artist = NSString::from_str(&metadata.artist);
        let album = NSString::from_str(&metadata.album);
        let duration = NSNumber::new_f64(
            metadata
                .duration
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0),
        );
        let elapsed = NSNumber::new_f64(position.as_secs_f64());
        let rate = NSNumber::new_f32(rate);

        let keys: [&NSString; 6] = [
            MPMediaItemPropertyTitle,
            MPMediaItemPropertyArtist,
            MPMediaItemPropertyAlbumTitle,
            MPMediaItemPropertyPlaybackDuration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            MPNowPlayingInfoPropertyPlaybackRate,
        ];
        let values: [&AnyObject; 6] = [
            title.as_ref(),
            artist.as_ref(),
            album.as_ref(),
            duration.as_ref(),
            elapsed.as_ref(),
            rate.as_ref(),
        ];
        let info = NSDictionary::from_slices(&keys, &values);
        MPNowPlayingInfoCenter::defaultCenter().setNowPlayingInfo(Some(&info));
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeMediaItemObserver"]
    #[ivars = ()]
    struct ItemObserver;

    unsafe impl NSObjectProtocol for ItemObserver {}

    impl ItemObserver {
        #[unsafe(method(cranposeItemDidPlayToEnd:))]
        fn did_play_to_end(&self, _notification: &NSNotification) {
            end_of_item();
        }

        #[unsafe(method(cranposeItemFailedToPlayToEnd:))]
        fn failed_to_play_to_end(&self, _notification: &NSNotification) {
            fail_open_item("playback stopped part way through the item".to_string());
        }
    }
);

impl ItemObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn end_of_item() {
    if LOOPING.load(Ordering::Acquire) && restart_open_item() {
        return;
    }
    if let Some(duration) = duration_of_open_item() {
        publish_playback_progress(PlaybackProgress::new(duration, duration));
    }
    GENERATION.fetch_add(1, Ordering::AcqRel);
    publish_playback_state(PlaybackState::Ended);
}

fn fail_open_item(reason: String) {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    publish_playback_state(PlaybackState::Failed(MediaError::Failed(reason)));
}

fn restart_open_item() -> bool {
    with_holder(|holder| unsafe {
        holder.player.seekToTime(cm_time(Duration::ZERO));
        start_playing(&holder.player);
    })
    .is_some()
}

fn duration_of_open_item() -> Option<Duration> {
    with_holder(duration_of).flatten()
}

fn duration_of(holder: &PlayerHolder) -> Option<Duration> {
    positive_seconds(unsafe { holder.item.duration() })
        .map(Duration::from_secs_f64)
        .or(holder.stated_duration)
}

fn positive_seconds(time: CMTime) -> Option<f64> {
    let seconds = unsafe { time.seconds() };
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

fn cm_time(position: Duration) -> CMTime {
    unsafe { CMTime::with_seconds(position.as_secs_f64(), SEEK_TIMESCALE) }
}

fn start_playing(player: &AVPlayer) {
    let speed = speed();
    unsafe {
        if speed == 1.0 {
            player.play();
        } else {
            player.setRate(speed);
        }
    }
}

fn url_for(uri: &str) -> Option<Retained<NSURL>> {
    if let Some(path) = cranpose_services::media::path_from_uri(uri) {
        let path = path.to_str()?;
        return Some(NSURL::fileURLWithPath(&NSString::from_str(path)));
    }
    if !is_streamable_uri(uri) {
        return None;
    }
    NSURL::URLWithString(&NSString::from_str(uri))
}

fn is_streamable_uri(uri: &str) -> bool {
    matches!(
        uri.split_once("://"),
        Some((scheme, rest))
            if !rest.is_empty()
                && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    )
}

fn progress_at(position: Duration, duration: Option<Duration>) -> PlaybackProgress {
    PlaybackProgress {
        position,
        duration,
        buffered: duration.unwrap_or(position),
    }
}

fn start_progress_thread() {
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let spawned = std::thread::Builder::new()
        .name("cranpose-media-progress".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(PROGRESS_INTERVAL);
                if GENERATION.load(Ordering::Acquire) != generation {
                    return;
                }
                match observe() {
                    Some(Observed::Playing(position, duration)) => {
                        publish_playback_progress(progress_at(position, duration));
                    }
                    Some(Observed::Failed(reason)) => {
                        fail_open_item(reason);
                        return;
                    }
                    None => return,
                }
            }
        });
    if let Err(error) = spawned {
        log::warn!("cranpose: no iOS media progress thread: {error}");
    }
}

enum Observed {
    Playing(Duration, Option<Duration>),
    Failed(String),
}

fn observe() -> Option<Observed> {
    with_holder(|holder| {
        if let Some(reason) = item_failure(holder) {
            return Observed::Failed(reason);
        }
        Observed::Playing(position_of(holder), duration_of(holder))
    })
}

fn item_failure(holder: &PlayerHolder) -> Option<String> {
    if unsafe { holder.item.status() } != AVPlayerItemStatus::Failed {
        return None;
    }
    Some(
        unsafe { holder.item.error() }
            .map(|error| error.localizedDescription().to_string())
            .unwrap_or_else(|| "the item could not be played".to_string()),
    )
}

fn position_of(holder: &PlayerHolder) -> Duration {
    let seconds = unsafe { holder.player.currentTime().seconds() };
    if seconds.is_finite() && seconds > 0.0 {
        Duration::from_secs_f64(seconds)
    } else {
        Duration::ZERO
    }
}

fn with_holder<R: Send>(action: impl FnOnce(&PlayerHolder) -> R + Send) -> Option<R> {
    on_main(move |_mtm| {
        let slot = player_slot()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        slot.as_ref().map(action)
    })
}

fn open_item(
    uri: &str,
    stated: Option<Duration>,
    mtm: MainThreadMarker,
) -> Result<Option<Duration>, MediaError> {
    let url = url_for(uri).ok_or_else(|| MediaError::UnsupportedSource(uri.to_owned()))?;
    let observer = ItemObserver::new();
    let holder = unsafe {
        let item = AVPlayerItem::initWithURL(AVPlayerItem::alloc(mtm), &url);
        let player = AVPlayer::initWithPlayerItem(AVPlayer::alloc(mtm), Some(&item));
        player.setVolume(volume());
        let center = NSNotificationCenter::defaultCenter();
        let scope: &AnyObject = item.as_ref();
        center.addObserver_selector_name_object(
            &observer,
            sel!(cranposeItemDidPlayToEnd:),
            Some(AVPlayerItemDidPlayToEndTimeNotification),
            Some(scope),
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(cranposeItemFailedToPlayToEnd:),
            Some(AVPlayerItemFailedToPlayToEndTimeNotification),
            Some(scope),
        );
        PlayerHolder {
            player,
            item,
            observer,
            stated_duration: stated,
        }
    };
    let duration = duration_of(&holder);
    *player_slot()
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(holder);
    Ok(duration)
}

fn close_item() {
    on_main(|_mtm| {
        let taken = player_slot()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(holder) = taken {
            unsafe {
                holder.player.pause();
                holder.player.replaceCurrentItemWithPlayerItem(None);
                NSNotificationCenter::defaultCenter().removeObserver(&holder.observer);
            }
        }
        unsafe {
            MPNowPlayingInfoCenter::defaultCenter().setNowPlayingInfo(None);
        }
    });
}

const IOS_AUDIO_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "aif", "aiff", "caf", "flac", "m4a", "m4b", "m4v", "mov", "mp3", "mp4", "wav",
];

impl MediaPlayer for IosMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            seeking: true,
            speed: true,
            looping: true,
            analysis: false,
            session: true,
            probing: true,
            equalizer: false,
        }
    }

    fn probe_duration(&self, item: &MediaItem) -> Option<Duration> {
        let url = url_for(&item.uri)?;
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        positive_seconds(unsafe { asset.duration() }).map(Duration::from_secs_f64)
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        IOS_AUDIO_EXTENSIONS.to_vec()
    }
    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.stop();
        let uri = item.uri.clone();
        let stated = item.metadata.duration;
        let duration = on_main(move |mtm| open_item(&uri, stated, mtm))?;
        publish_playback_progress(progress_at(Duration::ZERO, duration));
        publish_playback_state(PlaybackState::Paused);
        publish_now_playing(&item.metadata, Duration::ZERO, 0.0);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        activate_audio_session(true);
        let failure = with_holder(|holder| {
            let failure = item_failure(holder);
            if failure.is_none() {
                start_playing(&holder.player);
            }
            failure
        })
        .ok_or(MediaError::NothingLoaded)?;
        if let Some(reason) = failure {
            return Err(MediaError::Failed(reason));
        }
        start_progress_thread();
        publish_playback_state(PlaybackState::Playing);
        Ok(())
    }

    fn pause(&self) {
        with_holder(|holder| unsafe { holder.player.pause() });
        GENERATION.fetch_add(1, Ordering::AcqRel);
        publish_playback_state(PlaybackState::Paused);
    }

    fn stop(&self) {
        GENERATION.fetch_add(1, Ordering::AcqRel);
        close_item();
        activate_audio_session(false);
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        let duration = with_holder(|holder| {
            unsafe { holder.player.seekToTime(cm_time(position)) };
            duration_of(holder)
        })
        .ok_or(MediaError::NothingLoaded)?;
        publish_playback_progress(progress_at(position, duration));
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        *VOLUME.lock().unwrap_or_else(|error| error.into_inner()) = volume;
        with_holder(|holder| unsafe { holder.player.setVolume(volume) });
    }

    fn set_speed(&self, speed: f32) -> bool {
        let speed = speed.clamp(0.25, 4.0);
        *SPEED.lock().unwrap_or_else(|error| error.into_inner()) = speed;
        with_holder(|holder| unsafe {
            if holder.player.rate() != 0.0 {
                holder.player.setRate(speed);
            }
        });
        true
    }

    fn set_looping(&self, looping: bool) {
        LOOPING.store(looping, Ordering::Release);
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        let observed = with_holder(|holder| {
            let rate = unsafe { holder.player.rate() };
            (position_of(holder), rate, duration_of(holder))
        });
        let (position, rate, duration) = observed.unwrap_or((Duration::ZERO, 0.0, None));
        let mut metadata = metadata.clone();
        if metadata.duration.is_none() {
            metadata.duration = duration;
        }
        publish_now_playing(&metadata, position, rate);
    }
}

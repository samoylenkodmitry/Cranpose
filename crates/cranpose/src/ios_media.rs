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
use objc2::{
    AllocAnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
    sel,
};
use objc2_avf_audio::{
    AVAudioPlayer, AVAudioPlayerDelegate, AVAudioSession, AVAudioSessionCategoryPlayback,
    AVAudioSessionInterruptionNotification, AVAudioSessionInterruptionOptionKey,
    AVAudioSessionInterruptionOptions, AVAudioSessionInterruptionType,
    AVAudioSessionInterruptionTypeKey,
};
use objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol,
    NSString, NSURL,
};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle, MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime, MPNowPlayingInfoPropertyPlaybackRate,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

struct PlayerHolder {
    player: Retained<AVAudioPlayer>,
    _delegate: Retained<EndDelegate>,
    duration: Option<Duration>,
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
    #[name = "CranposeMediaEndDelegate"]
    #[ivars = ()]
    struct EndDelegate;

    unsafe impl NSObjectProtocol for EndDelegate {}

    unsafe impl AVAudioPlayerDelegate for EndDelegate {
        #[unsafe(method(audioPlayerDidFinishPlaying:successfully:))]
        unsafe fn did_finish(&self, _player: &AVAudioPlayer, successfully: bool) {
            if successfully {
                publish_end_of_item();
            } else {
                publish_playback_state(PlaybackState::Failed(MediaError::Failed(
                    "playback stopped part way through the item".to_string(),
                )));
            }
        }
    }
);

impl EndDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn publish_end_of_item() {
    if let Some(duration) = duration_of_open_item() {
        publish_playback_progress(PlaybackProgress::new(duration, duration));
    }
    GENERATION.fetch_add(1, Ordering::AcqRel);
    publish_playback_state(PlaybackState::Ended);
}

fn duration_of_open_item() -> Option<Duration> {
    player_slot()
        .lock()
        .ok()
        .and_then(|holder| holder.as_ref().and_then(|holder| holder.duration))
}

fn url_for(uri: &str) -> Option<Retained<NSURL>> {
    let path = cranpose_services::media::path_from_uri(uri)?;
    let path = path.to_str()?;
    Some(NSURL::fileURLWithPath(&NSString::from_str(path)))
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
                let Some((position, duration)) = observe_position() else {
                    return;
                };
                publish_playback_progress(progress_at(position, duration));
            }
        });
    if let Err(error) = spawned {
        log::warn!("cranpose: no iOS media progress thread: {error}");
    }
}

fn observe_position() -> Option<(Duration, Option<Duration>)> {
    let holder = player_slot().lock().ok()?;
    let holder = holder.as_ref()?;
    Some((
        Duration::from_secs_f64(unsafe { holder.player.currentTime() }.max(0.0)),
        holder.duration,
    ))
}

fn with_player<R>(action: impl FnOnce(&AVAudioPlayer) -> R) -> Option<R> {
    let holder = player_slot().lock().ok()?;
    let holder = holder.as_ref()?;
    Some(action(&holder.player))
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
        let player =
            unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
                .ok()?;
        let duration = unsafe { player.duration() };
        (duration.is_finite() && duration > 0.0).then(|| Duration::from_secs_f64(duration))
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        IOS_AUDIO_EXTENSIONS.to_vec()
    }
    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.stop();
        let url =
            url_for(&item.uri).ok_or_else(|| MediaError::UnsupportedSource(item.uri.clone()))?;
        let player =
            unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) }
                .map_err(|error| MediaError::Failed(format!("{error:?}")))?;
        let delegate = EndDelegate::new();
        let duration = unsafe { player.duration() };
        let duration = (duration.is_finite() && duration > 0.0)
            .then(|| Duration::from_secs_f64(duration))
            .or(item.metadata.duration);
        unsafe {
            player.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            player.setEnableRate(true);
            player.setVolume(*VOLUME.lock().unwrap_or_else(|error| error.into_inner()));
            player.setRate(*SPEED.lock().unwrap_or_else(|error| error.into_inner()));
            player.setNumberOfLoops(if LOOPING.load(Ordering::Acquire) {
                -1
            } else {
                0
            });
            player.prepareToPlay();
        }
        if let Ok(mut slot) = player_slot().lock() {
            *slot = Some(PlayerHolder {
                player,
                _delegate: delegate,
                duration,
            });
        }
        publish_playback_progress(progress_at(Duration::ZERO, duration));
        publish_playback_state(PlaybackState::Paused);
        publish_now_playing(&item.metadata, Duration::ZERO, 0.0);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        let started =
            with_player(|player| unsafe { player.play() }).ok_or(MediaError::NothingLoaded)?;
        if !started {
            return Err(MediaError::Failed(
                "the audio session refused to start the item".to_string(),
            ));
        }
        activate_audio_session(true);
        start_progress_thread();
        publish_playback_state(PlaybackState::Playing);
        Ok(())
    }

    fn pause(&self) {
        with_player(|player| unsafe { player.pause() });
        GENERATION.fetch_add(1, Ordering::AcqRel);
        publish_playback_state(PlaybackState::Paused);
    }

    fn stop(&self) {
        GENERATION.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut slot) = player_slot().lock()
            && let Some(holder) = slot.take()
        {
            unsafe {
                holder.player.stop();
                holder.player.setDelegate(None);
            }
        }
        unsafe {
            MPNowPlayingInfoCenter::defaultCenter().setNowPlayingInfo(None);
        }
        activate_audio_session(false);
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        let duration = duration_of_open_item();
        with_player(|player| unsafe { player.setCurrentTime(position.as_secs_f64()) })
            .ok_or(MediaError::NothingLoaded)?;
        publish_playback_progress(progress_at(position, duration));
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        *VOLUME.lock().unwrap_or_else(|error| error.into_inner()) = volume;
        with_player(|player| unsafe { player.setVolume(volume) });
    }

    fn set_speed(&self, speed: f32) -> bool {
        let speed = speed.clamp(0.25, 4.0);
        *SPEED.lock().unwrap_or_else(|error| error.into_inner()) = speed;
        with_player(|player| unsafe { player.setRate(speed) });
        true
    }

    fn set_looping(&self, looping: bool) {
        LOOPING.store(looping, Ordering::Release);
        with_player(|player| unsafe {
            player.setNumberOfLoops(if looping { -1 } else { 0 });
        });
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        let position =
            with_player(|player| Duration::from_secs_f64(unsafe { player.currentTime() }.max(0.0)))
                .unwrap_or_default();
        let rate = with_player(|player| {
            if unsafe { player.isPlaying() } {
                unsafe { player.rate() }
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);
        let mut metadata = metadata.clone();
        if metadata.duration.is_none() {
            metadata.duration = duration_of_open_item();
        }
        publish_now_playing(&metadata, position, rate);
    }
}

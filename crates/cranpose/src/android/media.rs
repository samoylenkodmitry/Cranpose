use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{
    EqualizerBand, EqualizerSettings, MediaCapabilities, MediaError, MediaItem, MediaMetadata,
    MediaPlayer, MediaSourceHandle, MediaSourceOpener, playback_progress,
    set_platform_media_player, set_platform_media_source_opener,
};

use super::media_session::{self, SESSION_PAUSED, SESSION_PLAYING, SESSION_STOPPED};

pub(crate) fn register(app: android_activity::AndroidApp) {
    media_session::set_activity(app);
    set_platform_media_source_opener(Arc::new(DocumentOpener));
    set_platform_media_player(Arc::new(AndroidMediaPlayer {
        player: SoftwareMediaPlayer::new(),
        speed: AtomicU32::new(1.0f32.to_bits()),
    }));
}

struct DocumentOpener;

impl MediaSourceOpener for DocumentOpener {
    fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle> {
        Ok(MediaSourceHandle::new(
            super::file_picker::open_content_uri(uri)?,
            super::file_picker::content_uri_length(uri),
        ))
    }
}

struct AndroidMediaPlayer {
    player: SoftwareMediaPlayer,
    speed: AtomicU32,
}

impl AndroidMediaPlayer {
    fn publish_session(&self, state: i32) {
        let progress = playback_progress();
        media_session::update_session(
            state,
            progress.position,
            progress.duration,
            f32::from_bits(self.speed.load(Ordering::Relaxed)),
        );
    }
}

impl MediaPlayer for AndroidMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            session: true,
            ..self.player.capabilities()
        }
    }

    fn probe_duration(&self, item: &MediaItem) -> Option<Duration> {
        self.player.probe_duration(item)
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        self.player.audio_extensions()
    }

    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.player.prepare(item)?;
        self.publish_session(SESSION_PAUSED);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        if !media_session::request_focus() {
            return Err(MediaError::Failed(
                "another app holds the audio output".to_owned(),
            ));
        }
        self.player.play()?;
        self.publish_session(SESSION_PLAYING);
        Ok(())
    }

    fn pause(&self) {
        self.player.pause();
        self.publish_session(SESSION_PAUSED);
    }

    fn stop(&self) {
        self.player.stop();
        self.publish_session(SESSION_STOPPED);
        media_session::abandon_focus();
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        self.player.seek_to(position)?;
        self.publish_session(if cranpose_services::playback_state().is_playing() {
            SESSION_PLAYING
        } else {
            SESSION_PAUSED
        });
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    fn set_balance(&self, balance: f32) -> bool {
        self.player.set_balance(balance)
    }

    fn set_speed(&self, speed: f32) -> bool {
        if !self.player.set_speed(speed) {
            return false;
        }
        self.speed.store(speed.to_bits(), Ordering::Relaxed);
        true
    }

    fn set_looping(&self, looping: bool) {
        self.player.set_looping(looping);
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        self.player.equalizer_bands()
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        self.player.set_equalizer(settings);
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        self.player.set_analysis_enabled(enabled)
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        media_session::set_metadata(metadata);
    }
}

//! The in-process media player's transport.
//!
//! One item at a time, one output device, opened when an item is and released
//! when playback stops. [`Sink`] owns the device and the decode thread; this
//! owns the state an application asks about. A progress thread runs only while
//! something is open: it publishes the position, drains the analysis tap, and
//! notices the end of the item, which the sink reports by having pushed
//! everything the source had and having had all of it taken.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_services::{
    EqualizerBand, EqualizerSettings, MediaCapabilities, MediaError, MediaItem, MediaPlayer,
    PlaybackProgress, PlaybackState, publish_playback_progress, publish_playback_state,
};
use parking_lot::Mutex;

use crate::{
    analysis::AnalysisTap, decode::Decoder, equalizer::EqualizerTap, sink::Sink,
    source::SampleSource,
};

/// How often the progress thread wakes while analysis samples are wanted.
const ANALYSIS_TICK: Duration = Duration::from_millis(8);
/// How often it wakes otherwise. A position that moves four times a second is
/// what a seek bar and a time label need; waking more often than that would
/// recompose the screen for nothing.
const IDLE_TICK: Duration = Duration::from_millis(100);
/// How often the position is published, whichever rate the thread wakes at.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// The open item and the device playing it.
struct Active {
    /// Dropping it closes the output stream and joins the decode thread.
    sink: Sink,
    duration: Option<Duration>,
}

struct Shared {
    active: Mutex<Option<Active>>,
    /// The last item opened, so the transport can reopen it after it ended.
    item: Mutex<Option<MediaItem>>,
    analysis: Arc<AnalysisTap>,
    equalizer: Arc<EqualizerTap>,
    looping: AtomicBool,
    volume: Mutex<f32>,
    speed: Mutex<f32>,
    /// Bumped whenever a session starts or ends, which is how the progress
    /// thread of a session that is over learns to exit.
    generation: AtomicU64,
}

/// Plays media through the platform output device, decoding in process.
///
/// Installed by [`install`](crate::install); applications drive it through
/// [`cranpose_services::media`] rather than holding it.
pub struct SoftwareMediaPlayer {
    shared: Arc<Shared>,
}

impl SoftwareMediaPlayer {
    /// Creates a player. No device is opened until an item is.
    pub fn new() -> SoftwareMediaPlayer {
        SoftwareMediaPlayer {
            shared: Arc::new(Shared {
                active: Mutex::new(None),
                item: Mutex::new(None),
                analysis: AnalysisTap::new(),
                equalizer: EqualizerTap::new(),
                looping: AtomicBool::new(false),
                volume: Mutex::new(1.0),
                speed: Mutex::new(1.0),
                generation: AtomicU64::new(0),
            }),
        }
    }
}

impl Default for SoftwareMediaPlayer {
    fn default() -> SoftwareMediaPlayer {
        SoftwareMediaPlayer::new()
    }
}

impl Shared {
    /// Opens `item` on a fresh device, paused at its start.
    fn open(self: &Arc<Self>, item: &MediaItem) -> Result<Option<Duration>, MediaError> {
        let (decoder, spool) = Decoder::open(&item.uri)?;
        let duration = decoder.total_duration().or(item.metadata.duration);
        // The equalizer runs first and the tap reads its output, so a
        // visualiser draws the signal that reaches the device rather than the
        // one that reached the equalizer.
        let source: Box<dyn SampleSource> =
            Box::new(self.analysis.wrap(self.equalizer.wrap(decoder)));

        let sink = Sink::open(source, spool, *self.volume.lock(), *self.speed.lock())?;

        *self.active.lock() = Some(Active { sink, duration });
        self.start_progress_thread();
        Ok(duration)
    }

    /// Ends the current session: the progress thread exits, the device closes.
    fn close(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.active.lock().take();
        self.analysis.drain();
    }

    fn start_progress_thread(self: &Arc<Self>) {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("cranpose-media-progress".to_string())
            .spawn(move || shared.run_progress(generation));
        if let Err(error) = spawned {
            log::warn!("cranpose-media: no progress thread: {error}");
        }
    }

    fn run_progress(self: Arc<Self>, generation: u64) {
        let mut published = Instant::now() - PROGRESS_INTERVAL;
        loop {
            let tick = if self.analysis.is_enabled() {
                ANALYSIS_TICK
            } else {
                IDLE_TICK
            };
            std::thread::sleep(tick);
            if self.generation.load(Ordering::Acquire) != generation {
                return;
            }
            self.analysis.drain();

            let Some(observed) = self.observe() else {
                return;
            };
            if observed.ended {
                self.finish(generation);
                return;
            }
            if published.elapsed() >= PROGRESS_INTERVAL {
                published = Instant::now();
                publish_playback_progress(progress_at(observed.position, observed.duration));
            }
        }
    }

    /// Reads the session without holding its lock across anything that
    /// publishes — an observer reacting to the end of an item calls straight
    /// back into the transport, and would meet its own lock on the way in.
    fn observe(&self) -> Option<Observation> {
        let active = self.active.lock();
        let active = active.as_ref()?;
        Some(Observation {
            position: active.sink.position(),
            duration: active.duration,
            ended: active.sink.ended(),
        })
    }

    /// Handles an item reaching its end: round the position off at the end,
    /// then either start it again or say that it finished.
    fn finish(self: &Arc<Self>, generation: u64) {
        let duration = self
            .active
            .lock()
            .as_ref()
            .and_then(|active| active.duration);
        self.close();
        if self.looping.load(Ordering::Acquire) {
            let item = self.item.lock().clone();
            if let Some(item) = item
                && self.open(&item).is_ok()
            {
                self.play_active();
                publish_playback_state(PlaybackState::Playing);
                return;
            }
        }
        if self.generation.load(Ordering::Acquire) != generation + 1 {
            // Something else already started a new session while this one was
            // finishing; its state is the one that counts.
            return;
        }
        if let Some(duration) = duration {
            publish_playback_progress(progress_at(duration, Some(duration)));
        }
        publish_playback_state(PlaybackState::Ended);
    }

    fn play_active(&self) {
        if let Some(active) = self.active.lock().as_ref() {
            active.sink.play();
        }
    }
}

struct Observation {
    position: Duration,
    duration: Option<Duration>,
    ended: bool,
}

fn progress_at(position: Duration, duration: Option<Duration>) -> PlaybackProgress {
    PlaybackProgress {
        position,
        duration,
        // A local file is entirely on disk, so it is entirely buffered.
        buffered: duration.unwrap_or(position),
    }
}

/// What the decoders compiled into this backend read. Symphonia is built with
/// every format and codec it has, so this is the widest list the workspace
/// states: a target that decodes in process carries its own decoders rather
/// than borrowing the platform's.
const SOFTWARE_AUDIO_EXTENSIONS: &[&str] = &[
    "aac", "aif", "aiff", "caf", "flac", "m4a", "m4b", "mka", "mkv", "mp1", "mp2", "mp3", "mp4",
    "oga", "ogg", "opus", "wav", "wave", "webm",
];

impl MediaPlayer for SoftwareMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            seeking: true,
            speed: true,
            looping: true,
            analysis: true,
            // Nothing here draws a lock screen. On a platform that has one,
            // the platform backend wraps this and reports `session: true`.
            session: false,
            equalizer: true,
            probing: true,
        }
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        SOFTWARE_AUDIO_EXTENSIONS.to_vec()
    }
    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        self.shared.close();
        *self.shared.item.lock() = Some(item.clone());
        let duration = self.shared.open(item)?;
        publish_playback_progress(progress_at(Duration::ZERO, duration));
        publish_playback_state(PlaybackState::Paused);
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        if self.shared.active.lock().is_none() {
            // The item played to its end and gave the device up; opening it
            // again is what makes the play button after the end restart it.
            let item = self
                .shared
                .item
                .lock()
                .clone()
                .ok_or(MediaError::NothingLoaded)?;
            self.shared.open(&item)?;
        }
        self.shared.play_active();
        publish_playback_state(PlaybackState::Playing);
        Ok(())
    }

    fn pause(&self) {
        if let Some(active) = self.shared.active.lock().as_ref() {
            active.sink.pause();
        }
        publish_playback_state(PlaybackState::Paused);
    }

    fn stop(&self) {
        self.shared.close();
        self.shared.item.lock().take();
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        let duration = {
            let active = self.shared.active.lock();
            let Some(active) = active.as_ref() else {
                return Err(MediaError::NothingLoaded);
            };
            active.sink.seek(position)?;
            active.duration
        };
        publish_playback_progress(progress_at(position, duration));
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        *self.shared.volume.lock() = volume;
        if let Some(active) = self.shared.active.lock().as_ref() {
            active.sink.set_volume(volume);
        }
    }

    fn set_speed(&self, speed: f32) -> bool {
        let speed = speed.clamp(0.25, 4.0);
        *self.shared.speed.lock() = speed;
        if let Some(active) = self.shared.active.lock().as_ref() {
            active.sink.set_speed(speed);
        }
        true
    }

    fn set_looping(&self, looping: bool) {
        self.shared.looping.store(looping, Ordering::Release);
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        crate::equalizer::bands()
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        self.shared
            .equalizer
            .set(settings.enabled, settings.preamp_db, &settings.gains_db);
    }

    fn probe_duration(&self, item: &MediaItem) -> Option<Duration> {
        // The decoder reads the container's header; no output device is opened,
        // so probing a playlist costs nothing but the file reads.
        Decoder::probe_duration(&item.uri).or(item.metadata.duration)
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        self.shared.analysis.set_enabled(enabled);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test here runs on a build machine with no output device, so what
    /// is exercised is everything up to opening one: the URI, the decoder, the
    /// transport's answers when nothing is open. Playing a real item belongs on
    /// a machine with speakers.
    #[test]
    fn playing_before_anything_is_opened_reports_that_nothing_is_loaded() {
        let player = SoftwareMediaPlayer::new();

        assert_eq!(player.play(), Err(MediaError::NothingLoaded));
        assert_eq!(
            player.seek_to(Duration::from_secs(1)),
            Err(MediaError::NothingLoaded)
        );
    }

    #[test]
    fn a_uri_this_backend_cannot_read_is_refused_before_a_device_is_opened() {
        let player = SoftwareMediaPlayer::new();

        assert_eq!(
            player.prepare(&MediaItem::new("https://host/stream.mp3")),
            Err(MediaError::UnsupportedSource(
                "https://host/stream.mp3".to_string()
            ))
        );
        assert_eq!(
            player.prepare(&MediaItem::new("content://media/audio/1")),
            Err(MediaError::UnsupportedSource(
                "content://media/audio/1".to_string()
            ))
        );
    }

    #[test]
    fn a_file_that_is_not_there_says_so_rather_than_failing_silently() {
        let player = SoftwareMediaPlayer::new();

        let error = player
            .prepare(&MediaItem::new("file:///nowhere/missing.mp3"))
            .expect_err("a missing file cannot be opened");

        assert!(matches!(error, MediaError::Failed(message) if message.contains("missing.mp3")));
    }

    #[test]
    fn the_backend_reports_what_it_can_actually_do() {
        let capabilities = SoftwareMediaPlayer::new().capabilities();

        assert!(capabilities.seeking);
        assert!(capabilities.speed);
        assert!(capabilities.looping);
        assert!(capabilities.analysis);
        assert!(capabilities.probing);
        assert!(!capabilities.session);
    }

    #[test]
    fn volume_and_speed_are_remembered_for_the_next_item() {
        let player = SoftwareMediaPlayer::new();

        player.set_volume(4.0);
        assert_eq!(*player.shared.volume.lock(), 1.0);
        player.set_volume(0.25);
        assert_eq!(*player.shared.volume.lock(), 0.25);

        assert!(player.set_speed(100.0));
        assert_eq!(*player.shared.speed.lock(), 4.0);
        assert!(player.set_speed(1.5));
        assert_eq!(*player.shared.speed.lock(), 1.5);
    }

    #[test]
    fn analysis_is_off_until_it_is_asked_for() {
        let player = SoftwareMediaPlayer::new();
        assert!(!player.shared.analysis.is_enabled());

        assert!(player.set_analysis_enabled(true));

        assert!(player.shared.analysis.is_enabled());
    }
}

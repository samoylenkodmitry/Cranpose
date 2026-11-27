use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose::{
    AudioRole, LoadControl, MediaItem, PlaybackParameters, PlayerBackend, PlayerBackendFactory,
    PlayerError, PlayerSink, TrackSelectionParameters,
};

struct TestBackend {
    released: Arc<AtomicBool>,
    live: Arc<AtomicUsize>,
}

impl PlayerBackend for TestBackend {
    fn open(&self, _item: &MediaItem, _start_position: Duration) {}
    fn play(&self) {}
    fn pause(&self) {}
    fn stop(&self) {}
    fn seek_to(&self, _position: Duration) {}
    fn set_volume(&self, _volume: f32) {}
    fn set_playback_parameters(&self, _parameters: PlaybackParameters) {}
    fn set_track_selection(&self, _parameters: &TrackSelectionParameters) {}
    fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.live.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) struct TestFactory {
    sink: Mutex<Option<PlayerSink>>,
    #[allow(dead_code, reason = "read by player_ui_integration")]
    pub(crate) released: Arc<AtomicBool>,
    #[allow(dead_code, reason = "read by player_memory")]
    pub(crate) live: Arc<AtomicUsize>,
    #[allow(dead_code, reason = "read by player_memory")]
    pub(crate) peak: Arc<AtomicUsize>,
}

impl TestFactory {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            sink: Mutex::new(None),
            released: Arc::new(AtomicBool::new(false)),
            live: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub(crate) fn sink(&self) -> PlayerSink {
        self.sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("sink initialized")
    }
}

impl PlayerBackendFactory for TestFactory {
    fn create(
        &self,
        sink: PlayerSink,
        _load_control: LoadControl,
        _audio_role: AudioRole,
    ) -> Result<Arc<dyn PlayerBackend>, PlayerError> {
        let now = self.live.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak.fetch_max(now, Ordering::AcqRel);
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        Ok(Arc::new(TestBackend {
            released: Arc::clone(&self.released),
            live: Arc::clone(&self.live),
        }))
    }
}

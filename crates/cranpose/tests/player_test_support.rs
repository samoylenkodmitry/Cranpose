use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cranpose::{
    AudioRole, LoadControl, MediaItem, PlaybackParameters, PlayerBackend, PlayerBackendFactory,
    PlayerError, PlayerSink, TrackSelectionParameters,
};

struct TestBackend {
    released: Arc<AtomicBool>,
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
    }
}

pub(crate) struct TestFactory {
    sink: Mutex<Option<PlayerSink>>,
    pub(crate) released: Arc<AtomicBool>,
}

impl TestFactory {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            sink: Mutex::new(None),
            released: Arc::new(AtomicBool::new(false)),
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
        *self.sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        Ok(Arc::new(TestBackend {
            released: Arc::clone(&self.released),
        }))
    }
}

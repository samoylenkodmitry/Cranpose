//! Who hears the media service: the observers of its playback state, position,
//! commands, audio focus and analysis samples.

use std::sync::Arc;

use cranpose_core::ProcessIds;
use parking_lot::Mutex;

use super::{
    AudioFocus, MediaCommand, MediaMetadata, MediaSamples, PlaybackProgress, PlaybackState,
    audio_focus, playback_progress, playback_state,
};

pub(super) struct ObserverList<T: ?Sized> {
    entries: Mutex<Vec<(u64, Arc<T>)>>,
}

impl<T: ?Sized> ObserverList<T> {
    const fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn add(&self, observer: Arc<T>) -> u64 {
        let id = NEXT_OBSERVER.next();
        self.entries.lock().push((id, observer));
        id
    }

    pub(super) fn remove(&self, id: u64) {
        self.entries.lock().retain(|(entry, _)| *entry != id);
    }

    pub(super) fn snapshot(&self) -> Vec<Arc<T>> {
        self.entries
            .lock()
            .iter()
            .map(|(_, observer)| Arc::clone(observer))
            .collect()
    }

    fn clear(&self) {
        self.entries.lock().clear();
    }
}

static NEXT_OBSERVER: ProcessIds = ProcessIds::starting_at(1);

type StateObserverFn = dyn Fn(PlaybackState) + Send + Sync;
type ProgressObserverFn = dyn Fn(PlaybackProgress) + Send + Sync;
type CommandObserverFn = dyn Fn(MediaCommand) + Send + Sync;
type FocusObserverFn = dyn Fn(AudioFocus) + Send + Sync;
type SampleObserverFn = dyn Fn(MediaSamples) + Send + Sync;
type MetadataObserverFn = dyn Fn(&MediaMetadata) + Send + Sync;

pub(super) static STATE_OBSERVERS: ObserverList<StateObserverFn> = ObserverList::new();
pub(super) static PROGRESS_OBSERVERS: ObserverList<ProgressObserverFn> = ObserverList::new();
pub(super) static COMMAND_OBSERVERS: ObserverList<CommandObserverFn> = ObserverList::new();
pub(super) static FOCUS_OBSERVERS: ObserverList<FocusObserverFn> = ObserverList::new();
pub(super) static SAMPLE_OBSERVERS: ObserverList<SampleObserverFn> = ObserverList::new();
static METADATA_OBSERVERS: ObserverList<MetadataObserverFn> = ObserverList::new();

/// Keeps a media observer registered until it is dropped.
pub struct MediaObserver {
    id: u64,
    remove: fn(u64),
}

impl Drop for MediaObserver {
    fn drop(&mut self) {
        (self.remove)(self.id);
    }
}

/// Registers `observer` for playback state. The current state is delivered at
/// once, so a screen composed mid-item shows what is happening rather than
/// waiting for the next change.
pub fn observe_playback_state(
    observer: impl Fn(PlaybackState) + Send + Sync + 'static,
) -> MediaObserver {
    let observer: Arc<StateObserverFn> = Arc::new(observer);
    let id = STATE_OBSERVERS.add(Arc::clone(&observer));
    observer(playback_state());
    MediaObserver {
        id,
        remove: |id| STATE_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for position updates. The current position is
/// delivered at once.
pub fn observe_playback_progress(
    observer: impl Fn(PlaybackProgress) + Send + Sync + 'static,
) -> MediaObserver {
    let observer: Arc<ProgressObserverFn> = Arc::new(observer);
    let id = PROGRESS_OBSERVERS.add(Arc::clone(&observer));
    observer(playback_progress());
    MediaObserver {
        id,
        remove: |id| PROGRESS_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for the metadata [`set_media_metadata`](super::set_media_metadata)
/// gives the open item, such as a cover that finished loading after playback started.
pub fn observe_media_metadata(
    observer: impl Fn(&MediaMetadata) + Send + Sync + 'static,
) -> MediaObserver {
    let id = METADATA_OBSERVERS.add(Arc::new(observer));
    MediaObserver {
        id,
        remove: |id| METADATA_OBSERVERS.remove(id),
    }
}

pub(super) fn metadata_changed(metadata: &MediaMetadata) {
    for observer in METADATA_OBSERVERS.snapshot() {
        observer(metadata);
    }
}

/// Registers `observer` for media-session commands.
pub fn observe_media_commands(
    observer: impl Fn(MediaCommand) + Send + Sync + 'static,
) -> MediaObserver {
    let id = COMMAND_OBSERVERS.add(Arc::new(observer));
    MediaObserver {
        id,
        remove: |id| COMMAND_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for audio-focus changes. The current focus is
/// delivered at once.
pub fn observe_audio_focus(observer: impl Fn(AudioFocus) + Send + Sync + 'static) -> MediaObserver {
    let observer: Arc<FocusObserverFn> = Arc::new(observer);
    let id = FOCUS_OBSERVERS.add(Arc::clone(&observer));
    observer(audio_focus());
    MediaObserver {
        id,
        remove: |id| FOCUS_OBSERVERS.remove(id),
    }
}

/// Registers `observer` for analysis samples.
pub fn observe_media_samples(
    observer: impl Fn(MediaSamples) + Send + Sync + 'static,
) -> MediaObserver {
    let id = SAMPLE_OBSERVERS.add(Arc::new(observer));
    MediaObserver {
        id,
        remove: |id| SAMPLE_OBSERVERS.remove(id),
    }
}

// Forgets every observer, with the platform player they observed.
pub(super) fn clear() {
    STATE_OBSERVERS.clear();
    PROGRESS_OBSERVERS.clear();
    COMMAND_OBSERVERS.clear();
    FOCUS_OBSERVERS.clear();
    SAMPLE_OBSERVERS.clear();
    METADATA_OBSERVERS.clear();
}

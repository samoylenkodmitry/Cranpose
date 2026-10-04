//! The pipelines a renderer draws with, recorded for the next launches of
//! the app: the pipeline cache file keeps them, and a later launch builds
//! them ahead of the draws that need them.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc, Mutex, MutexGuard, PoisonError,
    atomic::{AtomicBool, Ordering},
};

#[cfg(not(target_arch = "wasm32"))]
use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use crate::pipeline_records::{Recorded, ShaderPipelineRecord};

/// How long after a renderer's first frame its draws are its first screen's.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const FIRST_SCREEN_SPAN: Duration = Duration::from_secs(2);

/// A renderer's record of the pipelines it draws with, shared by the parts
/// that build pipelines and the watcher that writes the cache file.
#[derive(Clone, Default)]
pub(crate) struct PipelineRecorder {
    #[cfg(not(target_arch = "wasm32"))]
    state: Arc<RecorderState>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct RecorderState {
    drawn: Mutex<Drawn>,
    first_screen: Mutex<FirstScreen>,
    frame_drawn: AtomicBool,
}

/// When the first screen started: with the first frame, moved later by every
/// pipeline a draw then waited to build, so a slow compile does not end it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct FirstScreen {
    started: Option<Instant>,
    over: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl FirstScreen {
    /// Whether the first screen ended; once ended it stays so.
    fn over(&mut self) -> bool {
        self.over |= self
            .started
            .is_some_and(|started| started.elapsed() > FIRST_SCREEN_SPAN);
        self.over
    }
}

/// The pipelines a renderer drew with, each list in the order first drawn,
/// and whether its first screen drew with each.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct Drawn {
    /// The shape pipelines, by their keys' bits.
    pub(crate) shapes: Vec<Recorded<u64>>,
    pub(crate) shaders: Vec<Recorded<ShaderPipelineRecord>>,
    /// The fixed pipelines, by label.
    pub(crate) fixed: Vec<Recorded<&'static str>>,
}

impl PipelineRecorder {
    /// Starts the first screen with the renderer's first frame.
    pub(crate) fn begin_frame(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.first_screen().started.get_or_insert_with(Instant::now);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl PipelineRecorder {
    /// Whether a draw now is the first screen's.
    pub(crate) fn in_first_screen(&self) -> bool {
        !self.first_screen().over()
    }

    /// Moves the first screen's end later by `waited`, the time a draw spent
    /// building a pipeline.
    pub(crate) fn extend_first_screen(&self, waited: Duration) {
        let mut first_screen = self.first_screen();
        if !first_screen.over()
            && let Some(started) = &mut first_screen.started
        {
            *started += waited;
        }
    }

    fn first_screen(&self) -> MutexGuard<'_, FirstScreen> {
        self.state
            .first_screen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes a shape pipeline drawn with, by its key's bits.
    pub(crate) fn note_shape(&self, key: u64, first_screen: bool) {
        note(&mut self.drawn().shapes, key, first_screen);
    }

    /// Notes a runtime shader pipeline drawn with.
    pub(crate) fn note_shader(&self, record: ShaderPipelineRecord, first_screen: bool) {
        note(&mut self.drawn().shaders, record, first_screen);
    }

    /// Notes a fixed pipeline drawn with, by its label.
    pub(crate) fn note_fixed(&self, label: &'static str, first_screen: bool) {
        note(&mut self.drawn().fixed, label, first_screen);
    }

    /// Notes a frame submitted: a launch closed before it keeps the last
    /// launch's records.
    pub(crate) fn note_frame_drawn(&self) {
        if !self.state.frame_drawn.swap(true, Ordering::AcqRel) {
            crate::pipeline_disk_cache::note_change();
        }
    }

    pub(crate) fn frame_drawn(&self) -> bool {
        self.state.frame_drawn.load(Ordering::Acquire)
    }

    pub(crate) fn drawn(&self) -> MutexGuard<'_, Drawn> {
        self.state
            .drawn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Notes `entry`, drawn by the first screen or not.
#[cfg(not(target_arch = "wasm32"))]
fn note<T: PartialEq>(records: &mut Vec<Recorded<T>>, entry: T, first_screen: bool) {
    match records.iter_mut().find(|record| record.entry == entry) {
        Some(record) if first_screen && !record.first_screen => record.first_screen = true,
        Some(_) => return,
        None => records.push(Recorded {
            entry,
            age: 0,
            first_screen,
        }),
    }
    crate::pipeline_disk_cache::note_change();
}

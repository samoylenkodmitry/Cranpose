//! The pipelines a renderer draws with, recorded for the next launches of
//! the app: the pipeline cache file keeps them, and a later launch builds
//! them ahead of the draws that need them. A web page's records ship with
//! the page instead (see [`crate::WgpuRenderer::first_screen_pipelines`]).

#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use web_time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use crate::pipeline_records::ShaderPipelineRecord;
use crate::pipeline_records::{Recorded, RecordedPipelines};

/// How long after a renderer's first frame its draws are its first screen's.
pub(crate) const FIRST_SCREEN_SPAN: Duration = Duration::from_secs(2);

/// A renderer's record of the pipelines it draws with, shared by the parts
/// that build pipelines and the watcher that writes the cache file.
#[derive(Clone, Default)]
pub(crate) struct PipelineRecorder {
    state: Arc<RecorderState>,
}

#[derive(Default)]
struct RecorderState {
    drawn: Mutex<Drawn>,
    first_screen: Mutex<FirstScreen>,
    #[cfg(not(target_arch = "wasm32"))]
    frame_drawn: AtomicBool,
}

/// When the first screen started: with the first frame, moved later by every
/// pipeline a draw then waited to build and by every frame that drew a
/// placeholder, so a slow compile does not end it.
#[derive(Default)]
struct FirstScreen {
    started: Option<Instant>,
    over: bool,
}

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
pub(crate) type Drawn = RecordedPipelines<&'static str>;

impl PipelineRecorder {
    pub(crate) fn during_demand<T>(&self, build: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let value = build();
        self.extend_first_screen(started.elapsed());
        value
    }

    /// Starts the first screen with the renderer's first frame.
    pub(crate) fn begin_frame(&self) {
        self.first_screen().started.get_or_insert_with(Instant::now);
    }

    /// Keeps the first screen open past a frame that drew a placeholder: the
    /// pipelines it waits for draw the first screen once they land.
    pub(crate) fn hold_first_screen(&self) {
        let mut first_screen = self.first_screen();
        if !first_screen.over() {
            first_screen.started = Some(Instant::now());
        }
    }

    /// Whether a draw now is the first screen's.
    pub(crate) fn in_first_screen(&self) -> bool {
        !self.first_screen().over()
    }

    /// The pipelines the first screen drew with on `backend`, encoded for a
    /// later launch to build before its first frame; `None` while the first
    /// screen lasts.
    pub(crate) fn first_screen_records(&self, backend: wgpu::Backend) -> Option<Vec<u8>> {
        if self.in_first_screen() {
            return None;
        }
        crate::pipeline_records::encode_first_screen(&self.drawn(), backend)
    }

    /// Moves the first screen's end later by `waited`, the time a draw spent
    /// building a pipeline.
    fn extend_first_screen(&self, waited: Duration) {
        let mut first_screen = self.first_screen();
        if !first_screen.over
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn note_shader(&self, record: ShaderPipelineRecord, first_screen: bool) {
        note(&mut self.drawn().shaders, record, first_screen);
    }

    /// Notes a fixed pipeline drawn with, by its label.
    pub(crate) fn note_fixed(&self, label: &'static str, first_screen: bool) {
        note(&mut self.drawn().fixed, label, first_screen);
    }

    /// Notes a frame submitted: a launch closed before it keeps the last
    /// launch's records.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn note_frame_drawn(&self) {
        if !self.state.frame_drawn.swap(true, Ordering::AcqRel) {
            note_change();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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
    note_change();
}

/// Tells the cache file's watcher the records changed; the web keeps no file.
fn note_change() {
    #[cfg(not(target_arch = "wasm32"))]
    crate::pipeline_disk_cache::note_change();
}

#![expect(
    unsafe_code,
    reason = "runs the renderer in an AAudio stream's callback"
)]

use std::{
    ops::Deref,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use cranpose_services::AudioError;

use super::{
    library::{Library, monotonic_nanos},
    stream::{Callback, Direction, Leaked, Running, Stream, data_callback},
};
use crate::{
    backend::{NOMINAL_CHANNELS, Renderer},
    mixer::RenderStatus,
};

struct DataContext {
    library: &'static Library,
    renderer: Box<dyn Renderer>,
}

// An open 32-bit float output stream that runs a renderer. Dropping it closes the stream, then
// drops the renderer on the dropping thread: the stream is declared first, so it drops first.
pub(super) struct OutputStream {
    stream: Stream,
    _data: Leaked<DataContext>,
}

impl Deref for OutputStream {
    type Target = Stream;

    fn deref(&self) -> &Stream {
        &self.stream
    }
}

impl OutputStream {
    // `connected` turns false when AAudio reports the stream lost.
    pub(super) fn open(
        renderer: Box<dyn Renderer>,
        connected: Arc<AtomicBool>,
    ) -> Result<Self, AudioError> {
        let library = Library::load()?;
        let data = Leaked::new(DataContext { library, renderer });
        // SAFETY: `data` is the `DataContext` that `render` reads, and it lives until the stream
        // closes: `OutputStream` declares it after the stream.
        let stream = unsafe {
            Stream::open(
                library,
                Direction::Output,
                NOMINAL_CHANNELS as i32,
                data_callback::<DataContext>,
                data.as_ptr(),
                connected,
            )
        }?;
        Ok(Self {
            stream,
            _data: data,
        })
    }

    // How long from now until the hardware presents the frame that the next render call writes,
    // or `None` while the stream has no timestamp.
    //
    // The thread that owns the stream calls this, so the real-time callback does no extra work.
    pub(super) fn output_latency(&self) -> Option<Duration> {
        self.timestamp()?
            .latency(self.frames_written(), monotonic_nanos(), self.sample_rate())
    }

    pub(super) fn underrun_count(&self) -> i32 {
        self.xrun_count()
    }
}

impl Callback for DataContext {
    fn library(&self) -> &'static Library {
        self.library
    }

    fn run(
        &mut self,
        _stream: &Running,
        audio: &mut [f32],
        sample_rate: i32,
        channels: usize,
    ) -> bool {
        self.renderer
            .set_device_format(sample_rate as f32, channels);
        match self.renderer.render(audio) {
            RenderStatus::Continue => true,
            RenderStatus::Idle => false,
        }
    }
}

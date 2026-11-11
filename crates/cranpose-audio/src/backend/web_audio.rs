use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use cranpose_services::AudioError;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{AudioContext, AudioContextState, AudioScheduledSourceNode};

use crate::backend::{AudioSink, NOMINAL_CHANNELS, Renderer};

const BLOCK_FRAMES: usize = 1024;

// A page has no real-time callback on the thread that runs the composition,
// so the stream renders ahead of the sound. A frame that takes longer than
// this lead leaves a gap in the sound; a longer lead delays every sound more.
pub(crate) const LEAD_SECONDS: f64 = 0.1;

// The least time between the context's clock and the start of a block the
// stream schedules, so the block is not already late when the browser gets
// it. A context can move its clock in steps of 20 ms.
const START_MARGIN_SECONDS: f64 = 0.03;

// A browser starts a context only after the user has acted on the page.
const UNLOCK_EVENTS: [&str; 3] = ["pointerdown", "pointerup", "keydown"];

thread_local! {
    // The open streams; a sink names its stream by the slot it sits in.
    static STREAMS: RefCell<Vec<Option<Rc<Stream>>>> = const { RefCell::new(Vec::new()) };
}

struct Stream {
    context: AudioContext,
    renderer: RefCell<Box<dyn Renderer>>,
    interleaved: RefCell<Vec<f32>>,
    channel: RefCell<Vec<f32>>,
    next_start: Cell<f64>,
    wanted: Cell<bool>,
    refill: RefCell<Option<Closure<dyn FnMut()>>>,
    unlock: RefCell<Option<Closure<dyn FnMut()>>>,
}

impl Stream {
    fn refill(&self) {
        if !self.wanted.get() || self.context.state() != AudioContextState::Running {
            return;
        }
        while self.next_start.get() - self.context.current_time() < LEAD_SECONDS {
            if let Err(error) = self.schedule_block() {
                log::warn!("cranpose audio: the browser refused a block: {error:?}");
                return;
            }
        }
    }

    fn schedule_block(&self) -> Result<(), JsValue> {
        let sample_rate = self.context.sample_rate();
        let mut interleaved = self.interleaved.borrow_mut();
        let mut renderer = self.renderer.borrow_mut();
        renderer.set_device_format(sample_rate, NOMINAL_CHANNELS);
        let _ = renderer.render(&mut interleaved);

        let buffer = self.context.create_buffer(
            NOMINAL_CHANNELS as u32,
            BLOCK_FRAMES as u32,
            sample_rate,
        )?;
        let mut channel = self.channel.borrow_mut();
        for index in 0..NOMINAL_CHANNELS {
            for (sample, frame) in channel
                .iter_mut()
                .zip(interleaved.as_chunks::<NOMINAL_CHANNELS>().0)
            {
                *sample = frame[index];
            }
            buffer.copy_to_channel(&channel, index as i32)?;
        }

        let source = self.context.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        source.connect_with_audio_node(&self.context.destination())?;
        if let Some(refill) = self.refill.borrow().as_ref() {
            let scheduled: &AudioScheduledSourceNode = source.as_ref();
            scheduled.set_onended(Some(refill.as_ref().unchecked_ref()));
        }
        // A block the browser gets after its start plays late, over the block
        // that follows it. A hole of silence is the smaller fault. The clock
        // is read here, after the render: the first render of a page is slow.
        let earliest = self.context.current_time() + START_MARGIN_SECONDS;
        let start = self.next_start.get().max(earliest);
        source.start_with_when(start)?;
        self.next_start
            .set(start + BLOCK_FRAMES as f64 / f64::from(sample_rate));
        Ok(())
    }

    fn run(&self) {
        self.wanted.set(true);
        if self.context.state() == AudioContextState::Running {
            self.refill();
        } else {
            let _ = self.context.resume();
        }
    }

    fn pause(&self) {
        self.wanted.set(false);
        let _ = self.context.suspend();
    }

    fn close(&self) {
        self.wanted.set(false);
        self.context.set_onstatechange(None);
        if let (Some(window), Some(unlock)) = (web_sys::window(), self.unlock.borrow().as_ref()) {
            for event in UNLOCK_EVENTS {
                let _ = window
                    .remove_event_listener_with_callback(event, unlock.as_ref().unchecked_ref());
            }
        }
        let _ = self.context.close();
    }
}

pub(crate) fn open(renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    let context = AudioContext::new().map_err(|error| {
        AudioError::Backend(format!("the browser gave no audio context: {error:?}"))
    })?;
    let window = web_sys::window()
        .ok_or_else(|| AudioError::Backend("the page has no window to listen on".to_string()))?;

    let stream = Rc::new(Stream {
        context,
        renderer: RefCell::new(renderer),
        interleaved: RefCell::new(vec![0.0; BLOCK_FRAMES * NOMINAL_CHANNELS]),
        channel: RefCell::new(vec![0.0; BLOCK_FRAMES]),
        next_start: Cell::new(0.0),
        wanted: Cell::new(true),
        refill: RefCell::new(None),
        unlock: RefCell::new(None),
    });

    let refill = callback(&stream, Stream::refill);
    stream
        .context
        .set_onstatechange(Some(refill.as_ref().unchecked_ref()));
    *stream.refill.borrow_mut() = Some(refill);

    let unlock = callback(&stream, |stream| {
        if stream.wanted.get() && stream.context.state() == AudioContextState::Suspended {
            let _ = stream.context.resume();
        }
    });
    for event in UNLOCK_EVENTS {
        window
            .add_event_listener_with_callback(event, unlock.as_ref().unchecked_ref())
            .map_err(|error| {
                AudioError::Backend(format!("the page refused an input listener: {error:?}"))
            })?;
    }
    *stream.unlock.borrow_mut() = Some(unlock);

    stream.run();
    log::debug!(
        "cranpose audio: Web Audio stream at {} Hz",
        stream.context.sample_rate()
    );

    let slot = STREAMS.with_borrow_mut(|streams| {
        let slot = streams.iter().position(Option::is_none).unwrap_or_else(|| {
            streams.push(None);
            streams.len() - 1
        });
        streams[slot] = Some(stream);
        slot
    });
    Ok(Box::new(WebAudioSink { slot }))
}

fn callback(stream: &Rc<Stream>, run: fn(&Stream)) -> Closure<dyn FnMut()> {
    let stream: Weak<Stream> = Rc::downgrade(stream);
    Closure::new(move || {
        if let Some(stream) = stream.upgrade() {
            run(&stream);
        }
    })
}

// The stream holds values of the page, which stay on the thread that made
// them. The sink names its stream, so the sink itself can cross threads as
// the `AudioSink` contract asks.
struct WebAudioSink {
    slot: usize,
}

impl WebAudioSink {
    fn with_stream(&self, run: impl FnOnce(&Stream)) {
        let stream = STREAMS.with_borrow(|streams| streams.get(self.slot).cloned().flatten());
        if let Some(stream) = stream {
            run(&stream);
        }
    }
}

impl AudioSink for WebAudioSink {
    fn suspend(&self) {
        self.with_stream(Stream::pause);
    }

    fn resume(&self) {
        self.with_stream(Stream::run);
    }

    fn park(&self) {
        self.with_stream(Stream::pause);
    }
}

impl Drop for WebAudioSink {
    fn drop(&mut self) {
        let stream =
            STREAMS.with_borrow_mut(|streams| streams.get_mut(self.slot).and_then(Option::take));
        if let Some(stream) = stream {
            stream.close();
        }
    }
}

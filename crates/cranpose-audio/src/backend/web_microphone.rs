// The microphone in a browser. `getUserMedia` gives the microphone's stream once the person
// allows it. A script processor of an audio context of its own then hands each block to the
// recorder on the page's thread. A browser reports no capture time for a block.

use std::{cell::RefCell, rc::Rc};

use cranpose_services::AudioError;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    AudioContext, AudioProcessingEvent, MediaStream, MediaStreamConstraints, MediaStreamTrack,
    MediaTrackConstraints, ScriptProcessorNode,
};

use crate::backend::{
    microphone::{AudioSource, InputFormat, InputPreset, Recorder},
    page::PAGE,
};

// The frames of one block of the script processor: 21 ms at 48 kHz
const BLOCK_FRAMES: u32 = 1024;
// A voice is one channel. The browser mixes the microphone's channels down to it.
const CHANNELS: u32 = 1;

pub(super) struct Recording {
    context: AudioContext,
    recorder: RefCell<Box<dyn Recorder>>,
    scratch: RefCell<Vec<f32>>,
    // What runs once the person allowed the microphone
    running: RefCell<Option<Running>>,
    // The two answers to `getUserMedia`, kept while the promise may still call them
    answers: RefCell<Option<Answers>>,
}

struct Answers {
    _stream: Closure<dyn FnMut(JsValue)>,
    _refusal: Closure<dyn FnMut(JsValue)>,
}

struct Running {
    stream: MediaStream,
    processor: ScriptProcessorNode,
    _on_block: Closure<dyn FnMut(AudioProcessingEvent)>,
}

impl Recording {
    fn start(self: &Rc<Self>, stream: MediaStream) -> Result<(), JsValue> {
        let source = self.context.create_media_stream_source(&stream)?;
        let processor = self
            .context
            .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(
                BLOCK_FRAMES,
                CHANNELS,
                1,
            )?;
        let recording = Rc::downgrade(self);
        let on_block =
            Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
                if let Some(recording) = recording.upgrade() {
                    recording.take(&event);
                }
            });
        processor.set_onaudioprocess(Some(on_block.as_ref().unchecked_ref()));
        source.connect_with_audio_node(&processor)?;
        // A script processor runs only while its output leads to the destination. It writes no
        // sound, so the page plays nothing.
        processor.connect_with_audio_node(&self.context.destination())?;
        let _ = self.context.resume();
        *self.running.borrow_mut() = Some(Running {
            stream,
            processor,
            _on_block: on_block,
        });
        Ok(())
    }

    fn take(&self, event: &AudioProcessingEvent) {
        let Ok(buffer) = event.input_buffer() else {
            return;
        };
        let mut scratch = self.scratch.borrow_mut();
        let frames = (buffer.length() as usize).min(scratch.len());
        if buffer.copy_from_channel(&mut scratch[..frames], 0).is_err() {
            return;
        }
        let format = InputFormat {
            sample_rate: buffer.sample_rate(),
            channels: CHANNELS as usize,
        };
        self.recorder
            .borrow_mut()
            .record(&scratch[..frames], format, None);
    }

    fn stop(&self) {
        if let Some(running) = self.running.borrow_mut().take() {
            running.processor.set_onaudioprocess(None);
            let _ = running.processor.disconnect();
            for track in running.stream.get_tracks().iter() {
                track.unchecked_into::<MediaStreamTrack>().stop();
            }
        }
        let _ = self.context.close();
    }
}

pub(crate) fn open(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    let failed = |error: JsValue| {
        AudioError::Backend(format!("the browser refused the microphone: {error:?}"))
    };
    let window =
        web_sys::window().ok_or_else(|| AudioError::Backend("the page has no window".into()))?;
    let devices = window.navigator().media_devices().map_err(failed)?;
    let context = AudioContext::new().map_err(failed)?;
    let track = MediaTrackConstraints::new();
    let processed = preset == InputPreset::VoiceCommunication;
    track.set_echo_cancellation_bool(processed);
    track.set_noise_suppression_bool(processed);
    track.set_auto_gain_control_bool(processed);
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio_media_track_constraints(&track);
    let promise = devices
        .get_user_media_with_constraints(&constraints)
        .map_err(failed)?;
    let recording = Rc::new(Recording {
        context,
        recorder: RefCell::new(recorder),
        scratch: RefCell::new(vec![0.0; BLOCK_FRAMES as usize]),
        running: RefCell::new(None),
        answers: RefCell::new(None),
    });
    let weak = Rc::downgrade(&recording);
    let on_stream = Closure::<dyn FnMut(JsValue)>::new(move |stream: JsValue| {
        let (Some(recording), Ok(stream)) = (weak.upgrade(), stream.dyn_into::<MediaStream>())
        else {
            return;
        };
        if let Err(error) = recording.start(stream) {
            log::warn!("cranpose audio: the microphone did not start: {error:?}");
        }
    });
    let on_refusal = Closure::<dyn FnMut(JsValue)>::new(|error: JsValue| {
        log::warn!("cranpose audio: the browser gave no microphone: {error:?}");
    });
    let _ = promise.then2(&on_stream, &on_refusal);
    *recording.answers.borrow_mut() = Some(Answers {
        _stream: on_stream,
        _refusal: on_refusal,
    });
    let slot = PAGE.with(|page| page.recordings.insert(recording));
    Ok(Box::new(WebSource { slot }))
}

// The source names its recording by a slot of the page's list (see `page.rs`).
struct WebSource {
    slot: usize,
}

impl AudioSource for WebSource {}

impl Drop for WebSource {
    fn drop(&mut self) {
        let recording = PAGE.with(|page| page.recordings.take(self.slot));
        if let Some(recording) = recording {
            recording.stop();
        }
    }
}

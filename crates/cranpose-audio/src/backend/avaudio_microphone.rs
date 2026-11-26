#![expect(unsafe_code, reason = "Objective-C messages to AVAudioEngine")]

// The watchOS microphone: a tap on the input node of an `AVAudioEngine` of its own. The tap hands
// over buffers with one float buffer per channel, which the recorder gets interleaved.

use std::{cell::RefCell, ptr::NonNull, time::Duration};

use block2::RcBlock;
use cranpose_services::AudioError;
use dispatch2::{MainThreadBound, run_on_main};
use objc2::{MainThreadMarker, rc::Retained};
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};

use crate::backend::microphone::{AudioSource, InputFormat, InputPreset, Recorder, captured_at};

// The frames of one tap buffer that the engine is asked for, 10 ms at 48 kHz. The engine may hand
// over more at a time.
const TAP_FRAMES: u32 = 480;
// Frames interleaved per call into the scratch. A buffer with more goes to the recorder in parts.
const SCRATCH_FRAMES: usize = 4096;

type TapBlock = RcBlock<dyn Fn(NonNull<AVAudioPCMBuffer>, NonNull<AVAudioTime>)>;

unsafe extern "C" {
    // The clock of `AVAudioTime`'s host time
    fn mach_absolute_time() -> u64;
}

pub(crate) fn open(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
) -> Result<Box<dyn AudioSource>, AudioError> {
    let device = run_on_main(move |mtm| start(recorder, preset, mtm))?;
    Ok(Box::new(AvAudioSource { device }))
}

fn start(
    recorder: Box<dyn Recorder>,
    preset: InputPreset,
    mtm: MainThreadMarker,
) -> Result<MainThreadBound<Device>, AudioError> {
    // SAFETY: a new engine, used only on this thread until it is moved into the main-thread-bound
    // device below.
    let engine = unsafe { AVAudioEngine::new() };
    // SAFETY: the input node belongs to the live engine.
    let input = unsafe { engine.inputNode() };
    if preset == InputPreset::VoiceCommunication {
        // SAFETY: the engine has not started, which voice processing needs.
        if let Err(error) = unsafe { input.setVoiceProcessingEnabled_error(true) } {
            log::warn!("cranpose audio: the watch records without voice processing: {error}");
        }
    }
    // SAFETY: the input node and its format belong to the live engine.
    let format = unsafe { input.outputFormatForBus(0) };
    // SAFETY: reading two properties of a live format.
    let (sample_rate, channels) = unsafe { (format.sampleRate(), format.channelCount() as usize) };
    if !sample_rate.is_finite() || sample_rate <= 0.0 || channels == 0 {
        return Err(AudioError::Backend(format!(
            "the watch reports an input of {channels} channels at {sample_rate} Hz"
        )));
    }
    let format_of_samples = InputFormat {
        sample_rate: sample_rate as f32,
        channels,
    };
    let state = RefCell::new(TapState {
        recorder,
        scratch: vec![0.0; SCRATCH_FRAMES * channels],
        format: format_of_samples,
    });
    let block: TapBlock = RcBlock::new(
        move |buffer: NonNull<AVAudioPCMBuffer>, time: NonNull<AVAudioTime>| {
            // The engine calls the tap on one thread, one buffer at a time, so the borrow never finds
            // the state taken.
            let Ok(mut state) = state.try_borrow_mut() else {
                return;
            };
            // SAFETY: the engine passes a live buffer and its time for this call.
            unsafe { state.take(buffer.as_ref(), time.as_ref()) };
        },
    );
    // SAFETY: the node copies the block, the format is the node's own, and the engine starts after
    // the tap is in place.
    unsafe {
        input.installTapOnBus_bufferSize_format_block(
            0,
            TAP_FRAMES,
            Some(&format),
            RcBlock::as_ptr(&block),
        );
        engine.prepare();
        engine.startAndReturnError()
    }
    .map_err(|error| {
        AudioError::Backend(format!("failed to start the recording engine: {error}"))
    })?;
    log::debug!("cranpose audio: AVAudioEngine input at {sample_rate} Hz, {channels} channels");
    Ok(MainThreadBound::new(
        Device {
            engine,
            _block: block,
        },
        mtm,
    ))
}

struct TapState {
    recorder: Box<dyn Recorder>,
    scratch: Vec<f32>,
    format: InputFormat,
}

impl TapState {
    // Interleaves the buffer's channels and hands them to the recorder.
    // SAFETY: the caller passes a live float buffer and its time.
    unsafe fn take(&mut self, buffer: &AVAudioPCMBuffer, time: &AVAudioTime) {
        // SAFETY: the caller vouches for the buffer and the time.
        let (frames, channels, captured) = unsafe {
            let age = time
                .isHostTimeValid()
                .then(|| {
                    AVAudioTime::secondsForHostTime(mach_absolute_time())
                        - AVAudioTime::secondsForHostTime(time.hostTime())
                })
                .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok());
            (
                buffer.frameLength() as usize,
                buffer.floatChannelData(),
                age,
            )
        };
        if channels.is_null() {
            return;
        }
        let width = self.format.channels;
        let mut done = 0;
        while done < frames {
            let take = (frames - done).min(SCRATCH_FRAMES);
            for lane in 0..width {
                // SAFETY: a float buffer holds a pointer to `frameLength` floats for each of the
                // format's channels.
                let samples =
                    unsafe { std::slice::from_raw_parts((*channels.add(lane)).as_ptr(), frames) };
                for frame in 0..take {
                    self.scratch[frame * width + lane] = samples[done + frame];
                }
            }
            let offset = Duration::from_secs_f64(done as f64 / f64::from(self.format.sample_rate));
            let at = captured
                .and_then(|age| age.checked_sub(offset))
                .and_then(captured_at);
            self.recorder
                .record(&self.scratch[..take * width], self.format, at);
            done += take;
        }
    }
}

// What has to live as long as the recording runs. The node holds a copy of the block; it is kept
// here as well so that nothing depends on that.
struct Device {
    engine: Retained<AVAudioEngine>,
    _block: TapBlock,
}

// The engine lives on the main thread; the drop waits for the main thread to stop it.
struct AvAudioSource {
    device: MainThreadBound<Device>,
}

impl AudioSource for AvAudioSource {
    fn is_running(&self) -> bool {
        self.device.get_on_main(|device| {
            // SAFETY: reading the engine's state on its own thread.
            unsafe { device.engine.isRunning() }
        })
    }
}

impl Drop for AvAudioSource {
    fn drop(&mut self) {
        self.device.get_on_main(|device| {
            // SAFETY: removing the tap and stopping the engine on its own thread, before the
            // device and the recorder are dropped there.
            unsafe {
                device.engine.inputNode().removeTapOnBus(0);
                device.engine.stop();
            }
        });
    }
}

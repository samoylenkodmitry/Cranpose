#![expect(unsafe_code)]

//! The watchOS output device: an `AVAudioEngine` whose source node runs the
//! renderer.
//!
//! The source node takes the standard format, float samples with one buffer
//! per channel, at the rate of the output hardware, and the engine's main mixer
//! converts that to whatever the speaker or the headphones take. The session
//! is in the ambient category, so the sound mixes with other audio and keeps
//! quiet while the watch is silenced.

use std::{
    cell::RefCell,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use block2::RcBlock;
use cranpose_services::AudioError;
use dispatch2::{MainThreadBound, run_on_main};
use objc2::{AllocAnyThread, MainThreadMarker, rc::Retained, runtime::Bool};
use objc2_avf_audio::{
    AVAudioEngine, AVAudioFormat, AVAudioFrameCount, AVAudioSession, AVAudioSessionCategoryAmbient,
    AVAudioSourceNode,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};

use crate::backend::{AudioSink, NOMINAL_CHANNELS, Renderer};

/// Frames rendered per call into the interleaved scratch. A callback that asks
/// for more is served in several renders.
const SCRATCH_FRAMES: usize = 4096;

type RenderBlock = RcBlock<
    dyn Fn(
        NonNull<Bool>,
        NonNull<AudioTimeStamp>,
        AVAudioFrameCount,
        NonNull<AudioBufferList>,
    ) -> i32,
>;

pub(crate) fn open(renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    let device = run_on_main(move |mtm| start(renderer, mtm))?;
    Ok(Box::new(AvAudioSink {
        device,
        wanted: AtomicBool::new(true),
    }))
}

fn start(
    mut renderer: Box<dyn Renderer>,
    mtm: MainThreadMarker,
) -> Result<MainThreadBound<Device>, AudioError> {
    // SAFETY: the shared session is valid for the life of the process, and the
    // category constant is AVFAudio's own.
    unsafe {
        let session = AVAudioSession::sharedInstance();
        if let Some(ambient) = AVAudioSessionCategoryAmbient {
            if let Err(error) = session.setCategory_error(ambient) {
                log::warn!("failed to put the audio session in the ambient category: {error}");
            }
        }
    }

    // SAFETY: a new engine, used only on this thread until it is moved into
    // the main-thread-bound device below.
    let engine = unsafe { AVAudioEngine::new() };
    // SAFETY: the output node and its format belong to the live engine.
    let sample_rate = unsafe { engine.outputNode().outputFormatForBus(0).sampleRate() };
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(AudioError::Backend(format!(
            "the watch reports an output rate of {sample_rate} Hz"
        )));
    }
    // SAFETY: a standard format is float samples, one buffer per channel.
    let format = unsafe {
        AVAudioFormat::initStandardFormatWithSampleRate_channels(
            AVAudioFormat::alloc(),
            sample_rate,
            NOMINAL_CHANNELS as u32,
        )
    }
    .ok_or_else(|| AudioError::Backend(format!("no stereo float format at {sample_rate} Hz")))?;
    renderer.set_device_format(sample_rate as f32, NOMINAL_CHANNELS);

    let state = RefCell::new(RenderState {
        renderer,
        scratch: vec![0.0; SCRATCH_FRAMES * NOMINAL_CHANNELS],
    });
    let block: RenderBlock = RcBlock::new(
        move |_silence: NonNull<Bool>,
              _time: NonNull<AudioTimeStamp>,
              frames: AVAudioFrameCount,
              list: NonNull<AudioBufferList>| {
            // The engine runs this block on its render thread, one call at a
            // time, so the borrow never finds the state taken.
            let Ok(mut state) = state.try_borrow_mut() else {
                return 0;
            };
            // SAFETY: the engine passes a list of `mNumberBuffers` buffers,
            // each with room for `frames` frames of its channels, valid for
            // this call.
            unsafe { state.fill(frames as usize, list) };
            0
        },
    );
    // SAFETY: the node copies the block, and the format outlives the call.
    let source = unsafe {
        AVAudioSourceNode::initWithFormat_renderBlock(
            AVAudioSourceNode::alloc(),
            &format,
            RcBlock::as_ptr(&block),
        )
    };
    // SAFETY: the node is attached before it is connected, and both nodes
    // belong to this engine.
    unsafe {
        engine.attachNode(&source);
        engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
        engine.prepare();
        engine.startAndReturnError()
    }
    .map_err(|error| AudioError::Backend(format!("failed to start the audio engine: {error}")))?;

    log::debug!("cranpose audio: AVAudioEngine at {sample_rate} Hz");
    Ok(MainThreadBound::new(
        Device {
            engine,
            _source: source,
            _block: block,
        },
        mtm,
    ))
}

struct RenderState {
    renderer: Box<dyn Renderer>,
    scratch: Vec<f32>,
}

impl RenderState {
    /// Renders `frames` frames and spreads them over the list's buffers,
    /// whatever their channel layout. Channels the renderer did not produce
    /// get its last one; frames past a short buffer are dropped.
    // SAFETY: the caller passes a valid list whose buffers each hold `frames`
    // frames of `mNumberChannels` interleaved floats.
    unsafe fn fill(&mut self, frames: usize, list: NonNull<AudioBufferList>) {
        // SAFETY: the caller vouches for the list.
        let count = unsafe { (*list.as_ptr()).mNumberBuffers } as usize;
        // SAFETY: `mBuffers` is a C array of `count` entries.
        let buffers: &mut [AudioBuffer] = unsafe {
            std::slice::from_raw_parts_mut((*list.as_ptr()).mBuffers.as_mut_ptr(), count)
        };
        let mut done = 0;
        while done < frames {
            let take = (frames - done).min(SCRATCH_FRAMES);
            let rendered = &mut self.scratch[..take * NOMINAL_CHANNELS];
            let _ = self.renderer.render(rendered);
            let mut channel = 0;
            for buffer in buffers.iter_mut() {
                let width = buffer.mNumberChannels as usize;
                if width == 0 || buffer.mData.is_null() {
                    continue;
                }
                let capacity = buffer.mDataByteSize as usize / size_of::<f32>() / width;
                // SAFETY: the buffer holds `capacity` frames of `width` floats.
                let out = unsafe {
                    std::slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), capacity * width)
                };
                for frame in 0..take.min(capacity.saturating_sub(done)) {
                    for lane in 0..width {
                        let source = (channel + lane).min(NOMINAL_CHANNELS - 1);
                        out[(done + frame) * width + lane] =
                            rendered[frame * NOMINAL_CHANNELS + source];
                    }
                }
                channel += width;
            }
            done += take;
        }
    }
}

/// What has to live as long as the sound plays. The engine holds the node and
/// the node a copy of the block; both are kept here as well so that nothing
/// depends on that.
struct Device {
    engine: Retained<AVAudioEngine>,
    _source: Retained<AVAudioSourceNode>,
    _block: RenderBlock,
}

/// The engine lives on the main thread; every call from elsewhere waits for
/// the main thread to run it.
struct AvAudioSink {
    device: MainThreadBound<Device>,
    /// Whether the engine is meant to be running: false after a suspend or a
    /// park, true after a start or a resume.
    wanted: AtomicBool,
}

impl AvAudioSink {
    fn pause(&self) {
        self.wanted.store(false, Ordering::SeqCst);
        self.device.get_on_main(|device| {
            // SAFETY: pausing the engine on its own thread.
            unsafe { device.engine.pause() };
        });
    }
}

impl AudioSink for AvAudioSink {
    fn suspend(&self) {
        self.pause();
    }

    fn resume(&self) {
        self.wanted.store(true, Ordering::SeqCst);
        let started = self.device.get_on_main(|device| {
            // SAFETY: starting the engine on its own thread.
            unsafe { device.engine.startAndReturnError() }.map_err(|error| error.to_string())
        });
        if let Err(error) = started {
            log::warn!("failed to restart the audio engine: {error}");
        }
    }

    /// The system stops the engine when another app takes the audio, and when
    /// the output changes, for instance to headphones. A stopped engine that is
    /// meant to run is dead: the next sound opens a new one, at the new
    /// output's rate.
    fn is_running(&self) -> bool {
        if !self.wanted.load(Ordering::SeqCst) {
            return true;
        }
        self.device.get_on_main(|device| {
            // SAFETY: reading the engine's state on its own thread.
            unsafe { device.engine.isRunning() }
        })
    }

    fn park(&self) {
        self.pause();
    }
}

impl Drop for AvAudioSink {
    fn drop(&mut self) {
        self.device.get_on_main(|device| {
            // SAFETY: stopping the engine on its own thread, before the device
            // and the renderer are dropped there.
            unsafe { device.engine.stop() };
        });
    }
}

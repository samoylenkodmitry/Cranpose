//! Platform output devices.
//!
//! A backend's whole job is to obtain a real-time callback, hand it a
//! [`Renderer`], and keep the stream alive. Everything above the callback is
//! shared, so a new platform is one file.
//!
//! The renderer is a trait rather than the engine's own mixer because the mixer
//! is not the only thing in the workspace that needs a device: `cranpose-media`
//! decodes a track into the same kind of callback. One device per platform,
//! several things to put through it.

use cranpose_services::AudioError;

use crate::mixer::{Mixer, MixerSeed, RenderStatus};

#[cfg(all(feature = "aaudio", target_os = "android"))]
mod aaudio;

#[cfg(all(
    feature = "cpal-backend",
    not(any(target_os = "android", target_arch = "wasm32"))
))]
mod cpal_device;

/// The rate a renderer is built for before the device reports the one it
/// negotiated. AAudio only tells you after the stream is open, so every
/// renderer starts on an assumption and corrects it in its first callback.
pub const NOMINAL_SAMPLE_RATE: f32 = 48_000.0;

/// The channel count a renderer is built for, and what every backend asks the
/// device for.
pub const NOMINAL_CHANNELS: usize = 2;

/// What a device callback runs.
///
/// Both methods are called on the platform's real-time thread, so neither may
/// allocate, lock or block.
pub trait Renderer: Send {
    /// Reports the format the device actually negotiated. Called before every
    /// render, so it must return immediately when the format has not changed.
    fn set_device_format(&mut self, sample_rate: f32, channels: usize);

    /// Fills `out` with interleaved samples and says whether the device still
    /// has a reason to run.
    fn render(&mut self, out: &mut [f32]) -> RenderStatus;
}

impl Renderer for Mixer {
    fn set_device_format(&mut self, sample_rate: f32, channels: usize) {
        Mixer::set_device_format(self, sample_rate, channels);
    }

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        Mixer::render(self, out)
    }
}

/// A running output device. Dropping it stops the stream and drops the renderer
/// (and with it everything the renderer still held) on the thread that opened
/// it.
pub trait AudioSink: Send + Sync {
    /// Pauses the stream without discarding it, for an app going away.
    fn suspend(&self) {}
    /// Starts the stream again: after [`suspend`](AudioSink::suspend), and
    /// after the mixer gave the device up for want of anything to play.
    fn resume(&self) {}
    /// Whether the stream is actually running right now.
    ///
    /// NOT a cached flag: ask the platform. `streaming` is set by the engine
    /// and cleared only by the mixer's own data callback, so a stream the
    /// platform reclaims WITHOUT running that callback leaves every liveness
    /// signal the engine owns stuck at "fine" -- and cues are then written into
    /// a dead stream for the life of the process. Observed on a Pixel Watch 3.
    fn is_running(&self) -> bool {
        true
    }

    /// Releases the device after the mixer reported itself idle.
    ///
    /// This is the half of stopping that a real-time callback cannot do for
    /// itself. AAudio's callback returns `Stop`, which on current Android
    /// tears the stream down from the inside; this then makes it explicit, and
    /// is what releases the route on the older releases where returning `Stop`
    /// only ends the callback. cpal has no such return at all, so for that
    /// backend this is the only thing that stops the stream.
    ///
    /// Called from the UI thread, at most once per idle stretch, and only
    /// after the mixer has published that it is no longer streaming.
    fn park(&self) {}
}

/// Whether this build has a real output device compiled in.
pub fn is_compiled() -> bool {
    cfg!(all(feature = "aaudio", target_os = "android"))
        || cfg!(all(
            feature = "cpal-backend",
            not(any(target_os = "android", target_arch = "wasm32"))
        ))
}

/// Opens the platform output device, starts it, and runs `renderer` on it.
///
/// Each arm is a separate `cfg` block, so the explicit returns are what keeps
/// exactly one of them live per target instead of one expression with three
/// conditional halves.
#[allow(clippy::needless_return)]
pub fn open(renderer: Box<dyn Renderer>) -> Result<Box<dyn AudioSink>, AudioError> {
    #[cfg(all(feature = "aaudio", target_os = "android"))]
    {
        return aaudio::open(renderer);
    }

    #[cfg(all(
        feature = "cpal-backend",
        not(any(target_os = "android", target_arch = "wasm32"))
    ))]
    {
        return cpal_device::open(renderer);
    }

    #[cfg(not(any(
        all(feature = "aaudio", target_os = "android"),
        all(
            feature = "cpal-backend",
            not(any(target_os = "android", target_arch = "wasm32"))
        )
    )))]
    {
        drop(renderer);
        Err(AudioError::Unsupported)
    }
}

pub(crate) fn open_mixer(seed: MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> {
    open(Box::new(Mixer::new(
        seed,
        NOMINAL_SAMPLE_RATE,
        NOMINAL_CHANNELS,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_a_device_reports_unsupported() {
        if is_compiled() {
            return;
        }
        let (_command_tx, command_rx) = crate::ring::channel(4);
        let (retired_tx, _retired_rx) = crate::ring::channel(4);
        let seed = MixerSeed {
            commands: command_rx,
            retired: retired_tx,
            leaked_clips: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            underruns: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            streaming: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        assert!(matches!(open_mixer(seed), Err(AudioError::Unsupported)));
    }
}

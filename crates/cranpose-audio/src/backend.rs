//! Platform output devices.
//!
//! A backend's whole job is to obtain a real-time callback, hand it the
//! [`Mixer`](crate::mixer::Mixer), and keep the stream alive. All mixing,
//! resampling and voice management is shared, so a new platform is one file.

use crate::mixer::MixerSeed;
use cranpose_services::AudioError;

#[cfg(all(feature = "aaudio", target_os = "android"))]
mod aaudio;

#[cfg(all(
    feature = "cpal-backend",
    not(any(target_os = "android", target_arch = "wasm32"))
))]
mod cpal_device;

/// A running output device. Dropping it stops the stream and drops the mixer
/// (and with it every clip the mixer still held) on the thread that opened it.
pub trait AudioSink {
    /// Pauses the stream without discarding it, for an app going away.
    fn suspend(&self) {}
    /// Starts the stream again: after [`suspend`](AudioSink::suspend), and
    /// after the mixer gave the device up for want of anything to play.
    fn resume(&self) {}
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

/// Opens the platform output device and starts it.
///
/// Each arm is a separate `cfg` block, so the explicit returns are what keeps
/// exactly one of them live per target instead of one expression with three
/// conditional halves.
#[allow(clippy::needless_return)]
pub fn open(seed: MixerSeed) -> Result<Box<dyn AudioSink>, AudioError> {
    #[cfg(all(feature = "aaudio", target_os = "android"))]
    {
        return aaudio::open(seed);
    }

    #[cfg(all(
        feature = "cpal-backend",
        not(any(target_os = "android", target_arch = "wasm32"))
    ))]
    {
        return cpal_device::open(seed);
    }

    #[cfg(not(any(
        all(feature = "aaudio", target_os = "android"),
        all(
            feature = "cpal-backend",
            not(any(target_os = "android", target_arch = "wasm32"))
        )
    )))]
    {
        drop(seed);
        Err(AudioError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A build with no device compiled in must say so rather than pretend.
    /// Builds that do have one are not exercised here: opening a real stream
    /// belongs on a device, not in a unit test on a headless machine.
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
        assert!(matches!(open(seed), Err(AudioError::Unsupported)));
    }
}

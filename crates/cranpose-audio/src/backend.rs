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
    /// Pauses the stream without discarding it.
    fn suspend(&self) {}
    /// Restarts a suspended stream.
    fn resume(&self) {}
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
        };
        assert!(matches!(open(seed), Err(AudioError::Unsupported)));
    }
}

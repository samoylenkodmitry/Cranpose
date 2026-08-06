#![deny(unsafe_code)]
#![deny(missing_docs)]

//! The real-time audio engine behind [`cranpose_services::audio`].
//!
//! `cranpose-services` defines the Compose-shaped API — [`AudioPlayer`], the
//! [`SoundId`] handle, `ProvideAudio`, `rememberSoundBank` — and ships a no-op
//! default. This crate is the implementation an app installs when it wants
//! sound: a software mixer on the platform's real-time thread, fed through a
//! lock-free queue from the UI thread.
//!
//! ```rust,ignore
//! // Once, at startup, before the first composition.
//! cranpose_audio::install();
//! ```
//!
//! # What runs where
//!
//! | Thread | Work |
//! | --- | --- |
//! | UI | decode, clip and voice handle bookkeeping, one queue push per call |
//! | Audio (real-time) | drain the queue, resample, mix, clamp |
//!
//! The audio callback allocates nothing, locks nothing and logs nothing. Clips
//! reach it as `Arc<[f32]>` inside a command; clips it drops travel back over a
//! second queue so the deallocation happens on the UI thread.
//!
//! # Devices
//!
//! * Android and Wear OS: AAudio through the `ndk` crate (`aaudio` feature, on
//!   by default). No Java glue and no C++ toolchain.
//! * Desktop: `cpal` (`cpal-backend` feature, off by default because it links
//!   a system audio library).
//! * Anything else: [`AudioError::Unsupported`], and the service falls back to
//!   the no-op player so the app still runs.

mod backend;
mod engine;
mod mixer;
mod ring;

pub use engine::AudioEngine;
pub use mixer::{MAX_CLIPS, MAX_VOICES};

use cranpose_services::{set_platform_audio, AudioPlayerRef};
use std::rc::Rc;

/// Creates an engine without registering it, for an app that wants to hold the
/// handle itself.
pub fn create() -> Rc<AudioEngine> {
    Rc::new(AudioEngine::new())
}

/// Creates an engine and installs it as the platform audio player.
///
/// Call once at startup, on the thread that runs the composition. The output
/// device opens on the first sound, not here, so installing the engine in an
/// app that never plays anything costs nothing.
pub fn install() -> AudioPlayerRef {
    let engine: AudioPlayerRef = create();
    set_platform_audio(Rc::clone(&engine));
    engine
}

/// Whether this build has a real output device compiled in. `false` means
/// [`install`] registers an engine that will report
/// [`AudioError::Unsupported`](cranpose_services::AudioError::Unsupported).
pub fn has_device_backend() -> bool {
    backend::is_compiled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_services::{clear_platform_audio, default_audio};

    #[test]
    fn install_registers_the_engine_as_the_platform_player() {
        clear_platform_audio();
        assert!(!default_audio().is_available());
        install();
        assert_eq!(default_audio().is_available(), has_device_backend());
        clear_platform_audio();
        assert!(!default_audio().is_available());
    }

    #[test]
    fn create_does_not_register_anything() {
        clear_platform_audio();
        let engine = create();
        assert!(!engine.is_running());
        assert!(!default_audio().is_available());
    }
}

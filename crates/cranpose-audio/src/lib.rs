#![deny(missing_docs)]

//! The real-time audio engine behind [`cranpose_services::audio`].
//!
//! `cranpose-services` defines the Compose-shaped API — `AudioPlayer`, the
//! `SoundId` handle, `ProvideAudio`, `rememberSoundBank` — and ships a no-op
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
//! # When the device is open
//!
//! Only while there is sound to make. The output device opens on the first
//! [`play`](cranpose_services::AudioPlayer::play), not on
//! [`install`] and not on
//! [`load_clip`](cranpose_services::AudioPlayer::load_clip) — a clip load is a
//! queue push, and the queue exists before any mixer does — and the mixer gives
//! the stream up again once nothing has sounded for a couple of seconds. A
//! silent screen therefore costs no audio thread and no output route, whether
//! it is the first screen or one reached after an hour of play.
//!
//! # Devices
//!
//! * Android and Wear OS: AAudio through the `ndk` crate (`aaudio` feature, on
//!   by default). No Java glue and no C++ toolchain.
//! * Desktop: `cpal` (`cpal-backend` feature, off by default because it links
//!   a system audio library).
//! * Anything else: `AudioError::Unsupported`, and the service falls back to
//!   the no-op player so the app still runs.

pub mod backend;
mod engine;
mod mixer;
pub mod ring;

use std::sync::{Arc, OnceLock};

use cranpose_services::{set_platform_audio, AudioPlayerRef};
pub use engine::AudioEngine;
pub use mixer::{RenderStatus, IDLE_GRACE_SECONDS, MAX_CLIPS, MAX_VOICES};

static INSTALLED_ENGINE: OnceLock<Arc<AudioEngine>> = OnceLock::new();

/// Creates an engine without registering it, for an app that wants to hold the
/// handle itself.
pub fn create() -> Arc<AudioEngine> {
    Arc::new(AudioEngine::new())
}

/// Creates an engine and installs it as the platform audio player.
///
/// Call once at startup, on the thread that runs the composition. The output
/// device opens on the first sound, not here and not when clips are loaded, so
/// installing the engine in an app that never plays anything costs nothing.
pub fn install() -> AudioPlayerRef {
    let engine: AudioPlayerRef = INSTALLED_ENGINE.get_or_init(create).clone();
    set_platform_audio(Arc::clone(&engine));
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
    use cranpose_services::{clear_platform_audio, default_audio};
    use parking_lot::{Mutex, MutexGuard};

    use super::*;

    fn platform_audio_guard() -> MutexGuard<'static, ()> {
        static PLATFORM_AUDIO_LOCK: Mutex<()> = Mutex::new(());
        PLATFORM_AUDIO_LOCK.lock()
    }

    #[test]
    fn install_registers_the_engine_as_the_platform_player() {
        let _guard = platform_audio_guard();
        clear_platform_audio();
        assert!(!default_audio().is_available());
        install();
        assert_eq!(default_audio().is_available(), has_device_backend());
        clear_platform_audio();
        assert!(!default_audio().is_available());
    }

    #[test]
    fn create_does_not_register_anything() {
        let _guard = platform_audio_guard();
        clear_platform_audio();
        let engine = create();
        assert!(!engine.is_running());
        assert!(!default_audio().is_available());
    }

    #[test]
    fn install_reuses_the_registered_engine() {
        let _guard = platform_audio_guard();
        let first = install();
        let second = install();

        assert!(Arc::ptr_eq(&first, &second));
    }
}

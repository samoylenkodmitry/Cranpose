#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod backend;
mod engine;
mod mixer;
pub mod ring;

use std::sync::{Arc, OnceLock};

use cranpose_services::{AudioPlayerRef, set_platform_audio};
pub use engine::AudioEngine;
pub use mixer::{IDLE_GRACE_SECONDS, MAX_CLIPS, MAX_VOICES, RenderStatus};

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
#[path = "tests/audio_tests.rs"]
mod tests;

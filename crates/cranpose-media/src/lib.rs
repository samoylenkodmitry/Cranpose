#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

#[cfg(not(target_arch = "wasm32"))]
mod analysis;
#[cfg(not(target_arch = "wasm32"))]
mod decode;
#[cfg(not(target_arch = "wasm32"))]
mod equalizer;
#[cfg(not(target_arch = "wasm32"))]
mod http;
#[cfg(not(target_arch = "wasm32"))]
mod player;
#[cfg(not(target_arch = "wasm32"))]
mod sink;
#[cfg(not(target_arch = "wasm32"))]
mod source;
#[cfg(not(target_arch = "wasm32"))]
mod spool;

pub use cranpose_services::media::{path_from_uri, uri_for_path};
#[cfg(not(target_arch = "wasm32"))]
pub use player::{OutputFactory, SoftwareMediaPlayer};

/// Whether this build can decode media in process.
///
/// False on the web, which uses the browser's media stack.
pub fn is_supported() -> bool {
    cfg!(not(target_arch = "wasm32"))
}

/// Installs the in-process media player as the platform media player.
///
/// Does nothing on the targets that have their own backend, so calling it
/// unconditionally at startup is correct. Returns whether a backend was
/// installed.
///
/// Android is one of those targets, even though it decodes with this crate: the
/// player it installs is this one wrapped in the media session and the audio
/// focus that only the platform layer can provide, so installing the bare one
/// over it would cost an app its lock screen.
///
/// The output device is opened when an item is opened, not here: installing the
/// backend in an application that never plays anything costs nothing.
pub fn install() -> bool {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        cranpose_services::set_platform_media_player(std::sync::Arc::new(
            SoftwareMediaPlayer::new(),
        ));
        true
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
    {
        false
    }
}

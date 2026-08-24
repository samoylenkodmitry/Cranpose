#![deny(missing_docs)]

//! The desktop media backend behind [`cranpose_services::media`].
//!
//! `cranpose-services` defines the Compose-shaped API — `MediaPlayer`, the
//! observable `PlaybackState`, the audio-focus policy, the media-session
//! commands — and ships nothing that makes sound. On Android, iOS and the web
//! the platform already has a media stack and the `cranpose` crate registers a
//! backend for it. On desktop there is no such stack, so this crate is it:
//! `symphonia` for the decoders and `cpal` for the output device, fed through
//! the same wait-free ring the audio engine uses.
//!
//! ```rust,ignore
//! // Once, at startup, before the first composition.
//! cranpose_media::install();
//! ```
//!
//! # What it plays
//!
//! Local files, addressed as `file:` URIs — see [`uri_for_path`] — in every
//! container `symphonia` reads: MP3, AAC/MP4, FLAC, Vorbis, WAV, AIFF, ALAC.
//! Network URIs are refused with
//! [`MediaError::UnsupportedSource`](cranpose_services::MediaError::UnsupportedSource)
//! rather than downloaded first, because a media player that reads a whole
//! stream into memory before making a sound is not a media player.
//!
//! # Analysis samples
//!
//! Off until [`set_media_analysis_enabled`](cranpose_services::set_media_analysis_enabled)
//! asks for them, and taken from the samples on their way to the device, so a
//! visualiser draws what is actually being heard. The tap runs inside the
//! output callback: it allocates nothing, never blocks, and drops a block
//! rather than making the device wait — see
//! [`dropped_media_samples`](cranpose_services::dropped_media_samples).
//!
//! # Equalizer
//!
//! Ten peaking biquads per channel on the octave centres from 31 Hz to 16 kHz,
//! plus a preamp, applied by
//! [`set_media_equalizer`](cranpose_services::set_media_equalizer). The filters
//! run in the same source chain, and a curve applied mid-item takes effect
//! without interrupting it.

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod analysis;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod decode;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod desktop;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod equalizer;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod sink;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod source;

/// The `file:` URI helpers the media contract owns, re-exported so an
/// application that installs this backend does not have to name two crates to
/// build an item from a path.
pub use cranpose_services::media::{path_from_uri, uri_for_path};
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub use desktop::DesktopMediaPlayer;

/// Whether this build has a desktop media backend compiled in.
///
/// `false` on the targets whose own platform media stack the `cranpose` crate
/// registers instead — Android, iOS and the web — so an application can install
/// unconditionally and let each target use the right one.
pub fn is_supported() -> bool {
    cfg!(not(any(
        target_arch = "wasm32",
        target_os = "android",
        target_os = "ios"
    )))
}

/// Installs the desktop media player as the platform media player.
///
/// Does nothing on the targets that have their own backend, so calling it
/// unconditionally at startup is correct. Returns whether a backend was
/// installed.
///
/// The output device is opened when an item is opened, not here: installing the
/// backend in an application that never plays anything costs nothing.
pub fn install() -> bool {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        cranpose_services::set_platform_media_player(
            std::sync::Arc::new(DesktopMediaPlayer::new()),
        );
        true
    }
    #[cfg(any(target_arch = "wasm32", target_os = "android", target_os = "ios"))]
    {
        false
    }
}

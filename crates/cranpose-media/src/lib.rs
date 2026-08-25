#![deny(missing_docs)]

//! The in-process media backend behind [`cranpose_services::media`].
//!
//! `cranpose-services` defines the Compose-shaped API — `MediaPlayer`, the
//! observable `PlaybackState`, the audio-focus policy, the media-session
//! commands — and ships nothing that makes sound. This crate is what makes it:
//! `symphonia` for the decoders and [`cranpose_audio::backend`] for the output
//! device, fed through the same wait-free ring the audio engine uses.
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
//! Anything else is opened by the platform through
//! [`open_media_source`](cranpose_services::open_media_source): on Android that
//! is a `content://` document, which a provider backed by a network share hands
//! over as a pipe rather than a file. Such a stream is spooled to the
//! application's cache as it arrives, so playback starts at the front while the
//! rest is still coming and a seek waits only for the offset it needs. A URI no
//! platform claims is refused with
//! [`MediaError::UnsupportedSource`](cranpose_services::MediaError::UnsupportedSource)
//! rather than downloaded whole first, because a media player that reads an
//! entire stream into memory before making a sound is not a media player.
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

#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod analysis;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod decode;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod equalizer;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod player;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod sink;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod source;
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
mod spool;

/// The `file:` URI helpers the media contract owns, re-exported so an
/// application that installs this backend does not have to name two crates to
/// build an item from a path.
pub use cranpose_services::media::{path_from_uri, uri_for_path};
#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
pub use player::SoftwareMediaPlayer;

/// Whether this build can decode media in process.
///
/// `false` on the web and on iOS, which have a platform media stack the
/// `cranpose` crate registers instead.
pub fn is_supported() -> bool {
    cfg!(not(any(target_arch = "wasm32", target_os = "ios")))
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

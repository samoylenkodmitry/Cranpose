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
//!
//! Remote items, addressed as `http:` or `https:` URIs, in those same
//! containers. The decoder reads them over HTTP byte ranges: playback starts
//! from the front while the rest is still on the wire, a seek asks the server
//! for the offset it needs instead of waiting for the bytes in between, and
//! nothing is written to disk. A server that answers `Accept-Ranges: bytes`
//! seeks; one that does not plays forward and refuses to seek, which is what
//! [`SoftwareMediaPlayer`] reports rather than pretending otherwise.
//!
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
//! visualiser follows decoded audio after equalization. The tap runs on the
//! decoder thread before volume and balance, allocates no per-sample storage,
//! and drops a block rather than waiting for an observer — see
//! [`dropped_media_samples`](cranpose_services::dropped_media_samples).
//!
//! # Equalizer
//!
//! Ten peaking biquads per channel on the octave centres from 31 Hz to 16 kHz,
//! plus a preamp, applied by
//! [`set_media_equalizer`](cranpose_services::set_media_equalizer). The filters
//! run in the same source chain, and a curve applied mid-item takes effect
//! without interrupting it.

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

/// The `file:` URI helpers the media contract owns, re-exported so an
/// application that installs this backend does not have to name two crates to
/// build an item from a path.
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

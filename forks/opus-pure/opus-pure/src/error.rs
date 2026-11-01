//! The crate's error type.

use std::fmt;

/// Anything that can go wrong encoding, decoding, or parsing an Opus stream.
///
/// The enum is `#[non_exhaustive]`, so match with a `_` arm: new variants are
/// not a breaking change. The individual variants are not, deliberately —
/// marking a tuple variant `#[non_exhaustive]` stops callers destructuring it
/// at all, which would take away the message rather than protect it.
/// [`BufferTooSmall`](Self::BufferTooSmall) is the exception, because a struct
/// variant can be marked and still be matched with `..`.
///
/// Variants are grouped by who is at fault: [`Error::InvalidArgument`] and
/// [`Error::BufferTooSmall`] mean the *caller* passed something the codec cannot
/// honour, [`Error::InvalidPacket`] means the *bitstream* is malformed, and
/// [`Error::Internal`] means a codec stage failed in a way that is not
/// attributable to either.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A configuration value is outside the range Opus allows — an unsupported
    /// sample rate or channel count, a frame size no coding mode can carry, or a
    /// mapping family that does not accept the requested layout.
    InvalidArgument(&'static str),

    /// The caller's output slice cannot hold the result. Both sizes are in the
    /// unit of the call: bytes for `encode`, samples-per-channel for `decode`.
    #[non_exhaustive]
    BufferTooSmall {
        /// What the operation needs.
        needed: usize,
        /// What the caller provided.
        provided: usize,
    },

    /// The packet does not decode as Opus: a truncated or self-inconsistent
    /// frame-length field, an impossible frame count, padding that runs off the
    /// end, or a total duration above the 120 ms an Opus packet may carry.
    InvalidPacket(&'static str),

    /// The Ogg container is malformed: a missing capture pattern, an unsupported
    /// page version, a CRC mismatch, or an `OpusHead`/`OpusTags` packet that does
    /// not match RFC 7845.
    InvalidStream(&'static str),

    /// The underlying reader or writer failed.
    Io(std::io::Error),

    /// A codec stage failed. Reaching this from well-formed input is a bug in
    /// this crate; the string names the stage.
    Internal(&'static str),
}

impl Error {
    pub(crate) fn buffer_too_small(needed: usize, provided: usize) -> Self {
        Error::BufferTooSmall { needed, provided }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Error::BufferTooSmall { needed, provided } => {
                write!(f, "buffer too small: need {needed}, got {provided}")
            }
            Error::InvalidPacket(what) => write!(f, "invalid packet: {what}"),
            Error::InvalidStream(what) => write!(f, "invalid ogg stream: {what}"),
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Internal(what) => write!(f, "internal codec error: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Shorthand for a codec result.
pub type Result<T> = std::result::Result<T, Error>;

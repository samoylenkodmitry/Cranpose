//! Ogg encapsulation of Opus streams — RFC 7845.
//!
//! An `.opus` file is an Ogg physical bitstream whose first packet is an
//! [`OpusHead`] identification header, whose second is an [`OpusTags`] comment
//! header, and whose remaining packets are Opus audio. This module both writes
//! ([`OggOpusWriter`]) and reads ([`OggOpusReader`]) that framing; the codec
//! itself lives in [`OpusEncoder`](crate::OpusEncoder) and
//! [`OpusDecoder`](crate::OpusDecoder).
//!
//! # Granule positions and pre-skip
//!
//! Ogg Opus counts time in **48 kHz samples regardless of the encoder's sample
//! rate**. A packet's duration is therefore 960 for a 20 ms frame whether the
//! encoder ran at 16 kHz or 48 kHz, and [`OpusHead::pre_skip`] — the encoder
//! delay to discard from the decoder's output — is likewise at 48 kHz. Getting
//! either wrong yields a file that decodes but seeks and reports its duration
//! incorrectly.
//!
//! The granule positions are also what says where the audio *ends*: Opus codes
//! whole frames, so the last one is padded, and RFC 7845 §4.4 has the final
//! granule claim fewer samples than its packets carry. [`Trim`] applies that
//! and the pre-skip on the way out;
//! [`OggOpusWriter::write_packet_with_duration`] states it on the way in.
//!
//! # Scope
//!
//! One logical bitstream per file. Chained streams (several logical bitstreams
//! concatenated) and multiplexed streams (Opus alongside other codecs) are
//! rejected rather than silently mis-read, because each would need its own
//! decoder state and pre-skip handling.

mod crc;
mod header;
mod page;
mod reader;
mod trim;
mod writer;

pub use header::{GRANULE_RATE, OpusHead, OpusTags};
pub use reader::{OggOpusReader, OggPacket};
pub use trim::Trim;
pub use writer::OggOpusWriter;

#[cfg(test)]
mod tests;

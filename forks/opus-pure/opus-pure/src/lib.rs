//! Pure-Rust Opus audio codec (RFC 6716) with Ogg encapsulation (RFC 7845).
//!
//! Encoder and decoder for all three Opus coding modes — SILK for speech, CELT
//! for music, and the hybrid of both — plus a real Ogg container layer, so this
//! crate reads and writes `.opus` files rather than only raw packets.
//!
//! # Encoding to an `.opus` file
//!
//! ```
//! use opus_pure::{Application, MAX_PACKET_BYTES, OggOpusWriter, OpusEncoder, OpusHead};
//!
//! let (rate, channels, frame) = (48_000, 2, 960); // 20 ms stereo
//! let pcm = vec![0.0f32; frame * channels * 50];  // one second of silence
//!
//! let mut encoder = OpusEncoder::new(rate, channels, Application::Audio)?;
//! encoder.bitrate_bps = 96_000;
//!
//! // The header takes its pre-skip from the encoder's own delay rather than a
//! // constant, which is what makes it right for every `Application`.
//! let head = OpusHead::for_encoder(&encoder, rate as u32);
//! let mut writer = OggOpusWriter::new(Vec::new(), head)?;
//! let mut packet = vec![0u8; MAX_PACKET_BYTES];
//! for block in pcm.chunks_exact(frame * channels) {
//!     let n = encoder.encode(block, frame, &mut packet)?;
//!     writer.write_packet(&packet[..n])?;
//! }
//! let file: Vec<u8> = writer.finish()?;
//! assert_eq!(&file[..4], b"OggS");
//! # Ok::<(), opus_pure::Error>(())
//! ```
//!
//! [`finish`](OggOpusWriter::finish) writes the end-of-stream page and must be
//! called; dropping the writer flushes on a best-effort basis but cannot report
//! an I/O failure.
//!
//! # Integer PCM
//!
//! Both directions have a 16-bit entry point, for the many callers whose audio
//! is already `i16`. They are not wrappers over the float ones, any more than
//! libopus's are: [`encode_s16`](OpusEncoder::encode_s16) declares 16 bits of
//! input precision where [`encode`](OpusEncoder::encode) declares 24, and
//! [`decode_s16`](OpusDecoder::decode_s16) soft-clips before converting, which
//! [`decode`](OpusDecoder::decode) does not. See [`SoftClip`] for why that
//! second one matters and how to get it on the float path.
//!
//! ```
//! use opus_pure::{Application, MAX_PACKET_BYTES, OpusDecoder, OpusEncoder};
//!
//! let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
//! let mut decoder = OpusDecoder::new(48_000, 2)?;
//!
//! let pcm = vec![0i16; 960 * 2];                  // 20 ms of stereo at 48 kHz
//! let mut packet = vec![0u8; MAX_PACKET_BYTES];
//! let n = encoder.encode_s16(&pcm, 960, &mut packet)?;
//!
//! let mut out = vec![0i16; 960 * 2];
//! let samples = decoder.decode_s16(&packet[..n], 960, &mut out)?;
//! assert_eq!(samples, 960);
//! # Ok::<(), opus_pure::Error>(())
//! ```
//!
//! # Decoding one back
//!
//! ```
//! use opus_pure::{Application, MAX_PACKET_BYTES, MAX_PACKET_SAMPLES, OggOpusReader,
//!                OggOpusWriter, OpusEncoder, OpusHead, Trim};
//! # let (rate, channels, frame) = (48_000, 2, 960);
//! # let pcm = vec![0.0f32; frame * channels * 50];
//! # let mut encoder = OpusEncoder::new(rate, channels, Application::Audio)?;
//! # let mut writer = OggOpusWriter::new(Vec::new(), OpusHead::for_encoder(&encoder, 48_000))?;
//! # let mut packet = vec![0u8; MAX_PACKET_BYTES];
//! # for block in pcm.chunks_exact(frame * channels) {
//! #     let n = encoder.encode(block, frame, &mut packet)?;
//! #     writer.write_packet(&packet[..n])?;
//! # }
//! # let file: Vec<u8> = writer.finish()?;
//! let mut reader = OggOpusReader::new(std::io::Cursor::new(&file))?;
//! let head = reader.head().clone();
//! let channels = head.channel_count as usize;
//!
//! // Carries the channel count and the header's output gain.
//! let mut decoder = head.decoder(48_000)?;
//! // Takes the encoder delay off the front and the end-trim off the back.
//! let mut trim = Trim::new(&head, 48_000, channels)?;
//!
//! let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
//! let mut out = Vec::new();
//! for packet in reader.packets() {
//!     let packet = packet?;
//!     let n = decoder.decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)?;
//!     out.extend_from_slice(trim.keep(&packet, &block[..n * channels]));
//! }
//! // One second in, one second back, less the encoder delay that the stream
//! // above never flushed — see below.
//! assert_eq!(trim.samples_emitted(), 48_000 - u64::from(head.pre_skip));
//! # Ok::<(), opus_pure::Error>(())
//! ```
//!
//! # Where a stream begins and ends
//!
//! A decoded Opus stream is longer than the audio that went into it at both
//! ends, and RFC 7845 gives both corrections: the [`pre_skip`](OpusHead::pre_skip)
//! at the front (§4.2, the encoder's algorithmic delay) and an end-trim at the
//! back (§4.4, a final granule position deliberately short of what the packets
//! decode to). [`Trim`] applies the pair, which is worth reaching for even
//! though it is ten lines: the first correction is conspicuous when it is
//! missing and the second is silent, and every file `opusenc` writes carries
//! one.
//!
//! Writing them is the same job in reverse, and it is not automatic:
//! [`OggOpusWriter`] documents the tail arithmetic, and
//! [`write_packet_with_duration`](OggOpusWriter::write_packet_with_duration) is
//! what states the end-trim. The example above writes a whole number of frames
//! and no end-trim, so it comes back one encoder delay short — which is what
//! that arithmetic exists to fix.
//!
//! Build the header with [`OpusHead::for_encoder`] and the pre-skip is measured
//! from the encoder rather than assumed; [`OpusHead::new`] uses the conventional
//! 312, which is four milliseconds too many for
//! [`Application::RestrictedLowDelay`].
//!
//! # Working with raw packets
//!
//! [`OpusEncoder`] and [`OpusDecoder`] are usable on their own when the framing
//! comes from elsewhere (RTP, a custom container). [`Repacketizer`] combines and
//! splits packets, and [`encode_parallel`] encodes a clip across threads by
//! splitting it into chunks — a different encode from the serial one, and
//! [`parallel`] is explicit about how it differs.

// The public surface is a published contract, so both of these are structural
// rather than a convention CI happens to enforce. `missing_docs` because this
// crate's private internals are heavily commented and its public API once was
// not, which is exactly backwards for what docs.rs renders; `missing_debug_
// implementations` because a public type without `Debug` cannot appear in
// anyone else's derived one. Neither reaches private items.
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_range_loop)]

// The README is the first thing anyone reads and the last thing anyone checks,
// so its examples are compiled and run with the rest of the doctests. Nothing
// is rendered from here: this only exists so a change to the API that the
// README describes cannot pass CI while the README still shows the old one.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;

// ---- Public API ----
mod config;
mod decoder;
mod encoder;
mod error;
pub mod multistream;
pub mod ogg;
pub mod packet;
pub mod parallel;
pub mod repacketizer;
mod soft_clip;

// ---- Codec internals (no semver contract) ----
mod analysis;
mod analysis_data;
mod celt;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod cpu_features;
mod hp_cutoff;
mod range_coder;
mod silk;
mod toc;

/// Internal measurement hooks for the harnesses in [`reference/`][ref]. Not
/// public API: what this exposes can change or disappear without a version
/// bump. Compiled only under the non-default `probe` feature.
///
/// [ref]: https://github.com/stephenberry/opus-pure/tree/main/reference
#[cfg(feature = "probe")]
pub mod probe {
    /// CELT's band edges, in units of 200 Hz — RFC 6716 §4.3.1's `eband5ms`.
    ///
    /// A 2.5 ms MDCT at 48 kHz has 120 bins over 24 kHz, so one unit is 200 Hz
    /// and the last edge, 100, is the 20 kHz top of fullband. A harness that
    /// reports a result per band has to use the codec's own band layout rather
    /// than a copy of it: three tools in `reference/` once carried private
    /// copies of the test signal generators and spent months measuring audio no
    /// test encoded.
    pub const CELT_BAND_EDGES_200HZ: [i16; 22] = crate::celt::modes::EBAND_5MS;
}

pub use config::{Application, Bandwidth, OpusMode, RateControl, Signal};
pub use decoder::OpusDecoder;
pub use encoder::{MAX_PACKET_BYTES, OpusEncoder};
pub use error::{Error, Result};
pub use multistream::{ChannelLayout, OpusMSDecoder, OpusMSEncoder};
pub use ogg::{OggOpusReader, OggOpusWriter, OggPacket, OpusHead, OpusTags, Trim};
pub use packet::MAX_PACKET_SAMPLES;
pub use parallel::{DEFAULT_WARMUP_MS, ParallelConfig, ParallelPlan, encode_parallel};
pub use repacketizer::Repacketizer;
pub use soft_clip::SoftClip;

#[cfg(test)]
mod integration_tests {
    use crate::config::OpusMode;
    use crate::toc::{
        channels_from_toc, frame_duration_ms_from_toc, frame_rate_from_params, gen_toc,
        mode_from_toc,
    };
    use crate::{Application, Bandwidth, OpusDecoder, OpusEncoder, RateControl};

    fn frame_size_from_toc(toc: u8, sampling_rate: i32) -> Option<usize> {
        let mode = mode_from_toc(toc);
        match mode {
            OpusMode::CeltOnly => {
                let period = ((toc >> 3) & 0x03) as i32;
                let frame_rate = 400 >> period;
                if frame_rate == 0 || sampling_rate % frame_rate != 0 {
                    return None;
                }
                Some((sampling_rate / frame_rate) as usize)
            }
            OpusMode::SilkOnly => {
                let duration_ms = frame_duration_ms_from_toc(toc);
                Some((sampling_rate as i64 * duration_ms as i64 / 1000) as usize)
            }
            OpusMode::Hybrid => {
                let duration_ms = frame_duration_ms_from_toc(toc);
                Some((sampling_rate as i64 * duration_ms as i64 / 1000) as usize)
            }
        }
    }

    #[test]
    fn gen_toc_matches_celt_reference_values() {
        let sampling_rate = 48_000;
        let cases = [
            (120usize, 0xE0u8),
            (240usize, 0xE8u8),
            (480usize, 0xF0u8),
            (960usize, 0xF8u8),
        ];

        for (frame_size, expected_toc) in cases {
            let frame_rate = frame_rate_from_params(sampling_rate, frame_size).unwrap();
            let toc = gen_toc(OpusMode::CeltOnly, frame_rate, Bandwidth::Fullband, 1);
            assert_eq!(
                toc, expected_toc,
                "frame_size {} expected TOC {:02X} got {:02X}",
                frame_size, expected_toc, toc
            );
            let decoded_size = frame_size_from_toc(toc, sampling_rate).unwrap();
            assert_eq!(decoded_size, frame_size);
        }

        let stereo_toc = gen_toc(
            OpusMode::CeltOnly,
            frame_rate_from_params(sampling_rate, 960).unwrap(),
            Bandwidth::Fullband,
            2,
        );
        assert_eq!(channels_from_toc(stereo_toc), 2);
    }

    /// The SILK TOC configurations, including the 60 ms one that no sample rate
    /// divides evenly.
    ///
    /// `gen_toc` finds the duration by doubling the frame rate until it reaches
    /// 400, which needs `frame_rate_from_params` to hand it the *truncated*
    /// 48000/2880 = 16 rather than a rounded 17: 16 doubles to 512 in five
    /// steps and lands on config 3, and 17 lands on the same config only by
    /// accident at 48 kHz and on the wrong one at 8 kHz.
    #[test]
    fn gen_toc_covers_every_silk_duration() {
        for &(rate, ms, frame_size) in &[
            (48_000i32, 10i32, 480usize),
            (48_000, 20, 960),
            (48_000, 40, 1920),
            (48_000, 60, 2880),
            (8_000, 10, 80),
            (8_000, 20, 160),
            (8_000, 40, 320),
            (8_000, 60, 480),
        ] {
            let frame_rate = frame_rate_from_params(rate, frame_size)
                .unwrap_or_else(|| panic!("{rate} Hz / {ms} ms rejected"));
            let toc = gen_toc(OpusMode::SilkOnly, frame_rate, Bandwidth::Wideband, 1);
            assert_eq!(
                frame_duration_ms_from_toc(toc),
                ms,
                "{rate} Hz / {ms} ms produced TOC {toc:02X}"
            );
            assert_eq!(mode_from_toc(toc), OpusMode::SilkOnly);
            assert_eq!(frame_size_from_toc(toc, rate).unwrap(), frame_size);
        }
    }

    /// `frame_rate_from_params` answers for one coded *frame*, so the durations
    /// Opus can only express by packing several frames into one packet have no
    /// answer here. The encoder reaches them through `PacketDuration::layout`,
    /// which splits them into frames this function does recognise.
    #[test]
    fn frame_rate_rejects_durations_that_need_multi_frame_packets() {
        for &(rate, frame_size) in &[
            (48_000i32, 3840usize), // 80 ms
            (48_000, 4800),         // 100 ms
            (48_000, 5760),         // 120 ms
            (16_000, 1280),         // 80 ms
            (8_000, 640),           // 80 ms
        ] {
            assert!(
                frame_rate_from_params(rate, frame_size).is_none(),
                "{rate} Hz / {frame_size} samples was accepted"
            );
        }
        // And nothing that is not a frame duration at all.
        assert!(frame_rate_from_params(48_000, 0).is_none());
        assert!(frame_rate_from_params(48_000, 333).is_none());
        assert!(frame_rate_from_params(48_000, usize::MAX).is_none());
    }

    #[test]
    fn test_celt_decoder_large_frame_sizes() {
        let sampling_rate = 48000;
        let channels = 1;

        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        let frame_sizes = [120, 240, 480, 960];

        for frame_size in frame_sizes {
            let toc = gen_toc(
                OpusMode::CeltOnly,
                frame_rate_from_params(sampling_rate, frame_size).unwrap(),
                Bandwidth::Fullband,
                channels,
            );
            let packet = [toc, 0, 0, 0, 0];

            let mut output = vec![0.0f32; frame_size * channels];

            let _ = decoder.decode(&packet, frame_size, &mut output);
        }

        let channels = 2;
        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        for frame_size in frame_sizes {
            let toc = gen_toc(
                OpusMode::CeltOnly,
                frame_rate_from_params(sampling_rate, frame_size).unwrap(),
                Bandwidth::Fullband,
                channels,
            );
            let packet = [toc, 0, 0, 0, 0];

            let mut output = vec![0.0f32; frame_size * channels];
            let _ = decoder.decode(&packet, frame_size, &mut output);
        }
    }

    #[test]
    fn test_celt_decoder_edge_case_frame_sizes() {
        let sampling_rate = 48000;
        let channels = 1;
        let mut decoder = OpusDecoder::new(sampling_rate, channels).unwrap();

        let edge_sizes = [2048, 2167, 2168, 2169, 2880, 3072];

        for frame_size in edge_sizes {
            let mut output = vec![0.0f32; frame_size * channels];

            let _ = decoder.decode(&[0x80, 0, 0, 0], frame_size, &mut output);
        }
    }

    // Regression test for: "index out of bounds: the len is 48 but the index is 119"
    // Root cause: frame_size=48 at 48kHz gives frame_rate=1000, which is not a valid
    // Hybrid-mode frame rate but was not validated.  CELT's lm-search then silently
    // fell back to lm=0, computed n2=120, and wrote output[119] into a 48-element
    // slice.  Triggered via G.729-decoded PCM (8kHz) passed to a 48kHz Opus encoder
    // without proper resampling, so the encoder received 48 samples instead of 480.
    #[test]
    fn test_invalid_small_frame_size_returns_error_not_panic() {
        let mut enc = OpusEncoder::new(48000, 2, Application::Voip).unwrap();
        enc.bitrate_bps = 64000;
        enc.complexity = 5;
        enc.rate_control = RateControl::Cbr;

        // 48 samples at 48kHz = 1ms → frame_rate=1000, invalid for Hybrid mode.
        let input = vec![0.0f32; 48 * 2]; // stereo interleaved
        let mut output = vec![0u8; 256];

        let result = enc.encode(&input, 48, &mut output);
        assert!(
            result.is_err(),
            "encode with invalid frame_size=48 should return Err, not panic"
        );
    }

    // Also verify that the Audio application path (always Hybrid at 48 kHz) rejects
    // the same bad frame size.
    #[test]
    fn test_invalid_small_frame_size_audio_application_returns_error() {
        let mut enc = OpusEncoder::new(48000, 1, Application::Audio).unwrap();
        let input = vec![0.0f32; 48];
        let mut output = vec![0u8; 256];

        let result = enc.encode(&input, 48, &mut output);
        assert!(
            result.is_err(),
            "Audio/48kHz encoder with frame_size=48 should return Err"
        );
    }
}

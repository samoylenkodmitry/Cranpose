//! The decoder's output, pinned against libopus 1.6.1's.
//!
//! `reference_vectors.rs` pins the *encoder* byte-exactly against the C
//! reference. Nothing pinned the decoder, and a defect lived there because of
//! it: `render_silk_frame` skipped the SILK resampler whenever the API rate
//! already matched SILK's internal rate, which also skipped the resampler's
//! `delay_matrix_dec` input delay and put every SILK-only stream 4, 9 or 12
//! samples ahead of the reference decoder. Every existing test measured the
//! decoder against the encoder's *input* with a lag search, so all of them
//! aligned the error away.
//!
//! On the SILK path this crate's decoder is bit-exact with libopus, so the
//! frozen values below are hashes of libopus's own output, not of ours.
//!
//! ## The 2026-08-22 re-freeze
//!
//! Every value moved, and the encoder did not. `speech_like` and `music_like`
//! were built with `f32::sin` and `f32::powf`, which call the platform's libm,
//! and Apple's arm64 and x86_64 implementations differ in the last bits. The
//! input to this test was therefore a slightly different signal on each
//! architecture. SILK's quantiser absorbed that in five of the six
//! configurations; at 12 kHz stereo it did not, so `mb stereo` failed on x86_64
//! while the whole file passed on aarch64. The five that held were holding by
//! luck, not by construction.
//!
//! `tests/common/sin_turns` replaced those libm calls with a polynomial built
//! from correctly-rounded operations alone, `tests/test_signals.rs` pins the
//! signals it produces, and the values below were regenerated from libopus
//! 1.6.1 against the packets those signals now yield. All six configurations
//! are bit-identical on aarch64 and x86_64.
//!
//! ## Regenerating
//!
//! See `reference/plc/README.md`, or run `reference/verify.py`, which re-derives
//! every value here in one command. In short: dump the packets these
//! configurations produce, decode them with `opus_decode_float`, and hash the
//! result the same way `pcm_hash` does — with the same packets dropped for
//! `FROZEN_PLC`, and with the decoder's channel count set to 1 for
//! `FROZEN_DOWNMIX`. Only do that when the *encoder* has deliberately changed;
//! a decoder change that moves these hashes is the thing this file exists to
//! catch.

mod common;
use common::*;
use opus_pure::Application;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Hash decoded audio as the i16 samples it is exactly representable as, so the
/// value does not depend on float formatting.
fn pcm_hash(pcm: &[f32]) -> u64 {
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for &s in pcm {
        bytes.extend_from_slice(&((s * 32768.0).round() as i32 as i16).to_le_bytes());
    }
    fnv1a(&bytes)
}

/// Configurations whose whole packet is SILK, where this crate is bit-exact
/// with libopus. Hybrid and CELT decode through float paths that agree with the
/// C decoder to 139-157 dB but not to the bit, so they are not frozen here;
/// `bitstream_conformance.rs` gates their entropy layer instead.
///
/// `packets` guards the input to the decoder: if the encoder changes on purpose
/// that hash moves first, and the failure says so rather than blaming the
/// decoder. `pcm` is the hash of **libopus 1.6.1's** decode of those packets.
struct Frozen {
    rate: i32,
    channels: usize,
    bitrate: i32,
    label: &'static str,
    packets: u64,
    pcm: u64,
}

// A data table: kept aligned by hand, one configuration per pair of lines.
#[rustfmt::skip]
const FROZEN: &[Frozen] = &[
    Frozen { rate: 8_000,  channels: 1, bitrate: 12_000, label: "nb mono",
             packets: 0xea00_5aeb_1f63_7db5, pcm: 0x1350_2545_9e9d_0151 },
    Frozen { rate: 8_000,  channels: 2, bitrate: 16_000, label: "nb stereo",
             packets: 0x4aa2_1e97_2cf4_fc25, pcm: 0xd56e_cb68_4b29_8593 },
    Frozen { rate: 12_000, channels: 1, bitrate: 14_000, label: "mb mono",
             packets: 0x84d7_97b4_fc7a_029f, pcm: 0xe50d_400b_e8fc_1cec },
    Frozen { rate: 12_000, channels: 2, bitrate: 20_000, label: "mb stereo",
             packets: 0xe948_3f31_f5ea_890d, pcm: 0x3f9b_ce6a_4a67_1ed5 },
    Frozen { rate: 16_000, channels: 1, bitrate: 24_000, label: "wb mono",
             packets: 0x0663_41f0_d7c9_3719, pcm: 0x6a32_f1e4_bbe9_8e61 },
    Frozen { rate: 16_000, channels: 2, bitrate: 32_000, label: "wb stereo",
             packets: 0x39a0_a6eb_a30e_c576, pcm: 0x1036_f681_cc2e_0462 },
];

const FRAMES: usize = 40;

/// The same six configurations, decoded with packets dropped. `pcm` is the hash
/// of libopus 1.6.1's decode of the *same* packets with the *same* drops, so
/// these pin the concealment itself and the recovery after it, not just the
/// clean path.
///
/// Three defects lived here, all of them invisible in the concealed frame and
/// visible only in the frames after it. `BWE_AFTER_LOSS_Q16` was 64738 where
/// libopus has 63570, so the first good frame's LPC coefficients were expanded
/// by 0.988 instead of 0.97. `silk_Decode` re-anchors the gain dequantiser
/// (`LastGainIndex = 10`) after a concealed packet and this decoder did not, so
/// a stale index could hold the recovered frame's gain up. And concealment ran
/// before any packet had been decoded, where libopus returns silence and touches
/// no state at all, which left `lossCnt` and the output history a frame ahead.
///
/// `lost: &[0]` is that last case: the loss arrives before there is anything to
/// conceal from.
struct FrozenPlc {
    rate: i32,
    channels: usize,
    bitrate: i32,
    label: &'static str,
    lost: &'static [usize],
    pcm: u64,
}

#[rustfmt::skip]
const FROZEN_PLC: &[FrozenPlc] = &[
    FrozenPlc { rate: 8_000,  channels: 1, bitrate: 12_000, label: "nb mono",   lost: &[20],
                pcm: 0x7179_2013_1e5d_523b },
    FrozenPlc { rate: 8_000,  channels: 2, bitrate: 16_000, label: "nb stereo", lost: &[20],
                pcm: 0x4e9d_3008_9ac3_efce },
    FrozenPlc { rate: 12_000, channels: 1, bitrate: 14_000, label: "mb mono",   lost: &[20],
                pcm: 0x18f0_14d3_9fdf_ee4d },
    FrozenPlc { rate: 12_000, channels: 2, bitrate: 20_000, label: "mb stereo", lost: &[20],
                pcm: 0xfde5_411d_19d3_d9c5 },
    FrozenPlc { rate: 16_000, channels: 1, bitrate: 24_000, label: "wb mono",   lost: &[20],
                pcm: 0x6692_fb09_20a4_e7a5 },
    FrozenPlc { rate: 16_000, channels: 2, bitrate: 32_000, label: "wb stereo", lost: &[20],
                pcm: 0x7697_99df_c6d6_f77b },
    FrozenPlc { rate: 16_000, channels: 1, bitrate: 24_000, label: "wb mono burst",
                lost: &[10, 11, 12], pcm: 0x3983_73b2_6b79_723b },
    FrozenPlc { rate: 16_000, channels: 2, bitrate: 32_000, label: "wb stereo burst",
                lost: &[10, 11, 12], pcm: 0xbdc0_0191_c638_1536 },
    FrozenPlc { rate: 16_000, channels: 1, bitrate: 24_000, label: "wb mono first packet lost",
                lost: &[0], pcm: 0xe569_4339_993b_c653 },
];

/// The stereo configurations of `FROZEN`, decoded to a **mono** output.
///
/// Rendering a stereo stream to one channel is not a post-pass over a stereo
/// decode. libopus merges the channels *upstream* of synthesis, in each layer:
/// SILK emits the mid, which is `(L+R)/2` by construction and is never expanded
/// to L/R at all, and CELT sums the two denormalised spectra before a single
/// inverse MDCT. This crate used to decode the packet through a second, complete
/// decoder and average its two output channels, and failed 6 of 12 official
/// vectors at mono output that libopus passes, with the range coder in step for
/// all 20,075 packets.
///
/// What these rows pin is the *steady-state* half of that: the streams here
/// never change channel count, and on such a stream the old and new decodes
/// differ by around 106 dB — SILK's mid against an average of reconstructed
/// L/R, plus float rounding, since everything from the inverse MDCT to the
/// output is linear. The larger half was that two decoders meant neither saw
/// the whole stream, so both resumed from stale history at every mono/stereo
/// switch; that needs a switching stream to see, which is what the vector suite
/// has and CI does not (`reference/vectors/run.sh`).
///
/// Nothing else here covers either half — every other test in the repository
/// decodes at the stream's own channel count. `pcm` is the hash of libopus
/// 1.6.1 decoding these same stereo packets through a mono decoder.
///
/// `lost` is a second reason this table exists rather than three more rows in
/// `FROZEN`: concealing a stereo stream into a mono output is its own path
/// again. The side channel is still extrapolated, because the SILK decoder is
/// internally stereo, but the reconstruction that would turn mid and side back
/// into L/R never runs.
struct FrozenDownmix {
    label: &'static str,
    lost: &'static [usize],
    pcm: u64,
}

#[rustfmt::skip]
const FROZEN_DOWNMIX: &[FrozenDownmix] = &[
    FrozenDownmix { label: "nb stereo", lost: &[],   pcm: 0x5d9b_8420_f779_8143 },
    FrozenDownmix { label: "mb stereo", lost: &[],   pcm: 0x79fb_ee68_3183_271d },
    FrozenDownmix { label: "wb stereo", lost: &[],   pcm: 0x6c2f_e6b3_0ad1_df7d },
    FrozenDownmix { label: "nb stereo", lost: &[20], pcm: 0x9f0d_7bb7_1b2a_4df5 },
    FrozenDownmix { label: "mb stereo", lost: &[20], pcm: 0x3a1f_7f58_a5a2_7860 },
    FrozenDownmix { label: "wb stereo", lost: &[20], pcm: 0x5ed3_018e_7580_c73e },
];

/// The packets these two tests decode, so the concealment test can reuse the
/// clean test's `packets` guard rather than repeating it.
fn encode(rate: i32, channels: usize, bitrate: i32) -> Vec<Vec<u8>> {
    let frame = (rate / 50) as usize;
    let mono = speech_like(rate, frame * FRAMES);
    let pcm = if channels == 2 {
        interleave(&[mono, music_like(rate, frame * FRAMES)])
    } else {
        mono
    };
    Codec::new(rate, channels, Application::Voip)
        .bitrate(bitrate)
        .roundtrip(&pcm)
        .packets
}

/// Concealment and recovery, against libopus decoding the same packets with the
/// same packets dropped.
#[test]
fn silk_concealment_matches_libopus() {
    use opus_pure::OpusDecoder;
    for f in FROZEN_PLC {
        let frame = (f.rate / 50) as usize;
        let packets = encode(f.rate, f.channels, f.bitrate);
        let mut dec = OpusDecoder::new(f.rate, f.channels).expect("decoder");
        let mut out = vec![0.0f32; frame * f.channels];
        let mut pcm = Vec::with_capacity(frame * f.channels * packets.len());
        for (i, p) in packets.iter().enumerate() {
            let n = if f.lost.contains(&i) {
                dec.decode(&[], frame, &mut out).expect("conceal")
            } else {
                dec.decode(p, frame, &mut out).expect("decode")
            };
            pcm.extend_from_slice(&out[..n * f.channels]);
        }
        assert_eq!(
            pcm_hash(&pcm),
            f.pcm,
            "{}: concealing {:?} differs from libopus 1.6.1 concealing the same              packets. SILK is fixed point on both sides, so this is exact or it              is wrong.",
            f.label,
            f.lost
        );
    }
}

#[test]
fn silk_decode_matches_libopus() {
    for f in FROZEN {
        let frame = (f.rate / 50) as usize;
        let mono = speech_like(f.rate, frame * FRAMES);
        let pcm = if f.channels == 2 {
            interleave(&[mono, music_like(f.rate, frame * FRAMES)])
        } else {
            mono
        };
        let r = Codec::new(f.rate, f.channels, Application::Voip)
            .bitrate(f.bitrate)
            .roundtrip(&pcm);

        assert!(
            r.modes().iter().all(|m| *m == "silk"),
            "{}: every packet must be SILK for this comparison to mean anything",
            f.label
        );

        let mut blob = Vec::new();
        for p in &r.packets {
            blob.extend_from_slice(&(p.len() as u32).to_le_bytes());
            blob.extend_from_slice(p);
        }
        assert_eq!(
            fnv1a(&blob),
            f.packets,
            "{}: the encoder no longer produces the packets these reference \
             values were generated from — regenerate them (see the module docs) \
             rather than adjusting the decoder to match",
            f.label
        );

        assert_eq!(
            pcm_hash(&r.decoded),
            f.pcm,
            "{}: decoded output differs from libopus 1.6.1's decode of the same \
             packets",
            f.label
        );
    }
}

/// A stereo stream asked for mono output, against libopus doing the same.
#[test]
fn silk_stereo_decoded_to_mono_matches_libopus() {
    use opus_pure::OpusDecoder;
    for d in FROZEN_DOWNMIX {
        let f = FROZEN
            .iter()
            .find(|f| f.label == d.label)
            .unwrap_or_else(|| panic!("{}: no such configuration in FROZEN", d.label));
        assert_eq!(f.channels, 2, "{}: the stream has to be stereo", d.label);

        let frame = (f.rate / 50) as usize;
        let packets = encode(f.rate, f.channels, f.bitrate);
        // One channel out of a two-channel stream.
        let mut dec = OpusDecoder::new(f.rate, 1).expect("decoder");
        let mut out = vec![0.0f32; frame];
        let mut pcm = Vec::with_capacity(frame * packets.len());
        for (i, p) in packets.iter().enumerate() {
            let n = if d.lost.contains(&i) {
                dec.decode(&[], frame, &mut out).expect("conceal")
            } else {
                dec.decode(p, frame, &mut out).expect("decode")
            };
            pcm.extend_from_slice(&out[..n]);
        }
        assert_eq!(
            pcm_hash(&pcm),
            d.pcm,
            "{} losing {:?}: the mono downmix differs from libopus 1.6.1's. \
             SILK is fixed point on both sides, so this is exact or it is wrong.",
            d.label,
            d.lost
        );
    }
}

/// The delay that made the bug visible, asserted directly rather than through a
/// hash, so a regression says *what* moved.
///
/// libopus `silk/resampler.c` carries `delay_matrix_dec`, whose diagonal is
/// 4, 9 and 12 samples for 8, 12 and 16 kHz. That delay is applied by the
/// resampler's copy path, which is why `silk_Decode` calls `silk_resampler`
/// even when no rate conversion is needed.
#[test]
fn silk_output_carries_the_resampler_input_delay() {
    for &(rate, delay) in &[(8_000i32, 4usize), (12_000, 9), (16_000, 12)] {
        let frame = (rate / 50) as usize;
        // An impulse well inside the first frame: where it lands in the output
        // is the decoder's delay, and it does not need a reference decoder to
        // read off.
        let mut pcm = vec![0.0f32; frame * 8];
        pcm[frame] = 0.8;
        let r = Codec::new(rate, 1, Application::Voip)
            .bitrate(24_000)
            .roundtrip(&pcm);
        assert!(
            r.modes().iter().all(|m| *m == "silk"),
            "{rate} Hz: not SILK"
        );

        // The first `delay` samples come out of the resampler's delay buffer,
        // which starts zeroed, so they are exactly silent.
        assert!(
            r.decoded[..delay].iter().all(|&s| s == 0.0),
            "{rate} Hz: expected {delay} samples of resampler delay, got a \
             non-zero sample inside it"
        );
        assert!(
            r.decoded[delay..frame].iter().any(|&s| s != 0.0),
            "{rate} Hz: output is silent past the {delay}-sample delay"
        );
    }
}

/// Write the packets both tables above are built from, where the C reference
/// can reach them. Ignored: it regenerates inputs rather than checking
/// anything. See `reference/plc/README.md` for what to run over them.
///
/// One file per configuration, in the length-prefixed form `cpcm` and `cplc`
/// read: a 4-byte little-endian length, then the payload. `FROZEN_PLC` reuses
/// `FROZEN`'s packets, so the label is the only thing that distinguishes them
/// and the same file serves both tables.
#[test]
#[ignore]
fn dump_reference_packets() {
    use std::io::Write;

    let dir = "reference/work/plc";
    std::fs::create_dir_all(dir).expect("create reference/work/plc");

    for f in FROZEN {
        let packets = encode(f.rate, f.channels, f.bitrate);
        let stem = f.label.replace(' ', "_");
        let path = format!("{dir}/{stem}.pkt");
        let mut out = std::io::BufWriter::new(std::fs::File::create(&path).expect("create"));
        for p in &packets {
            out.write_all(&(p.len() as u32).to_le_bytes())
                .expect("write");
            out.write_all(p).expect("write");
        }
        out.flush().expect("flush");
        println!(
            "{path}  {} packets  {} Hz  {} ch  frame={}",
            packets.len(),
            f.rate,
            f.channels,
            f.rate / 50
        );
    }

    println!("\nlosses to pass as cplc's last argument:");
    for f in FROZEN_PLC {
        let stem = f.label.replace(' ', "_");
        let lost: Vec<String> = f.lost.iter().map(|i| i.to_string()).collect();
        println!("  {:<28} {}", stem, lost.join(","));
    }
}

//! The 16-bit PCM API, held against C libopus 1.6.1.
//!
//! `encode_s16` and `decode_s16` are not conveniences wrapped around the float
//! entry points. libopus draws a real distinction, in both directions:
//!
//! - Encoding, `opus_encode` declares 16 bits of input precision where
//!   `opus_encode_float` declares 24. That moves the floor below which a signal
//!   counts as digital silence, and the depth CELT quantises against.
//! - Decoding, `opus_decode` soft-clips its output and `opus_decode_float` does
//!   not, because converting to integer PCM is where exceeding ±1 stops being
//!   harmless and becomes audible distortion.
//!
//! Both are checked here against the reference rather than assumed, along with
//! the `opus_pcm_soft_clip` port that the second one rests on.
//!
//! # Regenerating the expected values
//!
//! `cargo test --release --test integer_pcm -- --ignored --nocapture` writes
//! this suite's inputs and its own results to `reference/work/s16/`; `reference/s16/README.md`
//! has the recipe for the reference tool that turns those into the constants
//! below. Two details of that recipe are load-bearing:
//!
//! - The **encoder** comparison needs a fixed-point libopus, as
//!   `tests/reference_vectors.rs` does and for the same reason: a float build
//!   takes SILK's float encoder path and will not match.
//! - The **decoder** and **soft clip** comparisons need a float build, since
//!   `OPTIONAL_CLIP` is 0 in a fixed-point one — soft clipping is float-build
//!   behaviour, and it is the behaviour this crate reproduces. That build must
//!   also disable floating-point contraction (`-ffp-contract=off`). The curve is
//!   `x + a·x·x`, which a compiler is free to fuse into an FMA; libopus built
//!   the usual way does fuse it, and the result differs from the unfused one in
//!   the last bit. Rust never contracts, so the unfused form is what this crate
//!   computes and what the C source says.

mod common;
use common::sin_turns;
use opus_pure::{
    Application, Error, OpusDecoder, OpusEncoder, OpusMSDecoder, OpusMSEncoder, RateControl,
    SoftClip,
};

/// FNV-1a over a byte string, matching `reference/s16/cs16.c`.
///
/// The expected values are whole-output hashes because the outputs run to
/// thousands of samples. Every assertion that uses one also reports where the
/// first difference is, so a failure says where and not only that.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn hash_i16(v: &[i16]) -> u64 {
    fnv1a(&v.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>())
}

fn hash_f32(v: &[f32]) -> u64 {
    fnv1a(&v.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>())
}

/// The 440 Hz half-scale sine `tests/reference_vectors.rs` encodes, in the
/// 16-bit form it quantises to.
///
/// That file establishes this signal as one on which this crate and libopus
/// agree byte for byte, and documents why its `f64::sin` is safe even though
/// libm differs between targets: the i16 quantisation on the next line is
/// twelve orders of magnitude coarser than the disagreement.
fn reference_sine(rate: i32, samples: usize) -> Vec<i16> {
    (0..samples)
        .map(|i| {
            let cycle = (440i64 * i as i64) % rate as i64;
            let phase = cycle as f64 / rate as f64;
            (f64::sin(2.0 * std::f64::consts::PI * phase) * 16384.0) as i16
        })
        .collect()
}

/// A loud chord: three partials close to full scale, each channel detuned so a
/// stereo pair is not two copies of one signal.
///
/// Loud on purpose — the decoder's output has to overshoot ±1 for the soft clip
/// to have anything to do, and `soft_clip_has_work_to_do_on_this_source` checks
/// that it really does, so this cannot rot into a signal that tests nothing.
/// Built from [`sin_turns`] rather than `f32::sin` so it is bit-identical on
/// every target, which the hashes below depend on.
fn loud_chord(rate: i32, channels: usize, samples: usize) -> Vec<i16> {
    let mut out = vec![0i16; samples * channels];
    for i in 0..samples {
        let t = i as f64 / rate as f64;
        for c in 0..channels {
            let detune = 1.0 + c as f64 * 0.004;
            let v = sin_turns(440.0 * detune * t) * 0.62
                + sin_turns(1109.0 * detune * t) * 0.24
                + sin_turns(2637.0 * detune * t) * 0.12;
            out[i * channels + c] = (v * 32_000.0) as i16;
        }
    }
    out
}

/// A float signal running past ±1 in both directions, for the soft clip.
///
/// A triangle rather than a sine: every value is exact in binary floating
/// point, so it is bit-identical everywhere without needing [`sin_turns`], and
/// its straight runs through the limit are the case the curve has to handle.
fn over_unity(channels: usize, samples: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; samples * channels];
    for i in 0..samples {
        // Period 128, a power of two, so the division is exact.
        let phase = (i % 128) as f32 / 128.0;
        let tri = if phase < 0.5 {
            4.0 * phase - 1.0
        } else {
            3.0 - 4.0 * phase
        };
        for c in 0..channels {
            // Channel 1 is quieter and inverted, so one channel clips where the
            // other does not: state leaking between channels would show here.
            out[i * channels + c] = tri * if c == 0 { 1.45 } else { -1.05 };
        }
    }
    out
}

const RATE: i32 = 8_000;
const FRAME: usize = 160; // 20 ms
const FRAMES: usize = 20;
const CLIP_BLOCK: usize = 960;

/// Configurations for the encoder comparison, all pure SILK at 8 kHz.
///
/// SILK is where this crate and libopus agree to the byte, both being ports of
/// the same fixed-point arithmetic; CELT is float on both sides and lands at
/// float rounding instead, which a hash cannot express. 20 frames is also
/// deliberate: past roughly 27 the two encoders' rate-control state drifts
/// apart on a steady tone, which `reference_vectors.rs` does not reach either.
const CASES: [(&str, i32, i32); 3] = [
    ("10 kb/s complexity 5", 10_000, 5),
    ("10 kb/s complexity 10", 10_000, 10),
    ("6 kb/s complexity 5", 6_000, 5),
];

/// `opus_encode` output for [`CASES`], from fixed-point libopus 1.6.1.
const ENCODE_EXPECTED: [u64; 3] = [
    0x1c3a_2be1_a799_6385,
    0xfe28_7604_2578_f947,
    0xc4f5_a474_f1c4_8240,
];

/// `opus_decode` output for the packets [`CASES`] produce from [`loud_chord`],
/// from float libopus 1.6.1 built without floating-point contraction.
const DECODE_EXPECTED: [u64; 3] = [
    0x54aa_9664_4161_bf0e,
    0xfba3_fd05_035e_e921,
    0x3ca7_f326_88ab_c1bc,
];

/// `opus_pcm_soft_clip` over [`over_unity`], in blocks of [`CLIP_BLOCK`].
const SOFT_CLIP_EXPECTED: u64 = 0xbc3b_0938_e8fd_f9c8;

/// VOIP and CBR, matching `tests/reference_vectors.rs`: those are the conditions
/// under which this crate and libopus agree byte for byte, VBR rate control
/// being the part that diverges.
fn encoder(bitrate: i32, complexity: i32) -> OpusEncoder {
    let mut enc = OpusEncoder::new(RATE, 1, Application::Voip).expect("encoder");
    enc.bitrate_bps = bitrate;
    enc.complexity = complexity;
    enc.rate_control = RateControl::Cbr;
    enc
}

fn encode_all_s16(pcm: &[i16], bitrate: i32, complexity: i32) -> Vec<Vec<u8>> {
    let mut enc = encoder(bitrate, complexity);
    let mut buf = vec![0u8; 4000];
    pcm.as_chunks::<FRAME>()
        .0
        .iter()
        .map(|chunk| {
            let n = enc.encode_s16(chunk, FRAME, &mut buf).expect("encode_s16");
            buf[..n].to_vec()
        })
        .collect()
}

fn decode_all_s16(packets: &[Vec<u8>]) -> Vec<i16> {
    let mut dec = OpusDecoder::new(RATE, 1).expect("decoder");
    let mut out = vec![0i16; FRAME];
    let mut all = Vec::new();
    for p in packets {
        let n = dec.decode_s16(p, FRAME, &mut out).expect("decode_s16");
        all.extend_from_slice(&out[..n]);
    }
    all
}

// ---- Against the reference ----

/// `encode_s16` produces exactly what C libopus's `opus_encode` produces.
///
/// This is the strongest statement available about the 16-bit encode path: not
/// that it resembles the reference, but that every byte of every packet is the
/// same. It covers the declared depth as much as the conversion, since the
/// reference is the entry point that declares 16 too.
#[test]
fn encode_s16_matches_libopus() {
    let pcm = reference_sine(RATE, FRAME * FRAMES);
    for (i, (name, bitrate, complexity)) in CASES.into_iter().enumerate() {
        let packets = encode_all_s16(&pcm, bitrate, complexity);
        assert_eq!(packets.len(), FRAMES, "{name}: wrong packet count");
        let bytes: Vec<u8> = packets.concat();
        assert_eq!(
            fnv1a(&bytes),
            ENCODE_EXPECTED[i],
            "{name}: encoder output moved. First packet was {}",
            hex(&packets[0])
        );
    }
}

/// `decode_s16` produces exactly what C libopus's `opus_decode` produces,
/// soft clipping and 16-bit conversion included.
///
/// The packets fed to both sides are this crate's own, so the comparison
/// isolates the decode: a difference here cannot be blamed on the encoder
/// having chosen something else to code.
#[test]
fn decode_s16_matches_libopus() {
    let pcm = loud_chord(RATE, 1, FRAME * FRAMES);
    for (i, (name, bitrate, complexity)) in CASES.into_iter().enumerate() {
        let packets = encode_all_s16(&pcm, bitrate, complexity);
        let decoded = decode_all_s16(&packets);
        assert_eq!(decoded.len(), FRAME * FRAMES, "{name}: wrong sample count");
        assert_eq!(
            hash_i16(&decoded),
            DECODE_EXPECTED[i],
            "{name}: decoder output moved"
        );
    }
}

/// [`SoftClip`] reproduces `opus_pcm_soft_clip` bit for bit, across block
/// boundaries as well as within a block.
#[test]
fn soft_clip_matches_libopus() {
    let mut pcm = over_unity(2, CLIP_BLOCK * 4);
    let mut clip = SoftClip::new(2);
    for block in pcm.chunks_mut(CLIP_BLOCK * 2) {
        clip.apply(block);
    }
    assert_eq!(hash_f32(&pcm), SOFT_CLIP_EXPECTED, "soft clip output moved");
}

// ---- Properties the reference comparison does not pin ----

/// The clipping source really does drive the decoder past ±1, so the tests
/// below exercise the curve rather than passing because nothing ever clipped.
#[test]
fn the_clipping_source_actually_clips() {
    let packets = clipping_packets(64_000);
    let mut dec = OpusDecoder::new(48_000, 2).unwrap();
    let mut out = vec![0.0f32; 960 * 2];
    let mut peak = 0.0f32;
    for p in &packets {
        let n = dec.decode(p, 960, &mut out).unwrap();
        peak = out[..n * 2].iter().fold(peak, |m, &v| m.max(v.abs()));
    }
    assert!(
        peak > 1.2,
        "the decoded peak is only {peak}, so the clipping tests below prove \
         nothing — the source needs sharper edges or more level"
    );
}

/// `encode_s16` is the float encoder told its input came from 16 bits, and
/// nothing else: same samples, same packets.
///
/// The conversion is `sample / 32768`, exact because the scale is a power of
/// two, so a caller who converts by hand and sets `lsb_depth` themselves gets
/// byte-identical output. Anything else would mean the 16-bit path had grown a
/// second behaviour to keep in step with the first.
#[test]
fn encode_s16_is_the_float_path_told_the_input_is_16_bit() {
    let pcm = loud_chord(48_000, 2, 960 * 10);
    let as_float: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();

    let mut a = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    a.bitrate_bps = 96_000;
    let mut b = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    b.bitrate_bps = 96_000;
    b.lsb_depth = 16;

    let (mut buf_a, mut buf_b) = (vec![0u8; 4000], vec![0u8; 4000]);
    for i in 0..10 {
        let (lo, hi) = (i * 960 * 2, (i + 1) * 960 * 2);
        let n = a.encode_s16(&pcm[lo..hi], 960, &mut buf_a).unwrap();
        let m = b.encode(&as_float[lo..hi], 960, &mut buf_b).unwrap();
        assert_eq!(&buf_a[..n], &buf_b[..m], "packet {i} differs");
    }
}

/// A caller who declares fewer than 16 bits gets that, not 16.
///
/// libopus takes the lesser of what the entry point implies and what the caller
/// asked for (`lsb_depth = IMIN(lsb_depth, st->lsb_depth)`), and this holds the
/// same rule. It doubles as proof that the declared depth reaches the coder at
/// all: 12 bits changes the packets where 16 does not, because 16 and 24 land on
/// the same side of every dynamic-allocation threshold on ordinary material —
/// libopus's two entry points agree byte for byte there too.
#[test]
fn a_declared_depth_below_16_is_honoured() {
    let pcm = loud_chord(48_000, 2, 960 * 10);
    let as_float: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();

    let mut shallow = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    shallow.bitrate_bps = 96_000;
    shallow.lsb_depth = 12;
    let mut matched = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    matched.bitrate_bps = 96_000;
    matched.lsb_depth = 12;
    let mut default = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    default.bitrate_bps = 96_000;

    let (mut a, mut b, mut c) = (vec![0u8; 4000], vec![0u8; 4000], vec![0u8; 4000]);
    let mut differed_from_default = false;
    for i in 0..10 {
        let (lo, hi) = (i * 960 * 2, (i + 1) * 960 * 2);
        let n = shallow.encode_s16(&pcm[lo..hi], 960, &mut a).unwrap();
        let m = matched.encode(&as_float[lo..hi], 960, &mut b).unwrap();
        let k = default.encode_s16(&pcm[lo..hi], 960, &mut c).unwrap();
        assert_eq!(&a[..n], &b[..m], "packet {i}: the lower depth was not used");
        differed_from_default |= a[..n] != c[..k];
    }
    assert!(
        differed_from_default,
        "12 bits and the default produced identical packets throughout, so the \
         declared depth is not reaching the coder"
    );
}

/// Everything the clipper returns is inside ±1, which is the promise the 16-bit
/// conversion downstream depends on.
#[test]
fn soft_clip_bounds_its_output() {
    let mut pcm = over_unity(2, CLIP_BLOCK * 3);
    // Well past the ±2 domain the curve is defined over, to reach the clamp
    // that precedes it, and a NaN, which C saturates to the lower bound.
    pcm[7] = 40.0;
    pcm[9] = -12.5;
    pcm[11] = f32::NAN;
    let mut clip = SoftClip::new(2);
    for block in pcm.chunks_mut(CLIP_BLOCK * 2) {
        clip.apply(block);
    }
    for (i, &s) in pcm.iter().enumerate() {
        assert!(s.abs() <= 1.0, "sample {i} came out at {s}");
    }
}

/// The curve carries across blocks, so an excursion split by a block boundary
/// is bent as one shape rather than two.
///
/// The check is that a clipper which has seen the previous block treats the
/// next one differently from a fresh one. That is exactly the state
/// [`SoftClip`] exists to hold, and `reset` is what puts it back.
#[test]
fn soft_clip_carries_the_curve_between_blocks() {
    let source = over_unity(1, CLIP_BLOCK * 2);

    let mut carried = source.clone();
    let mut clip = SoftClip::new(1);
    clip.apply(&mut carried[..CLIP_BLOCK]);
    clip.apply(&mut carried[CLIP_BLOCK..]);

    let mut fresh = source.clone();
    let mut clip = SoftClip::new(1);
    clip.apply(&mut fresh[..CLIP_BLOCK]);
    clip.reset();
    clip.apply(&mut fresh[CLIP_BLOCK..]);

    assert_eq!(
        carried[..CLIP_BLOCK],
        fresh[..CLIP_BLOCK],
        "the first block cannot depend on what comes after it"
    );
    assert_ne!(
        carried[CLIP_BLOCK..],
        fresh[CLIP_BLOCK..],
        "a reset clipper produced the same second block as one carrying state, \
         so the curve is not crossing the boundary at all"
    );
}

/// The 16-bit path clips and the float path does not, which is the whole
/// distinction libopus draws between `opus_decode` and `opus_decode_float`.
///
/// What the curve buys is measured as the longest *run* of samples sitting on
/// the 16-bit rail. That is the thing that is audible: saturation replaces a
/// whole excursion with a flat plateau, and it is the corner at each end of the
/// plateau, not the lost amplitude, that spreads energy across the spectrum.
/// Soft clipping bends the same excursion into a curve that touches the rail at
/// its peak and nowhere else, so isolated railed samples are the expected
/// result and a plateau is the failure.
#[test]
fn only_the_16_bit_path_clips() {
    /// Longest run of consecutive samples at either 16-bit rail.
    fn longest_plateau(pcm: &[i16]) -> usize {
        let (mut best, mut run) = (0, 0);
        for &s in pcm {
            run = if s == i16::MIN || s == i16::MAX {
                run + 1
            } else {
                0
            };
            best = best.max(run);
        }
        best
    }

    let packets = clipping_packets(64_000);

    // The float path leaves the overshoot in place, as libopus does.
    let mut dec = OpusDecoder::new(48_000, 2).unwrap();
    let mut float = vec![0.0f32; 960 * 2];
    let mut saturating = Vec::new();
    let mut over = 0usize;
    for p in &packets {
        let n = dec.decode(p, 960, &mut float).unwrap();
        over += float[..n * 2].iter().filter(|s| s.abs() > 1.0).count();
        // What a caller would get by converting that float output themselves,
        // which is the thing `decode_s16` exists to be better than.
        saturating.extend(
            float[..n * 2]
                .iter()
                .map(|&s| (s * 32768.0).clamp(-32768.0, 32767.0).round_ties_even() as i16),
        );
    }
    assert!(
        over > 0,
        "the float decode clipped, which libopus does not do"
    );

    let mut dec = OpusDecoder::new(48_000, 2).unwrap();
    let mut ints = vec![0i16; 960 * 2];
    let mut clipped = Vec::new();
    for p in &packets {
        let n = dec.decode_s16(p, 960, &mut ints).unwrap();
        clipped.extend_from_slice(&ints[..n * 2]);
    }

    let saturated = longest_plateau(&saturating);
    let softened = longest_plateau(&clipped);
    assert!(
        saturated >= 8,
        "converting the float output saturates for only {saturated} samples in \
         a row, so this source is not overloading hard enough to tell the two \
         apart"
    );
    assert!(
        softened * 4 <= saturated,
        "soft clipping left a plateau of {softened} samples against {saturated} \
         for plain saturation, which is not the improvement it exists to be"
    );
}

/// Float output is bit-identical whether or not 16-bit calls preceded it.
///
/// libopus clears the curve on every float decode for this reason, and so does
/// this. Without it, a caller alternating entry points would find float frames
/// silently bent by state belonging to the 16-bit path, where nothing was going
/// to clip them anyway.
#[test]
fn a_float_decode_is_never_bent_by_the_16_bit_path() {
    let packets = clipping_packets(64_000);
    let split = packets.len() / 2;

    let mut mixed = OpusDecoder::new(48_000, 2).unwrap();
    let mut ints = vec![0i16; 960 * 2];
    let mut a = vec![0.0f32; 960 * 2];
    for p in &packets[..split] {
        mixed.decode_s16(p, 960, &mut ints).unwrap();
    }
    let n = mixed.decode(&packets[split], 960, &mut a).unwrap();

    let mut pure = OpusDecoder::new(48_000, 2).unwrap();
    let mut b = vec![0.0f32; 960 * 2];
    for p in &packets[..split] {
        pure.decode(p, 960, &mut b).unwrap();
    }
    let m = pure.decode(&packets[split], 960, &mut b).unwrap();

    assert_eq!(n, m);
    assert_eq!(
        a[..n * 2],
        b[..m * 2],
        "the float frame after a run of 16-bit decodes differs from the same \
         frame reached through float decodes alone"
    );
}

// ---- Arguments ----

#[test]
fn short_buffers_are_errors_not_panics() {
    let pcm = vec![0i16; 160];
    let mut enc = encoder(10_000, 5);
    let mut packet = vec![0u8; 4000];

    assert!(matches!(
        enc.encode_s16(&pcm[..80], FRAME, &mut packet),
        Err(Error::InvalidArgument(_))
    ));
    let n = enc.encode_s16(&pcm, FRAME, &mut packet).unwrap();

    let mut dec = OpusDecoder::new(RATE, 1).unwrap();
    let mut out = vec![0i16; FRAME];
    assert!(matches!(
        dec.decode_s16(&packet[..n], FRAME, &mut out[..80]),
        Err(Error::BufferTooSmall { .. })
    ));
    assert!(dec.decode_s16(&packet[..n], FRAME, &mut out).is_ok());
}

// ---- Multistream ----

/// The surround path carries 16-bit PCM through in both directions.
#[test]
fn multistream_round_trips_16_bit_pcm() {
    const CH: usize = 6;
    let pcm = loud_chord(48_000, CH, 960 * 5);

    let mut enc = OpusMSEncoder::new(48_000, CH, 1, Application::Audio).unwrap();
    enc.set_bitrate(384_000);
    let mut dec = OpusMSDecoder::new(48_000, CH, 1).unwrap();

    let mut out = vec![0i16; 960 * CH];
    let mut packet = vec![0u8; 8000];
    let mut energy = 0.0f64;
    for i in 0..5 {
        let len = enc
            .encode_s16(&pcm[i * 960 * CH..(i + 1) * 960 * CH], 960, &mut packet)
            .unwrap();
        let n = dec.decode_s16(&packet[..len], 960, &mut out).unwrap();
        assert_eq!(n, 960);
        // Skip the first frame: it is the encoder's priming delay.
        if i > 0 {
            energy += out.iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
        }
    }
    assert!(energy > 0.0, "the surround round trip decoded to silence");
}

/// Packets whose decode rings well past ±1, which is what the clipping tests
/// need and what none of the 8 kHz cases above provide: a narrowband signal is
/// too band-limited to ring out of range.
///
/// A full-scale square wave does it. Its edges are exactly the discontinuity a
/// transform codec cannot represent in a finite number of coefficients, so the
/// reconstruction overshoots at every one — Gibbs ringing, not a defect. The
/// decoded peak here is around 1.6, which is far outside anything hard clipping
/// could disguise.
fn clipping_packets(bitrate: i32) -> Vec<Vec<u8>> {
    const CYCLE: usize = 120; // 400 Hz at 48 kHz
    const STEREO_FRAME: usize = 960 * 2; // 20 ms of interleaved stereo
    let pcm: Vec<i16> = (0..960 * 12 * 2)
        .map(|i| {
            let (sample, channel) = (i / 2, i % 2);
            // The channels are in antiphase, so a clipper that leaked state
            // between them would bend one where the other needs no bending.
            if (sample / (CYCLE / 2) + channel).is_multiple_of(2) {
                32_700
            } else {
                -32_700
            }
        })
        .collect();
    let mut enc = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    enc.bitrate_bps = bitrate;
    let mut buf = vec![0u8; 4000];
    pcm.as_chunks::<STEREO_FRAME>()
        .0
        .iter()
        .map(|chunk| {
            let n = enc.encode_s16(chunk, 960, &mut buf).unwrap();
            buf[..n].to_vec()
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write this suite's inputs, and its own results, where the C reference tool
/// can reach them. Ignored: it regenerates the constants above rather than
/// checking anything. See the module docs.
#[test]
#[ignore]
fn dump_reference_inputs() {
    use std::io::Write;
    std::fs::create_dir_all("reference/work/s16").unwrap();

    let write_i16 = |path: &str, v: &[i16]| {
        let mut f = std::fs::File::create(path).unwrap();
        for s in v {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
    };

    let sine = reference_sine(RATE, FRAME * FRAMES);
    write_i16("reference/work/s16/sine.s16", &sine);
    let loud = loud_chord(RATE, 1, FRAME * FRAMES);
    write_i16("reference/work/s16/loud.s16", &loud);

    let over = over_unity(2, CLIP_BLOCK * 4);
    let mut f = std::fs::File::create("reference/work/s16/over_unity.f32").unwrap();
    for s in &over {
        f.write_all(&s.to_le_bytes()).unwrap();
    }

    for (case, (name, bitrate, complexity)) in CASES.into_iter().enumerate() {
        let packets = encode_all_s16(&sine, bitrate, complexity);
        println!(
            "ours enc {name}: frames={} hash={:016x}",
            packets.len(),
            fnv1a(&packets.concat())
        );

        let loud_packets = encode_all_s16(&loud, bitrate, complexity);
        let mut pf = std::fs::File::create(format!("reference/work/s16/case{case}.pkt")).unwrap();
        for p in &loud_packets {
            pf.write_all(&(p.len() as u32).to_le_bytes()).unwrap();
            pf.write_all(p).unwrap();
        }
        println!(
            "ours dec {name}: hash={:016x}",
            hash_i16(&decode_all_s16(&loud_packets))
        );
    }

    let mut clipped = over.clone();
    let mut clip = SoftClip::new(2);
    for block in clipped.chunks_mut(CLIP_BLOCK * 2) {
        clip.apply(block);
    }
    println!("ours clip: hash={:016x}", hash_f32(&clipped));
}

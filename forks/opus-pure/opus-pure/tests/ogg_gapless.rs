//! A clip goes into an `.opus` file and the same number of samples comes back.
//!
//! Opus codes whole frames and runs behind its input, so a round trip is longer
//! than the audio at both ends unless two corrections are made. RFC 7845 §4.2
//! covers the front (the pre-skip) and §4.4 the back (the end-trim), and this
//! exercises the pair of them together, because each one is invisible on its
//! own: a stream that keeps its padding still sounds right played once, and a
//! stream that loses its tail still has a plausible length.
//!
//! `encode_gapless` and `decode_trimmed` below are `examples/encode.rs` and
//! `examples/decode.rs` reduced to their arithmetic. They are a transcription,
//! not the same code — an example has a `main` and cannot be linked — so this
//! pins the recipe's *behaviour* and not the examples themselves. Changing the
//! documented recipe means changing it in both places; what this guarantees is
//! that the recipe, as written here, is right.

mod common;
use common::*;
use opus_pure::{
    Application, MAX_PACKET_BYTES, MAX_PACKET_SAMPLES, OggOpusReader, OggOpusWriter, OpusEncoder,
    OpusHead, Trim, packet,
};

/// Rate, channels, bitrate.
const CASES: &[(i32, usize, i32)] = &[
    (48_000, 1, 96_000),
    (48_000, 2, 128_000),
    (24_000, 2, 48_000),
    (16_000, 1, 24_000),
    (8_000, 1, 16_000),
];

/// Encode interleaved `pcm` into a complete Ogg Opus stream, ending the file
/// where the audio ends. Mirrors `examples/encode.rs`.
fn encode_gapless(rate: i32, channels: usize, bitrate: i32, pcm: &[f32]) -> Vec<u8> {
    let frame = (rate / 50) as usize;
    let mut encoder = OpusEncoder::new(rate, channels, Application::Audio).unwrap();
    encoder.bitrate_bps = bitrate;
    let head = OpusHead::for_encoder(&encoder, rate as u32);

    let ticks = 48_000 / rate as usize;
    let total = pcm.len() / channels;
    // Pad past the audio by the encoder's own delay, or its last `pre_skip`
    // samples never come out; then round up to a whole frame.
    let frames = (total + (head.pre_skip as usize).div_ceil(ticks)).div_ceil(frame);
    let final_granule = u64::from(head.pre_skip) + (total * ticks) as u64;

    let mut w = OggOpusWriter::new(Vec::new(), head).unwrap();
    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    let per_frame = frame * channels;
    let mut block = vec![0.0f32; per_frame];

    for i in 0..frames {
        let start = (i * per_frame).min(pcm.len());
        let end = (start + per_frame).min(pcm.len());
        block[..end - start].copy_from_slice(&pcm[start..end]);
        block[end - start..].fill(0.0);

        let n = encoder.encode(&block, frame, &mut packet).unwrap();
        if i + 1 == frames {
            let duration = final_granule - w.granule() as u64;
            w.write_packet_with_duration(&packet[..n], duration as u32)
                .unwrap();
        } else {
            w.write_packet(&packet[..n]).unwrap();
        }
    }
    w.finish().unwrap()
}

/// Decode a stream to interleaved PCM with both trims applied. Mirrors
/// `examples/decode.rs`.
fn decode_trimmed(bytes: &[u8], rate: i32) -> Vec<f32> {
    let mut reader = OggOpusReader::new(std::io::Cursor::new(bytes)).unwrap();
    let head = reader.head().clone();
    let channels = head.channel_count as usize;
    let mut decoder = head.decoder(rate).unwrap();
    let mut trim = Trim::new(&head, rate, channels).unwrap();

    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut pcm = Vec::new();
    for p in reader.packets() {
        let p = p.unwrap();
        let n = decoder
            .decode(&p.data, MAX_PACKET_SAMPLES, &mut block)
            .unwrap();
        pcm.extend_from_slice(trim.keep(&p, &block[..n * channels]));
    }
    assert_eq!(
        trim.samples_emitted() as usize * channels,
        pcm.len(),
        "Trim's own count disagrees with what it handed back"
    );
    pcm
}

/// Interleaved test audio, `total` sample frames per channel.
fn source(rate: i32, channels: usize, total: usize) -> Vec<f32> {
    let mono = music_like(rate, total);
    if channels == 2 {
        interleave(&[mono.clone(), speech_like(rate, total)])
    } else {
        mono
    }
}

/// Lengths worth trying: on a frame boundary, and either side of one, since the
/// end-trim is exactly what handles the clip that does not divide evenly.
fn lengths(rate: i32) -> Vec<usize> {
    let frame = (rate / 50) as usize;
    let base = rate as usize / 2; // half a second, a whole number of frames
    vec![base, base + 1, base - 1, base + frame / 2, base + frame - 1]
}

/// The point of the whole exercise: what comes out is the length of what went
/// in, to the sample, whatever the clip's length happens to be.
#[test]
fn a_round_trip_returns_exactly_the_samples_that_went_in() {
    for &(rate, channels, bitrate) in CASES {
        for total in lengths(rate) {
            let src = source(rate, channels, total);
            let file = encode_gapless(rate, channels, bitrate, &src);
            let out = decode_trimmed(&file, rate);
            assert_eq!(
                out.len() / channels,
                total,
                "{rate} Hz/{channels}ch, {total} samples: round trip changed the length by {}",
                out.len() as i64 / channels as i64 - total as i64,
            );
        }
    }
}

/// The trimmed output starts where the source does. A pre-skip applied even one
/// sample wrong shows up here as a non-zero lag, which every other fidelity
/// test in this crate removes by construction.
#[test]
fn the_trimmed_output_needs_no_realignment() {
    for &(rate, channels, bitrate) in CASES {
        let total = rate as usize / 2 + 7;
        let src = source(rate, channels, total);
        let file = encode_gapless(rate, channels, bitrate, &src);
        let out = decode_trimmed(&file, rate);

        // Measure one channel: interleaved correlation would mix the two.
        let (a, b) = (
            deinterleave(&out, channels, 0),
            deinterleave(&src, channels, 0),
        );
        let (corr, lag) = aligned_correlation(&a, &b, 480);
        assert_eq!(
            lag, 0,
            "{rate} Hz/{channels}ch: output is {lag} samples late"
        );
        assert!(corr > 0.9, "{rate} Hz/{channels}ch: correlation {corr}");
    }
}

/// The end of the audio is audio, not the silence that padded the last frame.
///
/// This is the failure the pre-skip padding prevents: stop feeding the encoder
/// at the end of the input and its last `pre_skip` samples stay inside it, so a
/// file of exactly the right length ends in a fade to nothing.
#[test]
fn the_tail_of_the_audio_survives_the_encoder_delay() {
    for &(rate, channels, bitrate) in CASES {
        let total = rate as usize / 2 + 7;
        let src = source(rate, channels, total);
        let file = encode_gapless(rate, channels, bitrate, &src);
        let out = decode_trimmed(&file, rate);

        // The last 10 ms — well inside the 6.5 ms of encoder delay that would
        // otherwise be missing, plus the frame padding around it.
        let tail = rate as usize / 100 * channels;
        let (a, b) = (&out[out.len() - tail..], &src[src.len() - tail..]);
        let corr = correlation(a, b);
        assert!(
            corr > 0.7,
            "{rate} Hz/{channels}ch: last 10 ms correlates {corr} with the source"
        );
        assert!(
            energy(a) > energy(b) * 0.25,
            "{rate} Hz/{channels}ch: the audio fades out at the end"
        );
    }
}

/// The final granule is the pre-skip plus the audio, and the packets carry at
/// least that much. Under-claiming is the end-trim; over-claiming would promise
/// audio the file does not contain.
#[test]
fn the_final_granule_claims_the_audio_and_no_more() {
    for &(rate, channels, bitrate) in CASES {
        for total in lengths(rate) {
            let src = source(rate, channels, total);
            let file = encode_gapless(rate, channels, bitrate, &src);

            let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
            let pre_skip = i64::from(r.head().pre_skip);
            let (mut decodable, mut final_granule) = (0i64, -1i64);
            while let Some(p) = r.read_packet().unwrap() {
                decodable += packet::samples_48k(&p.data).unwrap() as i64;
                if p.page_granule >= 0 {
                    final_granule = p.page_granule;
                }
            }

            let label = format!("{rate} Hz/{channels}ch, {total} samples");
            let expected = pre_skip + (total * (48_000 / rate as usize)) as i64;
            assert_eq!(final_granule, expected, "{label}: final granule");
            assert!(
                decodable >= final_granule,
                "{label}: granule promises {} samples the packets do not carry",
                final_granule - decodable
            );
            // The trim is real: a clip that is not a whole number of frames
            // long must claim less than it decodes to.
            let frame_48k = 960i64;
            if (total * (48_000 / rate as usize)) as i64 % frame_48k != 0 {
                assert!(decodable > final_granule, "{label}: nothing was trimmed");
            }
        }
    }
}

/// A decoder that ignores the end-trim gets padding it should not have. This
/// pins the size of what the documented recipe used to hand back, so the
/// difference stays a fact rather than a claim.
#[test]
fn ignoring_the_end_trim_leaves_padding_behind() {
    let (rate, channels) = (48_000i32, 1usize);
    let frame = (rate / 50) as usize;
    let total = rate as usize / 2 + frame / 3;
    let src = source(rate, channels, total);
    let file = encode_gapless(rate, channels, 96_000, &src);

    // Pre-skip only, the way the recipe read before `Trim` existed.
    let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
    let head = r.head().clone();
    let mut d = head.decoder(rate).unwrap();
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut pcm = Vec::new();
    for p in r.packets() {
        let n = d
            .decode(&p.unwrap().data, MAX_PACKET_SAMPLES, &mut block)
            .unwrap();
        pcm.extend_from_slice(&block[..n * channels]);
    }
    pcm.drain(..head.pre_skip as usize * channels);

    let extra = pcm.len() / channels - total;
    assert!(
        extra > 0 && extra < frame,
        "expected under one frame of padding, got {extra} samples"
    );
    assert_eq!(decode_trimmed(&file, rate).len() / channels, total);
}

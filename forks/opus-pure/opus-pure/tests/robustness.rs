//! The codec must not panic, hang, or produce non-finite samples — whatever it
//! is handed.
//!
//! Two of the three bugs these cover were live in the code this crate was ported
//! from: a forward-MDCT assertion on any mode transition below 48 kHz, and a
//! SILK VAD out-of-bounds read on 2.5 ms frames.

mod common;
use common::*;
use opus_pure::{Application, Error, OpusDecoder, OpusEncoder};

const RATES: [i32; 5] = [8_000, 12_000, 16_000, 24_000, 48_000];
/// Every packet duration Opus defines, in tenths of a millisecond so that 2.5 ms
/// stays an integer and 60 ms — which divides no sample rate evenly — is
/// expressible at all.
const FRAME_TENTH_MS: [i32; 9] = [25, 50, 100, 200, 400, 600, 800, 1000, 1200];

/// Full sweep of rate x frame size x application x bitrate x channel count.
///
/// The encoder is free to reject a combination, but it must reject it with an
/// error rather than a panic, and every combination it accepts on the first
/// frame must keep working for the rest of the stream — the analysis switching
/// coding mode mid-stream used to break that.
#[test]
fn no_configuration_panics_or_fails_mid_stream() {
    let mut panicked = Vec::new();
    let mut broke_mid_stream = Vec::new();

    for &rate in &RATES {
        for &tenth_ms in &FRAME_TENTH_MS {
            let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
            for &app in &[Application::Voip, Application::Audio] {
                for &bitrate in &[6_000i32, 16_000, 32_000, 64_000, 192_000] {
                    for &channels in &[1usize, 2] {
                        let label = format!(
                            "{rate}Hz/{}ms/{app:?}/{bitrate}/{channels}ch",
                            tenth_ms as f32 / 10.0
                        );
                        // 60 packets everywhere would be 7 seconds of audio at
                        // 120 ms, times 900 configurations. The long durations
                        // get fewer, which is still a stream long enough for a
                        // mid-stream change to show up.
                        let frames = if tenth_ms >= 800 { 15 } else { 60 };
                        let mono = speech_like(rate, frame * frames);
                        let pcm = if channels == 2 {
                            interleave(&[mono.clone(), music_like(rate, frame * frames)])
                        } else {
                            mono
                        };

                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                let mut enc = OpusEncoder::new(rate, channels, app).unwrap();
                                enc.bitrate_bps = bitrate;
                                let mut dec = OpusDecoder::new(rate, channels).unwrap();
                                let mut first_ok = None;
                                for f in 0..frames {
                                    let mut pkt = vec![0u8; 4000];
                                    let r = enc.encode(
                                        &pcm[f * frame * channels..(f + 1) * frame * channels],
                                        frame,
                                        &mut pkt,
                                    );
                                    let ok = r.is_ok();
                                    match first_ok {
                                        None => first_ok = Some(ok),
                                        Some(first) if ok != first => return Err(f),
                                        Some(_) => {}
                                    }
                                    if let Ok(n) = r {
                                        let mut out = vec![0.0f32; frame * channels];
                                        dec.decode(&pkt[..n], frame, &mut out).unwrap();
                                        assert!(
                                            out.iter().all(|s| s.is_finite()),
                                            "non-finite sample decoded"
                                        );
                                    }
                                }
                                Ok(())
                            }));

                        match outcome {
                            Err(_) => panicked.push(label),
                            Ok(Err(f)) => broke_mid_stream.push(format!("{label} at frame {f}")),
                            Ok(Ok(())) => {}
                        }
                    }
                }
            }
        }
    }

    assert!(
        panicked.is_empty(),
        "configurations that panicked:\n  {}",
        panicked.join("\n  ")
    );
    assert!(
        broke_mid_stream.is_empty(),
        "configurations whose encode result changed mid-stream:\n  {}",
        broke_mid_stream.join("\n  ")
    );
}

/// Rejected configurations must say *why*, as an `InvalidArgument`, not some
/// other variant that a caller would handle differently.
#[test]
fn rejected_configurations_report_invalid_argument() {
    // Every duration Opus has a packet for now encodes, so what is left to
    // reject is durations Opus does not define. (2.5 ms was rejected here once —
    // at 16 kHz that is 40 samples and no CELT `lm` matched it — then 60 ms,
    // then 80 through 120 ms, which needed multi-frame packets.)
    let mut enc = OpusEncoder::new(16_000, 1, Application::Audio).unwrap();
    let mut pkt = vec![0u8; 4000];

    // 140 ms: past the 120 ms an Opus packet may carry at all.
    assert!(matches!(
        enc.encode(&vec![0.0; 2240], 2240, &mut pkt),
        Err(Error::InvalidArgument(_))
    ));

    // 30 ms: a whole number of milliseconds, and a multiple of 2.5 ms, but not
    // one of the nine durations RFC 6716 §3.1 defines.
    assert!(matches!(
        enc.encode(&vec![0.0; 480], 480, &mut pkt),
        Err(Error::InvalidArgument(_))
    ));

    // A frame size that does not divide the sample rate at all.
    assert!(matches!(
        enc.encode(&vec![0.0; 333], 333, &mut pkt),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn constructor_rejects_impossible_parameters() {
    assert!(matches!(
        OpusEncoder::new(44_100, 1, Application::Audio),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        OpusEncoder::new(48_000, 0, Application::Audio),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        OpusEncoder::new(48_000, 3, Application::Audio),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        OpusDecoder::new(44_100, 1),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        OpusDecoder::new(48_000, 3),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn undersized_output_buffer_is_an_error() {
    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let pcm = music_like(48_000, 960);
    let mut tiny = [0u8; 1];
    assert!(matches!(
        enc.encode(&pcm, 960, &mut tiny),
        Err(Error::BufferTooSmall { .. })
    ));
}

/// Random bytes must never panic the decoder. Every rejection has to come back
/// as an error, and anything accepted has to be finite.
#[test]
fn decoder_survives_arbitrary_input() {
    let mut rng = Lcg::new(0xD00D_F00D_1234_5678);
    for &(rate, channels) in &[(48_000i32, 1usize), (48_000, 2), (16_000, 1), (8_000, 2)] {
        let frame = (rate / 50) as usize;
        let mut dec = OpusDecoder::new(rate, channels).unwrap();
        for case in 0..2000 {
            let len = 1 + (rng.next_f32().abs() * 300.0) as usize;
            let packet: Vec<u8> = (0..len)
                .map(|_| ((rng.next_f32() + 1.0) * 127.5) as u8)
                .collect();
            let mut out = vec![0.0f32; frame * channels];
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                dec.decode(&packet, frame, &mut out)
            }));
            assert!(
                r.is_ok(),
                "{rate} Hz {channels}ch: decoder panicked on random case {case}"
            );
            if let Ok(Ok(_)) = r {
                assert!(
                    out.iter().all(|s| s.is_finite()),
                    "{rate} Hz: accepted a random packet but produced non-finite output"
                );
            }
        }
    }
}

/// Truncating a valid packet at every length must be safe. This is where a
/// length field that is trusted before it is checked shows up.
#[test]
fn truncated_packets_are_rejected_safely() {
    let rate = 48_000;
    let frame = 960;
    let src = music_like(rate, frame * 20);
    let stereo = interleave(&[src.clone(), src.clone()]);
    let packets = Codec::new(rate, 2, Application::Audio)
        .bitrate(96_000)
        .encode_all(&stereo);

    for pkt in &packets {
        let n = pkt.len();
        for cut in 0..n {
            let mut dec = OpusDecoder::new(rate, 2).unwrap();
            let mut out = vec![0.0f32; frame * 2];
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                dec.decode(&pkt[..cut], frame, &mut out)
            }));
            assert!(
                r.is_ok(),
                "panicked on a packet truncated to {cut} of {n} bytes"
            );
        }
    }
}

/// Flipping a single bit anywhere in a valid packet must not panic. A decoder
/// is allowed to produce noise here, never a crash.
#[test]
fn single_bit_flips_are_survivable() {
    let rate = 48_000;
    let frame = 960;
    let src = music_like(rate, frame * 8);
    let packets = Codec::new(rate, 1, Application::Audio)
        .bitrate(64_000)
        .encode_all(&src);

    for pkt in &packets {
        for byte in 0..pkt.len() {
            for bit in 0..8 {
                let mut corrupt = pkt.clone();
                corrupt[byte] ^= 1 << bit;
                let mut dec = OpusDecoder::new(rate, 1).unwrap();
                let mut out = vec![0.0f32; frame];
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    dec.decode(&corrupt, frame, &mut out)
                }));
                assert!(r.is_ok(), "panicked on bit {bit} of byte {byte}");
                if let Ok(Ok(_)) = r {
                    assert!(
                        out.iter().all(|s| s.is_finite()),
                        "non-finite output after a bit flip"
                    );
                }
            }
        }
    }
}

/// Extreme input must not produce non-finite output or NaN state.
#[test]
fn extreme_input_levels_are_handled() {
    let rate = 48_000;
    let frame = 960;
    let cases: [(&str, Vec<f32>); 4] = [
        (
            "full-scale square",
            (0..frame * 10)
                .map(|i| if (i / 32) % 2 == 0 { 1.0 } else { -1.0 })
                .collect(),
        ),
        (
            "over-range",
            (0..frame * 10)
                .map(|i| if i % 2 == 0 { 8.0 } else { -8.0 })
                .collect(),
        ),
        ("denormal", vec![1e-38f32; frame * 10]),
        ("dc offset", vec![0.9f32; frame * 10]),
    ];
    for (name, pcm) in cases {
        let decoded = Codec::new(rate, 1, Application::Audio)
            .bitrate(96_000)
            .roundtrip(&pcm)
            .decoded;
        assert!(
            decoded.iter().all(|s| s.is_finite()),
            "{name}: non-finite output"
        );
    }
}

/// Concealing more than 20 ms must not walk off the CELT decode buffer.
///
/// 20 ms is the longest frame CELT has, and its decode buffer holds exactly
/// one. The pitch branch of concealment indexes `DECODE_BUFFER_SIZE -
/// MAX_PERIOD - n`, which goes negative once `n` passes that, and the noise
/// branch slides the buffer by `n` and runs off the end of it. Nothing bounded
/// the request, so concealing a lost 40, 60 or 120 ms packet — which a caller
/// asks for by handing `decode` an empty slice, and which every stream coded at
/// those durations will eventually need — panicked instead. `decode_plc` now
/// conceals in 20 ms pieces as `opus_decoder.c:345` does.
///
/// The `decode_stream` fuzz target found this within a second of first running.
/// It is here as well as in `tests/fuzz-corpus/` because a named case says what
/// broke, where a corpus file only says that something did.
#[test]
fn concealment_longer_than_a_celt_frame_is_chunked() {
    for &rate in &RATES {
        for &channels in &[1usize, 2] {
            let frame = (rate / 50) as usize; // 20 ms, CELT's longest
            let mono = music_like(rate, frame * 4);
            let pcm = if channels == 2 {
                interleave(&[mono.clone(), mono.clone()])
            } else {
                mono
            };
            // 96 kb/s at 20 ms picks CELT, which is the mode that could not
            // conceal a long frame; the SILK path had a length check already.
            let packets = Codec::new(rate, channels, Application::Audio)
                .bitrate(96_000)
                .frame_samples(frame)
                .encode_all(&pcm);
            let mut dec = OpusDecoder::new(rate, channels).unwrap();
            let longest = rate as usize / 1000 * 120;
            let mut out = vec![0.0f32; longest * channels];
            dec.decode(&packets[0], longest, &mut out).unwrap();

            for &ms in &[25usize, 40, 60, 100, 120] {
                let want = rate as usize / 1000 * ms;
                let n = dec
                    .decode(&[], want, &mut out)
                    .unwrap_or_else(|e| panic!("{rate} Hz {channels}ch, {ms} ms: {e}"));
                assert_eq!(n, want, "{rate} Hz {channels}ch: concealed {n} of {want}");
                assert!(
                    out[..n * channels].iter().all(|s| s.is_finite()),
                    "{rate} Hz {channels}ch: non-finite sample concealing {ms} ms"
                );
            }
        }
    }
}

//! Encode -> decode fidelity across the supported configurations.
//!
//! Thresholds are set below measured values with margin, and they compare the
//! decoder's output against the encoder's *input*. SILK is a parametric coder,
//! so its waveform SNR is legitimately low; the assertions reflect what each
//! coding path actually achieves rather than one global number.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, Signal, packet};

/// CELT codes music near-transparently; measured 22.4 dB at 64 kb/s and 26.8 dB
/// at 128 kb/s, so these floors carry several dB of margin.
#[test]
fn celt_music_reconstructs_the_waveform() {
    for &(bitrate, min_snr, min_corr) in &[(64_000i32, 18.0f32, 0.99f32), (128_000, 23.0, 0.995)] {
        let rate = 48_000;
        let src = music_like(rate, rate as usize * 2);
        let r = Codec::new(rate, 1, Application::Audio)
            .bitrate(bitrate)
            .roundtrip(&src);
        assert!(
            r.modes().iter().all(|m| *m == "celt"),
            "expected CELT-only, got {:?}",
            r.modes()[0]
        );

        let skip = (rate / 50) as usize * 5;
        let snr = aligned_snr_db(&r.decoded[skip..], &src[skip..], (rate / 50) as usize);
        let (corr, _) = aligned_correlation(&r.decoded[skip..], &src[skip..], (rate / 50) as usize);
        assert!(
            snr >= min_snr,
            "{bitrate} bps: SNR {snr:.2} dB below {min_snr} dB"
        );
        assert!(
            corr >= min_corr,
            "{bitrate} bps: correlation {corr:.4} below {min_corr}"
        );
    }
}

/// SILK is parametric, so the bar is waveform *correlation* (measured 0.81-0.84)
/// and energy preservation, not SNR.
#[test]
fn silk_speech_preserves_the_signal() {
    for &(rate, bitrate) in &[(8_000i32, 12_000i32), (16_000, 16_000), (16_000, 24_000)] {
        let src = speech_like(rate, rate as usize * 2);
        let r = Codec::new(rate, 1, Application::Voip)
            .bitrate(bitrate)
            .roundtrip(&src);
        assert!(
            r.modes().iter().all(|m| *m == "silk"),
            "{rate} Hz: expected SILK-only"
        );

        let skip = (rate / 50) as usize * 5;
        let (corr, _) = aligned_correlation(&r.decoded[skip..], &src[skip..], (rate / 50) as usize);
        assert!(
            corr >= 0.70,
            "{rate} Hz {bitrate} bps: correlation {corr:.4} below 0.70"
        );

        // Output level must track the input; a decoder that halves or doubles
        // the level still correlates perfectly.
        let ratio = energy(&r.decoded[skip..]) / energy(&src[skip..]);
        assert!(
            (0.5..=2.0).contains(&ratio),
            "{rate} Hz: output/input energy ratio {ratio:.3} outside 0.5..2.0"
        );
    }
}

/// The encoder must respect its bitrate setting well enough to be useful; a
/// rate-control bug that ignores the target shows up here.
#[test]
fn bitrate_setting_controls_output_size() {
    let rate = 48_000;
    let src = music_like(rate, rate as usize * 2);
    let mut last = 0usize;
    for &bitrate in &[32_000i32, 64_000, 128_000] {
        let r = Codec::new(rate, 1, Application::Audio)
            .bitrate(bitrate)
            .roundtrip(&src);
        let actual = (r.bytes() * 8) as f32 / 2.0; // 2 seconds of audio
        assert!(
            actual > bitrate as f32 * 0.6 && actual < bitrate as f32 * 1.4,
            "{bitrate} bps target produced {actual:.0} bps"
        );
        assert!(
            r.bytes() > last,
            "raising the bitrate must not shrink the stream"
        );
        last = r.bytes();
    }
}

/// Silence in must be silence out — and must cost almost nothing, which is what
/// the CELT silence flag exists for.
#[test]
fn digital_silence_is_cheap_and_silent() {
    let rate = 48_000;
    let frames = 100;
    let src = vec![0.0f32; rate as usize * 2];
    let r = Codec::new(rate, 1, Application::Audio)
        .bitrate(64_000)
        .roundtrip(&src);

    let peak = r.decoded.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
    assert!(peak < 1e-4, "silence decoded to peak {peak:e}");
    assert!(
        r.bytes() < frames * 8,
        "{} bytes for {frames} silent frames; the silence flag is not working",
        r.bytes()
    );
}

/// A decoder fed the packets of a longer stream must stay in sync throughout —
/// this catches state that drifts rather than breaking immediately.
#[test]
fn long_stream_stays_in_sync() {
    let rate = 48_000;
    let src = music_like(rate, rate as usize * 10);
    let r = Codec::new(rate, 1, Application::Audio)
        .bitrate(96_000)
        .roundtrip(&src);

    // Compare the last second on its own: drift shows up at the end, and a
    // whole-stream metric averages it away.
    let tail = r.decoded.len() - rate as usize;
    let snr = aligned_snr_db(&r.decoded[tail..], &src[tail..], (rate / 50) as usize);
    assert!(
        snr >= 18.0,
        "final second SNR {snr:.2} dB — encoder/decoder state is drifting"
    );
}

/// Every frame size and rate combination the encoder accepts must round-trip.
#[test]
fn accepted_frame_sizes_round_trip() {
    let mut tested = 0;
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        // Tenths of a millisecond: 2.5 ms is not a whole number of them, and
        // 60 ms divides no sample rate evenly.
        for &tenth_ms in &[25i32, 50, 100, 200, 400, 600] {
            let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
            let src = speech_like(rate, frame * 30);
            let mut c = Codec::new(rate, 1, Application::Audio)
                .bitrate(48_000)
                .frame_samples(frame);
            // A rejected combination is robustness.rs's business, not this
            // test's; it only claims that what the encoder accepts round-trips.
            let Ok(packets) = c.try_encode_all(&src) else {
                continue;
            };
            let decoded = c.decode_all(&packets);
            assert_eq!(
                decoded.len(),
                src.len() / frame * frame,
                "{rate} Hz / {frame} samples"
            );
            assert!(packets.iter().all(|p| !p.is_empty()));
            tested += 1;
        }
    }
    assert!(tested >= 15, "only {tested} configurations exercised");
}

/// Every supported API sample rate must reconstruct the waveform, not merely
/// produce a decodable packet.
///
/// The CELT layer only implements the 48 kHz mode, so any lower rate is coded
/// by zero-stuffing up to 48 kHz and limiting the coded bands. Two things go
/// wrong if that is missing or half-done, and both are caught here: coding the
/// mirror image the zero-stuffing creates turns the output into full-scale
/// noise, and forgetting to scale the spectrum back up leaves it at exactly
/// `1 / upsample` of its proper amplitude. A 24 kHz encode used to decode to a
/// peak of over 600,000 for a source peaking at 0.4.
#[test]
fn every_sample_rate_reconstructs_the_waveform() {
    for &rate in &[48_000i32, 24_000, 16_000, 12_000, 8_000] {
        for &channels in &[1usize, 2] {
            let mono = music_like(rate, rate as usize * 2);
            let src = if channels == 1 {
                mono
            } else {
                interleave(&[mono.clone(), mono])
            };
            let r = Codec::new(rate, channels, Application::Audio)
                .bitrate(64_000)
                .roundtrip(&src);
            let label = format!("{rate} Hz/{channels}ch");

            let peak = r.decoded.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            assert!(
                peak <= 2.0,
                "{label}: decoded peak {peak:.1} — the output is noise, not audio"
            );

            let skip = (rate / 50) as usize * 5 * channels;
            let ratio = energy(&r.decoded[skip..]) / energy(&src[skip..]).max(1e-12);
            assert!(
                (0.85..=1.20).contains(&ratio),
                "{label}: output/input energy {ratio:.3} — expected roughly unity gain"
            );

            for c in 0..channels {
                let dec = deinterleave(&r.decoded[skip..], channels, c);
                let orig = deinterleave(&src[skip..], channels, c);
                let (corr, _) = aligned_correlation(&dec, &orig, (rate / 50) as usize);
                assert!(
                    corr >= 0.99,
                    "{label} ch{c}: correlation {corr:.4} below 0.99"
                );
            }
        }
    }
}

/// A forced bandwidth must not break the encoder at any sample rate.
///
/// SILK's internal rate follows the coded bandwidth, so narrowband from a
/// 48 kHz input means resampling 48 -> 8 kHz. When the encoder ignored the
/// bandwidth and ran SILK at its own rate anyway, the TOC advertised one
/// bandwidth while the encoder had coded another, and the decoder produced
/// noise — up to a peak of 113,007 for a source peaking at 0.5.
#[test]
fn every_forced_bandwidth_reconstructs_the_waveform() {
    let bandwidths = [
        Bandwidth::Narrowband,
        Bandwidth::Mediumband,
        Bandwidth::Wideband,
        Bandwidth::Superwideband,
        Bandwidth::Fullband,
    ];
    for &rate in &[48_000i32, 24_000, 16_000, 12_000, 8_000] {
        for bw in bandwidths {
            for &app in &[Application::Audio, Application::Voip] {
                let src = speech_like(rate, rate as usize);
                let frame = (rate / 50) as usize;
                let decoded = Codec::new(rate, 1, app)
                    .bitrate(48_000)
                    .bandwidth(bw)
                    .roundtrip(&src)
                    .decoded;

                let label = format!("{rate} Hz {bw:?} {app:?}");
                let peak = decoded.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
                assert!(
                    peak <= 2.0,
                    "{label}: decoded peak {peak:.1} — the output is noise, not audio"
                );
                let skip = frame * 5;
                let ratio = energy(&decoded[skip..]) / energy(&src[skip..]).max(1e-12);
                assert!(
                    (0.3..=3.0).contains(&ratio),
                    "{label}: output/input energy {ratio:.3} — a band-limited \
                     encode still has to come back at roughly the input level"
                );
                let (corr, _) = aligned_correlation(&decoded[skip..], &src[skip..], frame);
                // The peak and energy checks above are the real guard: every
                // failure mode this test exists for showed up as noise or as a
                // fixed fraction of the right amplitude. Correlation is a
                // backstop, and its bar is loose because hybrid mode on this
                // synthetic buzz legitimately reaches only ~0.66.
                assert!(corr >= 0.60, "{label}: correlation {corr:.4} below 0.60");
            }
        }
    }
}

/// Every packet duration Opus defines must work at every sample rate, not just
/// at 48 kHz.
///
/// The short durations are CELT-only, and CELT here has just the 48 kHz mode.
/// Until it learned to code lower rates by zero-stuffing, a 2.5 ms frame at
/// 16 kHz was 40 samples, matched no `lm`, and was rejected outright — so the
/// set of usable frame sizes silently depended on the sample rate.
///
/// 60 ms is the one duration that divides no sample rate evenly (48000 / 2880
/// is 16.67), which is why the encoder used to reject it outright. 80, 100 and
/// 120 ms have no single-frame configuration at any rate and were rejected until
/// the encoder learned to split a packet; `multiframe_packets.rs` covers that
/// framing, and this test asks only that the audio survives it.
#[test]
fn all_frame_durations_work_at_all_sample_rates() {
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        // Tenths of a millisecond, so 2.5 ms stays exact.
        for &tenth_ms in &[25i32, 50, 100, 200, 400, 600, 800, 1000, 1200] {
            let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
            // About a second of audio at every duration, rather than a fixed
            // packet count: 40 packets of 120 ms is 4.8 seconds, and running
            // that at every rate costs more than it proves.
            let packets = 40.min((10_000 / tenth_ms).max(12) as usize);
            let src = music_like(rate, frame * packets);
            let label = format!("{rate} Hz {}ms", tenth_ms as f32 / 10.0);
            let decoded = Codec::new(rate, 1, Application::Audio)
                .bitrate(64_000)
                .frame_samples(frame)
                .roundtrip(&src)
                .decoded;

            let peak = decoded.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            assert!(peak <= 2.0, "{label}: decoded peak {peak:.1}");
            let skip = frame * 3;
            // Search up to 20 ms, not one frame. The codec's delay is a property
            // of the codec — `Fs/400` of CELT overlap plus the `Fs/250` the CELT
            // input lags by, and more again through SILK's resampler — so at
            // 2.5 ms the delay is longer than the frame and a search bounded by
            // `frame` cannot find the alignment it is supposed to remove.
            let max_lag = frame.max((rate / 50) as usize);
            let (corr, _) = aligned_correlation(&decoded[skip..], &src[skip..], max_lag);
            assert!(corr >= 0.95, "{label}: correlation {corr:.4} below 0.95");
        }
    }
}

/// Changing the frame size on a live encoder must leave the stream intact.
///
/// Two separate defects lived here, both of which stopped the output following
/// the input from the switch onwards and never recovered:
///
/// - SILK is configured from the frame duration (`silk_setup_fs`): 20 -> 40 ms
///   leaves the 20 ms internal frame alone but doubles the frames per packet,
///   while 20 -> 10 ms halves the subframe count. The encoder reconfigured SILK
///   only when the internal *sample rate* changed, so after a duration change it
///   kept coding one 20 ms frame into a packet whose TOC announced two, and the
///   decoder read the second frame out of whatever followed.
/// - A 10 ms frame at 12 kHz is the one length that is not a whole number of
///   shell blocks, and the encoder signed the eight pulses past the frame with
///   whatever the previous, longer frame had left there. That is why the grid
///   below covers every rate rather than a representative one.
///
/// Every ordered pair of SILK's four durations is exercised at every sample
/// rate, mono and stereo, and each switched stream is held against the same
/// encoder run at the destination duration throughout. That reference is what
/// makes the threshold meaningful: it is the same content, same settings, same
/// measurement window, differing only in that it never switched.
#[test]
fn changing_frame_size_mid_stream_keeps_the_stream() {
    /// Encode `src` through one encoder whose frame size follows `segments`
    /// (duration in ms, packet count), decode, and report the coded modes.
    fn coded(
        rate: i32,
        channels: usize,
        src: &[f32],
        segments: &[(usize, usize)],
    ) -> (Vec<f32>, Vec<&'static str>) {
        let mut c = Codec::new(rate, channels, Application::Audio).bitrate(64_000);
        let (mut decoded, mut modes, mut pos) = (Vec::new(), Vec::new(), 0usize);
        for &(ms, count) in segments {
            // The point of the test: the same encoder changes frame size here.
            c.frame = rate as usize / 1000 * ms;
            let width = c.frame * channels;
            // `count` is usize::MAX for "to the end of the input".
            let end = width
                .saturating_mul(count)
                .saturating_add(pos)
                .min(src.len());
            let packets = c.encode_all(&src[pos..end]);
            pos += packets.len() * width;
            modes.extend(packets.iter().map(|p| packet_mode(p)));
            decoded.extend(c.decode_all(&packets));
        }
        (decoded, modes)
    }

    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        for &channels in &[1usize, 2] {
            // Long enough that the window measured after a 60 ms switch is
            // still a few hundred milliseconds of audio.
            let n = (rate as usize * 6) / 5;
            let left = speech_like(rate, n);
            let src = if channels == 1 {
                left
            } else {
                // Independent content per channel, so a stream that quietly
                // collapses to mono cannot pass.
                let mut right = music_like(rate, n + rate as usize / 2);
                right.drain(..rate as usize / 2);
                interleave(&[left, right])
            };

            // One unswitched run per destination duration, reused as the
            // reference for every pair that ends there.
            let mut reference = std::collections::BTreeMap::new();
            for &ms in &[10usize, 20, 40, 60] {
                reference.insert(ms, coded(rate, channels, &src, &[(ms, usize::MAX)]).0);
            }

            for &(from, to) in &[
                (10usize, 20usize),
                (20, 10),
                (20, 40),
                (40, 20),
                (10, 40),
                (40, 10),
                (10, 60),
                (60, 10),
                (20, 60),
                (60, 20),
                (40, 60),
                (60, 40),
            ] {
                let before = (n / 2) / (rate as usize / 1000 * from);
                let switch_at = before * (rate as usize / 1000 * from);
                let (switched, modes) =
                    coded(rate, channels, &src, &[(from, before), (to, usize::MAX)]);
                let plain = &reference[&to];

                // Start measuring four destination frames past the switch, so
                // this is about the steady state and not the seam.
                let tail = switch_at + rate as usize / 1000 * to * 4;
                let (mut switched_corr, mut plain_corr) = (1.0f32, 1.0f32);
                for c in 0..channels {
                    let s = deinterleave(&src, channels, c);
                    let a = deinterleave(&switched, channels, c);
                    let b = deinterleave(plain, channels, c);
                    let end = s.len().min(a.len()).min(b.len());
                    let lag = rate as usize / 100;
                    switched_corr =
                        switched_corr.min(aligned_correlation(&a[tail..end], &s[tail..end], lag).0);
                    plain_corr =
                        plain_corr.min(aligned_correlation(&b[tail..end], &s[tail..end], lag).0);
                }

                let label = format!("{rate} Hz {channels}ch {from}->{to}ms");
                if modes[before - 1] == modes[before] {
                    // Same coding mode on both sides: the switched stream has to
                    // match the unswitched one, not merely sound acceptable.
                    // Measured worst case is 0.979 with a delta of -0.001.
                    assert!(
                        switched_corr >= plain_corr - 0.02,
                        "{label}: correlation {switched_corr:.4} against {plain_corr:.4} \
                         for the same stream that never switched"
                    );
                    assert!(
                        switched_corr >= 0.95,
                        "{label}: correlation {switched_corr:.4} below 0.95"
                    );
                } else {
                    // The frame size also moved the encoder between SILK and
                    // CELT, and a mode change costs a cross-fade the reference
                    // never pays. Measured worst case is 0.905.
                    assert!(
                        switched_corr >= 0.85,
                        "{label}: correlation {switched_corr:.4} below 0.85 \
                         across a {} -> {} mode change",
                        modes[before - 1],
                        modes[before]
                    );
                }
            }
        }
    }
}

/// A 60 ms SILK packet must carry all three of the SILK frames it announces.
///
/// 60 ms is the only Opus duration that divides no sample rate evenly — 48000
/// samples per second over 2880 per frame is 16.67 — so the encoder used to
/// reject it outright rather than compute a frame rate it could not represent.
/// It is also the only duration SILK packs as three sub-frames in one *coded
/// frame* (10 and 20 ms pack one, 40 ms two), which makes the third reachable
/// for the first time. That is a SILK-internal structure, not Opus framing:
/// the packet here is a single code 0 frame, which is why the configuration is
/// pinned to SILK rather than left to the mode decision. CELT and hybrid reach
/// 60 ms the other way, as three 20 ms frames sharing a TOC —
/// `packet_format.rs` covers that framing.
///
/// Correlating the stream as a whole would not prove that: a third frame that
/// came out silent, doubled, or filled with the second frame's audio still
/// leaves two thirds of the stream correct, which is enough to pass a
/// whole-stream threshold. So every 20 ms third of every packet is measured
/// against the input it was coded from.
#[test]
fn sixty_ms_packets_carry_all_three_silk_frames() {
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        for &channels in &[1usize, 2] {
            let frame = rate as usize * 60 / 1000;
            let third = frame / 3;
            let packets = 20;
            let n = frame * packets;
            let src = if channels == 1 {
                music_like(rate, n)
            } else {
                // The two channels are the same material half a second apart,
                // which decorrelates them — a stream that quietly collapsed to
                // mono could not correlate with both.
                let shift = rate as usize / 2;
                let long = music_like(rate, n + shift);
                interleave(&[long[..n].to_vec(), long[shift..shift + n].to_vec()])
            };

            let label = format!("{rate} Hz {channels}ch");
            let mut c = Codec::new(rate, channels, Application::Voip)
                .signal_type(Signal::Voice)
                .bandwidth(Bandwidth::Wideband)
                .bitrate(48_000)
                .frame_samples(frame);
            let coded = c.encode_all(&src);
            for (f, p) in coded.iter().enumerate() {
                // Read straight from the TOC, independently of what the encoder
                // thinks it did: 60 ms is 2880 samples at 48 kHz, and SILK is
                // the only mode with a configuration for it in one frame.
                assert_eq!(
                    packet_samples_48k(p),
                    2880,
                    "{label}: packet {f} declares {} samples at 48 kHz",
                    packet_samples_48k(p)
                );
                assert_eq!(packet_mode(p), "silk", "{label}: packet {f} is not SILK");
                assert_eq!(
                    packet::frame_count(p).unwrap(),
                    1,
                    "{label}: packet {f} is not a single coded frame"
                );
            }
            let decoded = c.decode_all(&coded);

            for c in 0..channels {
                let s = deinterleave(&src, channels, c);
                let d = deinterleave(&decoded, channels, c);
                // One lag for the whole stream: the codec delay is fixed, and
                // searching per third would let a misplaced frame realign
                // itself and pass.
                let skip = frame * 2;
                let (_, lag) = aligned_correlation(&d[skip..], &s[skip..], rate as usize / 50);
                for f in 2..packets {
                    for k in 0..3 {
                        let start = f * frame + k * third;
                        if start + lag + third > d.len() {
                            break;
                        }
                        let want = &s[start..start + third];
                        let got = &d[start + lag..start + lag + third];
                        // Measured worst case across the grid is 0.971. The
                        // wideband clamp that keeps this SILK-only is what puts
                        // it below the 0.99 an unclamped encode reaches: at
                        // 24 and 48 kHz everything above 8 kHz is not coded.
                        let corr = correlation(got, want);
                        assert!(
                            corr >= 0.95,
                            "{label} ch{c}: packet {f} frame {k} of 3 correlates {corr:.4} \
                             with the audio it was coded from"
                        );
                    }
                }
            }
        }
    }
}

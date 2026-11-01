//! Opus packet framing: the TOC byte, the four frame-packing codes, and the
//! repacketizer (RFC 6716 §3).
//!
//! These assert on the *bytes*, so a change in how frames are packed shows up
//! here rather than as a mysterious interop failure.

mod common;
use common::*;
use opus_pure::{
    Application, Bandwidth, Error, OpusDecoder, RateControl, Repacketizer, Signal, packet,
    repacketizer,
};

fn encode_frames(
    rate: i32,
    channels: usize,
    app: Application,
    bitrate: i32,
    count: usize,
) -> Vec<Vec<u8>> {
    encode_frames_of(rate, channels, app, bitrate, count, (rate / 50) as usize)
}

fn encode_frames_of(
    rate: i32,
    channels: usize,
    app: Application,
    bitrate: i32,
    count: usize,
    frame: usize,
) -> Vec<Vec<u8>> {
    let mono = music_like(rate, frame * count);
    let pcm = if channels == 2 {
        interleave(&[mono.clone(), mono])
    } else {
        mono
    };
    Codec::new(rate, channels, app)
        .frame_samples(frame)
        .bitrate(bitrate)
        .encode_all(&pcm)
}

/// Single-frame packets must use code 0 and declare their own configuration.
#[test]
fn single_frame_packets_use_code_0() {
    for &(rate, channels, app) in &[
        (48_000i32, 1usize, Application::Audio),
        (48_000, 2, Application::Audio),
        (16_000, 1, Application::Voip),
    ] {
        for pkt in encode_frames(rate, channels, app, 64_000, 10) {
            assert_eq!(pkt[0] & 0x03, 0, "expected frame-packing code 0");
            assert_eq!(packet_is_stereo(&pkt), channels == 2);
            assert_eq!(packet::frame_count(&pkt).unwrap(), 1);
        }
    }
}

/// A packet must hold exactly the audio the encoder was asked for: its frame
/// count times the per-frame duration its TOC declares.
///
/// The two factors move independently. Durations up to 20 ms are always one
/// frame, so the TOC alone carries the answer. Past that only SILK has a
/// single-frame configuration, and 80, 100 and 120 ms have none at all, so the
/// encoder splits the packet and the TOC then describes one *frame* of it. A
/// split that lost or duplicated a frame, or one whose TOC named the packet's
/// duration rather than the frame's, would leave the decoder writing the wrong
/// number of samples.
#[test]
fn toc_duration_matches_the_encoder_frame_size() {
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        // Tenths of a millisecond, because 2.5 ms is not a whole number of them.
        for &tenth_ms in &[25i32, 50, 100, 200, 400, 600, 800, 1000, 1200] {
            let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
            let pcm = music_like(rate, frame * 4);
            let packets = Codec::new(rate, 1, Application::Audio)
                .frame_samples(frame)
                .bitrate(48_000)
                .try_encode_all(&pcm)
                .unwrap_or_else(|e| panic!("{rate} Hz / {frame} samples rejected: {e:?}"));
            let per_frame = packet::samples_per_frame(&packets[0], rate).unwrap();
            let frames = packet::frame_count(&packets[0]).unwrap();
            assert_eq!(
                frames * per_frame,
                frame,
                "{rate} Hz / {frame} samples: TOC declares {frames} x {per_frame}"
            );
        }
    }
}

/// The repacketizer must combine same-configuration packets and the decoder must
/// read the result back as the original frames.
#[test]
fn repacketizer_combines_and_round_trips() {
    let (rate, frame) = (48_000i32, 960usize);
    let packets = encode_frames(rate, 1, Application::Audio, 64_000, 6);

    for group in [2usize, 3] {
        let mut rp = Repacketizer::new();
        for pkt in packets.iter().take(group) {
            rp.cat(pkt).expect("packets share a configuration");
        }
        let combined = rp.out_range(0, group).expect("repacketize");

        assert_eq!(packet::frame_count(&combined).unwrap(), group);
        assert_eq!(
            combined[0] >> 3,
            packets[0][0] >> 3,
            "repacketizing must preserve the configuration"
        );

        // The combined packet decodes to `group` frames of audio.
        let mut dec = OpusDecoder::new(rate, 1).unwrap();
        let mut out = vec![0.0f32; frame * group];
        let n = dec.decode(&combined, frame * group, &mut out).unwrap();
        assert_eq!(n, frame * group);
        assert!(out.iter().all(|s| s.is_finite()));
    }
}

/// 60 ms is where the repacketizer's 120 ms ceiling first bites: two packets of
/// it fit exactly, three do not.
///
/// The ceiling is RFC 6716 §3.1's — no Opus packet may carry more than 120 ms of
/// audio — and at every shorter duration it is far enough away that nothing here
/// would notice a repacketizer that had forgotten it.
///
/// A 60 ms packet reaches the ceiling by either of the two routes the format
/// allows, so both are checked: SILK codes it as one long frame, and CELT as
/// three 20 ms frames sharing a TOC. The repacketizer counts frames, not
/// packets, so the second case arrives as six frames rather than two and would
/// slip past a ceiling that had only ever been tried on the first.
#[test]
fn repacketizer_stops_at_two_sixty_millisecond_frames() {
    let (rate, frame) = (48_000i32, 2880usize);
    let mono = music_like(rate, frame * 3);

    // (label, how the 60 ms is framed, frames per packet)
    let cases: [(&str, Codec, usize); 2] = [
        (
            "silk, one 60 ms frame",
            Codec::new(rate, 1, Application::Voip)
                .signal_type(Signal::Voice)
                .bandwidth(Bandwidth::Wideband)
                .bitrate(48_000)
                .frame_samples(frame),
            1,
        ),
        (
            "celt, three 20 ms frames",
            Codec::new(rate, 1, Application::Audio)
                .bitrate(64_000)
                .frame_samples(frame),
            3,
        ),
    ];

    for (label, mut codec, per_packet) in cases {
        let packets = codec.encode_all(&mono);
        for pkt in &packets {
            assert_eq!(packet_samples_48k(pkt), 2880, "{label}: not a 60 ms packet");
            assert_eq!(
                packet::frame_count(pkt).unwrap(),
                per_packet,
                "{label}: unexpected framing"
            );
        }

        let mut rp = Repacketizer::new();
        rp.cat(&packets[0]).expect("first 60 ms packet");
        rp.cat(&packets[1])
            .unwrap_or_else(|e| panic!("{label}: two 60 ms packets are exactly 120 ms: {e:?}"));
        let combined = rp
            .out_range(0, 2 * per_packet)
            .unwrap_or_else(|e| panic!("{label}: repacketize 2 x 60 ms: {e:?}"));
        assert_eq!(packet::frame_count(&combined).unwrap(), 2 * per_packet);
        assert_eq!(packet_samples_48k(&combined), 5760, "{label}");

        let mut dec = OpusDecoder::new(rate, 1).unwrap();
        let mut out = vec![0.0f32; frame * 2];
        assert_eq!(
            dec.decode(&combined, frame * 2, &mut out).unwrap(),
            frame * 2,
            "{label}"
        );
        assert!(out.iter().all(|s| s.is_finite()), "{label}");

        // A third packet would make 180 ms, which no Opus packet may carry.
        assert!(
            rp.cat(&packets[2]).is_err(),
            "{label}: the repacketizer accepted 180 ms in one packet"
        );
    }
}

/// Packets with different configurations cannot share one Opus packet.
#[test]
fn repacketizer_rejects_mixed_configurations() {
    let a = encode_frames(48_000, 1, Application::Audio, 64_000, 1);
    let b = encode_frames(16_000, 1, Application::Voip, 24_000, 1);
    assert_ne!(
        a[0][0] >> 3,
        b[0][0] >> 3,
        "test needs two different configurations"
    );

    let mut rp = Repacketizer::new();
    rp.cat(&a[0]).unwrap();
    assert!(
        rp.cat(&b[0]).is_err(),
        "mixing configurations must be refused"
    );
}

/// Padding must be transparent: a padded packet decodes exactly like the
/// original, and unpadding recovers it.
#[test]
fn padding_is_transparent() {
    let packets = encode_frames(48_000, 1, Application::Audio, 64_000, 4);
    for pkt in &packets {
        for extra in [1usize, 5, 64, 300] {
            let mut padded = pkt.clone();
            repacketizer::pad_packet(&mut padded, pkt.len() + extra).unwrap();
            assert_eq!(padded.len(), pkt.len() + extra);

            let unpadded = repacketizer::unpad_packet(&padded).unwrap();
            assert_eq!(
                &unpadded, pkt,
                "unpadding did not recover the original packet"
            );

            let mut a = OpusDecoder::new(48_000, 1).unwrap();
            let mut b = OpusDecoder::new(48_000, 1).unwrap();
            let (mut oa, mut ob) = (vec![0.0f32; 960], vec![0.0f32; 960]);
            a.decode(pkt, 960, &mut oa).unwrap();
            b.decode(&padded, 960, &mut ob).unwrap();
            assert_eq!(oa, ob, "padding changed the decoded audio");
        }
    }
}

/// A packet claiming more audio than the caller's buffer holds must be refused,
/// not truncated — silently dropping audio is worse than an error.
#[test]
fn oversized_packet_duration_is_refused() {
    let packets = encode_frames(48_000, 1, Application::Audio, 64_000, 3);
    let mut rp = Repacketizer::new();
    for p in &packets {
        rp.cat(p).unwrap();
    }
    let combined = rp.out_range(0, 3).unwrap();

    let mut dec = OpusDecoder::new(48_000, 1).unwrap();
    let mut too_small = vec![0.0f32; 960]; // room for one frame, packet holds three
    assert!(matches!(
        dec.decode(&combined, 960, &mut too_small),
        Err(Error::BufferTooSmall { .. })
    ));
}

/// Malformed frame-packing must be rejected with `InvalidPacket`.
#[test]
fn malformed_packing_is_rejected() {
    let mut dec = OpusDecoder::new(48_000, 1).unwrap();
    let mut out = vec![0.0f32; 960 * 6];

    // Code 3 with a zero frame count.
    assert!(matches!(
        dec.decode(&[0x83, 0x00], 960 * 6, &mut out),
        Err(Error::InvalidPacket(_))
    ));
    // Code 3 claiming more than 120 ms of audio (48 frames of 20 ms).
    assert!(matches!(
        dec.decode(&[0x83, 0x30, 1], 960 * 6, &mut out),
        Err(Error::InvalidPacket(_))
    ));
    // Code 1 whose payload cannot split in two (odd length).
    assert!(matches!(
        dec.decode(&[0x81, 0x00], 960 * 6, &mut out),
        Err(Error::InvalidPacket(_))
    ));
    // Code 2 with no length byte.
    assert!(matches!(
        dec.decode(&[0x82], 960 * 6, &mut out),
        Err(Error::InvalidPacket(_))
    ));
}

/// A code 1 packet with no payload is two zero-length frames, not a malformed
/// packet: `libopus`'s `opus_packet_parse` accepts it and conceals both frames.
/// The decoder used to carry its own parser that rejected it.
#[test]
fn code_1_with_no_payload_is_two_dtx_frames() {
    let mut dec = OpusDecoder::new(48_000, 1).unwrap();
    let mut out = vec![0.0f32; 960 * 6];
    // TOC 0x80: config 16 (CELT-only, NB, 2.5 ms), mono, code 1 -> 2 x 2.5 ms.
    let n = dec.decode(&[0x81], 960 * 6, &mut out).unwrap();
    assert_eq!(n, 2 * 120);
    assert!(out[..n].iter().all(|s| s.is_finite()));
}

/// An empty packet means "frame lost" and runs packet-loss concealment, which is
/// how `libopus` signals a drop.
///
/// Concealment extrapolates the last good frame and fades: measured against the
/// energy of the frame before the loss, the first concealed frame comes back at
/// 0.23-0.64 depending on the coding path, and each one after it is quieter
/// than the last. SILK fades faster than CELT, which looks like a defect and is
/// not: `silk_PLC_conceal` attenuates by `HARM_ATT_Q15` and
/// `PLC_RAND_ATTENUATE_*` once per *subframe*, and `silk_CNG` adds nothing back
/// because `CNG_smth_Gain_Q16` only updates on `TYPE_NO_VOICE_ACTIVITY` frames,
/// which an unbroken signal never produces.
///
/// Those numbers were checked against libopus 1.6.1's own decoder rather than
/// against a reading of the source: the same packets fed to `opus_decode_float`
/// with the same frames dropped conceal to within 0.7-1.1x of these energies at
/// every position, across SILK, hybrid and CELT.
#[test]
fn empty_packet_triggers_packet_loss_concealment() {
    for &(rate, app, bitrate) in &[
        (48_000i32, Application::Audio, 96_000i32),
        (24_000, Application::Voip, 32_000),
        (16_000, Application::Voip, 24_000),
        (8_000, Application::Voip, 16_000),
    ] {
        let frame = (rate / 50) as usize;
        let packets = encode_frames(rate, 1, app, bitrate, 12);
        let mut dec = OpusDecoder::new(rate, 1).unwrap();
        let mut last = vec![0.0f32; frame];
        for pkt in &packets {
            dec.decode(pkt, frame, &mut last).unwrap();
        }

        let good = energy(&last);
        let mut prev = good;
        for lost in 0..5 {
            let mut concealed = vec![0.0f32; frame];
            let n = dec.decode(&[], frame, &mut concealed).expect("PLC");
            assert_eq!(n, frame, "{rate} Hz: concealment must fill the whole frame");
            assert!(
                concealed.iter().all(|s| s.is_finite()),
                "{rate} Hz: non-finite sample in concealed frame {lost}"
            );
            let e = energy(&concealed);

            // The fade only ever goes down. A concealed frame louder than the
            // one before it is extrapolation running away, which is what the
            // attenuation ladder exists to prevent.
            assert!(
                e <= prev * 1.2,
                "{rate} Hz: concealed frame {lost} is {:.2}x the frame before it",
                e / prev.max(1e-20)
            );
            if lost == 0 {
                // Measured 0.23 (16 kHz SILK) to 0.64 (48 kHz CELT). A first
                // concealed frame at the noise floor means the extrapolation
                // bailed out and emitted silence instead of audio.
                assert!(
                    e >= good * 0.05,
                    "{rate} Hz: first concealed frame is {:.4} of the last good \
                     frame — concealment produced silence, not audio",
                    e / good.max(1e-20)
                );
            }
            prev = e;
        }

        // Decoding must recover once real packets resume.
        let mut recovered = vec![0.0f32; frame];
        dec.decode(&packets[0], frame, &mut recovered).unwrap();
        assert!(recovered.iter().all(|s| s.is_finite()));
    }
}

/// `force_bandwidth` must never talk the encoder into coding above the input's
/// Nyquist rate, with one exception that libopus shares.
///
/// It is a user override that bypasses the automatic selection, so the
/// sampling-rate clamp has to be applied after it — libopus does the same last
/// thing. Before that clamp existed, asking for superwideband at 8 kHz failed
/// with a misleading complaint about frame sizes, and asking for mediumband at
/// 8 kHz produced a packet that decoded to full-scale noise rather than audio.
///
/// The exception: CELT has no mediumband configuration. libopus applies the
/// Nyquist clamps (`opus_encoder.c:1643-1651`) and *then* widens a surviving
/// mediumband request to wideband (`:1681`). At 12 kHz the clamp caps every
/// request at mediumband, so under CELT *any* request from mediumband up ends
/// as wideband. Measured against libopus 1.6.1 at 12 kHz, 48 kb/s, music:
///
/// | forced | libopus TOC |
/// | --- | --- |
/// | narrowband | config 19 — CELT narrowband |
/// | mediumband, wideband, superwideband, fullband | config 23 — CELT wideband |
///
/// Nothing above the input's Nyquist is actually coded, since those bands are
/// empty; the TOC field names the mode's band layout, not a claim about content.
/// The case only became reachable here when the mode decision stopped forcing
/// SILK below 24 kHz.
#[test]
fn forced_bandwidth_is_capped_by_the_sampling_rate() {
    let forced = [
        Bandwidth::Narrowband,
        Bandwidth::Mediumband,
        Bandwidth::Wideband,
        Bandwidth::Superwideband,
        Bandwidth::Fullband,
    ];
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        // What the input can actually carry, at half the sampling rate.
        let nyquist = rate / 2;
        for bw in forced {
            let frame = (rate / 50) as usize;
            let src = music_like(rate, frame * 12);
            let packets = Codec::new(rate, 1, Application::Audio)
                .bitrate(48_000)
                .bandwidth(bw)
                .encode_all(&src);
            for pkt in &packets {
                let coded = packet_bandwidth_hz(pkt);
                if coded <= nyquist {
                    continue;
                }
                // The only documented way past Nyquist, and it has to land
                // exactly on wideband rather than merely "somewhere higher".
                assert!(
                    packet_mode(pkt) == "celt" && bw != Bandwidth::Narrowband,
                    "{rate} Hz forcing {bw:?}: coded {coded} Hz, above the \
                     {nyquist} Hz the input can carry, and this is not the CELT \
                     mediumband widening"
                );
                assert_eq!(
                    coded, 8_000,
                    "{rate} Hz forcing {bw:?}: CELT has no mediumband, so the \
                     clamped request must widen to wideband and no further"
                );
            }
        }
    }
}

/// RFC 6716 §3.4 caps a single Opus frame at 1275 bytes, and libopus rejects a
/// packet whose last frame is larger (`opus.c`: `if (last_size > 1275) return
/// OPUS_INVALID_PACKET`). A CBR target can legitimately exceed that — 60 ms
/// stereo at 192 kbps asks for 1440 bytes — so the encoder has to spend the
/// surplus on code 3 padding rather than on an over-long frame.
///
/// Sweeping the whole configuration space keeps this honest: encoding at a
/// bitrate high enough to overflow the limit used to emit code 0 packets of up
/// to 1623 bytes that no other Opus implementation would accept.
#[test]
fn no_frame_exceeds_the_1275_byte_limit() {
    let mut oversized = Vec::new();

    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        // Tenths of a millisecond, so 2.5 ms stays an integer.
        for &tenth_ms in &[25i32, 50, 100, 200, 400, 600] {
            let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
            for &app in &[Application::Voip, Application::Audio] {
                for &bitrate in &[64_000i32, 128_000, 192_000, 256_000, 510_000] {
                    for &channels in &[1usize, 2] {
                        for &cbr in &[false, true] {
                            let mono = music_like(rate, frame * 8);
                            let pcm = if channels == 2 {
                                interleave(&[mono.clone(), speech_like(rate, frame * 8)])
                            } else {
                                mono
                            };
                            let mut c = Codec::new(rate, channels, app)
                                .frame_samples(frame)
                                .bitrate(bitrate);
                            c.enc.rate_control = if cbr {
                                RateControl::Cbr
                            } else {
                                RateControl::ConstrainedVbr
                            };
                            // The encoder is free to reject a configuration;
                            // that is robustness.rs's concern, not this test's.
                            let Ok(packets) = c.try_encode_all(&pcm) else {
                                continue;
                            };

                            let label = format!(
                                "{rate}Hz/{}ms/{app:?}/{bitrate}/{channels}ch/{}",
                                tenth_ms as f32 / 10.0,
                                if cbr { "cbr" } else { "vbr" }
                            );
                            for pkt in &packets {
                                // Split the packet back into one code 0 packet
                                // per frame; each is its TOC byte plus that
                                // frame, so the frame is one byte shorter. This
                                // goes through the public repacketizer rather
                                // than the crate's internal parser, which is
                                // what a caller checking the same thing has.
                                let mut rp = repacketizer::Repacketizer::new();
                                match rp.cat(pkt) {
                                    Ok(()) => {
                                        for i in 0..rp.nb_frames() {
                                            let len = match rp.out_range(i, i + 1) {
                                                Ok(f) => f.len() - 1,
                                                Err(e) => {
                                                    oversized.push(format!(
                                                        "{label}: frame {i} unreadable: {e:?}"
                                                    ));
                                                    continue;
                                                }
                                            };
                                            if len > 1275 {
                                                oversized
                                                    .push(format!("{label}: frame of {len} bytes"));
                                            }
                                        }
                                    }
                                    Err(e) => oversized.push(format!(
                                        "{label}: {}-byte packet rejected: {e:?}",
                                        pkt.len()
                                    )),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        oversized.is_empty(),
        "{} configurations emitted a packet libopus would refuse:\n{}",
        oversized.len(),
        oversized.join("\n")
    );
}

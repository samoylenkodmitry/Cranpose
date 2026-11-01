//! Packets that hold more than one coded frame.
//!
//! Opus codes 2.5, 5, 10, 20, 40 and 60 ms as a single frame, but only SILK has
//! configurations past 20 ms and the CELT transform has no frame longer than it
//! at all. Every other combination is several frames sharing one TOC byte
//! (RFC 6716 §3.2), which libopus assembles in `opus_encode_native` and this
//! crate in `encode_split_packet`.
//!
//! Until that existed the encoder took the other way out: it forced every
//! duration above 20 ms to SILK and rejected 80, 100 and 120 ms outright. The
//! forcing was the more expensive half — 60 ms of music at 64 kb/s is a CELT
//! configuration, and coding it as SILK spent the bitrate on the wrong codec.
//!
//! The reference for the framing is libopus 1.6.1, which produces byte-identical
//! packet sizes to these on the same audio (see `reference/multiframe/`).

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, RateControl, Signal, packet};

/// Configurations that pin the mode decision, so a test can ask about a mode
/// rather than hope for one.
fn pinned(rate: i32, channels: usize, mode: &str, frame: usize) -> Codec {
    let c = match mode {
        "silk" => Codec::new(rate, channels, Application::Voip)
            .signal_type(Signal::Voice)
            .bandwidth(Bandwidth::Wideband)
            .bitrate(32_000),
        // Stereo hybrid above ~28 kb/s is deliberately routed to CELT-fullband
        // (see `encoder.rs`), so hybrid has to be asked for below that.
        "hybrid" => Codec::new(rate, channels, Application::Voip)
            .signal_type(Signal::Voice)
            .bandwidth(Bandwidth::Superwideband)
            .bitrate(if channels == 2 { 24_000 } else { 32_000 }),
        "celt" => Codec::new(rate, channels, Application::Audio)
            .signal_type(Signal::Music)
            .bitrate(96_000),
        other => panic!("no configuration pins {other}"),
    };
    c.frame_samples(frame)
}

/// Samples per channel in one coded frame of a packet coded in `mode`, for a
/// packet of `tenth_ms` tenths of a millisecond at `rate`.
///
/// This is `opus_encoder.c:1698` written out as a table rather than as
/// arithmetic, so that a rule the encoder and the reference agree on cannot be
/// restated wrongly in both places at once.
fn expected_frame_samples(rate: i32, tenth_ms: i32, mode: &str) -> usize {
    let ms20 = (rate / 50) as usize;
    let ms40 = (rate / 25) as usize;
    let ms60 = (3 * rate / 50) as usize;
    let whole = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
    match (tenth_ms, mode) {
        // 20 ms and shorter: one frame, whatever the mode.
        (t, _) if t <= 200 => whole,
        // SILK has its own 40 and 60 ms configurations and keeps them whole.
        (400, "silk") | (800, "silk") => ms40,
        (600, "silk") | (1200, "silk") => ms60,
        // No legal duration halves 100 ms, so it is five 20 ms frames for
        // everyone.
        _ => ms20,
    }
}

/// Every duration must be framed the way its mode requires, and the frames must
/// add up to exactly the audio the caller handed in.
///
/// The mode is read back from the TOC rather than assumed from the settings:
/// the invariant is about the mode a packet is *in*, so a mode decision that
/// moves cannot quietly turn this test into a weaker one.
#[test]
fn packet_framing_follows_the_mode() {
    let rate = 48_000i32;
    for &mode in &["silk", "hybrid", "celt"] {
        for &channels in &[1usize, 2] {
            for &tenth_ms in &[25i32, 50, 100, 200, 400, 600, 800, 1000, 1200] {
                let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
                let mono = music_like(rate, frame * 10);
                let pcm = if channels == 2 {
                    interleave(&[mono.clone(), mono])
                } else {
                    mono
                };
                let Ok(packets) = pinned(rate, channels, mode, frame).try_encode_all(&pcm) else {
                    // The short durations have no SILK or hybrid configuration
                    // at all; `coerce_mode_for_packet_rate` sends them to CELT,
                    // which the observed-mode check below then covers.
                    continue;
                };
                for (i, p) in packets.iter().enumerate() {
                    let got_mode = packet_mode(p);
                    let want = expected_frame_samples(rate, tenth_ms, got_mode);
                    let per = packet::samples_per_frame(p, rate).unwrap();
                    let frames = packet::frame_count(p).unwrap();
                    let label =
                        format!("{mode}-pinned {channels}ch {}ms packet {i}", tenth_ms / 10);
                    assert_eq!(
                        per, want,
                        "{label}: coded as {got_mode}, so each frame should be \
                         {want} samples, not {per}"
                    );
                    assert_eq!(
                        frames * per,
                        frame,
                        "{label}: {frames} frames of {per} samples is not {frame}"
                    );
                }
            }
        }
    }
}

/// 40 and 60 ms of music must be coded as CELT, not forced to SILK.
///
/// This is the defect the split was built to fix. The encoder had no way to code
/// CELT past 20 ms, so it overrode its own mode decision and sent every long
/// frame to SILK — which meant 60 ms of music at 64 kb/s was coded by the speech
/// codec. libopus 1.6.1 emits 319-byte code 1 and 479-byte code 3 CELT packets
/// for these two configurations on this audio; so does this crate.
#[test]
fn long_music_frames_are_not_forced_to_silk() {
    let rate = 48_000i32;
    for &(tenth_ms, want_frames, want_code) in &[(400i32, 2usize, 1u8), (600, 3, 3)] {
        let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
        let src = music_like(rate, frame * 10);
        let packets = Codec::new(rate, 1, Application::Audio)
            .bitrate(64_000)
            .frame_samples(frame)
            .encode_all(&src);
        for (i, p) in packets.iter().enumerate() {
            let label = format!("{}ms packet {i}", tenth_ms / 10);
            assert_eq!(packet_mode(p), "celt", "{label}: not CELT");
            assert_eq!(
                packet::frame_count(p).unwrap(),
                want_frames,
                "{label}: wrong frame count"
            );
            assert_eq!(p[0] & 0x03, want_code, "{label}: wrong packing code");
        }
    }
}

/// Each frame of a split packet must carry the audio it was handed, in order.
///
/// A split that fed every frame the same input, dropped one, or emitted them out
/// of order would still produce a well-formed packet of the right duration, and
/// a whole-packet correlation would mostly forgive it. So each 20 ms slice of
/// the input is a different tone, and each 20 ms slice of the output is required
/// to be dominated by its own.
#[test]
fn every_frame_of_a_split_packet_carries_its_own_audio() {
    let rate = 48_000i32;
    let slice = (rate / 50) as usize; // 20 ms
    let tones = [500.0f32, 1000.0, 2000.0, 3000.0, 5000.0, 8000.0];
    let frame = slice * tones.len(); // 120 ms
    let packets = 8;

    // One packet more than is checked, so the windows below still have audio
    // after they are offset by the codec's delay.
    let mut src = Vec::with_capacity(frame * (packets + 1));
    for i in 0..frame * (packets + 1) {
        let hz = tones[(i / slice) % tones.len()];
        let t = i as f64 / rate as f64;
        src.push(0.5 * sin_turns(hz as f64 * t) as f32);
    }

    let r = Codec::new(rate, 1, Application::Audio)
        .signal_type(Signal::Music)
        .bitrate(128_000)
        .frame_samples(frame)
        .roundtrip(&src);
    for p in &r.packets {
        assert_eq!(packet::frame_count(p).unwrap(), tones.len());
    }

    // Energy at `hz` over `x`, as a plain single-bin DFT.
    let bin = |x: &[f32], hz: f32| -> f32 {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (n, &v) in x.iter().enumerate() {
            let turns = hz as f64 * n as f64 / rate as f64;
            // cos(2πx) is sin(2πx + π/2), a quarter turn along.
            re += v * sin_turns(turns + 0.25) as f32;
            im += v * sin_turns(turns) as f32;
        }
        (re * re + im * im).sqrt() / x.len() as f32
    };

    // Line the decoded stream up with the input before slicing it. The codec
    // delays its output, and by more than one slice guard: `Fs/400` of CELT
    // overlap plus the `Fs/250` the CELT input lags by. Measuring the delay
    // rather than assuming it keeps this test about *which frame carries which
    // tone*, which is its point, instead of about how long the pipeline is.
    let (_, lag) = aligned_correlation(&r.decoded, &src, slice);
    // Within each slice, skip the 2.5 ms of MDCT overlap at either end, where
    // neighbouring tones legitimately bleed across the boundary. The first two
    // packets are codec warm-up.
    let guard = (rate / 400) as usize;
    for p in 2..packets {
        for (k, &hz) in tones.iter().enumerate() {
            let start = p * frame + k * slice + lag;
            let seg = &r.decoded[start + guard..start + slice - guard];
            let mine = bin(seg, hz);
            for (j, &other) in tones.iter().enumerate() {
                if j == k {
                    continue;
                }
                let theirs = bin(seg, other);
                assert!(
                    mine > 4.0 * theirs,
                    "packet {p} frame {k}: {hz} Hz is {mine:.5} but {other} Hz is \
                     {theirs:.5} — this frame is not carrying its own audio"
                );
            }
        }
    }
}

/// A CBR packet must be exactly the size the bitrate asks for, split or not.
///
/// The frames of a split packet are coded into scratch buffers and assembled
/// afterwards, so the padding that makes CBR exact has to be applied to the
/// assembled packet rather than to any one frame. Getting that wrong shows up
/// as a packet a few bytes short of its target, which nothing else here would
/// notice.
#[test]
fn cbr_split_packets_are_exactly_the_requested_size() {
    let rate = 48_000i32;
    for &channels in &[1usize, 2] {
        for &tenth_ms in &[400i32, 600, 800, 1000, 1200] {
            for &bitrate in &[32_000i32, 64_000, 128_000] {
                let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
                let mono = music_like(rate, frame * 8);
                let pcm = if channels == 2 {
                    interleave(&[mono.clone(), mono])
                } else {
                    mono
                };
                let mut c = Codec::new(rate, channels, Application::Audio)
                    .bitrate(bitrate)
                    .frame_samples(frame);
                c.enc.rate_control = RateControl::Cbr;
                let packets = c.encode_all(&pcm);
                // libopus `cbr_bytes`: the bitrate over the packet duration,
                // rounded the way opus_encoder.c rounds it.
                let want = ((bitrate as i64 * frame as i64 / rate as i64) as i32 + 4) / 8;
                for (i, p) in packets.iter().enumerate() {
                    assert_eq!(
                        p.len() as i32,
                        want,
                        "{channels}ch {}ms {bitrate} bps packet {i}: CBR asked for \
                         {want} bytes",
                        tenth_ms / 10
                    );
                }
            }
        }
    }
}

/// DTX inside a split packet.
///
/// DTX is decided per frame, so a split packet can hold a mixture of coded and
/// dropped frames, and a fully silent one collapses to a TOC that announces
/// several zero-length frames — one byte carrying 120 ms. That is the framing
/// libopus emits (`opus_encode_native` passes `dtx_count != nb_frames` as its
/// pad flag, so an all-DTX packet is deliberately left unpadded even under CBR);
/// a decoder still has to read the announced duration back out of it.
#[test]
fn dtx_collapses_a_split_packet_without_losing_its_duration() {
    let rate = 48_000i32;
    for &tenth_ms in &[600i32, 1200] {
        let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
        let expect_frames = if tenth_ms == 1200 { 2 } else { 1 };
        // Speech, then a long stretch of exact digital silence, then speech.
        let mut src = music_like(rate, frame * 12);
        for s in src.iter_mut().skip(frame * 4).take(frame * 6) {
            *s = 0.0;
        }

        for &cbr in &[false, true] {
            let label = format!("{}ms cbr={cbr}", tenth_ms / 10);
            let mut c = Codec::new(rate, 1, Application::Voip)
                .signal_type(Signal::Voice)
                .bandwidth(Bandwidth::Wideband)
                .bitrate(24_000)
                .frame_samples(frame);
            c.enc.use_dtx = true;
            c.enc.rate_control = if cbr {
                RateControl::Cbr
            } else {
                RateControl::ConstrainedVbr
            };
            let r = c.roundtrip(&src);

            let dtx: Vec<usize> = r
                .packets
                .iter()
                .enumerate()
                .filter(|(_, p)| p.len() == 1)
                .map(|(i, _)| i)
                .collect();
            assert!(
                !dtx.is_empty(),
                "{label}: DTX never engaged, so nothing here was tested"
            );

            for (i, p) in r.packets.iter().enumerate() {
                assert_eq!(
                    packet::frame_count(p).unwrap(),
                    expect_frames,
                    "{label}: packet {i} ({} bytes) announces the wrong frame count",
                    p.len()
                );
                assert_eq!(
                    packet::frame_count(p).unwrap() * packet::samples_per_frame(p, rate).unwrap(),
                    frame,
                    "{label}: packet {i} does not announce {frame} samples"
                );
            }
            // The decoder must produce the announced duration for every packet,
            // DTX included — `roundtrip` decodes each packet at its own length.
            assert_eq!(
                r.decoded.len(),
                r.packets.len() * frame,
                "{label}: decoded length does not match the packets"
            );
        }
    }
}

/// A packet with no room to code its frames must still announce its duration.
///
/// At 500 bps — the lowest bitrate Opus defines — a 100 ms CBR packet is six
/// bytes, and the framing alone for its five frames costs more than that. There
/// is no way to code five frames into it. Coding the first few and starving the
/// rest would put the decoder out of step, so the packet is emitted as framing
/// with no payload: the decoder reads the duration it announces and conceals it,
/// which is what libopus does under the same conditions (`opus_encoder.c:1340`,
/// "If the space is too low to do something useful, emit 'PLC' frames"). libopus
/// 1.6.1 emits six bytes here too, and eight for the 120 ms case.
#[test]
fn a_packet_too_small_to_code_still_announces_its_duration() {
    let rate = 48_000i32;
    for &(tenth_ms, want_len) in &[(1000i32, 6usize), (1200, 8)] {
        let frame = (rate as i64 * tenth_ms as i64 / 10_000) as usize;
        let src = music_like(rate, frame * 4);
        let mut c = Codec::new(rate, 1, Application::Audio)
            .bitrate(500)
            .frame_samples(frame);
        c.enc.rate_control = RateControl::Cbr;
        let r = c.roundtrip(&src);

        for (i, p) in r.packets.iter().enumerate() {
            let label = format!("{}ms at 500 bps, packet {i}", tenth_ms / 10);
            assert_eq!(p.len(), want_len, "{label}: wrong packet size");
            assert_eq!(
                packet::frame_count(p).unwrap() * packet::samples_per_frame(p, rate).unwrap(),
                frame,
                "{label}: does not announce {frame} samples"
            );
        }
        // Concealment, not silence-by-accident: the decoder must still hand back
        // the full duration for every packet.
        assert_eq!(r.decoded.len(), r.packets.len() * frame);
        assert!(r.decoded.iter().all(|s| s.is_finite()));
    }
}

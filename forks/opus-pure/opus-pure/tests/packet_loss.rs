//! Packet loss: concealment, and recovery through in-band FEC.
//!
//! Both were shipped untested. `use_inband_fec` is a public encoder field and
//! `decode_fec` a public decoder method, and nothing measured either, so an
//! encoder that wrote redundancy the decoder could not parse looked exactly
//! like one that worked. The assertions here compare a recovered frame against
//! the frame the receiver *would* have decoded had the packet arrived, and
//! against what plain concealment produces for the same loss: FEC that does not
//! beat concealment is not doing anything.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, OpusDecoder, Signal};

const FRAME_MS: i32 = 20;

/// One encoded stream plus the audio a lossless receiver would have decoded.
struct Stream {
    packets: Vec<Vec<u8>>,
    clean: Vec<f32>,
    rate: i32,
    channels: usize,
    frame: usize,
}

/// Speech-like source; the right channel is the same speech offset by half a
/// second, which decorrelates it without making it something SILK would refuse
/// to treat as speech.
fn source(rate: i32, channels: usize, secs: usize) -> Vec<f32> {
    let mono = speech_like(rate, rate as usize * secs);
    if channels == 1 {
        return mono;
    }
    let shift = rate as usize / 2;
    (0..mono.len())
        .flat_map(|i| [mono[i], mono[(i + shift) % mono.len()] * 0.9])
        .collect()
}

fn encode(
    rate: i32,
    channels: usize,
    frame_ms: i32,
    bitrate: i32,
    fec: bool,
    loss_perc: i32,
) -> Stream {
    // In-band FEC exists only in SILK and hybrid, so every stream here has to
    // be one the encoder codes with SILK. Since the mode decision stopped
    // forcing SILK below 24 kHz — libopus never did — a bare "16 kHz stereo at
    // 32 kb/s" is a CELT configuration on musical input, and CELT carries no
    // redundancy to test. Declaring the content as speech is what a caller
    // sending speech does anyway, via `OPUS_SET_SIGNAL`.
    let mut c = Codec::new(rate, channels, Application::Voip)
        .frame_ms(frame_ms)
        .bitrate(bitrate)
        .signal_type(Signal::Voice);
    c.enc.use_inband_fec = fec;
    c.enc.packet_loss_perc = loss_perc;
    let r = c.roundtrip(&source(rate, channels, 4));
    Stream {
        packets: r.packets,
        clean: r.decoded,
        rate,
        channels,
        frame: c.frame,
    }
}

impl Stream {
    fn reference_frame(&self, i: usize) -> &[f32] {
        let w = self.frame * self.channels;
        &self.clean[i * w..(i + 1) * w]
    }

    /// Decode the stream with packet `lost` dropped, recovering it either from
    /// the next packet's redundancy or by concealment, and return that one
    /// frame. Both paths decode the same prefix, so the decoder state they
    /// start the lost frame from is the same.
    fn recover(&self, lost: usize, use_fec: bool) -> Vec<f32> {
        let mut dec = OpusDecoder::new(self.rate, self.channels).unwrap();
        let mut buf = vec![0.0f32; self.frame * self.channels];
        let mut got = Vec::new();
        for (i, p) in self.packets.iter().enumerate().take(lost + 2) {
            if i == lost {
                if !use_fec {
                    dec.decode(&[], self.frame, &mut buf).unwrap();
                    got = buf.clone();
                }
                continue;
            }
            if i == lost + 1 && use_fec {
                dec.decode_fec(p, self.frame, &mut buf).unwrap();
                got = buf.clone();
            }
            dec.decode(p, self.frame, &mut buf).unwrap();
        }
        got
    }

    /// Loss positions worth scoring: far enough in that the decoder has settled,
    /// and carrying enough signal that a ratio against it means something.
    ///
    /// Both bounds are times rather than packet counts. Written as packet
    /// counts they silently meant something different for every frame duration,
    /// and a 60 ms stream ran out of positions entirely.
    fn loss_positions(&self) -> Vec<usize> {
        let packets_in = |ms: usize| (self.rate as usize * ms / 1000).div_ceil(self.frame).max(1);
        (packets_in(400)..self.packets.len().saturating_sub(4))
            .step_by(packets_in(100))
            .filter(|&i| rms(self.reference_frame(i)) > 0.02)
            .collect()
    }
}

fn rms(v: &[f32]) -> f32 {
    energy(v).sqrt()
}

/// Signal-to-error in dB against the frame the receiver would otherwise have had.
fn recovery_db(want: &[f32], got: &[f32]) -> f32 {
    assert_eq!(want.len(), got.len());
    let err: Vec<f32> = got.iter().zip(want).map(|(a, b)| a - b).collect();
    20.0 * (rms(want) / rms(&err).max(1e-12)).log10()
}

/// `(fraction of losses where FEC beat concealment, mean FEC dB, mean PLC dB)`.
fn score(s: &Stream) -> (f32, f32, f32) {
    let positions = s.loss_positions();
    assert!(
        positions.len() >= 10,
        "only {} scoreable loss positions",
        positions.len()
    );
    let (mut wins, mut fec_sum, mut plc_sum) = (0usize, 0.0f32, 0.0f32);
    for &lost in &positions {
        let want = s.reference_frame(lost);
        let fec = recovery_db(want, &s.recover(lost, true));
        let plc = recovery_db(want, &s.recover(lost, false));
        if fec > plc {
            wins += 1;
        }
        fec_sum += fec;
        plc_sum += plc;
    }
    let n = positions.len() as f32;
    (wins as f32 / n, fec_sum / n, plc_sum / n)
}

/// The headline: a frame recovered from redundancy must resemble the frame that
/// was lost, and must do so far better than extrapolating from the past.
///
/// Before the LBRR encoder was written this failed on every count. The encoder
/// stored the primary frame's excitation against gains it had then raised, and
/// coded the redundant indices with the wrong conditional-coding mode, so the
/// redundant payload both desynchronized the range coder and, where it decoded
/// at all, came out several times too loud.
///
/// The 60 ms entry is the only one that puts three SILK frames in a packet, so
/// it is the only one whose redundancy flags are coded with the three-frame
/// symbol table (`SILK_LBRR_FLAGS_3_ICDF`) on both sides.
#[test]
fn fec_recovers_a_lost_frame_far_better_than_concealment() {
    for &(rate, frame_ms, bitrate, label) in &[
        (16_000i32, 20i32, 24_000i32, "16 kHz 20 ms"),
        (16_000, 10, 24_000, "16 kHz 10 ms"),
        (16_000, 40, 24_000, "16 kHz 40 ms"),
        (16_000, 60, 24_000, "16 kHz 60 ms"),
        (8_000, 60, 24_000, "8 kHz 60 ms"),
        (48_000, 20, 24_000, "48 kHz 20 ms hybrid"),
    ] {
        let s = encode(rate, 1, frame_ms, bitrate, true, 30);
        let (win_rate, fec_db, plc_db) = score(&s);
        println!(
            "{label}: FEC {fec_db:+.2} dB, PLC {plc_db:+.2} dB, FEC wins {:.0}%",
            win_rate * 100.0
        );
        assert!(
            win_rate >= 0.9,
            "{label}: FEC beat concealment on only {:.0}% of losses",
            win_rate * 100.0
        );
        assert!(
            fec_db > 10.0,
            "{label}: recovered frame is only {fec_db:+.2} dB from the original"
        );
        assert!(
            fec_db > plc_db + 6.0,
            "{label}: FEC {fec_db:+.2} dB is no better than concealment's {plc_db:+.2} dB"
        );
    }
}

/// Stereo redundancy has to carry the side channel too. `decode_fec` used to
/// force the SILK decoder to one internal channel, so a recovered frame was the
/// mid duplicated to both outputs and the image snapped to the centre.
#[test]
fn fec_keeps_the_stereo_image() {
    let (mut collapsed, mut checked) = (0usize, 0usize);
    for &(rate, bitrate, label) in &[
        (16_000i32, 32_000i32, "16 kHz SILK"),
        (48_000, 24_000, "48 kHz hybrid"),
    ] {
        let s = encode(rate, 2, FRAME_MS, bitrate, true, 30);
        let (win_rate, fec_db, plc_db) = score(&s);
        println!(
            "{label} stereo: FEC {fec_db:+.2} dB, PLC {plc_db:+.2} dB, wins {:.0}%",
            win_rate * 100.0
        );
        assert!(
            win_rate >= 0.7 && fec_db > plc_db + 4.0,
            "{label}: stereo FEC {fec_db:+.2} dB vs concealment {plc_db:+.2} dB, \
             wins {:.0}%",
            win_rate * 100.0
        );

        // The recovered frame must be as much of a stereo image as the frame it
        // replaces. A mono collapse shows up as L and R becoming identical.
        for &lost in &s.loss_positions() {
            let want = s.reference_frame(lost);
            let (wl, wr) = (deinterleave(want, 2, 0), deinterleave(want, 2, 1));
            // Only meaningful where the encoder actually coded a side channel.
            if correlation(&wl, &wr).abs() > 0.9 {
                continue;
            }
            checked += 1;
            let got = s.recover(lost, true);
            let (gl, gr) = (deinterleave(&got, 2, 0), deinterleave(&got, 2, 1));
            if correlation(&gl, &gr).abs() > 0.9 {
                collapsed += 1;
            }
        }
    }
    // Not every configuration codes a side channel at every rate — at the
    // bottom of SILK's range libopus and this encoder both fall back to a
    // mid-only frame, and a mid-only frame is *supposed* to decode to L == R.
    // The check is only meaningful where a side channel was actually coded.
    assert!(
        checked >= 5,
        "no configuration coded enough stereo frames to check ({checked})"
    );
    assert_eq!(
        collapsed, 0,
        "{collapsed} of {checked} recovered frames collapsed to mono"
    );
}

/// The regression that actually shipped: turning FEC *on* destroyed the primary
/// stream. The redundant payload was written with a conditional-coding mode the
/// decoder does not use, so on every voiced frame it read one symbol the encoder
/// never wrote and the range decoder lost sync for the rest of the packet — with
/// no packet loss involved at all.
#[test]
fn enabling_fec_does_not_damage_the_stream_that_carries_it() {
    for &(rate, channels, bitrate) in &[
        (16_000i32, 1usize, 24_000i32),
        (16_000, 2, 32_000),
        (48_000, 1, 24_000),
    ] {
        let src = source(rate, channels, 2);
        let frame = (rate as usize * FRAME_MS as usize) / 1000;
        let mut quality = Vec::new();
        for &fec in &[false, true] {
            let mut c = Codec::new(rate, channels, Application::Voip)
                .frame_ms(FRAME_MS)
                .bitrate(bitrate);
            c.enc.use_inband_fec = fec;
            c.enc.packet_loss_perc = if fec { 30 } else { 0 };
            let out = c.roundtrip(&src).decoded;
            let ch0_in = deinterleave(&src, channels, 0);
            let ch0_out = deinterleave(&out, channels, 0);
            quality.push(aligned_snr_db(&ch0_out, &ch0_in, frame));
        }
        let (off, on) = (quality[0], quality[1]);
        println!("{rate} Hz {channels}ch: FEC off {off:+.2} dB, FEC on {on:+.2} dB");
        assert!(
            on > off - 1.5,
            "{rate} Hz {channels}ch: enabling FEC dropped clean-stream quality from \
             {off:+.2} dB to {on:+.2} dB"
        );
    }
}

/// Redundancy only exists in SILK's low band. Asked to recover from a packet
/// that cannot carry any — CELT-only, or a stream encoded with FEC off — the
/// decoder must fall back to concealment and produce exactly what a lost packet
/// would have produced, not a half-parsed frame.
#[test]
fn fec_falls_back_to_concealment_when_there_is_no_redundancy() {
    for &(rate, bitrate, app, fec, label) in &[
        (48_000i32, 96_000i32, Application::Audio, true, "CELT-only"),
        (16_000, 24_000, Application::Voip, false, "SILK, FEC off"),
    ] {
        let frame = (rate as usize * FRAME_MS as usize) / 1000;
        let src = source(rate, 1, 2);
        let mut c = Codec::new(rate, 1, app).frame_ms(FRAME_MS).bitrate(bitrate);
        c.enc.use_inband_fec = fec;
        c.enc.packet_loss_perc = if fec { 30 } else { 0 };
        let packets = c.encode_all(&src);

        let lost = 30usize;
        let mut fec_out = vec![0.0f32; frame];
        let mut plc_out = vec![0.0f32; frame];
        let mut d1 = OpusDecoder::new(rate, 1).unwrap();
        let mut d2 = OpusDecoder::new(rate, 1).unwrap();
        let mut scratch = vec![0.0f32; frame];
        for (i, p) in packets.iter().enumerate().take(lost + 2) {
            if i == lost {
                d2.decode(&[], frame, &mut plc_out).unwrap();
                continue;
            }
            if i == lost + 1 {
                d1.decode_fec(p, frame, &mut fec_out).unwrap();
            }
            d1.decode(p, frame, &mut scratch).unwrap();
            d2.decode(p, frame, &mut scratch).unwrap();
        }
        assert_eq!(
            fec_out, plc_out,
            "{label}: decode_fec did not fall back to plain concealment"
        );
    }
}

/// A concealed frame has to be audio: neither digital silence, which is an
/// audible dropout, nor an extrapolation that runs away.
#[test]
fn a_lost_packet_is_concealed_rather_than_silenced() {
    for &(rate, channels, bitrate) in &[(16_000i32, 1usize, 24_000i32), (48_000, 2, 64_000)] {
        let s = encode(rate, channels, FRAME_MS, bitrate, false, 0);
        for &lost in s.loss_positions().iter().take(6) {
            let want = s.reference_frame(lost);
            let got = s.recover(lost, false);
            let (want_rms, got_rms) = (rms(want), rms(&got));
            assert!(
                got_rms > want_rms * 0.05,
                "{rate} Hz {channels}ch frame {lost}: concealment produced {got_rms:.4} \
                 against a reference of {want_rms:.4} — effectively silence"
            );
            assert!(
                got_rms < want_rms * 4.0,
                "{rate} Hz {channels}ch frame {lost}: concealment ran away to {got_rms:.4} \
                 against a reference of {want_rms:.4}"
            );
            assert!(
                got.iter().all(|v| v.is_finite() && v.abs() < 4.0),
                "{rate} Hz {channels}ch frame {lost}: concealment left the rails"
            );
        }
    }
}

/// libopus only codes redundancy when the bitrate can carry it, and drops the
/// coded bandwidth to find room before giving up. At 8 kHz and 12 kb/s there is
/// no room at any bandwidth, so no redundancy is written even though the caller
/// asked for it — and the decoder must still behave, falling back to
/// concealment. Both encoders here report the same loss rate, so redundancy is
/// the only thing that differs between them.
#[test]
fn fec_is_not_coded_at_a_rate_that_cannot_afford_it() {
    let bytes = |s: &Stream| -> usize { s.packets.iter().map(|p| p.len()).sum() };

    let asked = encode(8_000, 1, FRAME_MS, 12_000, true, 30);
    let plain = encode(8_000, 1, FRAME_MS, 12_000, false, 30);
    assert_eq!(
        bytes(&asked),
        bytes(&plain),
        "narrowband at 12 kb/s spent bytes on redundancy it cannot afford"
    );
    let lost = 30;
    assert_eq!(
        asked.recover(lost, true),
        asked.recover(lost, false),
        "no redundancy was coded, so FEC recovery must equal concealment"
    );

    // ...and at a rate that can afford it, redundancy really is there.
    let rich = encode(16_000, 1, FRAME_MS, 24_000, true, 30);
    let lean = encode(16_000, 1, FRAME_MS, 24_000, false, 30);
    assert!(
        bytes(&rich) > bytes(&lean),
        "wideband at 24 kb/s coded no redundancy at all ({} vs {} bytes)",
        bytes(&rich),
        bytes(&lean)
    );
}

/// Redundancy is written into the *next* packet, so it outlives the frame that
/// produced it, and a bandwidth change in between moves SILK's internal rate and
/// frame length underneath it. The stream has to stay clean across the change,
/// and the frames after it have to stay recoverable.
///
/// (The encoder also drops pending redundancy outright when the layout moves,
/// which a bandwidth change alone does not strictly need: encoder and decoder
/// both derive the new frame length from the packet, so the redundant frame is
/// merely wrong rather than unparseable. The change of *packet duration* is
/// covered by `redundancy_pending_across_a_frame_size_change_is_dropped`.)
#[test]
fn redundancy_pending_across_a_bandwidth_change_is_dropped() {
    let rate = 16_000i32;
    let frame = rate as usize / 50;
    let src = source(rate, 1, 3);
    let switch = 40usize;

    let mut c = Codec::new(rate, 1, Application::Voip).bitrate(24_000);
    c.enc.use_inband_fec = true;
    c.enc.packet_loss_perc = 30;
    // Wideband, then narrowband: SILK drops from a 16 kHz internal rate to
    // 8 kHz, halving the frame length the redundancy was stored at.
    let mut packets = c.encode_all(&src[..switch * frame]);
    c.enc.force_bandwidth = Some(Bandwidth::Narrowband);
    packets.extend(c.encode_all(&src[switch * frame..]));
    assert!(
        packet_bandwidth_hz(&packets[switch]) < packet_bandwidth_hz(&packets[switch - 1]),
        "the encoder did not actually change bandwidth"
    );

    let s = Stream {
        clean: {
            let mut dec = OpusDecoder::new(rate, 1).unwrap();
            let mut buf = vec![0.0f32; frame];
            let mut out = Vec::new();
            for p in &packets {
                dec.decode(p, frame, &mut buf).unwrap();
                out.extend_from_slice(&buf);
            }
            out
        },
        packets,
        rate,
        channels: 1,
        frame,
    };

    // A desynchronized range decoder does not produce quiet audio, it produces
    // the wrong audio: the decoded level stops following the input.
    for i in switch..switch + 8 {
        let want = rms(&src[i * frame..(i + 1) * frame]);
        let got = rms(s.reference_frame(i));
        assert!(
            got > want * 0.4 && got < want * 2.5,
            "packet {i} decoded at {got:.3} for an input of {want:.3} — the payload              after the bandwidth change did not parse"
        );
    }

    // And the redundancy that follows the change still works.
    let (mut wins, mut scored) = (0usize, 0usize);
    for lost in (switch + 3..s.packets.len() - 2).step_by(3) {
        let want = s.reference_frame(lost);
        if rms(want) < 0.05 {
            continue;
        }
        scored += 1;
        if recovery_db(want, &s.recover(lost, true)) > recovery_db(want, &s.recover(lost, false)) {
            wins += 1;
        }
    }
    assert!(scored >= 5, "only {scored} losses scored after the change");
    assert!(
        wins * 4 >= scored * 3,
        "after a bandwidth change FEC beat concealment on only {wins}/{scored} losses"
    );
}

/// The same hazard as the bandwidth change above, at the layout change that
/// matters most: 20 ms of stored redundancy landing in a 10 ms packet.
///
/// Encoder and decoder both read the subframe count off the packet, so the
/// redundant frame parses; it simply describes twice the audio it is written as,
/// and a receiver recovering a loss from it would be worse off than concealing.
/// The encoder drops pending redundancy when the packet duration moves, so the
/// first packet of the new size carries none. What follows the change has to
/// keep working: FEC coded after it still has to beat concealment.
#[test]
fn redundancy_pending_across_a_frame_size_change_is_dropped() {
    let rate = 16_000i32;
    let (long, short) = (rate as usize / 50, rate as usize / 100); // 20 ms, 10 ms
    let src = source(rate, 1, 3);
    let switch = 40usize; // packets, not samples

    let mut c = Codec::new(rate, 1, Application::Voip)
        .frame_samples(long)
        .bitrate(24_000);
    c.enc.use_inband_fec = true;
    c.enc.packet_loss_perc = 30;
    let mut packets = c.encode_all(&src[..switch * long]);
    c.frame = short;
    packets.extend(c.encode_all(&src[switch * long..]));
    assert!(
        packet_samples_48k(&packets[switch]) * 2 == packet_samples_48k(&packets[switch - 1]),
        "the encoder did not actually change frame size"
    );

    // Decode the whole thing and compare, packet by packet, against the input
    // that produced it. A desynchronized range decoder does not produce quiet
    // audio, it produces the wrong audio: the level stops following the input.
    let mut dec = OpusDecoder::new(rate, 1).unwrap();
    let mut buf = vec![0.0f32; long];
    let mut pos = 0usize;
    for (i, p) in packets.iter().enumerate() {
        let frame = if i < switch { long } else { short };
        let got = dec.decode(p, frame, &mut buf).unwrap();
        if (switch..switch + 16).contains(&i) {
            let want = rms(&src[pos..pos + frame]);
            let have = rms(&buf[..got]);
            if want > 0.05 {
                assert!(
                    have > want * 0.4 && have < want * 2.5,
                    "packet {i} decoded at {have:.3} for an input of {want:.3}: the payload \
                     after the frame-size change did not parse"
                );
            }
        }
        pos += frame;
    }

    // The first packet of the new size must carry no redundancy at all: what was
    // pending describes 20 ms of audio and can only be written as a 10 ms frame
    // here. A receiver asking that packet to recover a loss has to get plain
    // concealment, byte for byte, rather than a frame of the wrong length.
    let mut d1 = OpusDecoder::new(rate, 1).unwrap();
    let mut d2 = OpusDecoder::new(rate, 1).unwrap();
    let mut wide = vec![0.0f32; long];
    for p in &packets[..switch] {
        d1.decode(p, long, &mut wide).unwrap();
        d2.decode(p, long, &mut wide).unwrap();
    }
    let mut from_fec = vec![0.0f32; short];
    let mut from_plc = vec![0.0f32; short];
    d1.decode_fec(&packets[switch], short, &mut from_fec)
        .unwrap();
    d2.decode(&[], short, &mut from_plc).unwrap();
    assert_eq!(
        from_fec, from_plc,
        "the first packet after the frame-size change still carried redundancy for a \
         frame of the previous length"
    );

    // And redundancy coded after the change still recovers a lost packet.
    let s = Stream {
        clean: {
            let mut dec = OpusDecoder::new(rate, 1).unwrap();
            let mut buf = vec![0.0f32; short];
            let mut out = Vec::new();
            for (i, p) in packets.iter().enumerate() {
                if i < switch {
                    let mut wide = vec![0.0f32; long];
                    dec.decode(p, long, &mut wide).unwrap();
                    continue;
                }
                dec.decode(p, short, &mut buf).unwrap();
                out.extend_from_slice(&buf);
            }
            out
        },
        packets: packets[switch..].to_vec(),
        rate,
        channels: 1,
        frame: short,
    };
    let (mut wins, mut scored) = (0usize, 0usize);
    for lost in (4..s.packets.len() - 2).step_by(3) {
        let want = s.reference_frame(lost);
        if rms(want) < 0.05 {
            continue;
        }
        scored += 1;
        if recovery_db(want, &s.recover(lost, true)) > recovery_db(want, &s.recover(lost, false)) {
            wins += 1;
        }
    }
    assert!(scored >= 5, "only {scored} losses scored after the change");
    assert!(
        wins * 4 >= scored * 3,
        "after a frame-size change FEC beat concealment on only {wins}/{scored} losses"
    );
}

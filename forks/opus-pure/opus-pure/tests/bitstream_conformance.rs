//! Range-coder agreement between the encoder and the decoder, per packet.
//!
//! ## Where this test came from
//!
//! The decoder was run against the official RFC 6716 / RFC 8251 test vectors
//! (the 12 `testvectorNN.bit` streams distributed from opus-codec.org). It
//! passes all 12 at stereo output, at 99.7-100.0% on `opus_compare`'s quality
//! metric, and all 12 again at mono output, at 96.2-99.9%; both legs decode all
//! 20,075 packets with **zero range-coder mismatches**.
//!
//! Those vectors are ~75 MB and are deliberately distributed separately from
//! libopus itself, so they are not in this repository and CI does not download
//! them. `reference/vectors/README.md` documents how to fetch and re-run them; that
//! is the check to repeat when the decoder changes structurally.
//!
//! What this file does is keep the *strongest signal* those vectors produced
//! running in CI on data generated at runtime, with nothing stored on disk.
//!
//! ## Why the range state is that signal
//!
//! Every Opus packet is one range-coded message. After decoding it, the
//! decoder's range state (`rng`) is a function of every symbol decoded and
//! every probability model used to decode it. libopus stores the encoder's
//! final state in the test-vector container and `opus_demo` compares the two
//! for exactly this reason: if the states differ, the entropy decode diverged,
//! no matter how close the audio sounds.
//!
//! That makes it a bit-exact conformance check that needs no reference PCM, is
//! immune to float rounding, and is identical on every architecture, because
//! the range coder is pure integer arithmetic. It covers CELT and hybrid as
//! fully as it covers SILK, which the PCM hashes in `decoder_conformance.rs`
//! cannot: those are frozen only for SILK, where this crate is bit-exact.
//!
//! ## What it does not cover
//!
//! It gates the entropy layer, not the synthesis that follows it. A defect in
//! the MDCT, the overlap-add or a resampler leaves the range state untouched;
//! the SILK resampler-delay bug that `decoder_conformance.rs` documents was
//! exactly that kind, and this test would not have caught it. The two files are
//! complements, not substitutes.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, OpusDecoder, OpusEncoder, Signal, packet};

/// One encoder feeding one decoder, checking the range state after every
/// packet. Returns the modes seen, so a caller can assert the stream actually
/// exercised what it meant to.
struct InStep {
    enc: OpusEncoder,
    dec: OpusDecoder,
    rate: i32,
    buf: Vec<u8>,
    pcm: Vec<f32>,
    modes: Vec<&'static str>,
    packets: usize,
    /// Length of the most recently encoded packet.
    buf_len: usize,
    /// Coded frames in each packet, so a caller can require that a stream
    /// really did exercise multi-frame packets rather than quietly stop.
    frames_per_packet: Vec<usize>,
}

impl InStep {
    fn new(rate: i32, channels: usize, app: Application) -> Self {
        Self {
            enc: OpusEncoder::new(rate, channels, app).expect("encoder"),
            dec: OpusDecoder::new(rate, channels).expect("decoder"),
            rate,
            buf: vec![0u8; 4000],
            // 120 ms, the most a single packet can decode to.
            pcm: vec![0.0f32; (rate as usize / 1000) * 120 * channels],
            modes: Vec::new(),
            packets: 0,
            buf_len: 0,
            frames_per_packet: Vec::new(),
        }
    }

    /// Encode one frame and decode it straight back, asserting the two range
    /// coders agree. `what` names the configuration in a failure.
    fn step(&mut self, input: &[f32], frame: usize, what: &str) {
        let len = self
            .enc
            .encode(input, frame, &mut self.buf)
            .unwrap_or_else(|e| panic!("{what}: packet {} failed to encode: {e:?}", self.packets));
        let enc_range = self.enc.final_range();
        self.buf_len = len;
        let packet = &self.buf[..len];
        self.modes.push(packet_mode(packet));
        self.frames_per_packet
            .push(packet::frame_count(packet).expect("the encoder emitted an unparseable packet"));

        let max = (self.rate as usize / 1000) * 120;
        self.dec
            .decode(packet, max, &mut self.pcm)
            .unwrap_or_else(|e| panic!("{what}: packet {} failed to decode: {e:?}", self.packets));

        // A DTX packet carries no coded range, and the encoder reports 0 for it
        // (opus_encoder.c st->rangeFinal). Nothing to compare.
        if enc_range != 0 {
            assert_eq!(
                self.dec.final_range(),
                enc_range,
                "{what}: packet {} left the decoder's range coder at {:#010x}, \
                 the encoder ended it at {enc_range:#010x} — the entropy decode \
                 diverged",
                self.packets,
                self.dec.final_range()
            );
        }
        self.packets += 1;
    }
}

/// Interleave a mono signal to `channels`, decorrelating the second channel so
/// stereo coding has a real side signal to carry.
fn widen(mono: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return mono.to_vec();
    }
    mono.iter()
        .enumerate()
        .flat_map(|(i, &s)| [s, s * 0.6 + mono[i / 3 % mono.len()] * 0.3])
        .collect()
}

/// The broad sweep: every mode the encoder picks on its own, across rate,
/// channel count, frame duration, bitrate and application.
#[test]
fn range_coder_state_agrees_on_every_packet() {
    const RATES: [(i32, usize); 6] = [
        (8_000, 1),
        (12_000, 1),
        (16_000, 2),
        (24_000, 1),
        (48_000, 1),
        (48_000, 2),
    ];
    let mut checked = 0usize;
    let mut split = 0usize;

    for &(rate, channels) in &RATES {
        // Every packet duration Opus defines above 5 ms. 40 ms and up may hold
        // several frames, so this is also where the split is checked: a packet
        // whose frames were budgeted or assembled wrongly leaves the decoder's
        // range coder somewhere the encoder's did not end.
        for &ms in &[10usize, 20, 40, 60, 80, 100, 120] {
            for &bitrate in &[8_000, 24_000, 64_000, 128_000] {
                for &app in &[Application::Voip, Application::Audio] {
                    let frame = (rate as usize / 1000) * ms;
                    // Roughly a fixed amount of audio per configuration rather
                    // than a fixed packet count: 20 packets of 120 ms is 2.4
                    // seconds at every rate, bitrate and application in the
                    // sweep.
                    let packets = if ms >= 80 { 10 } else { 20 };
                    let mono = speech_like(rate, frame * packets);
                    let pcm = widen(&mono, channels);

                    let what = format!("{rate} Hz/{channels}ch/{ms} ms/{bitrate} bps/{app:?}");
                    let mut s = InStep::new(rate, channels, app);
                    s.enc.bitrate_bps = bitrate;
                    for f in 0..pcm.len() / (frame * channels) {
                        let lo = f * frame * channels;
                        s.step(&pcm[lo..lo + frame * channels], frame, &what);
                    }
                    checked += s.packets;
                    split += s.frames_per_packet.iter().filter(|&&n| n > 1).count();
                }
            }
        }
    }

    // Guard against the sweep silently collapsing to nothing.
    assert!(checked > 5_000, "only {checked} packets checked");
    // 80 ms and up is always several frames, so a sweep that stopped producing
    // them has lost the split from its coverage without failing.
    assert!(
        split > 1_000,
        "only {split} of {checked} packets held more than one frame"
    );
}

/// The coverage the official vectors have that a fixed-configuration sweep does
/// not: transitions. Bandwidth, bitrate and signal type change every few frames
/// in one continuous stream, so the encoder crosses SILK/hybrid/CELT boundaries
/// mid-stream and the decoder has to follow without losing the range coder.
#[test]
fn range_state_holds_across_forced_transitions() {
    // Each of these was measured to land on the mode named beside it, for both
    // mono and stereo, so the stream is guaranteed to cross every boundary.
    // Kept aligned by hand: the mode beside each step is the point of the table.
    #[rustfmt::skip]
    const STEPS: [(Bandwidth, i32, Signal); 8] = [
        (Bandwidth::Narrowband, 8_000, Signal::Voice),      // silk
        (Bandwidth::Fullband, 128_000, Signal::Music),      // celt
        (Bandwidth::Superwideband, 12_000, Signal::Voice),  // hybrid
        (Bandwidth::Wideband, 20_000, Signal::Voice),       // silk
        (Bandwidth::Fullband, 256_000, Signal::Music),      // celt
        (Bandwidth::Fullband, 20_000, Signal::Voice),       // hybrid
        (Bandwidth::Mediumband, 24_000, Signal::Voice),     // silk
        (Bandwidth::Fullband, 128_000, Signal::Music),      // celt
    ];

    for &channels in &[1usize, 2] {
        let rate = 48_000;
        let frame = 960; // 20 ms
        let mono = speech_like(rate, frame * (STEPS.len() * 4));
        let pcm = widen(&mono, channels);
        let what = format!("forced transitions, {channels}ch");

        let mut s = InStep::new(rate, channels, Application::Voip);
        for f in 0..pcm.len() / (frame * channels) {
            // Hold each setting for 4 frames, then move on.
            let (bw, bitrate, signal) = STEPS[(f / 4) % STEPS.len()];
            s.enc.force_bandwidth = Some(bw);
            s.enc.bitrate_bps = bitrate;
            s.enc.signal_type = Some(signal);
            let lo = f * frame * channels;
            s.step(&pcm[lo..lo + frame * channels], frame, &what);
        }

        // The stream is only worth anything if it really did move between
        // modes. Require all three, and several switches.
        let switches = s.modes.windows(2).filter(|w| w[0] != w[1]).count();
        for mode in ["silk", "hybrid", "celt"] {
            assert!(
                s.modes.contains(&mode),
                "{what}: never reached {mode} — the stream is not testing transitions \
                 (saw {:?})",
                dedup(&s.modes)
            );
        }
        assert!(
            switches >= 6,
            "{what}: only {switches} mode switches in {} packets",
            s.packets
        );
    }
}

/// DTX punches TOC-only packets into the stream and FEC adds an LBRR frame to
/// packets that carry one. Both change what the range coder sees; the decoder
/// has to stay in step through them.
#[test]
fn dtx_and_fec_streams_keep_the_decoder_in_step() {
    for &(dtx, fec, loss) in &[(true, false, 0), (false, true, 20), (true, true, 30)] {
        for &(rate, channels) in &[(16_000i32, 1usize), (48_000, 2)] {
            let frame = (rate as usize / 1000) * 20;
            // Speech with long gaps, so DTX actually engages.
            let mut mono = speech_like(rate, frame * 30);
            for (i, s) in mono.iter_mut().enumerate() {
                if (i / (frame * 5)) % 2 == 1 {
                    *s = 0.0;
                }
            }
            let pcm = widen(&mono, channels);
            let what = format!("{rate} Hz/{channels}ch dtx={dtx} fec={fec} loss={loss}%");

            let mut s = InStep::new(rate, channels, Application::Voip);
            s.enc.bitrate_bps = 24_000;
            s.enc.use_dtx = dtx;
            s.enc.use_inband_fec = fec;
            s.enc.packet_loss_perc = loss;
            for f in 0..pcm.len() / (frame * channels) {
                let lo = f * frame * channels;
                s.step(&pcm[lo..lo + frame * channels], frame, &what);
            }
            assert!(s.packets > 25, "{what}: only {} packets", s.packets);
        }
    }
}

fn dedup(modes: &[&'static str]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for m in modes {
        if out.last() != Some(m) {
            out.push(m);
        }
    }
    out
}

/// Regression test for a defect this file's own range check found.
///
/// `celt_encoder.c` codes the digital-silence flag only when CELT owns a fresh
/// range coder (`tell == 1`), and forces `silence = 0` otherwise; `celt_decoder.c`
/// reads it under the same condition. This port gated the *write* on `tell == 1`
/// but kept `silence` set when the branch was skipped, so a digitally silent
/// **hybrid** frame took the silence shortcut: it shrank the range coder and
/// marked the budget spent, and CELT coded no layer at all. The decoder — ours
/// and libopus's alike, since both mirror the C — still read a CELT layer, so
/// the packet desynchronized. Confirmed against libopus 1.6.1: its decoder
/// reported the same range our decoder did, and both disagreed with what our
/// encoder claimed.
///
/// Digital silence inside a hybrid stream is the only way in: CELT-only frames
/// have `tell == 1`, so they were always correct, and non-silent hybrid frames
/// never set the flag.
#[test]
fn digitally_silent_hybrid_frames_still_code_a_celt_layer() {
    for &channels in &[1usize, 2] {
        let (rate, frame) = (48_000i32, 960usize);
        // Loud speech, then exact digital silence, then speech again.
        let mut mono = speech_like(rate, frame * 12);
        for s in mono.iter_mut().skip(frame * 4).take(frame * 4) {
            *s = 0.0;
        }
        let pcm = widen(&mono, channels);
        let what = format!("silent hybrid, {channels}ch");

        let mut s = InStep::new(rate, channels, Application::Voip);
        // Measured to select hybrid at both channel counts.
        s.enc.force_bandwidth = Some(Bandwidth::Superwideband);
        s.enc.bitrate_bps = 24_000;
        s.enc.signal_type = Some(Signal::Voice);

        let mut silent_lens = Vec::new();
        for f in 0..pcm.len() / (frame * channels) {
            let lo = f * frame * channels;
            // `step` is where the range agreement is asserted; a truncated
            // hybrid frame fails there.
            s.step(&pcm[lo..lo + frame * channels], frame, &what);
            if (5..8).contains(&f) {
                silent_lens.push(s.buf_len);
            }
        }

        assert!(
            s.modes.contains(&"hybrid"),
            "{what}: never reached hybrid, so the silence path was not exercised"
        );
        // The defect truncated these to ~18 bytes. A hybrid frame carrying a
        // real CELT layer is larger than that even on silence, so this pins the
        // symptom independently of the range check above.
        //
        // The threshold is 22 rather than the 30 it started at. Measured on this
        // input, libopus 1.6.1 codes these frames at 28-30 bytes and this crate
        // at 26-29, so 30 was pinned to a hybrid allocation that was itself
        // diverging from the reference: libopus would have failed it. 22 still
        // sits well clear of the ~18 the shortcut produced.
        for (i, &len) in silent_lens.iter().enumerate() {
            assert!(
                len > 22,
                "{what}: silent frame {i} coded {len} bytes — too small to carry \
                 a CELT layer, so the silence shortcut was taken in hybrid"
            );
        }
    }
}

/// Multi-frame packets, one mode at a time.
///
/// The sweep above reaches these too, but only through whatever mode its
/// bitrates happen to select. A split packet is assembled from frames coded into
/// separate buffers and then concatenated by the repacketizer, and each of the
/// three modes carves its bit budget up differently — SILK keeps its long frames
/// whole where CELT cannot — so each is pinned and checked in turn.
///
/// Range agreement across a split is a strictly stronger statement than across a
/// single frame: the decoder has to find every frame boundary the encoder's
/// framing implied, restart its range coder there, and finish each frame exactly
/// where the encoder did. Truncating one frame's payload by a single byte before
/// assembly fails here and nowhere else — the packet is still well-formed, still
/// the right duration, and still decodes to audio that sounds close enough.
#[test]
fn split_packets_keep_the_decoder_in_step() {
    let rate = 48_000i32;
    // (mode, bandwidth, bitrate, signal) — measured to select the named mode at
    // both channel counts.
    const PINNED: [(&str, Bandwidth, i32, Signal); 3] = [
        ("silk", Bandwidth::Wideband, 32_000, Signal::Voice),
        ("hybrid", Bandwidth::Fullband, 20_000, Signal::Voice),
        ("celt", Bandwidth::Fullband, 96_000, Signal::Music),
    ];

    for &(mode, bw, bitrate, signal) in &PINNED {
        for &channels in &[1usize, 2] {
            for &ms in &[40usize, 60, 80, 100, 120] {
                let frame = (rate as usize / 1000) * ms;
                let mono = speech_like(rate, frame * 10);
                let pcm = widen(&mono, channels);
                let what = format!("{mode}-pinned {channels}ch {ms} ms");

                let mut s = InStep::new(rate, channels, Application::Voip);
                s.enc.force_bandwidth = Some(bw);
                s.enc.bitrate_bps = bitrate;
                s.enc.signal_type = Some(signal);
                for f in 0..pcm.len() / (frame * channels) {
                    let lo = f * frame * channels;
                    s.step(&pcm[lo..lo + frame * channels], frame, &what);
                }

                assert!(
                    s.modes.iter().all(|m| *m == mode),
                    "{what}: meant to pin {mode}, got {:?}",
                    dedup(&s.modes)
                );
                // Every duration here splits in at least one mode, and 80 ms and
                // up splits in all of them.
                if ms >= 80 {
                    assert!(
                        s.frames_per_packet.iter().all(|&n| n > 1),
                        "{what}: {ms} ms must be several frames, saw {:?}",
                        s.frames_per_packet
                    );
                }
                assert!(s.packets >= 10, "{what}: only {} packets", s.packets);
            }
        }
    }
}

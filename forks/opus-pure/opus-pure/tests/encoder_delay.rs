//! The encoder's output delay, asserted as a number.
//!
//! Every other fidelity test in this suite measures through [`aligned_correlation`],
//! which searches for the lag that best lines the decoder's output up with the
//! encoder's input and then throws that lag away. That is the right thing when
//! the question is "does this sound like the input", and it is exactly wrong
//! when the question is "how far behind the input is it": a constant offset is
//! removed by construction, so no amount of that testing can see one.
//!
//! Three real defects lived in that blind spot. The *decoder's* SILK resampler
//! skipped its input delay whenever the API rate already matched SILK's internal
//! rate, putting every SILK stream 4, 9 or 12 samples early (see
//! `tests/decoder_conformance.rs`). The CELT layer was fed the newest input
//! rather than input lagging by `Fs/250`, so every CELT-only stream ran 4 ms
//! ahead of libopus and the timeline jumped by that much at a mode switch. And
//! the *encoder's* SILK resampler delay was applied twice whenever the API rate
//! did not match SILK's internal rate, putting every 24 and 48 kHz SILK and
//! hybrid stream 15 or 30 samples late. All three are described in
//! `docs/interop-validation.md`. This file exists so a fourth one fails here.
//!
//! Nothing here uses a reference decoder: the delays are asserted against the
//! arithmetic that produces them, so the test runs in CI.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, Signal};

/// Opus's algorithmic delay: `Fs/400` of CELT overlap plus the `Fs/250` the CELT
/// input lags the caller's by. At 48 kHz this is 312 samples, which is what
/// `OpusHead::RECOMMENDED_PRE_SKIP` declares.
fn expected_celt_delay(rate: i32) -> usize {
    (rate / 400 + rate / 250) as usize
}

/// Where the decoder's output sits relative to the encoder's input, and how
/// confident the measurement is.
fn measure_delay(decoded: &[f32], src: &[f32], max_lag: usize) -> (usize, f32) {
    let (corr, lag) = aligned_correlation(decoded, src, max_lag);
    (lag, corr)
}

/// A CELT-only stream's delay is `Fs/400 + Fs/250` exactly, at every rate.
///
/// This is the assertion the missing delay compensation failed. Before it was
/// added the lag here was `Fs/400` alone — 120 rather than 312 at 48 kHz.
#[test]
fn celt_only_delay_is_the_documented_algorithmic_delay() {
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        let frame = (rate / 50) as usize;
        let src = music_like(rate, frame * 100);
        let r = Codec::new(rate, 1, Application::Audio)
            .signal_type(Signal::Music)
            .bitrate(96_000)
            .roundtrip(&src);
        assert!(
            r.modes().iter().all(|m| *m == "celt"),
            "{rate} Hz: this configuration must be CELT throughout for the \
             delay below to be CELT's"
        );

        let skip = frame * 5;
        let (lag, corr) = measure_delay(&r.decoded[skip..], &src[skip..], frame * 2);
        assert!(
            corr > 0.99,
            "{rate} Hz: correlation {corr:.4} is too low to trust the lag it came \
             from; the delay measurement below would be meaningless"
        );
        assert_eq!(
            lag,
            expected_celt_delay(rate),
            "{rate} Hz: CELT-only output is {lag} samples behind its input, not \
             the {} that Fs/400 + Fs/250 asks for. A pre-skip of {} would trim \
             the wrong amount.",
            expected_celt_delay(rate),
            expected_celt_delay(rate)
        );
    }
}

/// A stream's delay must not change when the coding mode does.
///
/// This is the property a listener hears: a jump in the output timeline at a
/// mode switch is a skip or a repeat, however good each side sounds on its own.
/// It is also what the missing delay compensation broke, and what no
/// lag-searching test could see, because each side of the switch was measured
/// with its own lag.
///
/// Speech for the first half and music for the second, which is what moves the
/// mode decision; the switch is located from the packets rather than assumed.
#[test]
fn the_delay_does_not_move_when_the_mode_does() {
    for &(rate, bitrate) in &[(16_000i32, 24_000i32), (24_000, 24_000), (48_000, 24_000)] {
        let frame = (rate / 50) as usize;
        let half = frame * 60;
        let mut src = speech_like(rate, half);
        src.extend(music_like(rate, half));

        let r = Codec::new(rate, 1, Application::Audio)
            .bitrate(bitrate)
            .roundtrip(&src);
        let modes = r.modes();
        let sw = (1..modes.len())
            .find(|&k| modes[k] != modes[k - 1])
            .unwrap_or_else(|| {
                panic!(
                    "{rate} Hz: this configuration never changes mode, so it \
                                       cannot test what happens when it does"
                )
            });
        let (before, after) = (modes[sw - 1], modes[sw]);

        // Measure each side clear of the transition itself, where the decoder's
        // cross-fade legitimately blends the two.
        let a = (frame * 10, frame * (sw - 2));
        let b = (frame * (sw + 3), frame * 115);
        let (lag_a, corr_a) = measure_delay(&r.decoded[a.0..a.1], &src[a.0..a.1], frame * 2);
        let (lag_b, corr_b) = measure_delay(&r.decoded[b.0..b.1], &src[b.0..b.1], frame * 2);
        assert!(
            corr_a > 0.95 && corr_b > 0.95,
            "{rate} Hz: correlations {corr_a:.3}/{corr_b:.3} are too low to trust \
             the lags they came from"
        );

        let jump = lag_a as i64 - lag_b as i64;
        // Hybrid used to be allowed 28 samples here at 48 kHz, which is how a
        // doubled SILK resampler delay stayed in the crate: the allowance was
        // written to describe the defect rather than to bound what a listener
        // can hear. Every mode now lands in the same place.
        let allowed = 2;
        assert!(
            jump.abs() <= allowed,
            "{rate} Hz {before}->{after} at packet {sw}: the output jumps {jump} \
             samples across the switch ({lag_a} before, {lag_b} after). A listener \
             hears that as a skip."
        );
    }
}
/// SILK lands where CELT lands, at every API rate.
///
/// `delay_matrix_enc` exists to equalise the codec's total delay across every
/// (API rate, SILK internal rate) pair, so a stream sounds the same distance
/// behind its input whatever rate SILK picked. Nothing asserted that, and it
/// was wrong: the pass-through's delay was applied downstream in `silk_encode`
/// rather than by the resampler, so every pair that actually resampled got it
/// twice and came out 10 samples of SILK-internal rate late — 30 at 48 kHz,
/// 15 at 24 kHz, 0 at the rates where no resampling happens.
///
/// That it was 0 at 8, 12 and 16 kHz is why it survived: every byte-exact
/// configuration in `tests/reference_vectors.rs` ran at one of those three
/// rates, so the resampler's ratio path had no reference test at all. It has
/// eight now, one per ratio the encoder can ask for. This test is the cheaper
/// guard beside them: it needs no reference packets, so it also covers the
/// rates and signals those vectors do not.
///
/// SILK sits a little ahead of CELT even in libopus — about 4 samples at
/// 48 kHz, 2 at 16 kHz — because the equalisation is an integer table and not
/// exact. The tolerance covers that and nothing like the 30 above.
#[test]
fn silk_lands_where_celt_does_at_every_rate() {
    for &rate in &[8_000i32, 12_000, 16_000, 24_000, 48_000] {
        let frame = (rate / 50) as usize;
        let src = speech_like(rate, frame * 150);

        let r = Codec::new(rate, 1, Application::Voip)
            .bitrate(12_000)
            .bandwidth(Bandwidth::Wideband)
            .signal_type(Signal::Voice)
            .roundtrip(&src);
        let modes = r.modes();
        assert!(
            modes.iter().all(|m| *m == "silk"),
            "{rate} Hz: wanted a SILK-only stream to measure, got {modes:?}"
        );

        let (lag, corr) = measure_delay(&r.decoded, &src, frame * 2);
        assert!(
            corr > 0.8,
            "{rate} Hz: correlation {corr:.4} is too low to trust the lag"
        );

        let expected = expected_celt_delay(rate) as i64;
        // SILK lands 2 samples ahead of CELT at 8-16 kHz, 3 at 24 and 4 at 48;
        // libopus is within a sample of the same figures. One sample of headroom
        // on each, which still leaves the 15 and 30 above far outside.
        let tolerance = 3 + (rate / 24_000) as i64;
        let off = lag as i64 - expected;
        assert!(
            off.abs() <= tolerance,
            "{rate} Hz: SILK comes out {lag} samples behind its input where CELT \
             comes out {expected}, a difference of {off}. The two layers have to \
             land in the same place or a stream that switches mode steps at the \
             seam; `delay_matrix_enc` is what equalises them."
        );
    }
}

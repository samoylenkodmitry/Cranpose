//! Stereo behaviour, asserted as a *relationship* between the decoder's channels
//! and the encoder's.
//!
//! The suite this crate was ported from checked only that each decoded channel
//! had non-zero energy, which passes on inverted, mis-scaled or garbage audio —
//! and did, for years: `silk_encode_stereo` signalled a -1.68 mid/side
//! prediction weight on every stereo SILK and hybrid packet, and the decoded
//! channels came out as exact anti-correlated copies of mid. Every test here is
//! written so that defect would fail it.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, Signal};

struct Stereo {
    left: Vec<f32>,
    right: Vec<f32>,
    modes: Vec<&'static str>,
}

/// Round-trip stereo and split the channels, dropping the first 300 ms so the
/// measurements see the coder's steady state rather than its start-up.
fn roundtrip_stereo(
    rate: i32,
    app: Application,
    bitrate: i32,
    force_bw: Option<Bandwidth>,
    pcm: &[f32],
) -> Stereo {
    roundtrip_stereo_as(rate, app, bitrate, force_bw, None, pcm)
}

/// `roundtrip_stereo`, with the content type declared.
///
/// A test about *SILK's* stereo coding has to say so. Since the mode decision
/// stopped forcing SILK below 24 kHz — libopus never did — "16 kHz stereo at
/// 40 kb/s" is a CELT configuration on this input, and CELT's stereo is a
/// different mechanism entirely. Passing `Signal::Voice` is what a caller
/// sending speech does, via `OPUS_SET_SIGNAL`, and it keeps the mode decision
/// on the voice threshold where these rates land in SILK.
fn roundtrip_stereo_as(
    rate: i32,
    app: Application,
    bitrate: i32,
    force_bw: Option<Bandwidth>,
    signal: Option<Signal>,
    pcm: &[f32],
) -> Stereo {
    let mut c = Codec::new(rate, 2, app).bitrate(bitrate);
    if let Some(sig) = signal {
        c = c.signal_type(sig);
    }
    c.enc.force_bandwidth = force_bw;
    let r = c.roundtrip(pcm);
    assert!(
        r.packets.iter().all(|p| packet_is_stereo(p)),
        "a 2-channel encoder must set the TOC stereo bit"
    );
    let skip = c.frame * 15;
    Stereo {
        left: r.channel(0)[skip..].to_vec(),
        right: r.channel(1)[skip..].to_vec(),
        modes: r.modes(),
    }
}

/// Two decorrelated tones: the classic probe, because a mid/side defect shows up
/// as a strong negative left/right correlation.
fn decorrelated(rate: i32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    interleave(&[sine(rate, n, 440.0, 0.4), sine(rate, n, 836.0, 0.4)])
}

/// **The regression test for the mid/side prediction defect.**
///
/// Whatever a coding mode does with stereo — true stereo, or a mono downmix —
/// the decoded channels must never be anti-correlated when the input channels
/// are not. A negative correlation here means the decoder is applying a
/// mid/side prediction weight the encoder did not intend.
#[test]
fn decoded_channels_are_never_anti_correlated() {
    for &(rate, app, bitrate, bw) in &[
        (16_000i32, Application::Voip, 24_000i32, None),
        (16_000, Application::Voip, 32_000, None),
        (8_000, Application::Voip, 16_000, None),
        (24_000, Application::Voip, 20_000, None),
        (48_000, Application::Voip, 24_000, None),
        (
            48_000,
            Application::Audio,
            128_000,
            Some(Bandwidth::Fullband),
        ),
    ] {
        let src = decorrelated(rate, 1.5);
        let s = roundtrip_stereo(rate, app, bitrate, bw, &src);
        let lr = correlation(&s.left, &s.right);
        assert!(
            lr > -0.2,
            "{rate} Hz {bitrate} bps ({}): decoded L/R correlation {lr:+.4}. \
             Input channels are uncorrelated, so a strong negative value means a \
             stereo prediction weight is being applied that the encoder did not code.",
            s.modes[s.modes.len() - 1],
        );
    }
}

/// The decoded side channel must not carry more energy than the input's. The
/// defect inflated it by 2.6x.
#[test]
fn side_channel_energy_is_not_amplified() {
    for &(rate, app, bitrate) in &[
        (16_000i32, Application::Voip, 24_000i32),
        (24_000, Application::Voip, 20_000),
        (48_000, Application::Audio, 128_000),
    ] {
        let src = decorrelated(rate, 1.5);
        let s = roundtrip_stereo(rate, app, bitrate, None, &src);
        let skip = (rate / 50) as usize * 15;
        let in_l = deinterleave(&src, 2, 0);
        let in_r = deinterleave(&src, 2, 1);

        let out_side: Vec<f32> = s
            .left
            .iter()
            .zip(&s.right)
            .map(|(a, b)| (a - b) * 0.5)
            .collect();
        let in_side: Vec<f32> = in_l[skip..]
            .iter()
            .zip(&in_r[skip..])
            .map(|(a, b)| (a - b) * 0.5)
            .collect();

        let ratio = energy(&out_side) / energy(&in_side);
        assert!(
            ratio < 1.5,
            "{rate} Hz {bitrate} bps: decoded side energy is {ratio:.2}x the input's"
        );
    }
}

/// The mid channel must survive at roughly its input level in every mode. This
/// is the half of the signal that always has to be there.
#[test]
fn mid_channel_energy_is_preserved() {
    for &(rate, app, bitrate) in &[
        (16_000i32, Application::Voip, 24_000i32),
        (24_000, Application::Voip, 24_000),
        (48_000, Application::Audio, 128_000),
    ] {
        let src = decorrelated(rate, 1.5);
        let s = roundtrip_stereo(rate, app, bitrate, None, &src);
        let skip = (rate / 50) as usize * 15;
        let in_l = deinterleave(&src, 2, 0);
        let in_r = deinterleave(&src, 2, 1);

        let out_mid: Vec<f32> = s
            .left
            .iter()
            .zip(&s.right)
            .map(|(a, b)| (a + b) * 0.5)
            .collect();
        let in_mid: Vec<f32> = in_l[skip..]
            .iter()
            .zip(&in_r[skip..])
            .map(|(a, b)| (a + b) * 0.5)
            .collect();

        let ratio = energy(&out_mid) / energy(&in_mid);
        assert!(
            (0.5..=2.0).contains(&ratio),
            "{rate} Hz {bitrate} bps: decoded mid energy is {ratio:.2}x the input's"
        );
    }
}

/// CELT-only stereo is the path that codes a real stereo image: each decoded
/// channel must track its own input, not the other one.
#[test]
fn celt_stereo_keeps_the_channels_apart() {
    let rate = 48_000;
    let src = decorrelated(rate, 1.5);
    let s = roundtrip_stereo(
        rate,
        Application::Audio,
        128_000,
        Some(Bandwidth::Fullband),
        &src,
    );
    assert!(s.modes.iter().all(|m| *m == "celt"), "expected CELT-only");

    let skip = (rate / 50) as usize * 15;
    let in_l = deinterleave(&src, 2, 0);
    let in_r = deinterleave(&src, 2, 1);
    let lag = (rate / 50) as usize;

    let (ll, _) = aligned_correlation(&s.left, &in_l[skip..], lag);
    let (rr, _) = aligned_correlation(&s.right, &in_r[skip..], lag);
    assert!(
        ll.abs() > 0.9,
        "left channel tracks its input at only {ll:+.3}"
    );
    assert!(
        rr.abs() > 0.9,
        "right channel tracks its input at only {rr:+.3}"
    );

    let lr = correlation(&s.left, &s.right);
    assert!(
        lr.abs() < 0.3,
        "decoded channels are correlated at {lr:+.3}; the image collapsed"
    );
}

/// SILK codes a real adaptive mid/side pair, so at a rate that can afford a side
/// channel each decoded channel must track its *own* input, not the downmix.
///
/// Until the encoder gained `silk_stereo_LR_to_MS` it signalled `mid_only` on
/// every packet and both channels decoded to the same samples; the assertions
/// below are the ones that behaviour fails.
#[test]
fn silk_stereo_carries_a_real_side_channel() {
    for &(rate, bitrate) in &[(16_000i32, 40_000i32), (12_000, 36_000)] {
        let src = decorrelated(rate, 1.5);
        let s = roundtrip_stereo_as(
            rate,
            Application::Voip,
            bitrate,
            None,
            Some(Signal::Voice),
            &src,
        );
        assert!(
            s.modes.iter().all(|m| *m == "silk"),
            "{rate} Hz: expected SILK-only, got {:?}",
            s.modes.first()
        );

        let skip = (rate / 50) as usize * 15;
        let in_l = deinterleave(&src, 2, 0);
        let in_r = deinterleave(&src, 2, 1);
        let lag = (rate / 50) as usize;

        let (ll, _) = aligned_correlation(&s.left, &in_l[skip..], lag);
        let (rr, _) = aligned_correlation(&s.right, &in_r[skip..], lag);
        assert!(
            ll.abs() > 0.85,
            "{rate} Hz: left tracks its own input at only {ll:+.3}"
        );
        assert!(
            rr.abs() > 0.85,
            "{rate} Hz: right tracks its own input at only {rr:+.3}"
        );

        // A mid-only encode gives L == R exactly; anything close to that here
        // means the side channel was dropped.
        let lr = correlation(&s.left, &s.right);
        assert!(
            lr.abs() < 0.3,
            "{rate} Hz: decoded channels correlate at {lr:+.3}; the image collapsed"
        );

        let side: Vec<f32> = s
            .left
            .iter()
            .zip(&s.right)
            .map(|(a, b)| (a - b) * 0.5)
            .collect();
        let in_side: Vec<f32> = in_l[skip..]
            .iter()
            .zip(&in_r[skip..])
            .map(|(a, b)| (a - b) * 0.5)
            .collect();
        let kept = energy(&side) / energy(&in_side);
        assert!(
            kept > 0.7,
            "{rate} Hz: only {:.0}% of the side channel's energy survived",
            kept * 100.0
        );
    }
}

/// Below the rate at which a side channel is worth its bits, libopus narrows the
/// stereo image rather than spending the mid channel's budget on it, and
/// collapses to panned mono at the bottom. Narrowing must be monotonic in the
/// rate: a width that jumps around is a rate-control fault, and a width that
/// grows as the rate falls is a sign error.
#[test]
fn silk_stereo_narrows_the_image_as_the_rate_falls() {
    let rate = 16_000;
    let src = decorrelated(rate, 1.5);
    let skip = (rate / 50) as usize * 15;
    let in_l = deinterleave(&src, 2, 0);
    let in_r = deinterleave(&src, 2, 1);
    let in_side: Vec<f32> = in_l[skip..]
        .iter()
        .zip(&in_r[skip..])
        .map(|(a, b)| (a - b) * 0.5)
        .collect();

    let mut widths = Vec::new();
    for &bitrate in &[14_000i32, 20_000, 28_000, 40_000] {
        let s = roundtrip_stereo(rate, Application::Voip, bitrate, None, &src);
        let side: Vec<f32> = s
            .left
            .iter()
            .zip(&s.right)
            .map(|(a, b)| (a - b) * 0.5)
            .collect();
        widths.push((bitrate, energy(&side) / energy(&in_side)));
    }

    for w in windows_of_two(&widths) {
        let ((lo_rate, lo), (hi_rate, hi)) = w;
        assert!(
            hi >= lo - 0.05,
            "{hi_rate} b/s kept {hi:.3} of the side channel but {lo_rate} b/s kept {lo:.3}"
        );
    }
    let (_, widest) = *widths.last().unwrap();
    assert!(
        widest > 0.7,
        "even at 40 kb/s only {:.0}% of the side channel survived",
        widest * 100.0
    );
}

fn windows_of_two(v: &[(i32, f32)]) -> Vec<((i32, f32), (i32, f32))> {
    v.windows(2).map(|w| (w[0], w[1])).collect()
}

/// A concealed frame has to keep the stereo image. libopus carries the previous
/// frame's channel configuration through a loss and extrapolates both channels;
/// concealing the mid alone snaps the image to the centre for the length of the
/// loss, and at a mode switch — where concealment also feeds the cross-fade —
/// that lands in the middle of otherwise-good audio.
#[test]
fn silk_stereo_concealment_keeps_the_image() {
    let rate = 16_000;
    let bitrate = 40_000;
    let frame = (rate / 50) as usize;
    let src = decorrelated(rate, 1.0);

    let mut c = Codec::new(rate, 2, Application::Voip).bitrate(bitrate);
    let packets = c.encode_all(&src);

    // Decoded one packet at a time rather than through `decode_all`, because
    // dropping one of them is the whole point.
    let mut concealed = Vec::new();
    let mut last_good = Vec::new();
    for (i, pkt) in packets.iter().enumerate() {
        let mut o = vec![0.0f32; frame * 2];
        // Drop one packet well into the stream, once the coder has settled.
        if i == 30 {
            c.dec.decode(&[], frame, &mut o).expect("PLC");
            concealed = o;
            continue;
        }
        c.dec.decode(pkt, frame, &mut o).expect("decode");
        if i == 29 {
            last_good = o;
        }
    }

    let side = |v: &[f32]| -> f32 {
        let l = deinterleave(v, 2, 0);
        let r = deinterleave(v, 2, 1);
        energy(
            &l.iter()
                .zip(&r)
                .map(|(a, b)| (a - b) * 0.5)
                .collect::<Vec<f32>>(),
        )
    };

    let good = side(&last_good);
    let lost = side(&concealed);
    assert!(
        good > 1e-6,
        "the frame before the loss had no stereo image to keep"
    );
    assert!(
        lost > good * 0.15,
        "concealment kept only {:.1}% of the previous frame's side energy",
        100.0 * lost / good
    );
    assert!(
        concealed.iter().all(|s| s.is_finite()),
        "non-finite sample in the concealed frame"
    );
}

/// Phase-inverted input (L = -R) is pure side with zero mid. It must not blow up,
/// and a mid-only encoder must render it as near-silence rather than noise.
#[test]
fn phase_inverted_input_is_handled() {
    let rate = 16_000;
    let n = (rate as f32 * 1.5) as usize;
    let tone = sine(rate, n, 300.0, 0.5);
    let inverted: Vec<f32> = tone.iter().map(|s| -s).collect();
    let src = interleave(&[tone.clone(), inverted]);

    let s = roundtrip_stereo(rate, Application::Voip, 24_000, None, &src);
    let peak = s
        .left
        .iter()
        .chain(&s.right)
        .fold(0.0f32, |m, &v| m.max(v.abs()));
    assert!(
        peak.is_finite(),
        "phase-inverted input produced non-finite output"
    );
    assert!(
        peak < 1.5,
        "phase-inverted input produced peak {peak:.3}; expected near-silence"
    );
}

/// Identical channels must stay identical: mono content fed as stereo is the
/// most common real-world case and must not acquire a stereo image.
#[test]
fn identical_channels_stay_identical() {
    for &(rate, app, bitrate) in &[
        (16_000i32, Application::Voip, 24_000i32),
        (48_000, Application::Audio, 128_000),
    ] {
        let mono = music_like(rate, (rate as f32 * 1.5) as usize);
        let src = interleave(&[mono.clone(), mono]);
        let s = roundtrip_stereo(rate, app, bitrate, None, &src);
        let lr = correlation(&s.left, &s.right);
        assert!(
            lr > 0.95,
            "{rate} Hz: identical input channels decoded at correlation {lr:+.4}"
        );
    }
}

/// A 40 ms stereo SILK packet holds two SILK frames, and each one carries its
/// own stereo side-information.
///
/// libopus writes the prediction weights and the mid-only flag once per SILK
/// frame; its decoder reads them in the same place. Writing them only for the
/// first frame of the packet left the second frame's pair missing, so a
/// conforming decoder and this one parsed the same bytes differently from that
/// point on. It was invisible at 20 ms, where a packet holds exactly one frame.
///
/// The check is that a stereo 40 ms stream decodes to the same audio as the
/// equivalent 20 ms stream — a desynchronised parse cannot manage that.
#[test]
fn forty_millisecond_stereo_silk_codes_side_info_per_frame() {
    let rate = 16_000i32;
    let src = interleave(&[
        speech_like(rate, rate as usize * 2),
        sine(rate, rate as usize * 2, 300.0, 0.3),
    ]);

    let decode_at = |ms: i32| -> Vec<f32> {
        let mut c = Codec::new(rate, 2, Application::Voip)
            .frame_ms(ms)
            .bitrate(32_000)
            .signal_type(Signal::Voice);
        let r = c.roundtrip(&src);
        assert!(
            r.modes().iter().all(|m| *m == "silk"),
            "{ms}ms: expected SILK-only"
        );
        r.decoded
    };

    let short = decode_at(20);
    let long = decode_at(40);

    let n = short.len().min(long.len());
    assert!(n > rate as usize, "not enough audio to compare");
    for c in 0..2 {
        let a = deinterleave(&short[..n], 2, c);
        let b = deinterleave(&long[..n], 2, c);
        let (corr, _) = aligned_correlation(&b, &a, (rate / 50) as usize);
        assert!(
            corr >= 0.95,
            "ch{c}: 40 ms output correlates only {corr:.4} with the 20 ms output"
        );
    }
}

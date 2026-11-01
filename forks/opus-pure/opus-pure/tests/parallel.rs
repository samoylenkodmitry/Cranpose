//! `encode_parallel` against the serial encode it approximates.
//!
//! The contract this guards is not "the output is good" — a chunked encode of
//! decent audio is decent audio almost however it is chunked. It is that the
//! output is the stream *one continuous encoder* would have produced, and the
//! ways that can fail are not visible in a whole-clip average.
//!
//! Two of them, specifically. A worker primed on too little audio decides its
//! own coding mode, and then codes its **entire chunk** in it; averaged over a
//! long clip that is a fraction of a dB, and to a listener it is a mode switch
//! that should not be there. And the boundary itself costs a few frames, which
//! an average over hundreds cannot see. So these tests count disagreeing frames
//! per chunk, and measure SNR frame by frame.

mod common;
use common::*;
use opus_pure::{
    Application, Bandwidth, DEFAULT_WARMUP_MS, ParallelConfig, RateControl, Signal, encode_parallel,
};

const RATE: i32 = 48_000;
const FRAME: usize = 960; // 20 ms

fn config(channels: usize, bitrate: i32) -> ParallelConfig {
    let mut cfg = ParallelConfig::new(RATE, channels, Application::Audio);
    cfg.bitrate_bps = bitrate;
    cfg.threads = 4;
    cfg
}

/// The anchor: one worker and no priming is one continuous encoder, which is
/// exactly what `encode_parallel` claims to approximate.
fn serial(cfg: &ParallelConfig, pcm: &[f32]) -> Vec<Vec<u8>> {
    let mut c = *cfg;
    c.threads = 1;
    c.warmup_ms = 0;
    encode_parallel(&c, pcm, FRAME).unwrap()
}

fn stereo(mono: &[f32]) -> Vec<f32> {
    let lag = (RATE / 2000) as usize;
    let mut out = vec![0.0f32; mono.len() * 2];
    for i in 0..mono.len() {
        out[2 * i] = mono[i];
        out[2 * i + 1] = if i >= lag { mono[i - lag] * 0.79 } else { 0.0 };
    }
    out
}

/// Frames in `[lo, hi)` whose coding mode differs between the two streams.
fn mode_disagreement(a: &[Vec<u8>], b: &[Vec<u8>], lo: usize, hi: usize) -> usize {
    (lo..hi)
        .filter(|&i| packet_mode(&a[i]) != packet_mode(&b[i]))
        .count()
}

/// The plan is the contract for how the work is divided, so it has to describe
/// what actually happens rather than merely resemble it.
#[test]
fn plan_describes_what_encode_parallel_does() {
    let pcm = music_like(RATE, FRAME * 3000);
    let cfg = config(1, 64_000);
    let plan = cfg.plan(3000, FRAME);

    assert!(
        plan.workers > 1,
        "3000 frames should split across 4 threads"
    );
    assert_eq!(plan.warmup_frames, DEFAULT_WARMUP_MS as usize / 20);
    assert_eq!(plan.ranges.len(), plan.workers);

    // The ranges tile the clip exactly: contiguous, in order, no gap, no overlap.
    assert_eq!(plan.ranges[0].0, 0);
    assert_eq!(plan.ranges[plan.workers - 1].1, 3000);
    for w in 1..plan.workers {
        assert_eq!(plan.ranges[w - 1].1, plan.ranges[w].0);
    }

    // Worker 0 primes on nothing; every other worker primes on a full warm-up.
    assert_eq!(
        plan.redundant_frames,
        (plan.workers - 1) * plan.warmup_frames
    );
    assert!(
        plan.overhead() <= 0.25,
        "redundancy {:.3} exceeds the ceiling that caps the worker count",
        plan.overhead()
    );

    assert_eq!(encode_parallel(&cfg, &pcm, FRAME).unwrap().len(), 3000);
}

/// Below the redundancy ceiling there is nothing to gain from splitting, and the
/// caller is entitled to find that out without encoding first.
#[test]
fn short_input_falls_back_to_one_worker() {
    let cfg = config(1, 64_000);
    let warmup = cfg.warmup_frames(FRAME);
    let frames = warmup * 2; // less than the four warm-ups a chunk needs
    let pcm = music_like(RATE, FRAME * frames);

    let plan = cfg.plan(frames, FRAME);
    assert_eq!(plan.workers, 1);
    assert_eq!(plan.ranges, vec![(0, frames)]);
    assert_eq!(plan.redundant_frames, 0);
    assert_eq!(plan.overhead(), 0.0);

    let par = encode_parallel(&cfg, &pcm, FRAME).unwrap();
    assert_eq!(par.len(), frames);
    assert_eq!(
        par,
        serial(&cfg, &pcm),
        "a single worker is the serial encode"
    );
}

/// Warm-up is expressed in time because what it has to cover is a time constant,
/// so the same setting must mean the same audio at every frame duration.
#[test]
fn warm_up_is_the_same_audio_at_every_frame_size() {
    let cfg = config(1, 64_000);
    for &(frame, ms) in &[
        (120usize, 2.5f64),
        (240, 5.0),
        (480, 10.0),
        (960, 20.0),
        (2880, 60.0),
    ] {
        let frames = cfg.warmup_frames(frame);
        let covered = frames as f64 * ms;
        assert!(
            covered >= f64::from(cfg.warmup_ms) && covered < f64::from(cfg.warmup_ms) + ms,
            "{ms} ms frames: {frames} frames cover {covered} ms, wanted {} ms",
            cfg.warmup_ms
        );
    }
    assert_eq!(cfg.warmup_frames(0), 0);

    let mut none = cfg;
    none.warmup_ms = 0;
    assert_eq!(none.warmup_frames(FRAME), 0);
}

/// The regression this file exists for.
///
/// The coding mode comes from a content analysis two seconds deep. Priming for
/// less than that — the default used to be 160 ms — leaves each worker choosing
/// its own, and it then holds that choice for its whole chunk. On this clip a
/// 160 ms warm-up put 25 to 36 frames of every 800 into a mode the continuous
/// encoder never used; the default clears it.
#[test]
fn the_default_warm_up_keeps_the_serial_encoder_s_coding_mode() {
    let pcm = speech_like(RATE, FRAME * 6000); // 120 s
    let cfg = config(1, 16_000);
    let ser = serial(&cfg, &pcm);
    let par = encode_parallel(&cfg, &pcm, FRAME).unwrap();
    let plan = cfg.plan(6000, FRAME);

    for (w, &(lo, hi)) in plan.ranges.iter().enumerate() {
        let differ = mode_disagreement(&ser, &par, lo, hi);
        assert!(
            differ * 100 <= hi - lo,
            "chunk {w} [{lo}..{hi}) codes {differ} frames in a mode the serial \
             encoder did not use, over 1% of the chunk"
        );
    }
}

/// Chunking must not put a hole in the audio. Averaged over the clip it never
/// does; the question is only ever about individual frames.
#[test]
fn no_frame_is_much_worse_than_the_serial_encode() {
    let pcm = speech_like(RATE, FRAME * 3000); // 60 s
    let cfg = config(1, 24_000);
    let ser = serial(&cfg, &pcm);
    let par = encode_parallel(&cfg, &pcm, FRAME).unwrap();

    let decode = |p: &[Vec<u8>]| Codec::new(RATE, 1, Application::Audio).decode_all(p);
    let (ds, dp) = (decode(&ser), decode(&par));
    let (_, lag) = aligned_correlation(&ds, &pcm, FRAME);

    let frame_snr = |dec: &[f32], f: usize| -> f32 {
        let (a, b) = (lag + f * FRAME, lag + (f + 1) * FRAME);
        if b > dec.len() {
            return f32::INFINITY;
        }
        let (d, s) = (&dec[a..b], &pcm[f * FRAME..(f + 1) * FRAME]);
        let sig: f32 = s.iter().map(|x| x * x).sum();
        let err: f32 = d.iter().zip(s).map(|(x, y)| (x - y).powi(2)).sum();
        if err > 0.0 {
            10.0 * (sig / err).log10()
        } else {
            f32::INFINITY
        }
    };

    let frames = (ds.len().min(dp.len()) - lag) / FRAME;
    let mut worst = (0usize, 0.0f32);
    for f in 0..frames {
        let drop = frame_snr(&ds, f) - frame_snr(&dp, f);
        if drop > worst.1 {
            worst = (f, drop);
        }
    }
    assert!(
        worst.1 < 6.0,
        "frame {} is {:.2} dB below the serial encode",
        worst.0,
        worst.1
    );
}

/// The packet buffer each worker writes into is sized once, from the longest
/// packet an encode call can produce — and `OpusEncoder::encode` treats that
/// buffer's length as the packet's byte budget, exactly as libopus treats
/// `max_data_bytes`. So a buffer that is merely *generous* is not enough: too
/// small and the encoder quietly codes a smaller packet instead of failing.
///
/// It used to be a round 4000 bytes. At 120 ms stereo that capped the stream at
/// 266 kb/s however much was asked for, with no error anywhere. Nothing had run
/// the parallel path at a long frame size.
#[test]
fn a_long_frame_gets_the_bitrate_it_asked_for() {
    const FRAME_120MS: usize = 5760;
    const BITRATE: i32 = 510_000;
    let pcm = stereo(&music_like(RATE, FRAME_120MS * 300));
    let cfg = config(2, BITRATE);

    assert!(
        cfg.plan(300, FRAME_120MS).workers > 1,
        "the clip must actually split"
    );
    let par = encode_parallel(&cfg, &pcm, FRAME_120MS).unwrap();
    assert_eq!(par.len(), 300);

    let seconds = 300.0 * FRAME_120MS as f64 / f64::from(RATE);
    let delivered = par.iter().map(Vec::len).sum::<usize>() as f64 * 8.0 / seconds;
    assert!(
        delivered > f64::from(BITRATE) * 0.95,
        "asked for {} kb/s and got {:.0} kb/s: the worker's packet buffer is capping it",
        BITRATE / 1000,
        delivered / 1000.0,
    );
}

/// Two runs of the same input must produce the same bytes: the chunk boundaries
/// are fixed, and nothing else about a worker depends on scheduling.
#[test]
fn output_is_deterministic() {
    let pcm = stereo(&music_like(RATE, FRAME * 1200));
    let cfg = config(2, 96_000);
    assert_eq!(
        encode_parallel(&cfg, &pcm, FRAME).unwrap(),
        encode_parallel(&cfg, &pcm, FRAME).unwrap()
    );
}

/// Every setting on the config has to reach the worker encoders, because that
/// struct is the only way to configure them. A setting added to `OpusEncoder`
/// and forgotten here is silently unreachable through this path, so each of
/// these asserts on an effect visible in the bitstream.
#[test]
fn every_setting_reaches_the_worker_encoders() {
    let pcm = speech_like(RATE, FRAME * 1500); // 30 s
    let plan_ranges = |cfg: &ParallelConfig| cfg.plan(1500, FRAME).workers;

    // Bandwidth, read straight out of the TOC.
    let mut cfg = config(1, 32_000);
    cfg.force_bandwidth = Some(Bandwidth::Wideband);
    assert!(plan_ranges(&cfg) > 1, "the clip must actually split");
    for p in encode_parallel(&cfg, &pcm, FRAME).unwrap() {
        assert!(packet_bandwidth_hz(&p) <= 8_000, "forced bandwidth ignored");
    }

    // Signal type, which decides the coding mode outright at this rate.
    let mut voice = config(1, 16_000);
    voice.signal_type = Some(Signal::Voice);
    let mut music = config(1, 16_000);
    music.signal_type = Some(Signal::Music);
    let modes = |cfg: &ParallelConfig| {
        let mut m: Vec<&str> = encode_parallel(cfg, &pcm, FRAME)
            .unwrap()
            .iter()
            .map(|p| packet_mode(p))
            .collect();
        m.sort_unstable();
        m.dedup();
        m
    };
    assert_eq!(modes(&voice), ["hybrid"]);
    assert_eq!(modes(&music), ["celt"]);

    // Constant bitrate, which makes every packet the same size.
    let mut cbr = config(1, 32_000);
    cbr.rate_control = RateControl::Cbr;
    let sizes = encode_parallel(&cbr, &pcm, FRAME).unwrap();
    let first = sizes[0].len();
    assert!(
        sizes.iter().all(|p| p.len() == first),
        "CBR ignored: packet sizes vary"
    );

    // In-band FEC, which a decoder can be asked to recover a lost frame from.
    let mut fec = config(1, 32_000);
    fec.use_inband_fec = true;
    fec.packet_loss_perc = 20;
    let with_fec = encode_parallel(&fec, &pcm, FRAME).unwrap();
    let without = encode_parallel(&config(1, 32_000), &pcm, FRAME).unwrap();
    assert_ne!(with_fec, without, "in-band FEC settings ignored");
}

//! What the CELT layer actually delivers above 8 kHz in a hybrid stream.
//!
//! A hybrid packet splits at 8 kHz: SILK codes below it, CELT above. The CELT
//! side gets what is left of the bitrate, which at conversational rates is a
//! few kb/s for a 12 kHz-wide band — too little to code a waveform, so it is
//! filled with spectrally-shaped noise instead. Two things follow, and both are
//! why this file exists rather than another SNR assertion in `roundtrip.rs`:
//!
//! The band is a small share of the energy, so a broadband measurement cannot
//! see it. Every number here would move by hundredths of a dB in a full-band
//! SNR whatever the high band did.
//!
//! And SNR cannot judge it even when pointed straight at it. Noise that has the
//! right spectrum but no phase relationship to the source scores *worse* than
//! silence: emitting nothing in the band scores exactly 0 dB, and both this
//! crate and libopus 1.6.1 score below that. What is left to hold is the thing
//! CELT promises whether or not it can afford the shape — that the band arrives
//! at the level it left at, and that its moving envelope is tracked.
//!
//! Measured against libopus by `reference/highband/`, which is also where the
//! bounds below come from.

mod common;
use common::*;
use opus_pure::{Application, Bandwidth, Signal};

/// Where SILK stops and CELT starts in a hybrid packet.
const CROSSOVER_HZ: f64 = 8000.0;

/// The top of each bandwidth, so a stream is only measured over what it was
/// asked to code. Superwideband stops at 12 kHz and is silent above it by
/// design; charging it for that reads as a 1.3 dB deficit that is not one.
fn top_hz(bandwidth: Bandwidth) -> f64 {
    match bandwidth {
        Bandwidth::Superwideband => 12_000.0,
        _ => 24_000.0,
    }
}

/// 20 ms, so a block is a coded frame.
const BLOCK: usize = 960;

/// Blocks quieter than this against the loudest one are pauses. Including them
/// measures the ratio of one near-silence to another, which says nothing about
/// the codec and swamps everything that does.
const ACTIVE_FLOOR_DB: f64 = -40.0;

/// How the high band came through: its level against the source's in dB, and
/// the spread of that ratio across blocks once the level is taken out.
struct HighBand {
    level_db: f64,
    envelope_db: f64,
    active_blocks: usize,
}

fn high_band(source: &[f32], decoded: &[f32], rate: i32, top: f64) -> HighBand {
    // Both signals through the same linear-phase filters, so their delay cancels.
    let src = band_limit(source, rate, CROSSOVER_HZ, top);
    let dec = band_limit(decoded, rate, CROSSOVER_HZ, top);

    // The codec's own delay does not. It is a whole number of samples and
    // pinned as one by `encoder_delay.rs`, so an integer search finds it.
    let (_, lag) = aligned_correlation(&dec, &src, 1000);
    let n = (src.len() - lag).min(dec.len() - lag);
    let (src, dec) = (&src[..n], &dec[lag..lag + n]);

    let block_energy = |v: &[f32]| -> Vec<f64> {
        v.as_chunks::<BLOCK>()
            .0
            .iter()
            .map(|b| b.iter().map(|s| (*s as f64) * (*s as f64)).sum())
            .collect()
    };
    let (se, de) = (block_energy(src), block_energy(dec));
    let peak = se.iter().copied().fold(0.0f64, f64::max);
    let floor = peak * 10f64.powf(ACTIVE_FLOOR_DB / 10.0);

    let ratios: Vec<f64> = se
        .iter()
        .zip(&de)
        .filter(|(s, d)| **s >= floor && **d > 0.0)
        .map(|(s, d)| 10.0 * (d / s).log10())
        .collect();
    assert!(
        !ratios.is_empty(),
        "no block had energy above {CROSSOVER_HZ} Hz to measure"
    );
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    let var = ratios.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / ratios.len() as f64;
    HighBand {
        level_db: mean,
        envelope_db: var.sqrt(),
        active_blocks: ratios.len(),
    }
}

/// 3.6 s of speech, coded hybrid at `bitrate`, and what came back above 8 kHz.
fn hybrid_high_band(bitrate: i32, channels: usize, bandwidth: Bandwidth) -> HighBand {
    let rate = 48_000;
    let frames = 180;
    let mono = speech_like(rate, BLOCK * frames);
    let pcm = if channels == 1 {
        mono.clone()
    } else {
        let right = sine(rate, mono.len(), 660.0, 0.25);
        interleave(&[
            mono.clone(),
            mono.iter().zip(&right).map(|(a, b)| a * 0.75 + b).collect(),
        ])
    };

    let mut codec = Codec::new(rate, channels, Application::Voip)
        .bitrate(bitrate)
        .bandwidth(bandwidth)
        .signal_type(Signal::Voice);
    let rt = codec.roundtrip(&pcm);

    let modes = rt.modes();
    let hybrid = modes.iter().filter(|m| **m == "hybrid").count();
    assert!(
        hybrid * 10 >= modes.len() * 9,
        "{}: wanted a hybrid stream, got {hybrid} of {} packets in hybrid",
        codec.label(),
        modes.len()
    );

    high_band(
        &deinterleave(&pcm, channels, 0),
        &rt.channel(0),
        rate,
        top_hz(bandwidth),
    )
}

/// The property the CELT layer is actually holding up here: whatever it can
/// afford to code, the band comes back at the level it went in at.
///
/// The bound is 1.5 dB rather than something tighter because the band is a
/// noise fill, and a noise fill's level is only as accurate as the coarse
/// energy it was told to hit. Measured here, the level spans +0.25 dB at
/// 12 kb/s to -0.60 dB at 64 kb/s.
///
/// libopus 1.6.1 does something different with the same band: it quiets it in
/// proportion to how little it could afford to code, from -4 dB at 12 kb/s to
/// -0.4 dB at 64 kb/s. Neither behaviour is pinned as correct here, and which
/// sounds better is not a question this file can answer. What is pinned is that
/// ours does not drift, because an allocator change that silently starved the
/// high band would show up as level before it showed up anywhere else. See
/// `reference/highband/` for the comparison.
#[test]
fn the_hybrid_high_band_arrives_at_the_level_it_left_at() {
    for bitrate in [12_000, 20_000, 32_000, 64_000] {
        let hb = hybrid_high_band(bitrate, 1, Bandwidth::Fullband);
        println!(
            "{:>6} b/s mono: level {:+.2} dB, envelope {:.2} dB, {} active blocks",
            bitrate, hb.level_db, hb.envelope_db, hb.active_blocks
        );
        assert!(
            hb.level_db.abs() < 1.5,
            "{bitrate} b/s: high band came back {:+.2} dB against the source",
            hb.level_db
        );
        assert!(
            hb.envelope_db < 3.0,
            "{bitrate} b/s: high band envelope varies by {:.2} dB",
            hb.envelope_db
        );
    }
}

/// Stereo and superwideband take different paths through the allocator, and
/// stereo is where the hybrid rate split was wrong before: it was handing SILK
/// a stereo packet's whole rate and starving CELT of the high band entirely.
#[test]
fn the_high_band_holds_across_stereo_and_superwideband() {
    for (channels, bandwidth, label) in [
        (2, Bandwidth::Fullband, "fullband stereo"),
        (1, Bandwidth::Superwideband, "superwideband mono"),
        (2, Bandwidth::Superwideband, "superwideband stereo"),
    ] {
        let hb = hybrid_high_band(24_000, channels, bandwidth);
        println!(
            "{label}: level {:+.2} dB, envelope {:.2} dB, {} active blocks",
            hb.level_db, hb.envelope_db, hb.active_blocks
        );
        assert!(
            hb.level_db.abs() < 1.5,
            "{label}: high band came back {:+.2} dB against the source",
            hb.level_db
        );
    }
}

//! The test signals themselves, pinned.
//!
//! Everything frozen in `bitstream_stability.rs` and `decoder_conformance.rs`
//! is a hash of what the encoder did to one of these signals, so those hashes
//! are only meaningful while the signals are. Two properties have to hold, and
//! neither is obvious from reading the generators:
//!
//! 1. **The same bytes on every target.** The generators originally used
//!    `f32::sin` and `f32::powf`, which call the platform's libm. Apple's arm64
//!    and x86_64 implementations differ in the last bits, so every signal was a
//!    different signal on each architecture. Five of the six `decoder_conformance`
//!    configurations absorbed that in SILK's quantiser and one did not, which is
//!    why the suite passed on aarch64 and failed on x86_64. See
//!    [`common::sin_turns`] for what replaced them.
//!
//! 2. **The same bytes over time.** Editing a generator silently invalidates
//!    every frozen bitstream downstream. The hashes below make that edit fail
//!    here first, naming the cause, instead of surfacing as an unexplained
//!    encoder regression three files away.
//!
//! A failure here is not a bug in the codec. Either a generator changed on
//! purpose, in which case re-freeze this file and then every hash that depends
//! on it, or a libm call crept back into one, in which case take it back out.

mod common;
use common::*;

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Hash the raw `f32` bit patterns. Unlike `decoder_conformance`'s `pcm_hash`,
/// which quantises to i16 first, this has to catch a one-ulp drift.
fn hash(v: &[f32]) -> u64 {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for &s in v {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    fnv1a(&bytes)
}

/// Every rate the suite generates at, and enough samples to cover the longest
/// frozen stream.
const RATES: [i32; 5] = [8_000, 12_000, 16_000, 24_000, 48_000];

#[test]
fn generated_signals_are_reproducible() {
    const SPEECH: [u64; 5] = [
        0x513a_dfa2_4bb0_fb62,
        0x2751_495b_2d69_2a61,
        0xbb98_f203_debf_c308,
        0x065f_fa68_3ed8_033e,
        0xfba5_1af9_da24_4b46,
    ];
    const MUSIC: [u64; 5] = [
        0xc894_70d9_cd18_2f86,
        0xa2c7_199f_0729_093a,
        0xb286_5296_e986_860f,
        0xdd61_1577_280b_2f6c,
        0x11dd_c16e_e03d_c1cc,
    ];
    const SINE: [u64; 5] = [
        0x119a_8660_1950_39a5,
        0xf0c1_6e4b_3917_a4a5,
        0xd54c_87d7_afb4_7ca5,
        0xa2e3_5395_db70_b125,
        0x8684_7a28_1a40_a0a5,
    ];

    for (i, &rate) in RATES.iter().enumerate() {
        let n = (rate / 50) as usize * 100; // 2 s
        let cases = [
            ("speech_like", hash(&speech_like(rate, n)), SPEECH[i]),
            ("music_like", hash(&music_like(rate, n)), MUSIC[i]),
            ("sine", hash(&sine(rate, n, 440.0, 0.5)), SINE[i]),
        ];
        for (name, got, want) in cases {
            assert_eq!(
                got, want,
                "{name} at {rate} Hz, {got:#018x} vs {want:#018x}"
            );
        }
    }
}

/// `sin_turns` against the identities that define it, rather than against a
/// libm it deliberately does not call.
///
/// The tolerance is `f32::EPSILON`: these signals are `f32`, so agreeing to
/// better than an `f32` ulp is all the accuracy that can survive to the output.
#[test]
fn sin_turns_is_a_sine() {
    let eps = f32::EPSILON as f64;

    // The quarter-turn landmarks.
    for (x, want) in [
        (0.0, 0.0),
        (0.25, 1.0),
        (0.5, 0.0),
        (0.75, -1.0),
        (1.0, 0.0),
        (-0.25, -1.0),
    ] {
        assert!(
            (sin_turns(x) - want).abs() < eps,
            "sin_turns({x}) = {}, want {want}",
            sin_turns(x)
        );
    }

    // Periodicity, oddness, and the Pythagorean identity with the cosine taken
    // a quarter turn along. Sampled off any grid that would make them trivial.
    let mut x = -37.0f64;
    while x < 37.0 {
        let s = sin_turns(x);
        let c = sin_turns(x + 0.25);
        assert!((s - sin_turns(x + 1.0)).abs() < eps, "period at {x}");
        assert!((s + sin_turns(-x)).abs() < eps, "oddness at {x}");
        assert!((s * s + c * c - 1.0).abs() < eps, "s²+c² at {x}");
        // The double-angle identity, which no single symmetry above implies.
        assert!(
            (sin_turns(2.0 * x) - 2.0 * s * c).abs() < eps,
            "double angle at {x}"
        );
        x += 0.017_317;
    }
}

/// The generators have to stay *usable* as well as reproducible: a signal that
/// silently went to zero or clipped flat would keep a stable hash while making
/// every fidelity test downstream meaningless.
#[test]
fn generated_signals_are_still_signals() {
    for &rate in &RATES {
        let n = (rate / 50) as usize * 100;
        for (name, v) in [
            ("speech_like", speech_like(rate, n)),
            ("music_like", music_like(rate, n)),
            ("sine", sine(rate, n, 440.0, 0.5)),
        ] {
            let peak = v.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
            assert!(
                (0.05..=1.0).contains(&peak),
                "{name} at {rate} Hz: peak {peak} is not a usable level"
            );
            assert!(
                energy(&v) > 1e-4,
                "{name} at {rate} Hz: energy {} is too low to test with",
                energy(&v)
            );
            // Not a constant, and not clipped to a square wave.
            let flat = v.iter().filter(|&&s| s.abs() >= peak * 0.999).count();
            assert!(
                flat * 20 < v.len(),
                "{name} at {rate} Hz: {flat} of {} samples sit at the peak",
                v.len()
            );
        }
    }
}

//! Frozen-hash gate against accidental bitstream drift.
//!
//! The encoder is deterministic, so a refactor that is meant to preserve output
//! must reproduce these bytes exactly. A change that legitimately moves the
//! bitstream is not caught by this test — it is *reported* by it, and the hash
//! is re-frozen deliberately with a note saying why.
//!
//! Provenance: every hash below was verified byte-identical against upstream
//! `rusty-opus` 0.9.1 when this crate was forked from it, so they anchor the
//! port to its origin rather than merely to itself. The stereo SILK/hybrid
//! configurations are deliberately absent from that guarantee — see
//! `tests/stereo.rs` for why their bitstream had to change.
//!
//! The 24 kHz entry is also no longer upstream's. Upstream had no CELT support
//! below 48 kHz, and what it emitted at 24 kHz decoded to full-scale noise
//! (peaks past 600,000 for a source peaking at 0.4). Since CELT now codes lower
//! rates the way libopus does — zero-stuffing the input up to 48 kHz and
//! limiting the coded bands — the bitstream necessarily moved. Re-frozen
//! 2026-08-21; `tests/roundtrip.rs` asserts the audio it now produces.
//!
//! The three mono CELT entries (48 kHz at 64 and 128 kb/s, 24 kHz at 48 kb/s)
//! moved again on 2026-08-21, and are no longer upstream's either. The aarch64
//! `pitch_downsample` kernel advanced its input one sample per output instead
//! of two, so three of every four samples in the CELT prefilter's pitch buffer
//! were the wrong ones and the pitch estimate that drives the postfilter was
//! built on them. Fixing the kernel changes what the prefilter signals, hence
//! the bitstream. Only the mono entries moved: the stereo path never took the
//! vectorised branch. On x86-64, where the scalar path already ran, these
//! streams are unchanged. See `celt::pitch::tests::downsample_simd_matches_
//! the_scalar_path`.
//!
//! All five CELT entries moved again on 2026-08-21 when the NEON PVQ search
//! was removed — an aarch64-only change, since no other target dispatched to
//! it. It maximised the wrong
//! objective (`Rxy/Ryy` rather than the reference's `Rxy^2/Ryy`) on short
//! codewords and scored the rest with an eight-bit reciprocal-square-root
//! estimate, so it chose a worse codeword in 450 of 1880 measured cases and
//! gave up as much as 29.7% of the search objective. The scalar search it now
//! defers to reproduces libopus `op_pvq_search_c` exactly. Round-trip SNR rose
//! by up to 0.49 dB at unchanged bitrate; the encode sweep got about 1.5%
//! slower. See `celt::pvq::simd_tests`.
//!
//! The two 60 ms entries were added on 2026-08-21, when the encoder learned to
//! emit that duration; they have no upstream counterpart because upstream
//! rejected 60 ms.
//!
//! The 48 kHz 60 ms music entry moved on 2026-08-22, when the encoder learned to
//! split a packet into several frames. It had been SILK only because the encoder
//! forced every duration above 20 ms to SILK, which libopus does not: it splits
//! the packet into frames the chosen mode can code. That configuration is a
//! CELT one, and it now emits three 20 ms CELT frames sharing a TOC — the same
//! 479-byte code 3 packets libopus 1.6.1 produces from the same audio. Being
//! CELT, its hash is float and arch-gated like the other CELT entries. The
//! 100 and 120 ms entries were added at the same time, for the framing that had
//! no encoder before it.
//!
//! Every entry moved on 2026-08-22, and none of it was the encoder. The test
//! signals were built with `f32::sin` and `f32::powf`, which call the platform's
//! libm; Apple's arm64 and x86_64 implementations differ in the last bits, so
//! these hashes described a slightly different input on each architecture.
//! `tests/common/sin_turns` replaced those calls with a polynomial built from
//! correctly-rounded operations alone, `tests/test_signals.rs` pins the result,
//! and the values below were re-frozen from the signals that test now
//! guarantees. The three SILK entries are since byte-identical on aarch64 and
//! x86_64, where before they agreed only by the good luck of the quantiser
//! absorbing the difference.
//!
//! The eight CELT and hybrid entries moved on 2026-08-22, and the three SILK
//! ones did not. The CELT layer's input was missing libopus's 4 ms
//! `delay_compensation`: it coded the newest samples where libopus codes from
//! `Fs/250` back, so every CELT-only stream ran 4 ms ahead of the reference and
//! the timeline jumped by that much whenever the mode changed. Feeding CELT off
//! the delayed timeline necessarily moves the bytes. SILK always read the new
//! frame, in libopus and here alike, so the SILK entries are untouched — which
//! is the shape of the fix showing up in the table.
//!
//! The two hybrid entries were added on 2026-08-23. Nothing in this table had
//! been hybrid before — the 48 kHz music entries are CELT and the 8 and 16 kHz
//! speech ones SILK — so the whole hybrid path was outside this gate, and a
//! rate-control divergence from libopus sat there unreported until
//! `reference/speed/` measured delivered bitrate against the C library. They
//! have no upstream counterpart and are frozen from this crate. Being hybrid
//! they carry a CELT layer, so their hashes are float and arch-gated like the
//! CELT entries. The stereo one exists specifically to cover SILK's per-channel
//! hybrid allocation, which this port had been computing at the full stereo rate.
//!
//! Re-freeze with: `cargo test --release --test bitstream_stability -- --nocapture`

mod common;
use common::*;
use opus_pure::Application;

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn encode_stream(
    rate: i32,
    channels: usize,
    app: Application,
    bitrate: i32,
    kind: &str,
    frame_ms: i32,
) -> (Vec<u8>, bool) {
    let frame = (rate * frame_ms / 1000) as usize;
    let mono = if kind == "speech" {
        speech_like(rate, rate as usize * 2)
    } else {
        music_like(rate, rate as usize * 2)
    };
    let pcm: Vec<f32> = if channels == 2 {
        mono.iter().flat_map(|&s| [s, s * 0.8]).collect()
    } else {
        mono
    };
    let packets = Codec::new(rate, channels, app)
        .frame_samples(frame)
        .bitrate(bitrate)
        .encode_all(&pcm);
    // Whether every packet came out of SILK, which decides whether the hash is
    // reproducible off the architecture it was frozen on.
    let all_silk = packets.iter().all(|p| packet_mode(p) == "silk");
    (packets.concat(), all_silk)
}

/// One frozen configuration and the bytes it must still produce.
struct Frozen {
    rate: i32,
    channels: usize,
    application: Application,
    bitrate: i32,
    content: &'static str,
    frame_ms: i32,
    hash: u64,
    len: usize,
}

const FROZEN: &[Frozen] = &[
    Frozen {
        rate: 16_000,
        channels: 1,
        application: Application::Voip,
        bitrate: 16_000,
        content: "speech",
        frame_ms: 20,
        hash: 0x114b_965f_03ad_a54f,
        len: 3140,
    },
    Frozen {
        rate: 16_000,
        channels: 1,
        application: Application::Voip,
        bitrate: 24_000,
        content: "speech",
        frame_ms: 20,
        hash: 0xd1ac_f9fa_bc06_76d8,
        len: 4752,
    },
    Frozen {
        rate: 8_000,
        channels: 1,
        application: Application::Voip,
        bitrate: 12_000,
        content: "speech",
        frame_ms: 20,
        hash: 0xe832_89d1_489b_8307,
        len: 2648,
    },
    Frozen {
        rate: 48_000,
        channels: 1,
        application: Application::Audio,
        bitrate: 64_000,
        content: "music",
        frame_ms: 20,
        hash: 0xf460_5710_a4e4_f401,
        len: 16100,
    },
    Frozen {
        rate: 48_000,
        channels: 2,
        application: Application::Audio,
        bitrate: 96_000,
        content: "music",
        frame_ms: 20,
        hash: 0x5409_9062_2d28_dbe6,
        len: 24100,
    },
    Frozen {
        rate: 48_000,
        channels: 2,
        application: Application::Audio,
        bitrate: 128_000,
        content: "music",
        frame_ms: 20,
        hash: 0x1330_5dd3_02c1_f1a2,
        len: 32100,
    },
    Frozen {
        rate: 48_000,
        channels: 1,
        application: Application::Audio,
        bitrate: 128_000,
        content: "music",
        frame_ms: 20,
        hash: 0x7d26_9362_79ef_7dff,
        len: 32100,
    },
    Frozen {
        rate: 24_000,
        channels: 1,
        application: Application::Audio,
        bitrate: 48_000,
        content: "music",
        frame_ms: 20,
        hash: 0xe6a8_660c_5132_c125,
        len: 12100,
    },
    // Durations past 20 ms, where a packet may hold more than one frame. The
    // SILK entries stay single-frame (16 kHz speech at 60 ms is one frame of
    // three SILK sub-frames, the only layout that reaches the third); the music
    // entries are CELT, which has no frame longer than 20 ms and so codes these
    // as 3, 5 and 6 frames sharing a TOC.
    Frozen {
        rate: 16_000,
        channels: 1,
        application: Application::Voip,
        bitrate: 24_000,
        content: "speech",
        frame_ms: 60,
        hash: 0x80d7_c5bf_469c_d264,
        len: 4520,
    },
    Frozen {
        rate: 48_000,
        channels: 1,
        application: Application::Audio,
        bitrate: 64_000,
        content: "music",
        frame_ms: 60,
        hash: 0xf85a_841f_bd86_8035,
        len: 15807,
    },
    Frozen {
        rate: 48_000,
        channels: 1,
        application: Application::Audio,
        bitrate: 64_000,
        content: "music",
        frame_ms: 100,
        hash: 0xd0e6_552d_bf1c_ad2c,
        len: 15940,
    },
    Frozen {
        rate: 48_000,
        channels: 2,
        application: Application::Audio,
        bitrate: 96_000,
        content: "music",
        frame_ms: 120,
        hash: 0xf6bd_d5d7_d860_e829,
        len: 22976,
    },
    // Hybrid, mono and stereo. Added 2026-08-23: nothing in this table was
    // hybrid before, which is why a hybrid rate-control divergence from libopus
    // went unreported here until `reference/speed/` measured delivered bitrate.
    // The stereo entry covers the per-channel SILK allocation specifically.
    Frozen {
        rate: 48_000,
        channels: 1,
        application: Application::Voip,
        bitrate: 20_000,
        content: "speech",
        frame_ms: 20,
        hash: 0xb041_b110_172b_54d8,
        len: 4055,
    },
    Frozen {
        rate: 48_000,
        channels: 2,
        application: Application::Voip,
        bitrate: 20_000,
        content: "speech",
        frame_ms: 20,
        hash: 0x7418_e2b8_503c_1946,
        len: 3991,
    },
];

/// The architecture these hashes were captured on. The SILK layer is
/// fixed-point and reproduces bit-for-bit anywhere; the CELT layer is float, so
/// a different SIMD kernel summing the same terms in a different order can tip
/// a marginal quantisation decision and move the bytes. libopus has the same
/// property — its conformance is defined on the fixed-point decoder, not on
/// bit-equality of the float encoder. So on any other architecture this test
/// holds the byte *lengths*, which do carry across, and prints the hashes for
/// whoever wants to freeze that architecture too.
///
/// They were captured on aarch64 macOS and hold unchanged on aarch64 Linux, so
/// what they depend on is the architecture and not the C library underneath it.
/// CI runs both, which is what keeps that a checked claim.
const FROZEN_ARCH: &str = "aarch64";

#[test]
fn bitstream_matches_the_frozen_reference() {
    let same_arch = std::env::consts::ARCH == FROZEN_ARCH;
    let mut drift = Vec::new();
    for f in FROZEN {
        let (bytes, all_silk) = encode_stream(
            f.rate,
            f.channels,
            f.application,
            f.bitrate,
            f.content,
            f.frame_ms,
        );
        let got = fnv1a(&bytes);
        let label = format!(
            "{}/{}ch/{:?}/{}/{}/{}ms",
            f.rate, f.channels, f.application, f.bitrate, f.content, f.frame_ms
        );
        println!("{label:<44} -> {got:016x} ({} bytes)", bytes.len());
        // SILK is fixed-point throughout, so a stream with no CELT in it
        // reproduces bit-for-bit anywhere and its hash is enforced everywhere.
        // Read from the packets rather than assumed from the settings: which
        // mode a duration lands in is the mode decision's to make.
        let hash_matters = same_arch || all_silk;
        if (hash_matters && got != f.hash) || bytes.len() != f.len {
            drift.push(format!(
                "{label}: got {got:016x}/{} bytes, expected {:016x}/{} bytes",
                bytes.len(),
                f.hash,
                f.len
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "encoder output moved on {}:\n  {}\n\nIf the change is intentional, re-freeze the hashes \
         above and record why in this file's header.",
        std::env::consts::ARCH,
        drift.join("\n  ")
    );
}

/// Encoding the same input twice must give the same bytes. A codec that depends
/// on uninitialized memory, address layout, or ambient state fails here long
/// before the frozen hashes above are worth trusting.
#[test]
fn encoding_is_deterministic() {
    for f in FROZEN {
        let once = encode_stream(
            f.rate,
            f.channels,
            f.application,
            f.bitrate,
            f.content,
            f.frame_ms,
        );
        let twice = encode_stream(
            f.rate,
            f.channels,
            f.application,
            f.bitrate,
            f.content,
            f.frame_ms,
        );
        assert_eq!(
            once.0, twice.0,
            "{} Hz {}ch {} bps {} ms is not reproducible",
            f.rate, f.channels, f.bitrate, f.frame_ms
        );
    }
}

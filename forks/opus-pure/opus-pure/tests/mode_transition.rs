//! When the encoder changes coding mode mid-stream, the decoder has to bridge
//! the seam.
//!
//! The layer taking over starts with no overlap-add history, so its first
//! samples come out of silence. libopus covers that by concealing 5 ms in the
//! *outgoing* mode and cross-fading it over the head of the frame
//! (`opus_decoder.c` `pcm_transition`). Without it a mode switch punches a hole
//! in the output: measured against libopus decoding the very same bitstream,
//! the transition frame's worst sample was off by 1.22 on a signal peaking at
//! 0.4, and the stream's agreement collapsed from >100 dB to 26 dB.

mod common;
use common::*;
use opus_pure::Application;

const RATE: i32 = 48_000;
const FRAME: usize = 960; // 20 ms
const F2_5: usize = RATE as usize / 400;

/// Encode speech, then music, then speech again at a bitrate near the
/// SILK/CELT boundary, which makes the mode decision flip twice. Returns the
/// decoded output and the per-packet mode.
fn stream_with_mode_switches() -> (Vec<f32>, Vec<&'static str>) {
    let mut pcm = speech_like(RATE, RATE as usize);
    pcm.extend(music_like(RATE, RATE as usize));
    pcm.extend(speech_like(RATE, RATE as usize));

    let r = Codec::new(RATE, 1, Application::Audio)
        .frame_samples(FRAME)
        .bitrate(24_000)
        .roundtrip(&pcm);
    let modes = r.modes();
    (r.decoded, modes)
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt()
}

/// The output must not drop out at a mode switch.
///
/// The very start of the new frame is where a missing cross-fade shows: the
/// layer taking over has no overlap-add history, so its first samples climb out
/// of silence.
///
/// The head is compared against the local envelope, 20 ms straddling the seam.
/// It is not expected to reach that envelope even when everything works:
/// `apply_transition` copies the first 2.5 ms straight from the concealed
/// previous-mode audio and only fades over the 2.5 ms after that, and SILK's
/// concealment fades towards silence by design, so how high the head sits
/// depends on the content. What does not depend on the content is the gap
/// between working and broken. Measured on this stimulus: 13.5% of the envelope
/// with the cross-fade, 1.1% without, so the threshold sits at 5% with roughly
/// a factor of three either side.
///
/// Do not re-derive this threshold from a passing build alone. It is only
/// meaningful against a build with `apply_transition` disabled, and the last
/// time these numbers were recalibrated it was because a *different* fix moved
/// the mode decision and left a single marginal switch where there had been
/// two.
#[test]
fn a_mode_switch_does_not_punch_a_hole_in_the_output() {
    const HEAD: usize = 16;
    const ENVELOPE: usize = 10 * F2_5; // 10 ms either side

    let (out, modes) = stream_with_mode_switches();

    let switches: Vec<usize> = (1..modes.len())
        .filter(|&i| modes[i] != modes[i - 1])
        .collect();
    assert!(
        !switches.is_empty(),
        "the stimulus no longer makes the encoder switch mode, so this test \
         proves nothing — pick a signal or bitrate that does. Modes: {modes:?}"
    );

    for &i in &switches {
        let start = i * FRAME;
        assert!(start >= ENVELOPE && start + ENVELOPE <= out.len());
        let head = rms(&out[start..start + HEAD]);
        let envelope = rms(&out[start - ENVELOPE..start + ENVELOPE]);
        assert!(
            head > 0.05 * envelope,
            "{} -> {} at packet {i}: the first {HEAD} samples are at {:.2}% of \
             the surrounding envelope, so the seam was left open \
             (head {head:.6}, envelope {envelope:.6})",
            modes[i - 1],
            modes[i],
            100.0 * head / envelope,
        );
    }
}

/// Concealment is what the cross-fade is built from, so it has to produce
/// audio rather than silence. A CELT-only stream that loses one packet must
/// come back with energy in the concealed frame.
#[test]
fn celt_concealment_produces_audio_not_silence() {
    let pcm = music_like(RATE, FRAME * 12);
    let mut c = Codec::new(RATE, 1, Application::Audio)
        .frame_samples(FRAME)
        .bitrate(128_000); // high enough to stay in CELT throughout
    let packets = c.encode_all(&pcm);
    assert!(packets.iter().all(|p| packet_mode(p) == "celt"));

    // Decoded one packet at a time rather than through `decode_all`, because
    // dropping one of them is the whole point.
    let mut buf = vec![0.0f32; 5760];
    let mut last_good = Vec::new();
    let mut concealed = Vec::new();
    for (f, pkt) in packets.iter().enumerate() {
        if f == 10 {
            // Drop it: decode with no data at all, as a player would on loss.
            let m = c.dec.decode(&[], FRAME, &mut buf).unwrap();
            concealed = buf[..m].to_vec();
            continue;
        }
        let m = c.dec.decode(pkt, 5760, &mut buf).unwrap();
        if f == 9 {
            last_good = buf[..m].to_vec();
        }
    }

    let before = rms(&last_good);
    let after = rms(&concealed);
    assert!(
        after > 0.25 * before,
        "the concealed frame came back at {:.1}% of the level before it \
         (before {before:.4}, after {after:.4}) — the pitch-based branch \
         bailed out instead of extrapolating",
        100.0 * after / before,
    );
}

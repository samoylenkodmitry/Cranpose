//! Encode and decode throughput, measured against real time.
//!
//! The codec's correctness is pinned from many directions; its speed was not
//! measured at all until this existed. Two questions need answering, and they
//! want different units:
//!
//! - *Is it fast enough?* Answered in **x-realtime**: how many seconds of audio
//!   one second of CPU gets through. A caller deciding between this crate and a
//!   C binding is asking this.
//! - *Did a change slow it down?* Answered in **microseconds per frame**, which
//!   does not move when the clip length or the frame duration does, so numbers
//!   from different runs compare directly. `--save` and `--compare` do that
//!   comparison for you.
//!
//! ```text
//! cargo bench
//! cargo bench -- --filter silk          # only cases whose name contains "silk"
//! cargo bench -- --save before.tsv      # record a baseline
//! cargo bench -- --compare before.tsv   # ...and diff against it after a change
//! ```
//!
//! # Why the numbers are what they are
//!
//! Each case is timed several times and the **fastest** pass is reported, not
//! the mean. Benchmark noise on a general-purpose machine is one-sided: a
//! scheduler preemption, an interrupt or a frequency drop can only ever make a
//! pass slower, never faster. The minimum is therefore the best estimate of the
//! work the code actually does, and it is far more stable across runs than a
//! mean, which chases whatever else the machine was doing.
//!
//! What survives that is still not zero. Two consecutive full runs at the
//! default settings differ by 0.7% per row on average and 3% at worst, so a
//! `--compare` figure smaller than a few percent says nothing; `--reps` buys a
//! tighter number at the cost of wall time.
//!
//! A fresh encoder is built for every pass, outside the timed region. Encoder
//! state evolves — rate control, the content analysis, the mode decision and
//! their hysteresis all carry forward — so re-running a warmed encoder over the
//! same audio would measure something the first pass never sees.
//!
//! Packets are collected in a separate untimed pass. Copying each one out of
//! the encoder's buffer is allocator work that belongs to the caller, not to
//! the codec, and at 2.5 ms frames it is a measurable share of the total.
//!
//! The audio comes from the same generators the test suite uses, so it is
//! bit-identical on every target and a number measured here means the same
//! thing on another machine. Silence or a pure tone would be far cheaper to
//! code than real material and would flatter every row.
//!
//! # Reading the mode column
//!
//! Each case names the coding mode it is meant to exercise, and the table
//! reports the mode the encoder actually chose, read back from the TOC bytes.
//! They should agree. When they do not, the mode decision has moved, and the
//! timing is measuring something other than what the case is named for — which
//! is worth knowing before drawing a conclusion from the number beside it.

#[path = "../tests/common/mod.rs"]
mod common;

use common::{interleave, music_like, packet_mode, speech_like};
use opus_pure::{Application, Bandwidth, OpusDecoder, OpusEncoder, Signal};
use std::collections::BTreeMap;
use std::time::Instant;

/// Source material for a case. Speech and music take measurably different
/// paths: the analysis, the mode decision and SILK's own voice/unvoiced
/// classification all key off content.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Content {
    Speech,
    Music,
}

struct Case {
    /// Short name, unique within its section. `--filter` matches against it and
    /// `--compare` keys on it, so renaming a case orphans its baseline row.
    label: String,
    rate: i32,
    channels: usize,
    /// Frame duration in tenths of a millisecond, so 2.5 ms stays an integer.
    tenths: i32,
    bitrate: i32,
    bandwidth: Option<Bandwidth>,
    signal: Option<Signal>,
    app: Application,
    complexity: i32,
    content: Content,
    /// The coding mode this case exists to exercise, checked against what the
    /// encoder actually chose.
    expect: &'static str,
}

impl Case {
    fn frame_samples(&self) -> usize {
        (self.rate as i64 * self.tenths as i64 / 10_000) as usize
    }
}

/// Field defaults, so each case below states only what makes it distinct.
fn base() -> Case {
    Case {
        label: String::new(),
        rate: 48_000,
        channels: 1,
        tenths: 200,
        bitrate: 64_000,
        bandwidth: None,
        signal: None,
        app: Application::Audio,
        complexity: 9,
        content: Content::Music,
        expect: "celt",
    }
}

/// A speech case: VOIP, forced bandwidth and signal type, speech-like source.
fn voice(label: &str, channels: usize, bitrate: i32, bw: Bandwidth, expect: &'static str) -> Case {
    Case {
        label: label.to_string(),
        channels,
        bitrate,
        bandwidth: Some(bw),
        signal: Some(Signal::Voice),
        app: Application::Voip,
        content: Content::Speech,
        expect,
        ..base()
    }
}

/// A music case: AUDIO, fullband, music-like source.
fn music(label: &str, channels: usize, bitrate: i32) -> Case {
    Case {
        label: label.to_string(),
        channels,
        bitrate,
        bandwidth: Some(Bandwidth::Fullband),
        signal: Some(Signal::Music),
        ..base()
    }
}

/// The three coding modes at the rates and widths each one is used at.
///
/// The bitrate and bandwidth pairings are ones measured to land on each mode,
/// so these reach SILK, hybrid and CELT rather than hoping to. Stereo hybrid
/// wants a lower bitrate than the mono cases: above about 28 kb/s the encoder
/// picks CELT for two channels.
fn mode_cases() -> Vec<Case> {
    #[rustfmt::skip]
    const SPEECH: [(&str, usize, i32, Bandwidth, &str); 7] = [
        ("silk NB 8 kb/s mono",      1,  8_000, Bandwidth::Narrowband,    "silk"),
        ("silk MB 24 kb/s mono",     1, 24_000, Bandwidth::Mediumband,    "silk"),
        ("silk WB 20 kb/s mono",     1, 20_000, Bandwidth::Wideband,      "silk"),
        ("silk WB 32 kb/s stereo",   2, 32_000, Bandwidth::Wideband,      "silk"),
        ("hybrid SWB 12 kb/s mono",  1, 12_000, Bandwidth::Superwideband, "hybrid"),
        ("hybrid FB 20 kb/s mono",   1, 20_000, Bandwidth::Fullband,      "hybrid"),
        ("hybrid FB 24 kb/s stereo", 2, 24_000, Bandwidth::Fullband,      "hybrid"),
    ];
    #[rustfmt::skip]
    const MUSIC: [(&str, usize, i32); 3] = [
        ("celt FB 64 kb/s mono",     1,  64_000),
        ("celt FB 128 kb/s stereo",  2, 128_000),
        ("celt FB 256 kb/s stereo",  2, 256_000),
    ];

    let speech = SPEECH
        .iter()
        .map(|&(label, channels, bitrate, bw, expect)| voice(label, channels, bitrate, bw, expect));
    let music = MUSIC
        .iter()
        .map(|&(label, channels, bitrate)| music(label, channels, bitrate));
    speech.chain(music).collect()
}

/// Every packet duration RFC 6716 admits, on one otherwise-fixed case.
///
/// Per-frame cost should fall as the frame grows, because the per-packet
/// overhead — the TOC byte, the range coder's flush, the mode decision — is
/// paid once however long the frame is. What this shows is how much of the
/// short-frame cost is that overhead rather than coding.
fn frame_size_cases() -> Vec<Case> {
    const DURATIONS: [(&str, i32); 9] = [
        ("2.5 ms", 25),
        ("5 ms", 50),
        ("10 ms", 100),
        ("20 ms", 200),
        ("40 ms", 400),
        ("60 ms", 600),
        ("80 ms", 800),
        ("100 ms", 1000),
        ("120 ms", 1200),
    ];
    DURATIONS
        .iter()
        .map(|&(label, tenths)| Case {
            tenths,
            // 2.5 and 5 ms exist only for CELT, so the encoder overrides its
            // mode decision there; every duration here lands on CELT anyway.
            ..music(label, 2, 96_000)
        })
        .collect()
}

/// What the complexity setting costs, in both a SILK case and a CELT one.
///
/// The default is 9. Complexity gates the content analysis, the pitch search
/// and SILK's noise-shaping depth, so this is the one knob a caller can reach
/// for when the answer to "is it fast enough" is no. It is swept over both
/// coding modes because it does not buy the same thing in each: SILK spends it
/// on the delayed-decision quantiser, CELT mostly on the pitch search.
fn complexity_cases() -> Vec<Case> {
    const LEVELS: [(&str, i32); 4] = [("c0", 0), ("c5", 5), ("c8", 8), ("c10", 10)];
    let mut cases = Vec::new();
    for &(name, level) in &LEVELS {
        cases.push(Case {
            complexity: level,
            label: format!("silk WB mono {name}"),
            ..voice("", 1, 20_000, Bandwidth::Wideband, "silk")
        });
    }
    for &(name, level) in &LEVELS {
        cases.push(Case {
            complexity: level,
            label: format!("celt FB stereo {name}"),
            ..music("", 2, 96_000)
        });
    }
    cases
}

fn sections() -> Vec<(&'static str, Vec<Case>)> {
    vec![
        ("modes", mode_cases()),
        ("frame size, CELT 96 kb/s stereo", frame_size_cases()),
        ("complexity", complexity_cases()),
    ]
}

/// Deterministic source audio for a case, `seconds` long, interleaved.
fn source(case: &Case, seconds: f64) -> Vec<f32> {
    let n = (case.rate as f64 * seconds) as usize;
    let left = match case.content {
        Content::Speech => speech_like(case.rate, n),
        Content::Music => music_like(case.rate, n),
    };
    if case.channels == 1 {
        return left;
    }
    // The right channel lags by half a millisecond and sits about 2 dB down.
    // Two identical channels would make mid/side coding free and flatter every
    // stereo row; two unrelated ones never occur in real material.
    let lag = (case.rate / 2000).max(1) as usize;
    let mut right = vec![0.0f32; n];
    for i in lag..n {
        right[i] = left[i - lag] * 0.79;
    }
    interleave(&[left, right])
}

fn make_encoder(case: &Case) -> OpusEncoder {
    let mut enc = OpusEncoder::new(case.rate, case.channels, case.app).expect("encoder");
    enc.bitrate_bps = case.bitrate;
    enc.complexity = case.complexity;
    enc.force_bandwidth = case.bandwidth;
    enc.signal_type = case.signal;
    enc
}

/// One timed encode of the whole clip. Returns the seconds spent inside
/// `encode` and the total bytes it produced; the packets themselves are
/// dropped, so no allocator work lands in the measurement.
fn encode_pass(case: &Case, pcm: &[f32], out: &mut [u8]) -> (f64, usize) {
    let frame = case.frame_samples();
    let mut enc = make_encoder(case);
    let mut bytes = 0usize;
    let t = Instant::now();
    for chunk in pcm.chunks_exact(frame * case.channels) {
        bytes += enc.encode(chunk, frame, out).expect("encode");
    }
    (t.elapsed().as_secs_f64(), bytes)
}

/// The same encode, untimed, keeping the packets for the decode measurement.
fn collect_packets(case: &Case, pcm: &[f32]) -> Vec<Vec<u8>> {
    let frame = case.frame_samples();
    let mut enc = make_encoder(case);
    let mut out = vec![0u8; MAX_PACKET];
    pcm.chunks_exact(frame * case.channels)
        .map(|chunk| {
            let n = enc.encode(chunk, frame, &mut out).expect("encode");
            out[..n].to_vec()
        })
        .collect()
}

/// One timed decode of every packet, from a decoder that has seen nothing.
fn decode_pass(case: &Case, packets: &[Vec<u8>], out: &mut [f32]) -> f64 {
    let frame = case.frame_samples();
    let mut dec = OpusDecoder::new(case.rate, case.channels).expect("decoder");
    let t = Instant::now();
    for packet in packets {
        dec.decode(packet, frame, out).expect("decode");
    }
    t.elapsed().as_secs_f64()
}

/// Largest packet the encoder can emit: 1275 bytes per frame, up to 48 frames.
const MAX_PACKET: usize = 1275 * 48 + 2;

struct Row {
    label: String,
    /// The modes the encoder actually chose, joined by `+` if it changed mid-clip.
    mode: String,
    /// Set when `mode` does not contain what the case said it would exercise.
    mode_surprised: bool,
    enc_us_per_frame: f64,
    enc_realtime: f64,
    dec_us_per_frame: f64,
    dec_realtime: f64,
    kbps: f64,
}

fn run_case(case: &Case, seconds: f64, reps: usize) -> Row {
    let pcm = source(case, seconds);
    let frame = case.frame_samples();
    let frames = pcm.len() / case.channels / frame;
    assert!(frames > 0, "{}: clip is shorter than one frame", case.label);
    let audio_seconds = (frames * frame) as f64 / case.rate as f64;

    let packets = collect_packets(case, &pcm);
    let bytes: usize = packets.iter().map(|p| p.len()).sum();

    // Distinct modes in the order they first appear, so a stream that switches
    // reads as "silk+celt" rather than reporting whichever came first.
    let mut modes: Vec<&str> = Vec::new();
    for packet in &packets {
        let m = packet_mode(packet);
        if !modes.contains(&m) {
            modes.push(m);
        }
    }
    let mode = modes.join("+");

    let mut enc_buf = vec![0u8; MAX_PACKET];
    let mut dec_buf = vec![0.0f32; frame * case.channels];

    let mut enc_best = f64::INFINITY;
    let mut dec_best = f64::INFINITY;
    for _ in 0..reps {
        let (secs, _) = encode_pass(case, &pcm, &mut enc_buf);
        enc_best = enc_best.min(secs);
        dec_best = dec_best.min(decode_pass(case, &packets, &mut dec_buf));
    }

    Row {
        label: case.label.clone(),
        mode_surprised: !modes.contains(&case.expect),
        mode,
        enc_us_per_frame: enc_best * 1e6 / frames as f64,
        enc_realtime: audio_seconds / enc_best,
        dec_us_per_frame: dec_best * 1e6 / frames as f64,
        dec_realtime: audio_seconds / dec_best,
        kbps: bytes as f64 * 8.0 / audio_seconds / 1000.0,
    }
}

struct Args {
    filter: Option<String>,
    reps: usize,
    seconds: f64,
    save: Option<String>,
    compare: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        filter: None,
        reps: 5,
        seconds: 8.0,
        save: None,
        compare: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        // `cargo bench` passes this to every benchmark target, harness or not.
        if arg == "--bench" {
            continue;
        }
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--filter" => args.filter = Some(value()?),
            "--reps" => {
                args.reps = value()?
                    .parse()
                    .map_err(|_| "--reps needs a whole number".to_string())?
            }
            "--seconds" => {
                args.seconds = value()?
                    .parse()
                    .map_err(|_| "--seconds needs a number".to_string())?
            }
            "--save" => args.save = Some(value()?),
            "--compare" => args.compare = Some(value()?),
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unrecognised argument `{other}`")),
        }
    }
    if args.reps == 0 {
        return Err("--reps must be at least 1".to_string());
    }
    if args.seconds.is_nan() || args.seconds <= 0.0 {
        return Err("--seconds must be positive".to_string());
    }
    Ok(args)
}

const USAGE: &str = "\
usage: cargo bench -- [options]

  --filter <text>    run only cases whose name contains <text>
  --reps <n>         timed passes per case, fastest reported (default 5)
  --seconds <s>      audio per case (default 8)
  --save <file>      write this run's per-frame times for later comparison
  --compare <file>   show the change against a file written by --save
";

/// Per-frame times from a previous run, keyed `section/case`.
type Baseline = BTreeMap<String, (f64, f64)>;

fn read_baseline(path: &str) -> Result<Baseline, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut out = Baseline::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split('\t');
        let bad = || format!("{path}:{}: expected `key<TAB>encode<TAB>decode`", n + 1);
        let key = fields.next().ok_or_else(bad)?;
        let enc: f64 = fields.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        let dec: f64 = fields.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
        out.insert(key.to_string(), (enc, dec));
    }
    Ok(out)
}

fn write_baseline(path: &str, rows: &Baseline) -> Result<(), String> {
    let mut text = String::from("# opus-pure throughput: key\tencode us/frame\tdecode us/frame\n");
    for (key, (enc, dec)) in rows {
        text.push_str(&format!("{key}\t{enc:.4}\t{dec:.4}\n"));
    }
    std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))
}

/// Percentage change from `was` to `now`, as text. Positive means slower.
fn delta(now: f64, was: Option<f64>) -> String {
    match was {
        None => "-".to_string(),
        Some(was) if was > 0.0 => format!("{:+.1}%", (now - was) / was * 100.0),
        Some(_) => "-".to_string(),
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("error: {message}\n");
            }
            eprint!("{USAGE}");
            std::process::exit(if message.is_empty() { 0 } else { 2 });
        }
    };

    let baseline = match args.compare.as_deref().map(read_baseline) {
        Some(Ok(b)) => Some(b),
        Some(Err(e)) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
        None => None,
    };
    let comparing = baseline.is_some();

    println!("opus-pure throughput");
    println!(
        "  {}, {:.1} s of audio per case, fastest of {} pass{}",
        std::env::consts::ARCH,
        args.seconds,
        args.reps,
        if args.reps == 1 { "" } else { "es" }
    );

    let mut measured = Baseline::new();
    let mut surprises: Vec<String> = Vec::new();
    let mut ran = 0usize;

    for (section, cases) in sections() {
        let selected: Vec<&Case> = cases
            .iter()
            .filter(|c| match &args.filter {
                Some(f) => c.label.contains(f.as_str()) || section.contains(f.as_str()),
                None => true,
            })
            .collect();
        if selected.is_empty() {
            continue;
        }

        println!();
        println!("{section}");
        if comparing {
            println!(
                "  {:<24} {:<8} {:>9} {:>7} {:>8} {:>9} {:>7} {:>8} {:>8}",
                "case",
                "mode",
                "enc us/fr",
                "x-rt",
                "change",
                "dec us/fr",
                "x-rt",
                "change",
                "kb/s"
            );
            println!("  {}", "-".repeat(94));
        } else {
            println!(
                "  {:<24} {:<8} {:>9} {:>7} {:>9} {:>7} {:>8}",
                "case", "mode", "enc us/fr", "x-rt", "dec us/fr", "x-rt", "kb/s"
            );
            println!("  {}", "-".repeat(76));
        }

        for case in selected {
            let row = run_case(case, args.seconds, args.reps);
            let key = format!("{section}/{}", row.label);
            measured.insert(key.clone(), (row.enc_us_per_frame, row.dec_us_per_frame));
            ran += 1;

            let mode = if row.mode_surprised {
                surprises.push(format!(
                    "  {}: named for {}, encoder chose {}",
                    row.label, case.expect, row.mode
                ));
                format!("{}!", row.mode)
            } else {
                row.mode.clone()
            };

            if comparing {
                let was = baseline.as_ref().and_then(|b| b.get(&key)).copied();
                println!(
                    "  {:<24} {:<8} {:>9.2} {:>6.0}x {:>8} {:>9.2} {:>6.0}x {:>8} {:>8.1}",
                    row.label,
                    mode,
                    row.enc_us_per_frame,
                    row.enc_realtime,
                    delta(row.enc_us_per_frame, was.map(|w| w.0)),
                    row.dec_us_per_frame,
                    row.dec_realtime,
                    delta(row.dec_us_per_frame, was.map(|w| w.1)),
                    row.kbps,
                );
            } else {
                println!(
                    "  {:<24} {:<8} {:>9.2} {:>6.0}x {:>9.2} {:>6.0}x {:>8.1}",
                    row.label,
                    mode,
                    row.enc_us_per_frame,
                    row.enc_realtime,
                    row.dec_us_per_frame,
                    row.dec_realtime,
                    row.kbps,
                );
            }
        }
    }

    if ran == 0 {
        println!();
        println!("no case matched the filter");
        return;
    }

    if !surprises.is_empty() {
        println!();
        println!("the encoder did not choose the mode these cases are named for:");
        for line in &surprises {
            println!("{line}");
        }
        println!("  the timings stand, but they are not measuring what the name says.");
    }

    if let Some(path) = &args.save {
        match write_baseline(path, &measured) {
            Ok(()) => {
                println!();
                println!("wrote {path}; `cargo bench -- --compare {path}` diffs against it");
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        }
    }

    if comparing {
        println!();
        println!("change is per-frame time against the baseline: positive is slower.");
    }
}

//! Shared signal generation and measurement for the integration tests.
//!
//! The point of these helpers is that every assertion compares the decoder's
//! output against the encoder's *input*. A test that only checks "the output has
//! some energy" passes on inverted, mis-scaled, or garbage audio, which is how
//! the SILK stereo prediction defect survived the original suite.

#![allow(dead_code)]

use opus_pure::{Application, Bandwidth, Error, OpusDecoder, OpusEncoder, Signal};

/// The fuzz targets' bodies, so that `fuzz_corpus.rs` and the fuzz crate run
/// the same code rather than two copies of it.
pub mod fuzz;

/// `sin(2πx)`, built from IEEE-754 arithmetic alone so that a generated signal
/// is bit-identical on every target.
///
/// `f32::sin` and `f32::powf` call the platform's libm, and Apple's arm64 and
/// x86_64 implementations disagree in the last bits. Every signal below hashed
/// differently on the two architectures because of it, which is harmless for a
/// tolerance-based assertion but fatal for the frozen bitstreams in
/// `bitstream_stability.rs` and `decoder_conformance.rs`: the difference was
/// enough to move SILK's quantiser across a decision boundary at 12 kHz stereo,
/// so those hashes held on aarch64 and failed on x86_64. Which entries survived
/// was luck, and luck that any encoder change could spend.
///
/// Addition, subtraction, multiplication, division and square root are
/// correctly rounded by IEEE-754, and Rust never contracts them into an FMA, so
/// a polynomial built only from those evaluates identically everywhere.
///
/// The argument is in turns rather than radians because that makes the range
/// reduction exact: `x - x.floor()` is exact for every `|x| < 2^52`, where
/// subtracting a multiple of an approximated 2π would not be. Folding that into
/// the first quarter turn is exact for the same reason, leaving a Taylor series
/// for `sin` on `[0, π/2]` truncated where its next term is already far below
/// `f32` precision. Measured against libm, the worst error over the whole
/// argument range these generators reach is 1.9e-10, some 600 times smaller
/// than `f32::EPSILON`.
pub fn sin_turns(x: f64) -> f64 {
    // Odd Taylor coefficients, 1/3! through 1/15!.
    const C3: f64 = 1.0 / 6.0;
    const C5: f64 = 1.0 / 120.0;
    const C7: f64 = 1.0 / 5_040.0;
    const C9: f64 = 1.0 / 362_880.0;
    const C11: f64 = 1.0 / 39_916_800.0;
    const C13: f64 = 1.0 / 6_227_020_800.0;
    const C15: f64 = 1.0 / 1_307_674_368_000.0;

    let mut t = x - x.floor();
    let mut sign = 1.0f64;
    // sin(θ + π) = -sin θ, then sin(π - θ) = sin θ.
    if t >= 0.5 {
        t -= 0.5;
        sign = -1.0;
    }
    if t > 0.25 {
        t = 0.5 - t;
    }
    let u = t * std::f64::consts::TAU;
    let u2 = u * u;
    let p = C3 - u2 * (C5 - u2 * (C7 - u2 * (C9 - u2 * (C11 - u2 * (C13 - u2 * C15)))));
    sign * (u - u * u2 * p)
}

/// Deterministic LCG, so a failure reproduces exactly.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// Uniform in [-1, 1).
    pub fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32 / (1u32 << 23) as f32) - 1.0
    }
}

/// A sine at `hz`, `n` samples at `rate`.
pub fn sine(rate: i32, n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n)
        .map(|i| sin_turns(hz as f64 * i as f64 / rate as f64) as f32 * amp)
        .collect()
}

/// Interleave per-channel signals.
pub fn interleave(channels: &[Vec<f32>]) -> Vec<f32> {
    let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut out = Vec::with_capacity(n * channels.len());
    for i in 0..n {
        for c in channels {
            out.push(c[i]);
        }
    }
    out
}

/// Pull channel `c` out of an interleaved buffer.
pub fn deinterleave(pcm: &[f32], channels: usize, c: usize) -> Vec<f32> {
    pcm.iter().skip(c).step_by(channels).copied().collect()
}

/// Speech-like: a buzzy glottal pulse train through a slowly-moving formant,
/// with pauses. Exercises the SILK path far more honestly than a pure tone.
pub fn speech_like(rate: i32, n: usize) -> Vec<f32> {
    let mut rng = Lcg::new(0x51_1C_0F_11);
    let mut out = vec![0.0f32; n];
    let mut phase = 0.0f32;
    let mut f1 = 0.0f32;
    let mut f2 = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f64 / rate as f64;
        // Voiced/unvoiced/pause cycle at ~2.5 Hz. The envelope is raised to the
        // 1.5 as `b * sqrt(b)` rather than `powf(1.5)`: the same value, and
        // `sqrt` is correctly rounded where `powf` is a libm call.
        let b = sin_turns(2.5 * t) as f32 * 0.5 + 0.5;
        let env = b * b.sqrt();
        let f0 = 110.0 + 30.0 * sin_turns(0.7 * t) as f32;
        phase += f0 / rate as f32;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let excitation = if phase < 0.05 { 1.0 } else { -0.05 } + rng.next_f32() * 0.08;
        // Two cascaded one-pole resonators standing in for formants.
        f1 += 0.35 * (excitation - f1);
        f2 += 0.12 * (f1 - f2);
        *s = (f2 * 6.0 * env).clamp(-0.95, 0.95);
    }
    out
}

/// Music-like: a chord with vibrato plus a periodic transient.
pub fn music_like(rate: i32, n: usize) -> Vec<f32> {
    let mut rng = Lcg::new(0x5EED_CAFE);
    let freqs = [220.0f32, 277.18, 329.63, 440.0, 554.37, 659.25];
    (0..n)
        .map(|i| {
            let t = i as f64 / rate as f64;
            let vib = sin_turns(5.0 * t) * 0.002;
            let beat: f32 = if (t * 2.0).fract() < 0.05 { 1.8 } else { 1.0 };
            let mut s = 0.0f32;
            for (k, f) in freqs.iter().enumerate() {
                s += sin_turns(*f as f64 * (1.0 + vib) * t) as f32 * (0.22 / (k as f32 + 1.0));
            }
            ((s * beat + rng.next_f32() * 0.02) * 0.5).clamp(-0.98, 0.98)
        })
        .collect()
}

/// Windowed-sinc low-pass, Blackman window, linear phase.
///
/// Built from [`sin_turns`] rather than libm for the same reason the generators
/// are: a filter whose taps differ in the last bits between architectures makes
/// every measurement taken through it architecture-dependent too.
///
/// An odd tap count keeps the group delay an exact integer, which is what lets
/// a caller run the same filter over source and decoded audio and have the
/// filter's own delay cancel instead of correcting for it.
pub fn lowpass(taps: usize, cutoff_norm: f64) -> Vec<f64> {
    assert!(taps % 2 == 1, "an odd tap count keeps the delay an integer");
    let m = (taps - 1) as f64;
    let mut h = vec![0.0f64; taps];
    let mut sum = 0.0;
    for (i, v) in h.iter_mut().enumerate() {
        let x = i as f64 - m / 2.0;
        let sinc = if x == 0.0 {
            2.0 * cutoff_norm
        } else {
            sin_turns(cutoff_norm * x) / (std::f64::consts::PI * x)
        };
        let t = i as f64 / m;
        let w = 0.42 - 0.5 * sin_turns(t + 0.25) + 0.08 * sin_turns(2.0 * t + 0.25);
        *v = sinc * w;
        sum += *v;
    }
    for v in h.iter_mut() {
        *v /= sum;
    }
    h
}

/// Zero-padded convolution, so the output is the same length as the input.
///
/// The taps that would reach outside `x` are skipped rather than multiplied by
/// a zero, which leaves the summation order and therefore the result exactly
/// what a bounds-checked inner loop gives, and takes the branch out of it.
pub fn convolve(x: &[f32], h: &[f64]) -> Vec<f32> {
    let (n, taps, half) = (x.len(), h.len(), h.len() / 2);
    (0..n)
        .map(|i| {
            // j = i + k - half must land in [0, n).
            let k_lo = half.saturating_sub(i);
            let k_hi = taps.min(n + half - i);
            h[k_lo..k_hi]
                .iter()
                .zip(&x[i + k_lo - half..i + k_hi - half])
                .map(|(c, v)| c * *v as f64)
                .sum::<f64>() as f32
        })
        .collect()
}

/// The part of `x` between `lo_hz` and `hi_hz`, as the difference of two
/// linear-phase low-passes. Their delays are equal, so the result is not moved
/// in time and can be compared sample for sample against a band of the source
/// taken the same way.
///
/// `hi_hz` at or above Nyquist skips the upper filter, which is what a fullband
/// measurement wants; below it, it is what keeps a superwideband stream from
/// being charged for the 12-20 kHz it is silent in on purpose.
pub fn band_limit(x: &[f32], rate: i32, lo_hz: f64, hi_hz: f64) -> Vec<f32> {
    // 511 taps puts the transition inside 520 Hz at 48 kHz, well within one
    // CELT band, so the measurement does not straddle the edge it is there to
    // separate.
    const TAPS: usize = 511;
    let below = convolve(x, &lowpass(TAPS, lo_hz / rate as f64));
    if hi_hz >= rate as f64 / 2.0 {
        return x.iter().zip(&below).map(|(a, b)| a - b).collect();
    }
    let upper = convolve(x, &lowpass(TAPS, hi_hz / rate as f64));
    upper.iter().zip(&below).map(|(a, b)| a - b).collect()
}

/// Everything above `cutoff_hz`.
pub fn highpass(x: &[f32], rate: i32, cutoff_hz: f64) -> Vec<f32> {
    band_limit(x, rate, cutoff_hz, rate as f64)
}

/// Mean energy per sample.
pub fn energy(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32
}

/// Normalized correlation. `+1` identical, `-1` inverted, `0` unrelated.
pub fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let (a, b) = (&a[..n], &b[..n]);
    let (ea, eb) = (energy(a), energy(b));
    if ea <= 0.0 || eb <= 0.0 {
        return 0.0;
    }
    (a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / n as f32) / (ea.sqrt() * eb.sqrt())
}

/// Best correlation of `decoded` against `source` over codec delays up to
/// `max_lag`, with the lag that achieved it.
///
/// Opus delays its output, so an un-aligned correlation of a periodic signal
/// measures the phase shift rather than the fidelity.
pub fn aligned_correlation(decoded: &[f32], source: &[f32], max_lag: usize) -> (f32, usize) {
    let mut best = (f32::NEG_INFINITY, 0usize);
    for lag in 0..=max_lag {
        if lag >= decoded.len() {
            break;
        }
        let c = correlation(&decoded[lag..], source);
        if c > best.0 {
            best = (c, lag);
        }
    }
    best
}

/// Signal-to-noise ratio in dB after aligning out the codec delay.
///
/// Reported against the source energy, so a decoder that outputs silence scores
/// 0 dB rather than something misleadingly large.
pub fn aligned_snr_db(decoded: &[f32], source: &[f32], max_lag: usize) -> f32 {
    let (_, lag) = aligned_correlation(decoded, source, max_lag);
    let d = &decoded[lag.min(decoded.len())..];
    let n = d.len().min(source.len());
    if n == 0 {
        return f32::NEG_INFINITY;
    }
    let (d, s) = (&d[..n], &source[..n]);
    // Match gain before measuring, so a constant level offset is not counted as
    // noise; a codec is allowed a little overall gain drift.
    let num: f32 = d.iter().zip(s).map(|(x, y)| x * y).sum();
    let den: f32 = d.iter().map(|x| x * x).sum();
    let g = if den > 0.0 { num / den } else { 0.0 };
    let noise: f32 = d
        .iter()
        .zip(s)
        .map(|(x, y)| (g * x - y).powi(2))
        .sum::<f32>()
        / n as f32;
    let sig = energy(s);
    if noise <= 0.0 {
        return f32::INFINITY;
    }
    10.0 * (sig / noise).log10()
}

/// Coding mode named by a packet's TOC byte (RFC 6716 §3.1).
pub fn packet_mode(packet: &[u8]) -> &'static str {
    match packet[0] >> 3 {
        0..=11 => "silk",
        12..=15 => "hybrid",
        _ => "celt",
    }
}

/// Whether the TOC declares a stereo stream.
pub fn packet_is_stereo(packet: &[u8]) -> bool {
    packet[0] & 0x04 != 0
}

/// Samples at 48 kHz that one Opus packet decodes to, read straight from its
/// TOC byte (RFC 6716 §3.1). Derived independently of the encoder's own
/// bookkeeping, so it can be used to audit what the muxer claims.
pub fn packet_samples_48k(packet: &[u8]) -> usize {
    let config = packet[0] >> 3;
    // Tenths of a millisecond, so the 2.5 ms CELT frame stays an integer.
    let per_frame = if config < 12 {
        [100, 200, 400, 600][(config % 4) as usize]
    } else if config < 16 {
        [100, 200][(config % 2) as usize]
    } else {
        [25, 50, 100, 200][(config % 4) as usize]
    };
    let frames = match packet[0] & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => (packet[1] & 0x3F) as usize,
    };
    per_frame * frames * 48 / 10
}

/// The audio bandwidth a packet's TOC declares, in Hz (RFC 6716 §3.1).
pub fn packet_bandwidth_hz(packet: &[u8]) -> i32 {
    match packet[0] >> 3 {
        0..=3 => 4_000,    // SILK narrowband
        4..=7 => 6_000,    // SILK mediumband
        8..=11 => 8_000,   // SILK wideband
        12..=13 => 12_000, // hybrid superwideband
        14..=15 => 20_000, // hybrid fullband
        16..=19 => 4_000,  // CELT narrowband
        20..=23 => 8_000,  // CELT wideband
        24..=27 => 12_000, // CELT superwideband
        _ => 20_000,       // CELT fullband
    }
}

/// A matched encoder/decoder pair and the frame size they step the input in.
///
/// Every integration test needs this, and each one used to build it by hand.
/// That is how the suite ended up with four spellings of the same loop, three
/// different decode-buffer sizes, and `expect("encode")` as the only clue when
/// one of them failed.
///
/// Construct with [`Codec::new`], chain the builder methods for the frame size
/// and bitrate, and reach into `enc` for the settings only one or two tests
/// touch:
///
/// ```ignore
/// let mut c = Codec::new(48_000, 2, Application::Voip).frame_ms(40).bitrate(64_000);
/// c.enc.use_inband_fec = true;
/// let r = c.roundtrip(&pcm);
/// ```
pub struct Codec {
    pub enc: OpusEncoder,
    pub dec: OpusDecoder,
    pub rate: i32,
    pub channels: usize,
    /// Samples per channel per packet.
    pub frame: usize,
}

impl Codec {
    /// 20 ms frames at the encoder's default bitrate.
    pub fn new(rate: i32, channels: usize, app: Application) -> Self {
        Codec {
            enc: OpusEncoder::new(rate, channels, app).expect("encoder"),
            dec: OpusDecoder::new(rate, channels).expect("decoder"),
            rate,
            channels,
            frame: (rate / 50) as usize,
        }
    }

    pub fn frame_ms(self, ms: i32) -> Self {
        let n = (self.rate as i64 * ms as i64 / 1000) as usize;
        self.frame_samples(n)
    }

    /// Frame size in samples per channel, for the durations that divide no
    /// sample rate evenly (2.5 ms) or that a test wants to name directly.
    pub fn frame_samples(mut self, n: usize) -> Self {
        self.frame = n;
        self
    }

    pub fn bitrate(mut self, bps: i32) -> Self {
        self.enc.bitrate_bps = bps;
        self
    }

    pub fn bandwidth(mut self, bw: Bandwidth) -> Self {
        self.enc.force_bandwidth = Some(bw);
        self
    }

    /// Tell the encoder what the content is, the way `OPUS_SET_SIGNAL` does.
    /// The mode decision reads it, so this is how a test pins a configuration to
    /// SILK or CELT without guessing at a bitrate that happens to land there.
    pub fn signal_type(mut self, signal: Signal) -> Self {
        self.enc.signal_type = Some(signal);
        self
    }

    /// How the tests describe this configuration when an assertion fails.
    pub fn label(&self) -> String {
        format!(
            "{} Hz/{}ch/{:.1}ms",
            self.rate,
            self.channels,
            self.frame as f32 * 1000.0 / self.rate as f32
        )
    }

    /// The largest packet the decoder has to be ready for: RFC 6716 caps a
    /// packet at 120 ms, whatever frame size the encoder was handed.
    fn max_packet_samples(&self) -> usize {
        self.rate as usize * 120 / 1000
    }

    /// Encode every whole frame of `pcm`, one packet each. Panics on a frame the
    /// encoder refuses; use [`Codec::try_encode_all`] where the configuration
    /// itself is what is under test.
    pub fn encode_all(&mut self, pcm: &[f32]) -> Vec<Vec<u8>> {
        let label = self.label();
        self.try_encode_all(pcm)
            .unwrap_or_else(|e| panic!("{label}: encode failed: {e}"))
    }

    /// Encode every whole frame of `pcm`, stopping at the first frame the
    /// encoder refuses and returning its error.
    pub fn try_encode_all(&mut self, pcm: &[f32]) -> Result<Vec<Vec<u8>>, Error> {
        let width = self.frame * self.channels;
        let mut packets = Vec::with_capacity(pcm.len() / width.max(1));
        for chunk in pcm.chunks_exact(width) {
            // The output buffer is the encoder's byte budget, not just storage:
            // under CBR it caps the target size. 4000 is past the 1276-byte
            // limit of a one-frame packet at every duration, so it never binds.
            // `reference_vectors.rs` passes 1275 on purpose, to match the C
            // harness the frozen vectors were generated with, and so keeps its
            // own loop.
            let mut pkt = vec![0u8; 4000];
            let n = self.enc.encode(chunk, self.frame, &mut pkt)?;
            pkt.truncate(n);
            packets.push(pkt);
        }
        Ok(packets)
    }

    /// Decode `packets` in order, returning interleaved samples.
    pub fn decode_all(&mut self, packets: &[Vec<u8>]) -> Vec<f32> {
        let label = self.label();
        let cap = self.max_packet_samples();
        let mut buf = vec![0.0f32; cap * self.channels];
        let mut out = Vec::with_capacity(packets.len() * self.frame * self.channels);
        for (i, p) in packets.iter().enumerate() {
            let got = self
                .dec
                .decode(p, cap, &mut buf)
                .unwrap_or_else(|e| panic!("{label}: decode of packet {i} failed: {e}"));
            out.extend_from_slice(&buf[..got * self.channels]);
        }
        out
    }

    /// Encode and decode `pcm`, keeping both sides.
    pub fn roundtrip(&mut self, pcm: &[f32]) -> Roundtrip {
        let packets = self.encode_all(pcm);
        let decoded = self.decode_all(&packets);
        Roundtrip {
            packets,
            decoded,
            channels: self.channels,
        }
    }
}

/// What one round trip produced.
pub struct Roundtrip {
    pub packets: Vec<Vec<u8>>,
    /// Interleaved, in the decoder's channel count.
    pub decoded: Vec<f32>,
    pub channels: usize,
}

impl Roundtrip {
    /// The coding mode each packet's TOC declares.
    pub fn modes(&self) -> Vec<&'static str> {
        self.packets.iter().map(|p| packet_mode(p)).collect()
    }

    /// Total encoded size, which is what bitrate assertions compare.
    pub fn bytes(&self) -> usize {
        self.packets.iter().map(|p| p.len()).sum()
    }

    /// One channel of the decoded output.
    pub fn channel(&self, c: usize) -> Vec<f32> {
        deinterleave(&self.decoded, self.channels, c)
    }
}

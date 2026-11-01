//! Chunk-parallel Opus encoding: split the input into contiguous frame ranges
//! and encode them on separate threads.
//!
//! Opus carries real inter-frame state — SILK's LTP, noise shaping, NLSF
//! interpolation and bit reservoir, CELT's pre-emphasis, overlap, prefilter and
//! energy prediction, the high-pass filter, the input resampler, and the content
//! analysis that chooses between them — so a frame range cannot be encoded from
//! a cold encoder and dropped into the middle of a stream. Each worker therefore
//! **primes** its encoder by re-encoding the audio immediately before its chunk
//! and discarding those packets, so that by the time it reaches its own first
//! frame its state approximates the state a continuous encoder would have had.
//!
//! Priming is neither free nor exact, and what it costs and buys is the whole
//! subject of this module.
//!
//! # What priming converges, and what it does not
//!
//! The signal path converges quickly. Its memory is a handful of frames — the
//! LTP lag, the noise-shaping delay, one MDCT overlap — and a stable filter
//! forgets its initial conditions. Around 160 ms of priming settles it.
//!
//! **The content analysis does not.** It keeps a hundred-entry ring of 20 ms
//! observations and averages the music/speech probability over it, and the
//! encoder applies hysteresis on top of that when it picks between SILK, hybrid
//! and CELT. That memory is two seconds deep, and until it fills, a worker's
//! mode decision is its own rather than the one a continuous encoder would have
//! made. The consequence is not a seam at the boundary: it is the **entire
//! chunk** coded in a different mode.
//!
//! Measured on 120 s of synthetic speech at 16 kb/s split four ways, where a
//! continuous encoder settles on CELT and stays there:
//!
//! | `warmup_ms` | worst chunk, frames in a mode the serial encoder did not use | worst frame vs serial |
//! | --- | --- | --- |
//! | 160 | 36 of 1500 | −14.82 dB |
//! | 500 | 16 of 1500 | −14.53 dB |
//! | 1000 | 0 of 1500 | −4.10 dB |
//! | 2000 (the default) | 0 of 1500 | −4.10 dB |
//!
//! Hence [`DEFAULT_WARMUP_MS`], which is the analysis's own history depth rather
//! than a tuned number. A caller who pins [`ParallelConfig::signal_type`] takes
//! the analysis out of the mode decision entirely and can prime far less; that
//! is the cheapest way to buy a short warm-up.
//!
//! # What is left after priming
//!
//! Even fully primed, a chunked encode is not the serial encode. The rate
//! controllers — CELT's VBR reservoir and drift, SILK's bit reservoir — are
//! deliberately long-memory integrators, and a worker's differs from the
//! continuous encoder's for some frames after its boundary. That shows as a
//! bitrate dip of a few percent lasting tens of frames, and as a per-frame SNR
//! difference at the boundary of a few dB against a serial encode of the same
//! audio. Constant bitrate removes nearly all of it, there being no reservoir to
//! be wrong about.
//!
//! This part is inherent rather than a defect awaiting a fix: at a chunk
//! boundary one packet was produced by an encoder that did not produce the
//! packet before it, and no amount of priming changes that. It is why this is an
//! opt-in path and not what [`crate::OpusEncoder`] does by itself.
//!
//! `reference/parallel/` measures all of the above, and is where the numbers
//! here come from.
//!
//! # Cost
//!
//! Every worker but the first re-encodes `warmup_ms` of audio it will not emit.
//! With `w` workers that is `(w - 1) * warmup_ms` of redundant encoding, and
//! [`ParallelConfig::plan`] reports it before any of it is done. The worker
//! count is capped so redundancy stays at or below a quarter of the useful work,
//! which with the default warm-up means one worker per 8 s of audio.
//!
//! Deterministic: fixed chunk boundaries mean identical output across runs. Uses
//! only `std::thread`.

use crate::analysis::DETECT_SIZE;
use crate::{Application, Bandwidth, OpusEncoder, RateControl, Result, Signal};

/// Default priming length, in milliseconds.
///
/// This is the depth of the content analysis's history: `DETECT_SIZE`
/// observations of 20 ms each, from `src/analysis.rs`. Priming for less leaves a
/// worker choosing its coding mode from a partly-filled analysis, which is what
/// [the module documentation](self#what-priming-converges-and-what-it-does-not)
/// measures.
pub const DEFAULT_WARMUP_MS: u32 = DETECT_SIZE as u32 * 20;

/// The redundancy ceiling that caps the worker count: a chunk is never shorter
/// than this many warm-ups, which holds re-encoded audio at or below a quarter
/// of the useful work.
const MIN_CHUNK_WARMUPS: usize = 4;

/// The largest packet [`OpusEncoder::encode`] can produce.
///
/// Re-exported here under its old local name because a worker allocates one of
/// these once and reuses it for every frame; see [`MAX_PACKET_BYTES`] for why
/// the size has to be exact rather than merely generous.
use crate::encoder::MAX_PACKET_BYTES as MAX_PACKET;

/// Configuration for a parallel encode.
///
/// Every setting [`OpusEncoder`] exposes appears here, because a worker's
/// encoder is built from this struct and nothing else: a setting missing from
/// this list is one the parallel path cannot reach at all. The defaults are
/// [`OpusEncoder`]'s own, apart from `bitrate_bps` and `complexity`, which have
/// none there.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ParallelConfig {
    /// Rate of the PCM being encoded, as [`OpusEncoder::new`] takes it.
    pub sample_rate: i32,
    /// Channels in that PCM, interleaved. 1 or 2.
    pub channels: usize,
    /// What to optimise for, as [`OpusEncoder::new`] takes it.
    pub application: Application,
    /// [`OpusEncoder::bitrate_bps`]. Defaults to 64000 here, where the encoder
    /// itself has no default of its own.
    pub bitrate_bps: i32,
    /// [`OpusEncoder::complexity`]. Defaults to 9 here, where the encoder
    /// itself has no default of its own.
    pub complexity: i32,
    /// [`OpusEncoder::rate_control`].
    pub rate_control: RateControl,
    /// [`OpusEncoder::use_inband_fec`].
    pub use_inband_fec: bool,
    /// Discontinuous transmission. Its trigger counts *consecutive* inactive
    /// frames and each worker starts that count at zero, so a chunked encode
    /// emits fewer DTX packets than a serial one over the same silence.
    pub use_dtx: bool,
    /// [`OpusEncoder::packet_loss_perc`].
    pub packet_loss_perc: i32,
    /// Pinning this, or [`Self::signal_type`], takes the content analysis out of
    /// the coding-mode decision and lets `warmup_ms` be much shorter.
    pub force_bandwidth: Option<Bandwidth>,
    /// See [`Self::force_bandwidth`].
    pub signal_type: Option<Signal>,
    /// [`OpusEncoder::max_bandwidth`]. Unlike [`Self::force_bandwidth`] this
    /// only caps the automatic choice, so it does not remove the analysis from
    /// the decision and does not shorten the warm-up.
    pub max_bandwidth: Bandwidth,
    /// [`OpusEncoder::lsb_depth`].
    pub lsb_depth: i32,
    /// Audio each worker re-encodes before its own chunk to prime encoder state,
    /// then discards. Milliseconds rather than frames because what it has to
    /// cover is a time constant; see [`DEFAULT_WARMUP_MS`]. `0` disables priming,
    /// which is naive chunking and audibly wrong.
    pub warmup_ms: u32,
    /// Worker count; `0` selects `available_parallelism`. Capped by the
    /// redundancy ceiling — ask [`Self::plan`] what will actually be used.
    pub threads: usize,
}

impl ParallelConfig {
    /// A configuration with the defaults described on this type.
    ///
    /// The three arguments are the ones [`OpusEncoder::new`] fixes for an
    /// encoder's life; the remaining fields are ordinary and can be set on the
    /// returned value before it is handed to [`encode_parallel`].
    pub fn new(sample_rate: i32, channels: usize, application: Application) -> Self {
        ParallelConfig {
            sample_rate,
            channels,
            application,
            bitrate_bps: 64_000,
            complexity: 9,
            rate_control: RateControl::ConstrainedVbr,
            use_inband_fec: false,
            use_dtx: false,
            packet_loss_perc: 0,
            force_bandwidth: None,
            signal_type: None,
            max_bandwidth: Bandwidth::Fullband,
            lsb_depth: 24,
            warmup_ms: DEFAULT_WARMUP_MS,
            threads: 0,
        }
    }

    /// Warm-up expressed in frames of `frame_size` samples per channel, rounded
    /// up so a frame duration that does not divide `warmup_ms` still covers it.
    pub fn warmup_frames(&self, frame_size: usize) -> usize {
        if frame_size == 0 || self.warmup_ms == 0 || self.sample_rate <= 0 {
            return 0;
        }
        let frame_us = (frame_size as u64 * 1_000_000) / self.sample_rate as u64;
        if frame_us == 0 {
            return 0;
        }
        ((self.warmup_ms as u64 * 1000).div_ceil(frame_us)) as usize
    }

    /// How [`encode_parallel`] will divide `total_frames`, without encoding
    /// anything.
    ///
    /// Worth asking before a large job: the worker count is capped by the
    /// redundancy ceiling, so a short clip or a long warm-up can leave far fewer
    /// threads in use than were requested, or fall back to serial encoding
    /// altogether.
    pub fn plan(&self, total_frames: usize, frame_size: usize) -> ParallelPlan {
        let warmup = self.warmup_frames(frame_size);
        let requested = if self.threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            self.threads
        };
        let min_chunk = (warmup * MIN_CHUNK_WARMUPS).max(1);
        let workers = requested.max(1).min((total_frames / min_chunk).max(1));

        // Contiguous, balanced frame ranges [start, end).
        let mut ranges = Vec::with_capacity(workers);
        if total_frames > 0 {
            let (base, rem) = (total_frames / workers, total_frames % workers);
            let mut start = 0usize;
            for w in 0..workers {
                let len = base + usize::from(w < rem);
                ranges.push((start, start + len));
                start += len;
            }
        }
        // Worker 0 begins at frame 0 with nothing before it to prime from.
        let redundant_frames = ranges.iter().map(|&(start, _)| start.min(warmup)).sum();

        ParallelPlan {
            workers,
            warmup_frames: warmup,
            redundant_frames,
            ranges,
        }
    }
}

/// How a parallel encode will be divided up, from [`ParallelConfig::plan`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParallelPlan {
    /// Threads that will actually run. `1` means the encode is serial, whatever
    /// was requested.
    pub workers: usize,
    /// Frames of priming each worker after the first re-encodes and discards.
    pub warmup_frames: usize,
    /// Total frames that will be encoded and thrown away.
    pub redundant_frames: usize,
    /// Half-open `[start, end)` frame range per worker, in output order.
    pub ranges: Vec<(usize, usize)>,
}

impl ParallelPlan {
    /// Redundant work as a fraction of the useful work; `0.0` when serial.
    pub fn overhead(&self) -> f64 {
        let useful: usize = self.ranges.iter().map(|&(s, e)| e - s).sum();
        if useful == 0 {
            0.0
        } else {
            self.redundant_frames as f64 / useful as f64
        }
    }
}

/// Encode `pcm` (interleaved f32, `channels`-interleaved) in `frame_size`
/// samples-per-channel frames across several threads, returning one Opus packet
/// per frame in order.
///
/// Output is deterministic and always the same length as a serial encode, but it
/// is a *different* encode: see [the module documentation](self) for what
/// differs and by how much. Falls back to a single encoder when the input is too
/// short to split, which [`ParallelConfig::plan`] will say in advance.
pub fn encode_parallel(
    cfg: &ParallelConfig,
    pcm: &[f32],
    frame_size: usize,
) -> Result<Vec<Vec<u8>>> {
    let step = frame_size * cfg.channels;
    if step == 0 {
        return Ok(Vec::new());
    }
    let total_frames = pcm.len() / step;
    if total_frames == 0 {
        return Ok(Vec::new());
    }

    let plan = cfg.plan(total_frames, frame_size);
    if plan.workers <= 1 {
        return encode_serial(cfg, pcm, frame_size);
    }
    let warmup = plan.warmup_frames;

    let mut chunks: Vec<Result<Vec<Vec<u8>>>> = Vec::with_capacity(plan.workers);
    std::thread::scope(|scope| {
        let handles: Vec<_> = plan
            .ranges
            .iter()
            .map(|&(cstart, cend)| {
                scope.spawn(move || encode_chunk(cfg, pcm, frame_size, warmup, cstart, cend))
            })
            .collect();
        for h in handles {
            // A worker returning `Err` is the caller's argument coming back and
            // is propagated below. A worker that *panicked* is a bug in this
            // crate, and `resume_unwind` carries the original payload and
            // message up rather than replacing it with one of ours.
            match h.join() {
                Ok(r) => chunks.push(r),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });

    // Concatenate in range order. A worker's error is the whole encode's:
    // the chunks are contiguous audio, so a gap in the middle is not a result
    // anybody can use.
    let mut out = Vec::with_capacity(total_frames);
    for c in chunks {
        out.extend(c?);
    }
    Ok(out)
}

/// Encode frames `[cstart, cend)` with a fresh encoder primed by re-encoding the
/// `warmup` frames before `cstart`, whose packets are discarded.
fn encode_chunk(
    cfg: &ParallelConfig,
    pcm: &[f32],
    frame_size: usize,
    warmup: usize,
    cstart: usize,
    cend: usize,
) -> Result<Vec<Vec<u8>>> {
    let step = frame_size * cfg.channels;
    let mut enc = new_encoder(cfg)?;
    let warm_start = cstart.saturating_sub(warmup);
    let mut buf = vec![0u8; MAX_PACKET];
    let mut packets = Vec::with_capacity(cend - cstart);
    for f in warm_start..cend {
        let frame = &pcm[f * step..(f + 1) * step];
        let n = enc.encode(frame, frame_size, &mut buf)?;
        if f >= cstart {
            packets.push(buf[..n].to_vec());
        }
    }
    Ok(packets)
}

/// Single-threaded reference: every frame through one continuous encoder. The
/// correctness and quality anchor for [`encode_parallel`].
fn encode_serial(cfg: &ParallelConfig, pcm: &[f32], frame_size: usize) -> Result<Vec<Vec<u8>>> {
    let step = frame_size * cfg.channels;
    if step == 0 {
        return Ok(Vec::new());
    }
    let total_frames = pcm.len() / step;
    let mut enc = new_encoder(cfg)?;
    let mut buf = vec![0u8; MAX_PACKET];
    let mut packets = Vec::with_capacity(total_frames);
    for f in 0..total_frames {
        let frame = &pcm[f * step..(f + 1) * step];
        let n = enc.encode(frame, frame_size, &mut buf)?;
        packets.push(buf[..n].to_vec());
    }
    Ok(packets)
}

fn new_encoder(cfg: &ParallelConfig) -> Result<OpusEncoder> {
    let mut enc = OpusEncoder::new(cfg.sample_rate, cfg.channels, cfg.application)?;
    enc.bitrate_bps = cfg.bitrate_bps;
    enc.complexity = cfg.complexity;
    enc.rate_control = cfg.rate_control;
    enc.use_inband_fec = cfg.use_inband_fec;
    enc.use_dtx = cfg.use_dtx;
    enc.packet_loss_perc = cfg.packet_loss_perc;
    enc.force_bandwidth = cfg.force_bandwidth;
    enc.signal_type = cfg.signal_type;
    enc.max_bandwidth = cfg.max_bandwidth;
    enc.lsb_depth = cfg.lsb_depth;
    Ok(enc)
}

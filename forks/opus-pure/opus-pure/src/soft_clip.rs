//! Soft clipping for float output on its way to integer PCM.
//!
//! An Opus decoder's float output is not bounded by ±1. The codec rings, and a
//! signal mastered close to full scale comes back slightly over it — this is
//! true of libopus as well, and is not a defect in either. It only becomes
//! audible when the float is converted to integer PCM, where anything past the
//! range saturates into hard, broadband distortion.
//!
//! Hard clamping is the obvious fix and the wrong one: it flattens the peak
//! into a straight line, and the corner at each end of that line is a
//! discontinuity in the first derivative that spreads energy across the whole
//! spectrum. What this does instead is bend the waveform down smoothly — the
//! libopus `opus_pcm_soft_clip` algorithm, which fits `x + a·x²` over the run
//! between the zero crossings either side of a peak, choosing `a` so the peak
//! lands exactly at 1.
//!
//! # Why it holds state
//!
//! A peak can straddle a frame boundary. Fitting each frame in isolation would
//! bend one half of such a peak and leave the other half alone, putting a step
//! at the boundary — the same discontinuity the curve exists to avoid, just
//! moved. So the coefficient in use at the end of a frame carries into the
//! next, and the first samples of that frame continue the curve until the
//! signal crosses zero. [`SoftClip`] is that carry.
//!
//! This is why it is a type rather than a function: the state has to live
//! somewhere, one copy per stream, and a caller who has to remember to thread a
//! scratch array through every call will eventually not.
//!
//! # When you need it
//!
//! [`OpusDecoder::decode_s16`] applies this already, matching libopus, where
//! `opus_decode` soft-clips and `opus_decode_float` does not. Reach for
//! [`SoftClip`] directly when you take the float output yourself and convert it
//! to integer PCM downstream.
//!
//! [`OpusDecoder::decode_s16`]: crate::OpusDecoder::decode_s16

/// Carries the soft-clipping curve across frame boundaries for one stream.
///
/// One per decoder, or per stream if you are clipping several. Construct it
/// with the channel count the buffers you will pass are interleaved at.
///
/// ```
/// use opus_pure::SoftClip;
///
/// let mut clip = SoftClip::new(2);
/// let mut pcm = vec![0.0f32; 960 * 2];
/// // ...decode into `pcm`...
/// clip.apply(&mut pcm);
/// ```
#[derive(Clone, Debug)]
pub struct SoftClip {
    /// The `a` in `x + a·x²` left over from the previous frame, per channel.
    /// Zero means no curve is in progress.
    mem: Vec<f32>,
}

impl SoftClip {
    /// A new soft clipper for interleaved audio with `channels` channels.
    pub fn new(channels: usize) -> Self {
        Self {
            mem: vec![0.0; channels],
        }
    }

    /// The channel count this was built for.
    pub fn channels(&self) -> usize {
        self.mem.len()
    }

    /// Forget any curve in progress, as if the next frame were the first.
    ///
    /// Use this on a discontinuity — a seek, or a new stream through the same
    /// clipper — where carrying the previous curve forward would bend audio
    /// that has nothing to do with it.
    pub fn reset(&mut self) {
        self.mem.fill(0.0);
    }

    /// Soft-clip `pcm` in place, into ±1.
    ///
    /// `pcm` is interleaved at [`channels`](Self::channels). Any trailing
    /// samples that do not complete a frame are left untouched, and a zero
    /// channel count or an empty buffer is a no-op rather than an error: this
    /// is a filter, and having nothing to filter is not a mistake.
    pub fn apply(&mut self, pcm: &mut [f32]) {
        let channels = self.mem.len();
        if channels == 0 {
            return;
        }
        let n = pcm.len() / channels;
        if n == 0 {
            return;
        }

        // Bring everything into [-2, +2] first. That is the domain the curve
        // below is fitted over; outside it the fit's derivative is zero, so
        // clamping here introduces no discontinuity that the curve would then
        // have to smooth (libopus `opus_limit2_checkwithin1`).
        for s in &mut pcm[..n * channels] {
            *s = if *s > -2.0 { *s } else { -2.0 };
            *s = if *s < 2.0 { *s } else { 2.0 };
        }

        for c in 0..channels {
            self.mem[c] = clip_channel(pcm, n, channels, c, self.mem[c]);
        }
    }
}

/// Soft-clip one channel of an interleaved buffer, returning the curve
/// coefficient to carry into the next frame.
///
/// A direct port of the per-channel body of libopus `opus_pcm_soft_clip_impl`.
fn clip_channel(pcm: &mut [f32], n: usize, channels: usize, c: usize, carried: f32) -> f32 {
    let at = |i: usize| i * channels + c;
    let mut a = carried;

    // Finish the curve the previous frame left in progress. It applies until
    // the signal crosses zero, which is where the peak it was fitted to ends.
    // A zero `a` stops this immediately, since `x * 0.0 >= 0.0`.
    for i in 0..n {
        let x = pcm[at(i)];
        if x * a >= 0.0 {
            break;
        }
        pcm[at(i)] = x + a * x * x;
    }

    let mut curr = 0usize;
    // The first sample as the previous step left it, kept for the ramp below.
    let x0 = pcm[at(0)];

    loop {
        // Find the next sample outside ±1. Everything before it is already in
        // range and needs no curve.
        let mut peak = curr;
        while peak < n {
            let x = pcm[at(peak)];
            // Kept as the two comparisons libopus writes rather than a range
            // test: the clamp above has already removed any NaN, on which the
            // two spellings would disagree.
            #[allow(clippy::manual_range_contains)]
            if x > 1.0 || x < -1.0 {
                break;
            }
            peak += 1;
        }
        if peak == n {
            // Nothing left over the limit, so no curve carries forward.
            a = 0.0;
            break;
        }

        // Widen to the zero crossings either side: the curve has to span a
        // whole excursion, or its ends would not meet the signal smoothly.
        let x_peak = pcm[at(peak)];
        let mut start = peak;
        let mut end = peak;
        let mut maxval = x_peak.abs();
        let mut peak_pos = peak;
        while start > 0 && x_peak * pcm[at(start - 1)] >= 0.0 {
            start -= 1;
        }
        while end < n && x_peak * pcm[at(end)] >= 0.0 {
            let mag = pcm[at(end)].abs();
            if mag > maxval {
                maxval = mag;
                peak_pos = end;
            }
            end += 1;
        }

        // The excursion runs off the front of the frame: its zero crossing is
        // in audio already delivered, so the curve has no left-hand anchor.
        let clipped_at_start = start == 0 && x_peak * pcm[at(0)] >= 0.0;

        // Choose `a` so that maxval + a·maxval² lands exactly on 1.
        a = (maxval - 1.0) / (maxval * maxval);
        // libopus nudges `a` up by 2^-22 so that a compiler reassociating this
        // arithmetic cannot leave the result a hair over 1. Far too small to
        // hear, and it keeps the output inside the range the caller was
        // promised even at 24-bit.
        a += a * 2.4e-7;
        if x_peak > 0.0 {
            a = -a;
        }

        for i in start..end {
            let x = pcm[at(i)];
            pcm[at(i)] = x + a * x * x;
        }

        if clipped_at_start && peak_pos >= 2 {
            // No left-hand anchor, so the curve has just moved sample 0. Ramp
            // that offset away over the run up to the peak, rather than
            // stepping at the boundary with the previous frame.
            let mut offset = x0 - pcm[at(0)];
            let delta = offset / peak_pos as f32;
            for i in curr..peak_pos {
                offset -= delta;
                pcm[at(i)] += offset;
                pcm[at(i)] = pcm[at(i)].clamp(-1.0, 1.0);
            }
        }

        curr = end;
        if curr == n {
            break;
        }
    }

    a
}

/// One float sample as 16-bit PCM, by libopus's rule (`FLOAT2INT16`).
///
/// Full scale is 32768, not 32767: the scale factor is a power of two, so the
/// conversion is exact in the direction that matters and the asymmetry lives in
/// the saturation instead. Rounding is to nearest with ties to even, which is
/// what every `float2int` libopus selects does — SSE `cvtss2si`, NEON
/// `vcvtns_s32_f32` and `lrintf` under the default rounding mode alike. A cast
/// would truncate towards zero instead and pull the whole signal inwards by up
/// to half an LSB.
///
/// NaN saturates to the negative rail, as it does in C, where the comparison
/// against the lower bound is false and hands back the bound.
#[inline]
pub(crate) fn float_to_i16(x: f32) -> i16 {
    let x = x * 32768.0;
    let x = if x > -32768.0 { x } else { -32768.0 };
    let x = if x < 32767.0 { x } else { 32767.0 };
    x.round_ties_even() as i16
}

/// One 16-bit PCM sample as a float, by libopus's rule (`INT16TORES`).
///
/// Exact for every input: 32768 is a power of two, so this only shifts the
/// exponent.
#[inline]
pub(crate) fn i16_to_float(x: i16) -> f32 {
    x as f32 * (1.0 / 32768.0)
}

//! Port of libopus 1.3.1 `src/analysis.c` + `src/mlp.c` (float build): the
//! tonality/music/bandwidth analysis that drives mode selection, bandwidth
//! detection, VBR boosts and prefilter damping in the reference encoder.
//!
//! Faithful line-for-line port. The MLP (dense 25→32, GRU →24, dense →2) uses
//! the shipped int8 weight tables in [`crate::analysis_data`]. The FFT is the
//! CELT mode's N=480 kiss-FFT (same twiddles/bitrev as the reference; the
//! forward scale 1/N is applied in the input copy, as C's `opus_fft` does).

// `1.442695` is libopus's own truncated log2(e), not an approximation of the
// std constant. Substituting `f32::consts::LOG2_E` (1.4426950408889634) changes
// the band-energy logs and moves the bitstream, so clippy's suggestion is wrong
// here and the lint is off for this file.
#![allow(clippy::approx_constant)]

use crate::analysis_data::*;
use crate::celt::kiss_fft::{KissCpx, KissFftState, opus_fft_impl};

pub const NB_FRAMES: usize = 8;
pub const NB_TBANDS: usize = 18;
pub const ANALYSIS_BUF_SIZE: usize = 720; // 30 ms at 24 kHz
/// Most samples one `tonality_analysis` call adds to `inmem`: `run_analysis`
/// hands it at most 20 ms at a time, which is 480 once the length is converted
/// to the 24 kHz analysis rate at the top of the call.
const ANALYSIS_STEP: usize = 480;
/// Length of the downmix scratch `tonality_analysis` keeps. The 48 kHz path
/// reads two input samples per analysis sample, and the 16 kHz path holds two
/// thirds of a step three times over, so both need exactly this many; 24 kHz
/// needs fewer.
const DOWNMIX_SCRATCH_LEN: usize = 2 * ANALYSIS_STEP;
pub const DETECT_SIZE: usize = 100;
pub const ANALYSIS_COUNT_MAX: i32 = 10000;
pub const LEAK_BANDS: usize = 19;
const NB_TONAL_SKIP_BANDS: usize = 9;
const TRANSITION_PENALTY: f32 = 10.0;
const LEAKAGE_OFFSET: f32 = 2.5;
const LEAKAGE_SLOPE: f32 = 2.0;

/// celt.h `AnalysisInfo` (float build).
#[derive(Clone, Copy, Debug)]
pub struct AnalysisInfo {
    pub valid: bool,
    pub tonality: f32,
    pub tonality_slope: f32,
    pub noisiness: f32,
    pub activity: f32,
    pub music_prob: f32,
    pub music_prob_min: f32,
    pub music_prob_max: f32,
    pub bandwidth: i32,
    pub activity_probability: f32,
    pub max_pitch_ratio: f32,
    /// Q6 per-band boost (celt dynalloc leakage compensation).
    pub leak_boost: [u8; LEAK_BANDS],
}

impl Default for AnalysisInfo {
    fn default() -> Self {
        AnalysisInfo {
            valid: false,
            tonality: 0.0,
            tonality_slope: 0.0,
            noisiness: 0.0,
            activity: 0.0,
            music_prob: 0.0,
            music_prob_min: 0.0,
            music_prob_max: 0.0,
            bandwidth: 0,
            activity_probability: 0.0,
            max_pitch_ratio: 0.0,
            leak_boost: [0; LEAK_BANDS],
        }
    }
}

// ---------------------------------------------------------------- MLP (mlp.c)

const WEIGHTS_SCALE: f32 = 1.0 / 128.0;
const MAX_NEURONS: usize = 32;

/// The comparisons are deliberately reversed so that a NaN input falls through
/// to the explicit `is_nan` check rather than being classified as in-range;
/// clippy's `partial_cmp` suggestion would lose that.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn tansig_approx(x: f32) -> f32 {
    if !(x < 8.0) {
        return 1.0;
    }
    if !(x > -8.0) {
        return -1.0;
    }
    if x.is_nan() {
        return 0.0;
    }
    let (x, sign) = if x < 0.0 { (-x, -1.0f32) } else { (x, 1.0f32) };
    let i = (0.5 + 25.0 * x).floor() as usize;
    let x = x - 0.04 * i as f32;
    let y = TANSIG_TABLE[i];
    let dy = 1.0 - y * y;
    let y = y + x * dy * (1.0 - y * x);
    sign * y
}

fn sigmoid_approx(x: f32) -> f32 {
    0.5 + 0.5 * tansig_approx(0.5 * x)
}

fn gemm_accum(
    out: &mut [f32],
    weights: &[i8],
    rows: usize,
    cols: usize,
    col_stride: usize,
    x: &[f32],
) {
    for i in 0..rows {
        for j in 0..cols {
            out[i] += weights[j * col_stride + i] as f32 * x[j];
        }
    }
}

struct DenseLayer {
    bias: &'static [i8],
    input_weights: &'static [i8],
    nb_inputs: usize,
    nb_neurons: usize,
    sigmoid: bool,
}

struct GruLayer {
    bias: &'static [i8],
    input_weights: &'static [i8],
    recurrent_weights: &'static [i8],
    nb_inputs: usize,
    nb_neurons: usize,
}

const LAYER0: DenseLayer = DenseLayer {
    bias: &LAYER0_BIAS,
    input_weights: &LAYER0_WEIGHTS,
    nb_inputs: 25,
    nb_neurons: 32,
    sigmoid: false,
};
const LAYER1: GruLayer = GruLayer {
    bias: &LAYER1_BIAS,
    input_weights: &LAYER1_WEIGHTS,
    recurrent_weights: &LAYER1_RECUR_WEIGHTS,
    nb_inputs: 32,
    nb_neurons: 24,
};
const LAYER2: DenseLayer = DenseLayer {
    bias: &LAYER2_BIAS,
    input_weights: &LAYER2_WEIGHTS,
    nb_inputs: 24,
    nb_neurons: 2,
    sigmoid: true,
};

fn compute_dense(layer: &DenseLayer, output: &mut [f32], input: &[f32]) {
    let (m, n) = (layer.nb_inputs, layer.nb_neurons);
    for i in 0..n {
        output[i] = layer.bias[i] as f32;
    }
    gemm_accum(output, layer.input_weights, n, m, n, input);
    for o in output.iter_mut().take(n) {
        *o *= WEIGHTS_SCALE;
        *o = if layer.sigmoid {
            sigmoid_approx(*o)
        } else {
            tansig_approx(*o)
        };
    }
}

fn compute_gru(gru: &GruLayer, state: &mut [f32], input: &[f32]) {
    let (m, n) = (gru.nb_inputs, gru.nb_neurons);
    let stride = 3 * n;
    let mut z = [0.0f32; MAX_NEURONS];
    let mut r = [0.0f32; MAX_NEURONS];
    let mut h = [0.0f32; MAX_NEURONS];
    let mut tmp = [0.0f32; MAX_NEURONS];

    // Update gate.
    for i in 0..n {
        z[i] = gru.bias[i] as f32;
    }
    gemm_accum(&mut z, gru.input_weights, n, m, stride, input);
    gemm_accum(&mut z, gru.recurrent_weights, n, n, stride, state);
    for zi in z.iter_mut().take(n) {
        *zi = sigmoid_approx(WEIGHTS_SCALE * *zi);
    }

    // Reset gate.
    for i in 0..n {
        r[i] = gru.bias[n + i] as f32;
    }
    gemm_accum(&mut r, &gru.input_weights[n..], n, m, stride, input);
    gemm_accum(&mut r, &gru.recurrent_weights[n..], n, n, stride, state);
    for ri in r.iter_mut().take(n) {
        *ri = sigmoid_approx(WEIGHTS_SCALE * *ri);
    }

    // Output.
    for i in 0..n {
        h[i] = gru.bias[2 * n + i] as f32;
    }
    for i in 0..n {
        tmp[i] = state[i] * r[i];
    }
    gemm_accum(&mut h, &gru.input_weights[2 * n..], n, m, stride, input);
    gemm_accum(&mut h, &gru.recurrent_weights[2 * n..], n, n, stride, &tmp);
    for i in 0..n {
        state[i] = z[i] * state[i] + (1.0 - z[i]) * tansig_approx(WEIGHTS_SCALE * h[i]);
    }
}

// ------------------------------------------------------- helpers (analysis.c)

fn fast_atan2f(y: f32, x: f32) -> f32 {
    const CA: f32 = 0.43157974;
    const CB: f32 = 0.678_484;
    const CC: f32 = 0.08595542;
    const CE: f32 = std::f32::consts::PI / 2.0;
    let x2 = x * x;
    let y2 = y * y;
    if x2 + y2 < 1e-18 {
        return 0.0;
    }
    if x2 < y2 {
        let den = (y2 + CB * x2) * (y2 + CC * x2);
        -x * y * (y2 + CA * x2) / den + if y < 0.0 { -CE } else { CE }
    } else {
        let den = (x2 + CB * y2) * (x2 + CC * y2);
        x * y * (x2 + CA * y2) / den + if y < 0.0 { -CE } else { CE }
            - if x * y < 0.0 { -CE } else { CE }
    }
}

/// silk_resampler_down2_hp (float build): 2:1 all-pass halfband with a
/// complementary high-pass branch; returns the HP branch energy.
fn resampler_down2_hp(s: &mut [f32; 3], out: &mut [f32], input: &[f32]) -> f32 {
    let len2 = input.len() / 2;
    let mut hp_ener = 0.0f64;
    for k in 0..len2 {
        let in32 = input[2 * k];
        let y = in32 - s[0];
        let x = 0.6074371 * y;
        let out32 = s[0] + x;
        s[0] = in32 + x;
        let mut out32_hp = out32;

        let in32 = input[2 * k + 1];
        let y = in32 - s[1];
        let x = 0.15063 * y;
        let out32 = out32 + s[1] + x;
        s[1] = in32 + x;

        let y = -in32 - s[2];
        let x = 0.15063 * y;
        out32_hp = out32_hp + s[2] + x;
        s[2] = -in32 + x;

        hp_ener += (out32_hp as f64) * (out32_hp as f64);
        out[k] = 0.5 * out32;
    }
    hp_ener as f32
}

/// downmix_and_resample: mixes the requested channels of `x` (interleaved f32,
/// ±1 range — C's downmix_float×CELT_SIG_SCALE then ÷32768 nets to this) into
/// `y` at 24 kHz. Returns the >12 kHz HP energy (48 kHz input only).
fn downmix_and_resample(
    x: &[f32],
    y: &mut [f32],
    s: &mut [f32; 3],
    scratch_downmix: &mut [f32; DOWNMIX_SCRATCH_LEN],
    scratch_3x: &mut [f32; DOWNMIX_SCRATCH_LEN],
    subframe: usize,
    offset: usize,
    channels: usize,
    fs: i32,
) -> f32 {
    if subframe == 0 {
        return 0.0;
    }
    let (subframe, offset) = match fs {
        48000 => (subframe * 2, offset * 2),
        16000 => (subframe * 2 / 3, offset * 2 / 3),
        _ => (subframe, offset),
    };
    // downmix all channels (c1=0, c2=-2), scale 1/C.
    let scale = 1.0f32 / channels as f32;
    let tmp = &mut scratch_downmix[..subframe];
    for (j, t) in tmp.iter_mut().enumerate() {
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += x[(offset + j) * channels + c];
        }
        *t = sum * scale;
    }
    match fs {
        48000 => resampler_down2_hp(s, y, tmp),
        24000 => {
            y[..subframe].copy_from_slice(tmp);
            0.0
        }
        16000 => {
            // "Don't do this at home": zero-order-hold 3x then down2.
            let tmp3x = &mut scratch_3x[..3 * subframe];
            for j in 0..subframe {
                tmp3x[3 * j] = tmp[j];
                tmp3x[3 * j + 1] = tmp[j];
                tmp3x[3 * j + 2] = tmp[j];
            }
            resampler_down2_hp(s, y, tmp3x)
        }
        _ => 0.0,
    }
}

// ------------------------------------------------------------ analysis state

pub struct TonalityAnalysisState {
    pub fs: i32,
    angle: [f32; 240],
    d_angle: [f32; 240],
    d2_angle: [f32; 240],
    inmem: [f32; ANALYSIS_BUF_SIZE],
    mem_fill: usize,
    prev_band_tonality: [f32; NB_TBANDS],
    prev_tonality: f32,
    prev_bandwidth: i32,
    e: [[f32; NB_TBANDS]; NB_FRAMES],
    log_e: [[f32; NB_TBANDS]; NB_FRAMES],
    low_e: [f32; NB_TBANDS],
    high_e: [f32; NB_TBANDS],
    mean_e: [f32; NB_TBANDS + 1],
    mem: [f32; 32],
    cmean: [f32; 8],
    std: [f32; 9],
    etracker: f32,
    low_e_count: f32,
    e_count: usize,
    count: i32,
    analysis_offset: i32,
    write_pos: usize,
    read_pos: usize,
    read_subframe: i32,
    hp_ener_accum: f32,
    initialized: bool,
    rnn_state: [f32; MAX_NEURONS],
    downmix_state: [f32; 3],
    scratch_downmix: [f32; DOWNMIX_SCRATCH_LEN],
    scratch_3x: [f32; DOWNMIX_SCRATCH_LEN],
    info: [AnalysisInfo; DETECT_SIZE],
}

impl TonalityAnalysisState {
    pub fn new(fs: i32) -> Self {
        TonalityAnalysisState {
            fs,
            angle: [0.0; 240],
            d_angle: [0.0; 240],
            d2_angle: [0.0; 240],
            inmem: [0.0; ANALYSIS_BUF_SIZE],
            mem_fill: 0,
            prev_band_tonality: [0.0; NB_TBANDS],
            prev_tonality: 0.0,
            prev_bandwidth: 0,
            e: [[0.0; NB_TBANDS]; NB_FRAMES],
            log_e: [[0.0; NB_TBANDS]; NB_FRAMES],
            low_e: [0.0; NB_TBANDS],
            high_e: [0.0; NB_TBANDS],
            mean_e: [0.0; NB_TBANDS + 1],
            mem: [0.0; 32],
            cmean: [0.0; 8],
            std: [0.0; 9],
            etracker: 0.0,
            low_e_count: 0.0,
            e_count: 0,
            count: 0,
            analysis_offset: 0,
            write_pos: 0,
            read_pos: 0,
            read_subframe: 0,
            hp_ener_accum: 0.0,
            initialized: false,
            rnn_state: [0.0; MAX_NEURONS],
            downmix_state: [0.0; 3],
            scratch_downmix: [0.0; DOWNMIX_SCRATCH_LEN],
            scratch_3x: [0.0; DOWNMIX_SCRATCH_LEN],
            info: [AnalysisInfo::default(); DETECT_SIZE],
        }
    }

    pub fn initialized(&self) -> bool {
        self.initialized
    }

    pub fn reset(&mut self) {
        let fs = self.fs;
        *self = TonalityAnalysisState::new(fs);
    }

    /// Where [`tonality_get_info`] will read next.
    ///
    /// [`run_analysis`] analyses a whole packet at once but consumes only one
    /// frame's worth of the ring. An encoder splitting that packet into several
    /// coded frames takes a snapshot here first and restores it before pulling
    /// each frame's own slice, so frame *i* sees the analysis of the audio it
    /// actually codes (`opus_encoder.c` `analysis_read_pos_bak`).
    pub fn read_position(&self) -> AnalysisReadPos {
        AnalysisReadPos {
            pos: self.read_pos,
            subframe: self.read_subframe,
        }
    }

    pub fn set_read_position(&mut self, at: AnalysisReadPos) {
        self.read_pos = at.pos;
        self.read_subframe = at.subframe;
    }
}

/// An opaque snapshot of [`TonalityAnalysisState::read_position`]. Opaque so the
/// two halves of the position cannot be swapped or advanced by hand.
#[derive(Clone, Copy, Debug)]
pub struct AnalysisReadPos {
    pos: usize,
    subframe: i32,
}

/// tonality_get_info: interpolate the ring of per-20ms analyses into one
/// AnalysisInfo for a frame of `len` samples (encoder rate), applying the
/// music/speech hysteresis thresholds (music_prob_min/max).
pub fn tonality_get_info(tonal: &mut TonalityAnalysisState, len: usize) -> AnalysisInfo {
    let mut pos = tonal.read_pos as i32;
    let mut curr_lookahead = tonal.write_pos as i32 - tonal.read_pos as i32;
    if curr_lookahead < 0 {
        curr_lookahead += DETECT_SIZE as i32;
    }

    tonal.read_subframe += len as i32 / (tonal.fs / 400);
    while tonal.read_subframe >= 8 {
        tonal.read_subframe -= 8;
        tonal.read_pos += 1;
    }
    if tonal.read_pos >= DETECT_SIZE {
        tonal.read_pos -= DETECT_SIZE;
    }

    // On long frames, look at the second analysis window rather than the first.
    if len as i32 > tonal.fs / 50 && pos != tonal.write_pos as i32 {
        pos += 1;
        if pos == DETECT_SIZE as i32 {
            pos = 0;
        }
    }
    if pos == tonal.write_pos as i32 {
        pos -= 1;
    }
    if pos < 0 {
        pos = DETECT_SIZE as i32 - 1;
    }
    let pos0 = pos;
    let mut info = tonal.info[pos as usize];
    if !info.valid {
        return info;
    }
    let mut tonality_max = info.tonality;
    let mut tonality_avg = info.tonality;
    let mut tonality_count = 1;
    // Look at the neighbouring frames and pick largest bandwidth found (to be safe).
    let mut bandwidth_span = 6;
    // If possible, look ahead for a tone to compensate for the delay in the tone detector.
    for _ in 0..3 {
        pos += 1;
        if pos == DETECT_SIZE as i32 {
            pos = 0;
        }
        if pos == tonal.write_pos as i32 {
            break;
        }
        tonality_max = tonality_max.max(tonal.info[pos as usize].tonality);
        tonality_avg += tonal.info[pos as usize].tonality;
        tonality_count += 1;
        info.bandwidth = info.bandwidth.max(tonal.info[pos as usize].bandwidth);
        bandwidth_span -= 1;
    }
    pos = pos0;
    // Look back in time to see if any has a wider bandwidth than the current frame.
    for _ in 0..bandwidth_span {
        pos -= 1;
        if pos < 0 {
            pos = DETECT_SIZE as i32 - 1;
        }
        if pos == tonal.write_pos as i32 {
            break;
        }
        info.bandwidth = info.bandwidth.max(tonal.info[pos as usize].bandwidth);
    }
    info.tonality = (tonality_avg / tonality_count as f32).max(tonality_max - 0.2);

    let mut mpos = pos0;
    let mut vpos = pos0;
    // If we have enough look-ahead, compensate for the ~5-frame delay in the
    // music prob and ~1 frame delay in the VAD prob.
    if curr_lookahead > 15 {
        mpos += 5;
        if mpos >= DETECT_SIZE as i32 {
            mpos -= DETECT_SIZE as i32;
        }
        vpos += 1;
        if vpos >= DETECT_SIZE as i32 {
            vpos -= DETECT_SIZE as i32;
        }
    }

    // Transition-badness thresholds (see the long comment in analysis.c).
    let mut prob_min = 1.0f32;
    let mut prob_max = 0.0f32;
    let vad_prob = tonal.info[vpos as usize].activity_probability;
    let mut prob_count = 0.1f32.max(vad_prob);
    let mut prob_avg = 0.1f32.max(vad_prob) * tonal.info[mpos as usize].music_prob;
    loop {
        mpos += 1;
        if mpos == DETECT_SIZE as i32 {
            mpos = 0;
        }
        if mpos == tonal.write_pos as i32 {
            break;
        }
        vpos += 1;
        if vpos == DETECT_SIZE as i32 {
            vpos = 0;
        }
        if vpos == tonal.write_pos as i32 {
            break;
        }
        let pos_vad = tonal.info[vpos as usize].activity_probability;
        prob_min =
            ((prob_avg - TRANSITION_PENALTY * (vad_prob - pos_vad)) / prob_count).min(prob_min);
        prob_max =
            ((prob_avg + TRANSITION_PENALTY * (vad_prob - pos_vad)) / prob_count).max(prob_max);
        prob_count += 0.1f32.max(pos_vad);
        prob_avg += 0.1f32.max(pos_vad) * tonal.info[mpos as usize].music_prob;
    }
    info.music_prob = prob_avg / prob_count;
    prob_min = (prob_avg / prob_count).min(prob_min);
    prob_max = (prob_avg / prob_count).max(prob_max);
    prob_min = prob_min.max(0.0);
    prob_max = prob_max.min(1.0);

    // If we don't have enough look-ahead, do our best to make a decent decision.
    if curr_lookahead < 10 {
        let mut pmin = prob_min;
        let mut pmax = prob_max;
        let mut pos = pos0;
        // Look for min/max in the past.
        for _ in 0..(tonal.count - 1).clamp(0, 15) {
            pos -= 1;
            if pos < 0 {
                pos = DETECT_SIZE as i32 - 1;
            }
            pmin = pmin.min(tonal.info[pos as usize].music_prob);
            pmax = pmax.max(tonal.info[pos as usize].music_prob);
        }
        // Bias against switching on active audio.
        pmin = 0.0f32.max(pmin - 0.1 * vad_prob);
        pmax = 1.0f32.min(pmax + 0.1 * vad_prob);
        prob_min += (1.0 - 0.1 * curr_lookahead as f32) * (pmin - prob_min);
        prob_max += (1.0 - 0.1 * curr_lookahead as f32) * (pmax - prob_max);
    }
    info.music_prob_min = prob_min;
    info.music_prob_max = prob_max;
    info
}

/// One 20 ms (at the analysis rate) tonality_analysis step over `x`
/// (interleaved f32 at the encoder rate).
fn tonality_analysis(
    tonal: &mut TonalityAnalysisState,
    kfft: &KissFftState,
    x: &[f32],
    len: usize,
    offset: usize,
    channels: usize,
    lsb_depth: i32,
) {
    const N: usize = 480;
    const N2: usize = 240;
    let pi4 = (std::f64::consts::PI.powi(4)) as f32;

    if !tonal.initialized {
        tonal.mem_fill = 240;
        tonal.initialized = true;
    }
    let alpha = 1.0 / (10.min(1 + tonal.count) as f32);
    let alpha_e = 1.0 / (25.min(1 + tonal.count) as f32);
    // Noise floor related decay for bandwidth detection: -2.2 dB/second.
    let mut alpha_e2 = 1.0 / (100.min(1 + tonal.count) as f32);
    if tonal.count <= 1 {
        alpha_e2 = 1.0;
    }

    let (mut len, mut offset) = (len, offset);
    if tonal.fs == 48000 {
        len /= 2;
        offset /= 2;
    } else if tonal.fs == 16000 {
        len = 3 * len / 2;
        offset = 3 * offset / 2;
    }

    {
        let fill = (len).min(ANALYSIS_BUF_SIZE - tonal.mem_fill);
        let mf = tonal.mem_fill;
        let hp = downmix_and_resample(
            x,
            &mut tonal.inmem[mf..mf + fill],
            &mut tonal.downmix_state,
            &mut tonal.scratch_downmix,
            &mut tonal.scratch_3x,
            fill,
            offset,
            channels,
            tonal.fs,
        );
        tonal.hp_ener_accum += hp;
    }

    if tonal.mem_fill + len < ANALYSIS_BUF_SIZE {
        tonal.mem_fill += len;
        // Don't have enough to update the analysis.
        return;
    }
    let hp_ener = tonal.hp_ener_accum;
    let write_pos_now = tonal.write_pos;
    tonal.write_pos += 1;
    if tonal.write_pos >= DETECT_SIZE {
        tonal.write_pos -= DETECT_SIZE;
    }

    // is_digital_silence (float build): a THRESHOLD at 1 LSB, not exact zero.
    let silence_thresh = 1.0f32 / (1i64 << lsb_depth) as f32;
    let is_silence = tonal.inmem.iter().fold(0.0f32, |m, &v| m.max(v.abs())) <= silence_thresh;

    let mut fft_in = [KissCpx::new(0.0, 0.0); N];
    let mut fft_out = [KissCpx::new(0.0, 0.0); N];
    let mut tonality = [0.0f32; 240];
    let mut noisiness = [0.0f32; 240];
    for i in 0..N2 {
        let w = ANALYSIS_WINDOW[i];
        fft_in[i] = KissCpx::new(w * tonal.inmem[i], w * tonal.inmem[N2 + i]);
        fft_in[N - i - 1] =
            KissCpx::new(w * tonal.inmem[N - i - 1], w * tonal.inmem[N + N2 - i - 1]);
    }
    tonal
        .inmem
        .copy_within(ANALYSIS_BUF_SIZE - 240..ANALYSIS_BUF_SIZE, 0);
    let remaining = len - (ANALYSIS_BUF_SIZE - tonal.mem_fill);
    {
        let hp = downmix_and_resample(
            x,
            &mut tonal.inmem[240..240 + remaining],
            &mut tonal.downmix_state,
            &mut tonal.scratch_downmix,
            &mut tonal.scratch_3x,
            remaining,
            offset + ANALYSIS_BUF_SIZE - tonal.mem_fill,
            channels,
            tonal.fs,
        );
        tonal.hp_ener_accum = hp;
    }
    tonal.mem_fill = 240 + remaining;

    if is_silence {
        // On silence, copy the previous analysis.
        let prev_pos = (write_pos_now + DETECT_SIZE - 1) % DETECT_SIZE;
        tonal.info[write_pos_now] = tonal.info[prev_pos];
        return;
    }

    // opus_fft: scale in the bitrev input copy, then in-place FFT.
    let scale = kfft.scale();
    for (i, v) in fft_in.iter().enumerate() {
        fft_out[kfft.bitrev[i] as usize] = KissCpx::new(scale * v.r, scale * v.i);
    }
    opus_fft_impl(kfft, &mut fft_out);
    let out = &fft_out;

    let info_idx = write_pos_now;
    if out[0].r.is_nan() {
        tonal.info[info_idx].valid = false;
        return;
    }

    let a = &mut tonal.angle;
    let da = &mut tonal.d_angle;
    let d2a = &mut tonal.d2_angle;
    let mut tonality2 = [0.0f32; 240];
    for i in 1..N2 {
        let x1r = out[i].r + out[N - i].r;
        let x1i = out[i].i - out[N - i].i;
        let x2r = out[i].i + out[N - i].i;
        let x2i = out[N - i].r - out[i].r;

        let angle = (0.5 / std::f64::consts::PI) as f32 * fast_atan2f(x1i, x1r);
        let d_angle = angle - a[i];
        let d2_angle = d_angle - da[i];

        let angle2 = (0.5 / std::f64::consts::PI) as f32 * fast_atan2f(x2i, x2r);
        let d_angle2 = angle2 - angle;
        let d2_angle2 = d_angle2 - d_angle;

        let mut mod1 = d2_angle - d2_angle.round_ties_even();
        noisiness[i] = mod1.abs();
        mod1 *= mod1;
        mod1 *= mod1;

        let mut mod2 = d2_angle2 - d2_angle2.round_ties_even();
        noisiness[i] += mod2.abs();
        mod2 *= mod2;
        mod2 *= mod2;

        let avg_mod = 0.25 * (d2a[i] + mod1 + 2.0 * mod2);
        // This introduces an extra delay of 2 frames in the detection.
        tonality[i] = 1.0 / (1.0 + 40.0 * 16.0 * pi4 * avg_mod) - 0.015;
        // No delay on this detection, but it's less reliable.
        tonality2[i] = 1.0 / (1.0 + 40.0 * 16.0 * pi4 * mod2) - 0.015;

        a[i] = angle2;
        da[i] = d_angle2;
        d2a[i] = mod2;
    }
    for i in 2..N2 - 1 {
        let tt = tonality2[i].min(tonality2[i - 1].max(tonality2[i + 1]));
        tonality[i] = 0.9 * tonality[i].max(tt - 0.1);
    }

    let mut frame_tonality = 0.0f32;
    let mut max_frame_tonality = 0.0f32;
    let mut frame_noisiness = 0.0f32;
    let mut frame_stationarity = 0.0f32;
    if tonal.count == 0 {
        for b in 0..NB_TBANDS {
            tonal.low_e[b] = 1e10;
            tonal.high_e[b] = -1e10;
        }
    }
    let mut relative_e = 0.0f32;
    let mut frame_loudness = 0.0f32;
    let mut log_e = [0.0f32; NB_TBANDS];
    let mut band_log2 = [0.0f32; NB_TBANDS + 1];
    let mut band_tonality = [0.0f32; NB_TBANDS];
    let mut slope = 0.0f32;
    // The energy of the very first band is special because of DC.
    {
        let x1r = 2.0 * out[0].r;
        let x2r = 2.0 * out[0].i;
        let mut e = x1r * x1r + x2r * x2r;
        for i in 1..4 {
            let bin_e = out[i].r * out[i].r
                + out[N - i].r * out[N - i].r
                + out[i].i * out[i].i
                + out[N - i].i * out[N - i].i;
            e += bin_e;
        }
        band_log2[0] = 0.5 * 1.442695 * ((e + 1e-10) as f64).ln() as f32;
    }
    for b in 0..NB_TBANDS {
        let mut e = 0.0f32;
        let mut t_e = 0.0f32;
        let mut n_e = 0.0f32;
        for i in TBANDS[b]..TBANDS[b + 1] {
            let bin_e = out[i].r * out[i].r
                + out[N - i].r * out[N - i].r
                + out[i].i * out[i].i
                + out[N - i].i * out[N - i].i;
            e += bin_e;
            t_e += bin_e * 0.0f32.max(tonality[i]);
            n_e += bin_e * 2.0 * (0.5 - noisiness[i]);
        }
        // Check for extreme band energies that could cause NaNs later. The
        // comparison is reversed so a NaN energy fails it rather than passing.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        let extreme = !(e < 1e9);
        if extreme || e.is_nan() {
            tonal.info[info_idx].valid = false;
            return;
        }

        tonal.e[tonal.e_count][b] = e;
        frame_noisiness += n_e / (1e-15 + e);

        frame_loudness += ((e + 1e-10) as f64).sqrt() as f32;
        log_e[b] = ((e + 1e-10) as f64).ln() as f32;
        band_log2[b + 1] = 0.5 * 1.442695 * log_e[b];
        tonal.log_e[tonal.e_count][b] = log_e[b];
        if tonal.count == 0 {
            tonal.high_e[b] = log_e[b];
            tonal.low_e[b] = log_e[b];
        }
        if tonal.high_e[b] > tonal.low_e[b] + 7.5 {
            if tonal.high_e[b] - log_e[b] > log_e[b] - tonal.low_e[b] {
                tonal.high_e[b] -= 0.01;
            } else {
                tonal.low_e[b] += 0.01;
            }
        }
        if log_e[b] > tonal.high_e[b] {
            tonal.high_e[b] = log_e[b];
            tonal.low_e[b] = tonal.low_e[b].max(tonal.high_e[b] - 15.0);
        } else if log_e[b] < tonal.low_e[b] {
            tonal.low_e[b] = log_e[b];
            tonal.high_e[b] = tonal.high_e[b].min(tonal.low_e[b] + 15.0);
        }
        relative_e += (log_e[b] - tonal.low_e[b]) / (1e-5 + (tonal.high_e[b] - tonal.low_e[b]));

        let mut l1 = 0.0f32;
        let mut l2 = 0.0f32;
        for i in 0..NB_FRAMES {
            l1 += (tonal.e[i][b] as f64).sqrt() as f32;
            l2 += tonal.e[i][b];
        }

        let mut stationarity =
            (l1 / (1e-15 + NB_FRAMES as f64 * l2 as f64).sqrt() as f32).min(0.99);
        stationarity *= stationarity;
        stationarity *= stationarity;
        frame_stationarity += stationarity;
        band_tonality[b] = (t_e / (1e-15 + e)).max(stationarity * tonal.prev_band_tonality[b]);
        frame_tonality += band_tonality[b];
        if b >= NB_TBANDS - NB_TONAL_SKIP_BANDS {
            // C analysis.c: `band_tonality[b-NB_TBANDS+NB_TONAL_SKIP_BANDS]` with
            // `int b` — the intermediate (b - NB_TBANDS) is negative there, but the
            // guarded final index is >= 0. Written left-to-right in usize, that
            // intermediate underflows (debug panic; release wraps back to the same
            // final index C computes). Reorder so no intermediate goes negative:
            // the guard gives b + NB_TONAL_SKIP_BANDS >= NB_TBANDS, and the final
            // index is identical to the C reference in all builds.
            frame_tonality -= band_tonality[b + NB_TONAL_SKIP_BANDS - NB_TBANDS];
        }
        max_frame_tonality =
            max_frame_tonality.max((1.0 + 0.03 * (b as f32 - NB_TBANDS as f32)) * frame_tonality);
        slope += band_tonality[b] * (b as f32 - 8.0);
        tonal.prev_band_tonality[b] = band_tonality[b];
    }

    let mut leakage_from = [0.0f32; NB_TBANDS + 1];
    let mut leakage_to = [0.0f32; NB_TBANDS + 1];
    leakage_from[0] = band_log2[0];
    leakage_to[0] = band_log2[0] - LEAKAGE_OFFSET;
    for b in 1..NB_TBANDS + 1 {
        let leak_slope = LEAKAGE_SLOPE * (TBANDS[b] - TBANDS[b - 1]) as f32 / 4.0;
        leakage_from[b] = (leakage_from[b - 1] + leak_slope).min(band_log2[b]);
        leakage_to[b] = (leakage_to[b - 1] - leak_slope).max(band_log2[b] - LEAKAGE_OFFSET);
    }
    for b in (0..NB_TBANDS - 1).rev() {
        let leak_slope = LEAKAGE_SLOPE * (TBANDS[b + 1] - TBANDS[b]) as f32 / 4.0;
        leakage_from[b] = (leakage_from[b + 1] + leak_slope).min(leakage_from[b]);
        leakage_to[b] = (leakage_to[b + 1] - leak_slope).max(leakage_to[b]);
    }
    for b in 0..NB_TBANDS + 1 {
        // leak_boost: analysis leakage INTO a weak band b (leakage_to) +
        // synthesis leakage FROM a loud band b (leakage_from).
        let boost = 0.0f32.max(leakage_to[b] - band_log2[b])
            + 0.0f32.max(band_log2[b] - (leakage_from[b] + LEAKAGE_OFFSET));
        tonal.info[info_idx].leak_boost[b] = 255.min((0.5 + 64.0 * boost).floor() as i32) as u8;
    }

    let mut spec_variability = 0.0f32;
    for i in 0..NB_FRAMES {
        let mut mindist = 1e15f32;
        for j in 0..NB_FRAMES {
            let mut dist = 0.0f32;
            for k in 0..NB_TBANDS {
                let tmp = tonal.log_e[i][k] - tonal.log_e[j][k];
                dist += tmp * tmp;
            }
            if j != i {
                mindist = mindist.min(dist);
            }
        }
        spec_variability += mindist;
    }
    spec_variability =
        ((spec_variability / NB_FRAMES as f32 / NB_TBANDS as f32) as f64).sqrt() as f32;

    let mut bandwidth_mask = 0.0f32;
    let mut bandwidth = 0i32;
    let mut max_e = 0.0f32;
    let lsb = 0.max(lsb_depth - 8);
    let mut noise_floor = 5.7e-4 / (1u32 << lsb) as f32;
    noise_floor *= noise_floor;
    let mut below_max_pitch = 0.0f32;
    let mut above_max_pitch = 0.0f32;
    let mut is_masked = [false; NB_TBANDS + 1];
    for b in 0..NB_TBANDS {
        let band_start = TBANDS[b];
        let band_end = TBANDS[b + 1];
        let mut e = 0.0f32;
        for i in band_start..band_end {
            let bin_e = out[i].r * out[i].r
                + out[N - i].r * out[N - i].r
                + out[i].i * out[i].i
                + out[N - i].i * out[N - i].i;
            e += bin_e;
        }
        max_e = max_e.max(e);
        if band_start < 64 {
            below_max_pitch += e;
        } else {
            above_max_pitch += e;
        }
        tonal.mean_e[b] = ((1.0 - alpha_e2) * tonal.mean_e[b]).max(e);
        let em = e.max(tonal.mean_e[b]);
        // Band is "active" if within 90 dB of the peak AND above the noise floor.
        if e * 1e9 > max_e
            && (em > 3.0 * noise_floor * (band_end - band_start) as f32
                || e > noise_floor * (band_end - band_start) as f32)
        {
            bandwidth = b as i32 + 1;
        }
        is_masked[b] = e
            < (if tonal.prev_bandwidth > b as i32 {
                0.01
            } else {
                0.05
            }) * bandwidth_mask;
        // Simple follower with 13 dB/Bark slope for the spreading function.
        bandwidth_mask = (0.05 * bandwidth_mask).max(e);
    }
    // The energy above 12 kHz comes from the resampler's HP branch.
    if tonal.fs == 48000 {
        let noise_ratio = if tonal.prev_bandwidth == 20 {
            10.0
        } else {
            30.0
        };
        let e = hp_ener * (1.0 / (60.0 * 60.0));
        above_max_pitch += e;
        tonal.mean_e[NB_TBANDS] = ((1.0 - alpha_e2) * tonal.mean_e[NB_TBANDS]).max(e);
        let em = e.max(tonal.mean_e[NB_TBANDS]);
        if em > 3.0 * noise_ratio * noise_floor * 160.0 || e > noise_ratio * noise_floor * 160.0 {
            bandwidth = 20;
        }
        is_masked[NB_TBANDS] = e
            < (if tonal.prev_bandwidth == 20 {
                0.01
            } else {
                0.05
            }) * bandwidth_mask;
    }
    tonal.info[info_idx].max_pitch_ratio = if above_max_pitch > below_max_pitch {
        below_max_pitch / above_max_pitch
    } else {
        1.0
    };
    // If the last band is just aliasing noise, don't include it.
    if bandwidth == 20 && is_masked[NB_TBANDS] {
        bandwidth -= 2;
    } else if bandwidth > 0 && bandwidth <= NB_TBANDS as i32 && is_masked[bandwidth as usize - 1] {
        bandwidth -= 1;
    }
    if tonal.count <= 2 {
        bandwidth = 20;
    }
    frame_loudness = 20.0 * (frame_loudness as f64).log10() as f32;
    tonal.etracker = (tonal.etracker - 0.003).max(frame_loudness);
    tonal.low_e_count *= 1.0 - alpha_e;
    if frame_loudness < tonal.etracker - 30.0 {
        tonal.low_e_count += alpha_e;
    }

    let mut bfcc = [0.0f32; 8];
    let mut mid_e = [0.0f32; 8];
    for i in 0..8 {
        let mut sum = 0.0f32;
        for b in 0..16 {
            sum += DCT_TABLE[i * 16 + b] * log_e[b];
        }
        bfcc[i] = sum;
    }
    for i in 0..8 {
        let mut sum = 0.0f32;
        for b in 0..16 {
            sum += DCT_TABLE[i * 16 + b] * 0.5 * (tonal.high_e[b] + tonal.low_e[b]);
        }
        mid_e[i] = sum;
    }

    frame_stationarity /= NB_TBANDS as f32;
    relative_e /= NB_TBANDS as f32;
    if tonal.count < 10 {
        relative_e = 0.5;
    }
    frame_noisiness /= NB_TBANDS as f32;
    tonal.info[info_idx].activity = frame_noisiness + (1.0 - frame_noisiness) * relative_e;
    let mut frame_tonality = max_frame_tonality / (NB_TBANDS - NB_TONAL_SKIP_BANDS) as f32;
    frame_tonality = frame_tonality.max(tonal.prev_tonality * 0.8);
    tonal.prev_tonality = frame_tonality;

    slope /= 64.0;
    tonal.info[info_idx].tonality_slope = slope;

    tonal.e_count = (tonal.e_count + 1) % NB_FRAMES;
    tonal.count = (tonal.count + 1).min(ANALYSIS_COUNT_MAX);
    tonal.info[info_idx].tonality = frame_tonality;

    let mut features = [0.0f32; 25];
    for i in 0..4 {
        features[i] = -0.12299 * (bfcc[i] + tonal.mem[i + 24])
            + 0.49195 * (tonal.mem[i] + tonal.mem[i + 16])
            + 0.69693 * tonal.mem[i + 8]
            - 1.4349 * tonal.cmean[i];
    }
    for i in 0..4 {
        tonal.cmean[i] = (1.0 - alpha) * tonal.cmean[i] + alpha * bfcc[i];
    }
    for i in 0..4 {
        features[4 + i] =
            0.63246 * (bfcc[i] - tonal.mem[i + 24]) + 0.31623 * (tonal.mem[i] - tonal.mem[i + 16]);
    }
    for i in 0..3 {
        features[8 + i] = 0.53452 * (bfcc[i] + tonal.mem[i + 24])
            - 0.26726 * (tonal.mem[i] + tonal.mem[i + 16])
            - 0.53452 * tonal.mem[i + 8];
    }

    if tonal.count > 5 {
        for i in 0..9 {
            tonal.std[i] = (1.0 - alpha) * tonal.std[i] + alpha * features[i] * features[i];
        }
    }
    for i in 0..4 {
        features[i] = bfcc[i] - mid_e[i];
    }

    for i in 0..8 {
        tonal.mem[i + 24] = tonal.mem[i + 16];
        tonal.mem[i + 16] = tonal.mem[i + 8];
        tonal.mem[i + 8] = tonal.mem[i];
        tonal.mem[i] = bfcc[i];
    }
    for i in 0..9 {
        features[11 + i] = (tonal.std[i] as f64).sqrt() as f32 - STD_FEATURE_BIAS[i];
    }
    features[18] = spec_variability - 0.78;
    features[20] = tonal.info[info_idx].tonality - 0.154723;
    features[21] = tonal.info[info_idx].activity - 0.724643;
    features[22] = frame_stationarity - 0.743717;
    features[23] = tonal.info[info_idx].tonality_slope + 0.069216;
    features[24] = tonal.low_e_count - 0.067930;

    let mut layer_out = [0.0f32; MAX_NEURONS];
    let mut frame_probs = [0.0f32; 2];
    compute_dense(&LAYER0, &mut layer_out, &features);
    let mut rnn_state = tonal.rnn_state;
    compute_gru(&LAYER1, &mut rnn_state, &layer_out);
    tonal.rnn_state = rnn_state;
    compute_dense(&LAYER2, &mut frame_probs, &tonal.rnn_state);

    // Probability of speech or music vs noise.
    tonal.info[info_idx].activity_probability = frame_probs[1];
    tonal.info[info_idx].music_prob = frame_probs[0];

    tonal.info[info_idx].bandwidth = bandwidth;
    tonal.prev_bandwidth = bandwidth;
    tonal.info[info_idx].noisiness = frame_noisiness;
    tonal.info[info_idx].valid = true;
}

/// run_analysis: feed the frame through 20 ms analysis steps, then read the
/// interpolated info for this frame.
pub fn run_analysis(
    analysis: &mut TonalityAnalysisState,
    kfft: &KissFftState,
    analysis_pcm: &[f32],
    analysis_frame_size: usize,
    frame_size: usize,
    channels: usize,
    fs: i32,
    lsb_depth: i32,
) -> AnalysisInfo {
    let mut analysis_frame_size = analysis_frame_size & !1;
    // Avoid overflow/wrap-around of the analysis buffer.
    analysis_frame_size = analysis_frame_size.min((DETECT_SIZE - 5) * fs as usize / 50);

    let mut pcm_len = analysis_frame_size as i32 - analysis.analysis_offset;
    let mut offset = analysis.analysis_offset;
    while pcm_len > 0 {
        tonality_analysis(
            analysis,
            kfft,
            analysis_pcm,
            (fs as usize / 50).min(pcm_len as usize),
            offset as usize,
            channels,
            lsb_depth,
        );
        offset += fs / 50;
        pcm_len -= fs / 50;
    }
    analysis.analysis_offset = analysis_frame_size as i32;
    analysis.analysis_offset -= frame_size as i32;

    tonality_get_info(analysis, frame_size)
}

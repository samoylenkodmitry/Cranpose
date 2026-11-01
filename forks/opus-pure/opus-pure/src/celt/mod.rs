pub(crate) mod bands;
pub(crate) mod kiss_fft;
pub(crate) mod lpc;
pub(crate) mod mdct;
pub(crate) mod modes;
pub(crate) mod pitch;
pub(crate) mod pvq;
pub(crate) mod quant_bands;
pub(crate) mod rate;

use crate::celt::bands::{
    SPREAD_NONE, SPREAD_NORMAL, compute_band_energies, denormalise_bands, haar1, log2amp,
    normalise_bands, quant_all_bands, spreading_decision,
};
use crate::celt::modes::{CeltMode, SPREAD_ICDF, TAPSET_ICDF, TF_SELECT_TABLE, TRIM_ICDF};
use crate::celt::quant_bands::{
    quant_coarse_energy_advanced, quant_energy_finalise, quant_fine_energy, unquant_coarse_energy,
    unquant_energy_finalise, unquant_fine_energy,
};
use crate::celt::rate::clt_compute_allocation;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::cpu_features::FeatureCache;
use crate::range_coder::BITRES;
use crate::range_coder::RangeCoder;

/// Runtime CPU-feature gates for the x86 kernels in this layer.
///
/// A dispatch test must name *every* feature the kernel it guards declares in
/// its `#[target_feature]` list. Testing only `avx` before calling a function
/// declared `avx,fma` runs FMA instructions on AVX-without-FMA hardware
/// (Sandy Bridge, Ivy Bridge), which is undefined behaviour. Routing the
/// combined tests through these two helpers keeps the guard and the kernel it
/// guards from drifting apart.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
pub(crate) fn have_avx_fma() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| {
        std::arch::is_x86_feature_detected!("avx") && std::arch::is_x86_feature_detected!("fma")
    })
}

/// Companion to [`have_avx_fma`] for kernels declared `avx2,fma`.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
pub(crate) fn have_avx2_fma() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    })
}

/// Companion for kernels declared `avx` alone (the MDCT's TDAC fold).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline]
pub(crate) fn have_avx() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| std::arch::is_x86_feature_detected!("avx"))
}

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

const MAX_FRAME_SIZE: usize = 2880;

/// Decoder history buffer, libopus `DEC_PITCH_BUF_SIZE` (modes.h). It holds the
/// last frame plus a full `COMBFILTER_MAXPERIOD` of postfilter history, and it
/// doubles as the window the packet-loss concealment searches for a pitch
/// period — so its size is not free to choose. A larger buffer would hand the
/// concealment a longer search window than the reference and let it settle on a
/// different lag.
const DECODE_BUFFER_SIZE: usize = 2048;
/// CELT packet-loss-concealment constants (celt_decoder.c).
const PLC_LPC_ORDER: usize = 24;
const PLC_PITCH_LAG_MAX: usize = 720;
const PLC_PITCH_LAG_MIN: usize = 100;

/// What the decoder produced for the previous frame (celt_decoder.c:67). The
/// concealment reads it to decide whether it is starting a burst — and so has to
/// search for a pitch period and fit an LPC filter — or continuing one, where it
/// reuses both and fades. A plain "consecutive losses" counter cannot express
/// `FRAME_NONE`, which is a fresh decoder rather than a run of losses.
const FRAME_NONE: i32 = 0;
const FRAME_NORMAL: i32 = 1;
const FRAME_PLC_NOISE: i32 = 2;
const FRAME_PLC_PERIODIC: i32 = 3;

/// A burst this long stops being extrapolated from the last pitch period and
/// becomes shaped noise instead: 40 units of 2.5 ms, i.e. 100 ms
/// (celt_decoder.c:724). Counting *duration* rather than lost frames is what
/// makes the switch happen at the same place whatever the frame size is.
const PLC_NOISE_AFTER: i32 = 40;

const INV_TABLE: [u8; 128] = [
    255, 255, 156, 110, 86, 70, 59, 51, 45, 40, 37, 33, 31, 28, 26, 25, 23, 22, 21, 20, 19, 18, 17,
    16, 16, 15, 15, 14, 13, 13, 12, 12, 12, 12, 11, 11, 11, 10, 10, 10, 9, 9, 9, 9, 9, 9, 8, 8, 8,
    8, 8, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
    5, 5, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 3, 3,
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2,
];

const MAX_TRANSIENT_LEN: usize = 3000;

pub(crate) use crate::analysis::AnalysisInfo;

fn transient_analysis(
    input: &[f32],
    len: usize,
    channels: usize,
    tf_estimate: &mut f32,
    tf_chan: &mut usize,
    allow_weak_transients: bool,
    weak_transient: &mut bool,
    _tone_freq: f32,
    toneishness: f32,
    tmp: &mut [f32],
    tmp2: &mut [f32],
) -> bool {
    let mut mask_metric = 0.0f32;
    let mut forward_decay = 0.0625f32;

    *weak_transient = false;
    if allow_weak_transients {
        forward_decay = 0.03125f32;
    }

    let len2 = len / 2;
    debug_assert!(len <= MAX_TRANSIENT_LEN);

    for c in 0..channels {
        let mut mem0 = 0.0f32;
        let mut mem1 = 0.0f32;

        for i in 0..len {
            let x = input[c * len + i];
            let y = mem0 + x;
            let mem00 = mem0;
            mem0 = mem0 - x + 0.5 * mem1;
            mem1 = x - mem00;
            tmp[i] = y;
        }

        tmp[..12].fill(0.0);

        let mut mean = 0.0f32;
        mem0 = 0.0f32;
        for i in 0..len2 {
            let x2 = (tmp[2 * i] * tmp[2 * i] + tmp[2 * i + 1] * tmp[2 * i + 1]) / 16.0;
            mean += x2 / 4096.0;
            mem0 = x2 + (1.0 - forward_decay) * mem0;
            tmp2[i] = forward_decay * mem0;
        }

        mem0 = 0.0f32;
        let mut max_e = 0.0f32;
        for i in (0..len2).rev() {
            mem0 = tmp2[i] + 0.875 * mem0;
            tmp2[i] = 0.125 * mem0;
            if tmp2[i] > max_e {
                max_e = tmp2[i];
            }
        }

        mean = (mean * max_e * 0.5 * (len2 as f32)).sqrt();
        let norm = (len2 as f32) / (1e-10 + mean);

        let mut unmask = 0.0f32;
        for i in (12..(len2 - 5)).step_by(4) {
            let id = (64.0 * norm * (tmp2[i] + 1e-10)).floor() as i32;
            let id = id.clamp(0, 127) as usize;
            unmask += INV_TABLE[id] as f32;
        }

        unmask = 64.0 * unmask * 4.0 / (6.0 * (len2 as f32 - 17.0));
        if unmask > mask_metric {
            *tf_chan = c;
            mask_metric = unmask;
        }
    }

    let mut is_transient = mask_metric > 200.0;

    if toneishness > 0.98 && _tone_freq < 0.026 {
        is_transient = false;
        mask_metric = 0.0;
    }

    *tf_estimate = (mask_metric - 150.0).clamp(0.0, 1.0);

    is_transient
}

fn l1_metric(tmp: &[f32], n: usize, lm: i32, bias: f32) -> f32 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        if n >= 16 && std::arch::is_x86_feature_detected!("avx") {
            return l1_metric_avx(tmp, n, lm, bias);
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if n >= 16 {
            return unsafe { l1_metric_neon(tmp, n, lm, bias) };
        }
    }

    let mut l1 = 0.0f32;
    for &tv in tmp[..n].iter() {
        l1 += tv.abs();
    }
    l1 + (lm as f32) * bias * l1
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn sum_abs_avx(x: &[f32], n: usize) -> f32 {
    use std::arch::x86_64::*;

    // The loads below walk `n` elements; trimming to `n` here is what puts
    // them in bounds.
    let x = &x[..n];

    let mut sum0 = _mm256_setzero_ps();
    let mut sum1 = _mm256_setzero_ps();
    let mut i = 0usize;
    let sign_mask = _mm256_set1_ps(-0.0);

    while i + 16 <= n {
        let v0 = _mm256_loadu_ps(x.as_ptr().add(i));
        let v1 = _mm256_loadu_ps(x.as_ptr().add(i + 8));
        sum0 = _mm256_add_ps(sum0, _mm256_andnot_ps(sign_mask, v0));
        sum1 = _mm256_add_ps(sum1, _mm256_andnot_ps(sign_mask, v1));
        i += 16;
    }

    while i + 8 <= n {
        let v = _mm256_loadu_ps(x.as_ptr().add(i));
        sum0 = _mm256_add_ps(sum0, _mm256_andnot_ps(sign_mask, v));
        i += 8;
    }

    let sum = _mm256_add_ps(sum0, sum1);
    let hi = _mm256_extractf128_ps(sum, 1);
    let lo = _mm256_castps256_ps128(sum);
    let s4 = _mm_add_ps(lo, hi);
    let t1 = _mm_movehl_ps(s4, s4);
    let s2 = _mm_add_ps(s4, t1);
    let t2 = _mm_shuffle_ps(s2, s2, 0x55);
    let mut out = _mm_cvtss_f32(_mm_add_ss(s2, t2));

    for j in i..n {
        out += x[j].abs();
    }

    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn l1_metric_avx(tmp: &[f32], n: usize, lm: i32, bias: f32) -> f32 {
    let l1 = sum_abs_avx(tmp, n);
    l1 + (lm as f32) * bias * l1
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn l1_metric_neon(tmp: &[f32], n: usize, lm: i32, bias: f32) -> f32 {
    // The loads below walk `n` elements; trimming to `n` here is what puts
    // them in bounds.
    let tmp = &tmp[..n];

    unsafe {
        let mut sum4 = vdupq_n_f32(0.0);
        let mut i = 0;

        while i + 15 < n {
            let v0 = vld1q_f32(tmp.as_ptr().add(i));
            let v1 = vld1q_f32(tmp.as_ptr().add(i + 4));
            let v2 = vld1q_f32(tmp.as_ptr().add(i + 8));
            let v3 = vld1q_f32(tmp.as_ptr().add(i + 12));

            sum4 = vaddq_f32(sum4, vabsq_f32(v0));
            sum4 = vaddq_f32(sum4, vabsq_f32(v1));
            sum4 = vaddq_f32(sum4, vabsq_f32(v2));
            sum4 = vaddq_f32(sum4, vabsq_f32(v3));

            i += 16;
        }

        while i + 3 < n {
            let v = vld1q_f32(tmp.as_ptr().add(i));
            sum4 = vaddq_f32(sum4, vabsq_f32(v));
            i += 4;
        }

        let sum2 = vpaddq_f32(sum4, sum4);
        let sum1 = vpaddq_f32(sum2, sum2);
        let mut l1 = vgetq_lane_f32(sum1, 0);

        while i < n {
            l1 += tmp[i].abs();
            i += 1;
        }

        l1 + (lm as f32) * bias * l1
    }
}

const MAX_NB_EBANDS: usize = 21;

const MAX_TF_TMP: usize = 176;

fn tf_analysis(
    mode: &CeltMode,
    len: usize,
    is_transient: bool,
    tf_res: &mut [i32],
    lambda: i32,
    x: &[f32],
    n0: usize,
    lm: i32,
    tf_estimate: f32,
    tf_chan: usize,
    importance: &[f32],
) -> i32 {
    debug_assert!(len <= MAX_NB_EBANDS);
    let mut metric = [0i32; MAX_NB_EBANDS];
    let mut tmp = [0.0f32; MAX_TF_TMP];
    let mut tmp_1 = [0.0f32; MAX_TF_TMP];

    let bias = 0.04 * (-0.25f32).max(0.5 - tf_estimate);

    for (i, metric_i) in metric[..len].iter_mut().enumerate() {
        let n = ((mode.e_bands[i + 1] - mode.e_bands[i]) as usize) << lm;
        let narrow = (mode.e_bands[i + 1] - mode.e_bands[i]) == 1;
        let offset = tf_chan * n0 + ((mode.e_bands[i] as usize) << lm);
        tmp[..n].copy_from_slice(&x[offset..offset + n]);

        let mut l1 = l1_metric(&tmp[..n], n, if is_transient { lm } else { 0 }, bias);
        let mut best_l1 = l1;
        let mut best_level = 0;

        if is_transient && !narrow {
            tmp_1[..n].copy_from_slice(&tmp[..n]);
            haar1(&mut tmp_1[..n], n >> lm, 1 << lm);
            l1 = l1_metric(&tmp_1[..n], n, lm + 1, bias);
            if l1 < best_l1 {
                best_l1 = l1;
                best_level = -1;
            }
        }

        for k in 0..(lm + if is_transient || narrow { 0 } else { 1 }) {
            let b = if is_transient { lm - k - 1 } else { k + 1 };

            haar1(&mut tmp[..n], n >> k, 1 << k);
            l1 = l1_metric(&tmp[..n], n, b, bias);

            if l1 < best_l1 {
                best_l1 = l1;
                best_level = k + 1;
            }
        }

        if is_transient {
            *metric_i = 2 * best_level;
        } else {
            *metric_i = -2 * best_level;
        }

        if narrow && (*metric_i == 0 || *metric_i == -2 * lm) {
            *metric_i -= 1;
        }
    }

    let mut tf_select = 0;
    let mut selcost = [0.0f32; 2];

    for sel in 0..2 {
        let mut cost0 = importance[0]
            * ((metric[0]
                - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * sel] as i32)
                as f32)
                .abs();
        let mut cost1 = importance[0]
            * ((metric[0]
                - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * sel + 1]
                    as i32) as f32)
                .abs()
            + (if is_transient { 0.0 } else { lambda as f32 });

        for i in 1..len {
            let curr0 = cost0.min(cost1 + lambda as f32);
            let curr1 = (cost0 + lambda as f32).min(cost1);
            cost0 = curr0
                + importance[i]
                    * ((metric[i]
                        - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * sel]
                            as i32) as f32)
                        .abs();
            cost1 = curr1
                + importance[i]
                    * ((metric[i]
                        - 2 * TF_SELECT_TABLE[lm as usize]
                            [4 * (is_transient as usize) + 2 * sel + 1]
                            as i32) as f32)
                        .abs();
        }
        selcost[sel] = cost0.min(cost1);
    }

    // C: tf_select=1 is only allowed on transients (celt_encoder.c:108).
    if selcost[1] < selcost[0] && is_transient {
        tf_select = 1;
    }

    let mut cost0 = importance[0]
        * ((metric[0]
            - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * tf_select] as i32)
            as f32)
            .abs();
    let mut cost1 = importance[0]
        * ((metric[0]
            - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * tf_select + 1]
                as i32) as f32)
            .abs()
        + (if is_transient { 0.0 } else { lambda as f32 });

    tf_res[0] = if cost0 < cost1 { 0 } else { 1 };

    for i in 1..len {
        let curr0 = cost0.min(cost1 + lambda as f32);
        let curr1 = (cost0 + lambda as f32).min(cost1);
        cost0 = curr0
            + importance[i]
                * ((metric[i]
                    - 2 * TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 * tf_select]
                        as i32) as f32)
                    .abs();
        cost1 = curr1
            + importance[i]
                * ((metric[i]
                    - 2 * TF_SELECT_TABLE[lm as usize]
                        [4 * (is_transient as usize) + 2 * tf_select + 1]
                        as i32) as f32)
                    .abs();
        tf_res[i] = if cost0 < cost1 { 0 } else { 1 };
    }

    tf_select as i32
}

fn tf_encode(
    start: usize,
    end: usize,
    is_transient: bool,
    tf_res: &mut [i32],
    lm: i32,
    mut tf_select: i32,
    rc: &mut RangeCoder,
) -> i32 {
    let mut curr = 0;
    let mut tf_changed = 0;
    let mut logp = if is_transient { 2 } else { 4 };
    let mut budget = rc.storage as i32 * 8;
    let mut tell = rc.tell();

    let tf_select_rsv = if lm > 0 && tell + logp < budget { 1 } else { 0 };
    budget -= tf_select_rsv;

    for tf_res_i in tf_res[start..end].iter_mut() {
        if tell + logp <= budget {
            rc.encode_bit_logp(*tf_res_i ^ curr != 0, logp as u32);
            tell = rc.tell();
            curr = *tf_res_i;
            tf_changed |= curr;
        } else {
            *tf_res_i = curr;
        }
        logp = if is_transient { 4 } else { 5 };
    }

    if tf_select_rsv != 0
        && TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + (tf_changed as usize)]
            != TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 + (tf_changed as usize)]
    {
        rc.encode_bit_logp(tf_select != 0, 1);
    } else {
        tf_select = 0;
    }

    for tf_res_i in tf_res[start..end].iter_mut() {
        *tf_res_i = TF_SELECT_TABLE[lm as usize]
            [4 * (is_transient as usize) + 2 * (tf_select as usize) + (*tf_res_i as usize)]
            as i32;
    }

    tf_changed
}

fn tf_decode(
    start: usize,
    end: usize,
    is_transient: bool,
    tf_res: &mut [i32],
    lm: i32,
    rc: &mut RangeCoder,
) {
    let mut curr = 0;
    let mut tf_changed = 0;
    let mut logp = if is_transient { 2 } else { 4 };
    let budget = rc.storage as i32 * 8;
    let mut tell = rc.tell();

    let tf_select_rsv = if lm > 0 && tell + logp < budget { 1 } else { 0 };
    let budget = budget - tf_select_rsv;

    for tf_res_i in tf_res[start..end].iter_mut() {
        if tell + logp <= budget {
            curr ^= if rc.decode_bit_logp(logp as u32) {
                1
            } else {
                0
            };
            tell = rc.tell();
            tf_changed |= curr;
        }
        *tf_res_i = curr;
        logp = if is_transient { 4 } else { 5 };
    }

    let mut tf_select = 0;
    let _budget = budget + tf_select_rsv;
    if tf_select_rsv > 0
        && TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + (tf_changed as usize)]
            != TF_SELECT_TABLE[lm as usize][4 * (is_transient as usize) + 2 + (tf_changed as usize)]
    {
        tf_select = if rc.decode_bit_logp(1) { 1 } else { 0 };
    }

    for tf_res_i in tf_res[start..end].iter_mut() {
        *tf_res_i = TF_SELECT_TABLE[lm as usize]
            [4 * (is_transient as usize) + 2 * (tf_select as usize) + (*tf_res_i as usize)]
            as i32;
    }
}

fn stereo_analysis(m: &CeltMode, x: &[f32], lm: i32, n0: usize) -> bool {
    let mut sum_lr = 1e-9f32;
    let mut sum_ms = 1e-9f32;

    for i in 0..13 {
        let start = (m.e_bands[i] as usize) << lm;
        let end = (m.e_bands[i + 1] as usize) << lm;
        for j in start..end {
            let l = x[j];
            let r = x[n0 + j];
            let m_val = l + r;
            let s_val = l - r;
            sum_lr += l.abs() + r.abs();
            sum_ms += m_val.abs() + s_val.abs();
        }
    }

    sum_ms *= std::f32::consts::FRAC_1_SQRT_2;
    let mut thetas = 13;
    if lm <= 1 {
        thetas -= 8;
    }

    let left = (((m.e_bands[13] as usize) << (lm + 1)) + thetas) as f32 * sum_ms;
    let right = ((m.e_bands[13] as usize) << (lm + 1)) as f32 * sum_lr;

    left > right
}

const COMBFILTER_MINPERIOD: usize = 15;
const COMBFILTER_MAXPERIOD: usize = 1024;

const PREFILTER_GAINS: [[f32; 3]; 3] = [
    [0.306_640_6, 0.217_041, 0.129_638_7],
    [0.463_867_2, 0.268_066_4, 0.0],
    [0.799_804_7, 0.100_097_7, 0.0],
];

fn comb_filter_const(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    #[cfg(target_arch = "aarch64")]
    {
        comb_filter_const_neon(y, x, y_idx, x_idx, t, n, g10, g11, g12);
    }
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    unsafe {
        if have_avx_fma() {
            comb_filter_const_avx(y, x, y_idx, x_idx, t, n, g10, g11, g12);
            return;
        }
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
    unsafe {
        comb_filter_const_sse(y, x, y_idx, x_idx, t, n, g10, g11, g12);
        #[allow(clippy::needless_return)]
        return;
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse")
    )))]
    {
        comb_filter_const_scalar(y, x, y_idx, x_idx, t, n, g10, g11, g12);
    }
}

#[inline]
#[allow(dead_code)]
fn comb_filter_const_scalar(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    let mut x1;
    let mut x2;
    let mut x3;
    let mut x4;
    let mut x0;

    x4 = x[x_idx - t - 2];
    x3 = x[x_idx - t - 1];
    x2 = x[x_idx - t];
    x1 = x[x_idx - t + 1];

    for i in 0..n {
        x0 = x[x_idx + i - t + 2];
        y[y_idx + i] = x[x_idx + i] + g10 * x2 + g11 * (x1 + x3) + g12 * (x0 + x4);
        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
    }
}

#[cfg(target_arch = "aarch64")]
fn comb_filter_const_neon(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    unsafe { comb_filter_const_neon_impl(y, x, y_idx, x_idx, t, n, g10, g11, g12) }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn comb_filter_const_neon_impl(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    use std::arch::aarch64::*;

    let g10v = vdupq_n_f32(g10);
    let g11v = vdupq_n_f32(g11);
    let g12v = vdupq_n_f32(g12);

    // The taps reach two samples either side of the period, so the kernel
    // reads `x[x_idx - t - 2 .. x_idx + n]` and writes `y[y_idx .. y_idx + n]`.
    // Naming both spans is what keeps the pointer walk below inside the
    // buffers; nothing else here ties `n`, `t` and the indices to the lengths
    // this function was handed.
    assert!(
        t >= 2,
        "comb filter period {t} is shorter than the tap reach"
    );
    let xspan = &x[x_idx - t - 2..x_idx + n];
    let xbase = xspan.as_ptr().add(t + 2);
    let ybase = y[y_idx..y_idx + n].as_mut_ptr();

    let mut x0v = vld1q_f32(xbase.sub(t + 2));

    let mut i = 0;
    while i + 4 <= n {
        let x4v = vld1q_f32(xbase.add(i).sub(t - 2));

        let x2v = vextq_f32(x0v, x4v, 2);

        let x1v = vextq_f32(x0v, x4v, 1);

        let x3v = vextq_f32(x0v, x4v, 3);

        let xi = vld1q_f32(xbase.add(i));

        let mut yi = xi;
        yi = vfmaq_f32(yi, g10v, x2v);
        yi = vfmaq_f32(yi, g11v, vaddq_f32(x1v, x3v));
        yi = vfmaq_f32(yi, g12v, vaddq_f32(x4v, x0v));
        vst1q_f32(ybase.add(i), yi);

        x0v = x4v;
        i += 4;
    }

    let x0v_arr: [f32; 4] = std::mem::transmute(x0v);
    let mut sx4 = x0v_arr[0];
    let mut sx3 = x0v_arr[1];
    let mut sx2 = x0v_arr[2];
    let mut sx1 = x0v_arr[3];

    while i < n {
        let sx0 = x[x_idx + i - t + 2];
        y[y_idx + i] = x[x_idx + i] + g10 * sx2 + g11 * (sx1 + sx3) + g12 * (sx0 + sx4);
        sx4 = sx3;
        sx3 = sx2;
        sx2 = sx1;
        sx1 = sx0;
        i += 1;
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse"))]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn comb_filter_const_sse(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    use std::arch::x86_64::*;

    let g10v = _mm_set1_ps(g10);
    let g11v = _mm_set1_ps(g11);
    let g12v = _mm_set1_ps(g12);

    // The taps reach two samples either side of the period, so the kernel
    // reads `x[x_idx - t - 2 .. x_idx + n]` and writes `y[y_idx .. y_idx + n]`.
    // Naming both spans is what keeps the pointer walk below inside the
    // buffers; nothing else here ties `n`, `t` and the indices to the lengths
    // this function was handed.
    assert!(
        t >= 2,
        "comb filter period {t} is shorter than the tap reach"
    );
    let xspan = &x[x_idx - t - 2..x_idx + n];
    let xbase = xspan.as_ptr().add(t + 2);
    let ybase = y[y_idx..y_idx + n].as_mut_ptr();
    let mut x0v = _mm_loadu_ps(xbase.sub(t + 2));

    let mut i = 0;
    while i + 4 <= n {
        let x4v = _mm_loadu_ps(xbase.add(i).sub(t - 2));

        let x2v = _mm_shuffle_ps(x0v, x4v, 0x4e);

        let x1v = _mm_shuffle_ps(x0v, x2v, 0x99);

        let x3v = _mm_shuffle_ps(x2v, x4v, 0x99);

        let xi = _mm_loadu_ps(xbase.add(i));

        let mut yi = xi;
        yi = _mm_add_ps(yi, _mm_mul_ps(g10v, x2v));
        let yi2 = _mm_add_ps(
            _mm_mul_ps(g11v, _mm_add_ps(x3v, x1v)),
            _mm_mul_ps(g12v, _mm_add_ps(x4v, x0v)),
        );
        yi = _mm_add_ps(yi, yi2);
        _mm_storeu_ps(ybase.add(i), yi);

        x0v = x4v;
        i += 4;
    }

    let x0v_arr: [f32; 4] = std::mem::transmute(x0v);
    let mut sx4 = x0v_arr[0];
    let mut sx3 = x0v_arr[1];
    let mut sx2 = x0v_arr[2];
    let mut sx1 = x0v_arr[3];

    while i < n {
        let sx0 = x[x_idx + i - t + 2];
        y[y_idx + i] = x[x_idx + i] + g10 * sx2 + g11 * (sx1 + sx3) + g12 * (sx0 + sx4);
        sx4 = sx3;
        sx3 = sx2;
        sx2 = sx1;
        sx1 = sx0;
        i += 1;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn comb_filter_const_avx(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    use std::arch::x86_64::*;

    let g10v = _mm256_set1_ps(g10);
    let g11v = _mm256_set1_ps(g11);
    let g12v = _mm256_set1_ps(g12);

    // The taps reach two samples either side of the period, so the kernel
    // reads `x[x_idx - t - 2 .. x_idx + n]` and writes `y[y_idx .. y_idx + n]`.
    // Naming both spans is what keeps the pointer walk below inside the
    // buffers; nothing else here ties `n`, `t` and the indices to the lengths
    // this function was handed.
    assert!(
        t >= 2,
        "comb filter period {t} is shorter than the tap reach"
    );
    let xspan = &x[x_idx - t - 2..x_idx + n];
    let xbase = xspan.as_ptr().add(t + 2);
    let ybase = y[y_idx..y_idx + n].as_mut_ptr();

    let mut i = 0;

    while i + 16 <= n {
        let xi_a = _mm256_loadu_ps(xbase.add(i));
        let x0_a = _mm256_loadu_ps(xbase.add(i).sub(t + 2));
        let x4_a = _mm256_loadu_ps(xbase.add(i).sub(t - 2));

        let x2_a = _mm256_loadu_ps(xbase.add(i).sub(t));
        let x1x3_a = _mm256_add_ps(
            _mm256_loadu_ps(xbase.add(i).sub(t + 1)),
            _mm256_loadu_ps(xbase.add(i).sub(t - 1)),
        );
        let x0x4_a = _mm256_add_ps(x0_a, x4_a);

        let mut yi_a = xi_a;
        yi_a = _mm256_fmadd_ps(g10v, x2_a, yi_a);
        yi_a = _mm256_fmadd_ps(g11v, x1x3_a, yi_a);
        yi_a = _mm256_fmadd_ps(g12v, x0x4_a, yi_a);
        _mm256_storeu_ps(ybase.add(i), yi_a);

        let j = i + 8;
        let xi_b = _mm256_loadu_ps(xbase.add(j));
        let x0_b = _mm256_loadu_ps(xbase.add(j).sub(t + 2));
        let x4_b = _mm256_loadu_ps(xbase.add(j).sub(t - 2));
        let x2_b = _mm256_loadu_ps(xbase.add(j).sub(t));
        let x1x3_b = _mm256_add_ps(
            _mm256_loadu_ps(xbase.add(j).sub(t + 1)),
            _mm256_loadu_ps(xbase.add(j).sub(t - 1)),
        );
        let x0x4_b = _mm256_add_ps(x0_b, x4_b);

        let mut yi_b = xi_b;
        yi_b = _mm256_fmadd_ps(g10v, x2_b, yi_b);
        yi_b = _mm256_fmadd_ps(g11v, x1x3_b, yi_b);
        yi_b = _mm256_fmadd_ps(g12v, x0x4_b, yi_b);
        _mm256_storeu_ps(ybase.add(j), yi_b);

        i += 16;
    }

    while i + 8 <= n {
        let xi = _mm256_loadu_ps(xbase.add(i));
        let x0 = _mm256_loadu_ps(xbase.add(i).sub(t + 2));
        let x4 = _mm256_loadu_ps(xbase.add(i).sub(t - 2));
        let x2 = _mm256_loadu_ps(xbase.add(i).sub(t));
        let x1x3 = _mm256_add_ps(
            _mm256_loadu_ps(xbase.add(i).sub(t + 1)),
            _mm256_loadu_ps(xbase.add(i).sub(t - 1)),
        );
        let x0x4 = _mm256_add_ps(x0, x4);

        let mut yi = xi;
        yi = _mm256_fmadd_ps(g10v, x2, yi);
        yi = _mm256_fmadd_ps(g11v, x1x3, yi);
        yi = _mm256_fmadd_ps(g12v, x0x4, yi);
        _mm256_storeu_ps(ybase.add(i), yi);

        i += 8;
    }

    if i + 4 <= n {
        comb_filter_const_sse_fma(y, x, y_idx + i, x_idx + i, t, n - i, g10, g11, g12);
        return;
    }

    let mut sx4 = x[x_idx + i - t - 2];
    let mut sx3 = x[x_idx + i - t - 1];
    let mut sx2 = x[x_idx + i - t];
    let mut sx1 = x[x_idx + i - t + 1];
    while i < n {
        let sx0 = x[x_idx + i - t + 2];
        y[y_idx + i] = x[x_idx + i] + g10 * sx2 + g11 * (sx1 + sx3) + g12 * (sx0 + sx4);
        sx4 = sx3;
        sx3 = sx2;
        sx2 = sx1;
        sx1 = sx0;
        i += 1;
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn comb_filter_const_sse_fma(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t: usize,
    n: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) {
    use std::arch::x86_64::*;

    let g10v = _mm_set1_ps(g10);
    let g11v = _mm_set1_ps(g11);
    let g12v = _mm_set1_ps(g12);

    // The taps reach two samples either side of the period, so the kernel
    // reads `x[x_idx - t - 2 .. x_idx + n]` and writes `y[y_idx .. y_idx + n]`.
    // Naming both spans is what keeps the pointer walk below inside the
    // buffers; nothing else here ties `n`, `t` and the indices to the lengths
    // this function was handed.
    assert!(
        t >= 2,
        "comb filter period {t} is shorter than the tap reach"
    );
    let xspan = &x[x_idx - t - 2..x_idx + n];
    let xbase = xspan.as_ptr().add(t + 2);
    let ybase = y[y_idx..y_idx + n].as_mut_ptr();
    let mut x0v = _mm_loadu_ps(xbase.sub(t + 2));

    let mut i = 0;
    while i + 4 <= n {
        let x4v = _mm_loadu_ps(xbase.add(i).sub(t - 2));
        let x2v = _mm_shuffle_ps(x0v, x4v, 0x4e);
        let x1v = _mm_shuffle_ps(x0v, x2v, 0x99);
        let x3v = _mm_shuffle_ps(x2v, x4v, 0x99);
        let xi = _mm_loadu_ps(xbase.add(i));

        let mut yi = xi;
        yi = _mm_fmadd_ps(g10v, x2v, yi);
        yi = _mm_fmadd_ps(g11v, _mm_add_ps(x1v, x3v), yi);
        yi = _mm_fmadd_ps(g12v, _mm_add_ps(x0v, x4v), yi);
        _mm_storeu_ps(ybase.add(i), yi);

        x0v = x4v;
        i += 4;
    }

    let x0v_arr: [f32; 4] = std::mem::transmute(x0v);
    let mut sx4 = x0v_arr[0];
    let mut sx3 = x0v_arr[1];
    let mut sx2 = x0v_arr[2];
    let mut sx1 = x0v_arr[3];
    while i < n {
        let sx0 = x[x_idx + i - t + 2];
        y[y_idx + i] = x[x_idx + i] + g10 * sx2 + g11 * (sx1 + sx3) + g12 * (sx0 + sx4);
        sx4 = sx3;
        sx3 = sx2;
        sx2 = sx1;
        sx1 = sx0;
        i += 1;
    }
}

fn comb_filter(
    y: &mut [f32],
    x: &[f32],
    y_idx: usize,
    x_idx: usize,
    t0: usize,
    t1: usize,
    n: usize,
    g0: f32,
    g1: f32,
    tapset0: i32,
    tapset1: i32,
    window: &[f32],
    overlap: usize,
) {
    if g0 == 0.0 && g1 == 0.0 {
        if x_idx != y_idx || !std::ptr::eq(x.as_ptr(), y.as_ptr()) {
            y[y_idx..y_idx + n].copy_from_slice(&x[x_idx..x_idx + n]);
        }
        return;
    }

    let t0 = t0.clamp(
        COMBFILTER_MINPERIOD,
        x_idx.saturating_sub(2).max(COMBFILTER_MINPERIOD),
    );
    let t1 = t1.clamp(
        COMBFILTER_MINPERIOD,
        x_idx.saturating_sub(2).max(COMBFILTER_MINPERIOD),
    );

    let g00 = g0 * PREFILTER_GAINS[tapset0 as usize][0];
    let g01 = g0 * PREFILTER_GAINS[tapset0 as usize][1];
    let g02 = g0 * PREFILTER_GAINS[tapset0 as usize][2];

    let g10 = g1 * PREFILTER_GAINS[tapset1 as usize][0];
    let g11 = g1 * PREFILTER_GAINS[tapset1 as usize][1];
    let g12 = g1 * PREFILTER_GAINS[tapset1 as usize][2];

    let mut x1 = x[x_idx - t1 + 1];
    let mut x2 = x[x_idx - t1];
    let mut x3 = x[x_idx - t1 - 1];
    let mut x4 = x[x_idx - t1 - 2];

    let mut inner_overlap = overlap;
    if g0 == g1 && t0 == t1 && tapset0 == tapset1 {
        inner_overlap = 0;
    }

    let mut i = 0;
    while i < inner_overlap && i < n {
        let x0 = x[x_idx + i - t1 + 2];
        let f = window[i] * window[i];
        y[y_idx + i] = x[x_idx + i]
            + (1.0 - f)
                * (g00 * x[x_idx + i - t0]
                    + g01 * (x[x_idx + i - t0 + 1] + x[x_idx + i - t0 - 1])
                    + g02 * (x[x_idx + i - t0 + 2] + x[x_idx + i - t0 - 2]))
            + f * (g10 * x2 + g11 * (x1 + x3) + g12 * (x0 + x4));

        x4 = x3;
        x3 = x2;
        x2 = x1;
        x1 = x0;
        i += 1;
    }

    if i < n {
        if g1 == 0.0 {
            y[y_idx + i..y_idx + n].copy_from_slice(&x[x_idx + i..x_idx + n]);
        } else {
            comb_filter_const(y, x, y_idx + i, x_idx + i, t1, n - i, g10, g11, g12);
        }
    }
}

/// libopus `prefilter_and_fold` (celt_decoder.c:576).
///
/// A concealed frame writes a whole MDCT window — frame plus overlap — into the
/// decode buffer and leaves the overlap tail *unfolded*, because it does not yet
/// know what the next frame will be. Whatever runs next, decode or concealment,
/// folds it first: the postfilter is undone over that tail (a negative gain,
/// since the decoder re-applies it after its own MDCT), and the result is
/// time-domain-alias-cancelled so it blends with the next frame's overlap-add
/// instead of stepping at the seam.
///
/// `n` is the *next* frame's length, and the tail sits at `DECODE_BUFFER_SIZE - n`
/// because the buffer has already been slid.
#[allow(clippy::too_many_arguments)]
fn prefilter_and_fold(
    decode_mem: &mut [f32],
    etmp: &mut [f32],
    channels: usize,
    mem_size: usize,
    n: usize,
    window: &[f32],
    overlap: usize,
    period_old: usize,
    period: usize,
    gain_old: f32,
    gain: f32,
    tapset_old: i32,
    tapset: i32,
) {
    for ch in 0..channels {
        let base = ch * mem_size + DECODE_BUFFER_SIZE - n;
        comb_filter(
            etmp,
            decode_mem,
            0,
            base,
            period_old,
            period,
            overlap,
            -gain_old,
            -gain,
            tapset_old,
            tapset,
            &[],
            0,
        );
        for i in 0..overlap / 2 {
            decode_mem[base + i] =
                window[i] * etmp[overlap - 1 - i] + window[overlap - 1 - i] * etmp[i];
        }
    }
}

/// Added to the input rather than to the running sum, which keeps it off the
/// de-emphasis recursion's dependency chain. The reference makes the same note.
const VERY_SMALL: f32 = 1e-30f32;
/// `celt_decoder.c`'s `SIG2RES` for a float build: CELT carries the signal at
/// `CELT_SIG_SCALE`, and the caller wants it at unity.
const SIG2RES: f32 = 1.0 / 32768.0;

/// One channel of `celt_decoder.c`'s `deemphasis`: the de-emphasis recursion,
/// and the downsampling that follows it.
///
/// The recursion runs over every sample at 48 kHz whatever the output rate, and
/// only every `downsample`-th result reaches the caller. Which results those are
/// is decided outside the recursion, because `i % downsample` on a divisor the
/// compiler cannot see is an integer division per sample. The reference splits
/// the same way, into a `downsample == 1` loop and a second pass that picks
/// samples out of a scratch buffer.
///
/// The reference wraps each sum in `SATURATE(.., SIG_SAT)`, which `arch.h`
/// defines as the identity for a float build; see [`deemphasis_stereo`] for why
/// transcribing it anyway was worse than useless here.
///
/// `out` receives `input.len() / downsample` samples spaced `stride` apart, so a
/// caller can fill a planar buffer with `stride == 1` or interleave directly
/// with `stride == channels`. Returns the filter memory for the next frame.
fn deemphasis(
    input: &[f32],
    out: &mut [f32],
    stride: usize,
    downsample: usize,
    coef: f32,
    mem: f32,
) -> f32 {
    let mut m = mem;
    if downsample == 1 {
        for (o, x) in out.iter_mut().step_by(stride).zip(input) {
            let val = *x + VERY_SMALL + m;
            m = val * coef;
            *o = val * SIG2RES;
        }
    } else {
        // The first sample of each group is the one the caller sees; the rest
        // only advance the filter. `chunks_exact` drops any partial group, as
        // the reference's `Nd = N/downsample` does.
        for (o, group) in out
            .iter_mut()
            .step_by(stride)
            .zip(input.chunks_exact(downsample))
        {
            let val = group[0] + VERY_SMALL + m;
            m = val * coef;
            *o = val * SIG2RES;
            for x in &group[1..] {
                m = (*x + VERY_SMALL + m) * coef;
            }
        }
    }
    m
}

/// Both channels of the de-emphasis recursion in one pass, at the output rate.
///
/// `celt_decoder.c`'s `deemphasis_stereo_simple`. Each output feeds the next
/// input, so one channel on its own runs at the latency of the filter rather
/// than the throughput of the machine, and the processor spends most of the loop
/// waiting. The two channels are independent, so interleaving them lets one fill
/// the other's stalls; that is the whole reason the reference keeps a separate
/// routine for the case, and stereo at the output rate is the common one.
///
/// The reference's `SATURATE(x, SIG_SAT)` is the identity in a float build
/// (`arch.h`), so it is absent here. Transcribing it cost more than the dead
/// work suggests: a `clamp` between the add and the multiply lands *on* the
/// recursion's dependency chain and roughly doubles its length. Measured
/// against libopus on fullband stereo, dropping it alone took 3.5 us off a
/// 47.9 us frame.
fn deemphasis_stereo(left: &[f32], right: &[f32], out: &mut [f32], coef: f32, mem: &mut [f32]) {
    let (mut m0, mut m1) = (mem[0], mem[1]);
    for ((frame, x0), x1) in out.as_chunks_mut::<2>().0.iter_mut().zip(left).zip(right) {
        let t0 = *x0 + VERY_SMALL + m0;
        let t1 = *x1 + VERY_SMALL + m1;
        m0 = t0 * coef;
        m1 = t1 * coef;
        frame[0] = t0 * SIG2RES;
        frame[1] = t1 * SIG2RES;
    }
    mem[0] = m0;
    mem[1] = m1;
}

/// In-place comb filter: buf[y_idx..y_idx+n] is both input and output.
/// Reference samples at buf[y_idx + i - T + offset] may already be filtered
/// if T < i, matching C libopus's in-place comb_filter(out, out, ...) behavior.
fn comb_filter_inplace(
    buf: &mut [f32],
    y_idx: usize,
    t0: usize,
    t1: usize,
    n: usize,
    g0: f32,
    g1: f32,
    tapset0: i32,
    tapset1: i32,
    window: &[f32],
    overlap: usize,
) {
    if g0 == 0.0 && g1 == 0.0 {
        // nothing to do; buf[y_idx..] already holds the input
        return;
    }

    let t0 = t0.clamp(COMBFILTER_MINPERIOD, y_idx - 2);
    let t1 = t1.clamp(COMBFILTER_MINPERIOD, y_idx - 2);

    let g00 = g0 * PREFILTER_GAINS[tapset0 as usize][0];
    let g01 = g0 * PREFILTER_GAINS[tapset0 as usize][1];
    let g02 = g0 * PREFILTER_GAINS[tapset0 as usize][2];

    let g10 = g1 * PREFILTER_GAINS[tapset1 as usize][0];
    let g11 = g1 * PREFILTER_GAINS[tapset1 as usize][1];
    let g12 = g1 * PREFILTER_GAINS[tapset1 as usize][2];

    let mut inner_overlap = overlap;
    if g0 == g1 && t0 == t1 && tapset0 == tapset1 {
        inner_overlap = 0;
    }

    let mut i = 0;
    while i < inner_overlap && i < n {
        let idx = y_idx + i;
        let f = window[i] * window[i];
        let s = buf[idx]; // original input (not yet overwritten at idx)
        let r0 = buf[idx - t0];
        let r0p1 = buf[idx - t0 + 1];
        let r0m1 = buf[idx - t0 - 1];
        let r0p2 = buf[idx - t0 + 2];
        let r0m2 = buf[idx - t0 - 2];
        let r1 = buf[idx - t1];
        let r1p1 = buf[idx - t1 + 1];
        let r1m1 = buf[idx - t1 - 1];
        let r1p2 = buf[idx - t1 + 2];
        let r1m2 = buf[idx - t1 - 2];
        buf[idx] = s
            + (1.0 - f) * (g00 * r0 + g01 * (r0p1 + r0m1) + g02 * (r0p2 + r0m2))
            + f * (g10 * r1 + g11 * (r1p1 + r1m1) + g12 * (r1p2 + r1m2));
        i += 1;
    }

    // Constant region: only new filter (t1, g1). The feedback delay t1 >=
    // COMBFILTER_MINPERIOD (15) >= 10, so an 8-wide vector at [idx, idx+8) never
    // reads its own writes: the batch's read span [idx-t1-2, idx-t1+9] is disjoint
    // from the write span [idx, idx+8) iff t1 >= 10 — the past outputs it reads are
    // already finalized, exactly as the scalar loop sees them.
    #[cfg(target_arch = "x86_64")]
    {
        if i + 8 <= n && t1 >= 10 && std::arch::is_x86_feature_detected!("avx2") {
            unsafe {
                i = comb_filter_const_avx2(buf, y_idx, i, n, t1, g10, g11, g12);
            }
        }
    }
    while i < n {
        let idx = y_idx + i;
        let s = buf[idx];
        let r1 = buf[idx - t1];
        let r1p1 = buf[idx - t1 + 1];
        let r1m1 = buf[idx - t1 - 1];
        let r1p2 = buf[idx - t1 + 2];
        let r1m2 = buf[idx - t1 - 2];
        buf[idx] = s + g10 * r1 + g11 * (r1p1 + r1m1) + g12 * (r1p2 + r1m2);
        i += 1;
    }
}

/// AVX2 comb-filter constant region: 8 samples/iter, bit-exact vs the scalar
/// tail below. Uses separate mul+add (NOT FMA) in the scalar op order
/// `s + g10*r1 + g11*(r1p1+r1m1) + g12*(r1p2+r1m2)` so every rounding matches.
/// Requires `t1 >= 10` (the batch reads [idx-t1-2, idx-t1+9] stay clear of the
/// [idx, idx+8) writes). Returns the index `i` where the scalar tail resumes.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn comb_filter_const_avx2(
    buf: &mut [f32],
    y_idx: usize,
    mut i: usize,
    n: usize,
    t1: usize,
    g10: f32,
    g11: f32,
    g12: f32,
) -> usize {
    use std::arch::x86_64::*;
    let vg10 = _mm256_set1_ps(g10);
    let vg11 = _mm256_set1_ps(g11);
    let vg12 = _mm256_set1_ps(g12);
    // Reads reach back to `buf[y_idx + i - t1 - 2]` and forward to the taps at
    // `buf[y_idx + i - t1 + 9]`; writes end at `buf[y_idx + n]`. The loads and
    // stores below use absolute offsets into `buf`, so this is the only place
    // those bounds are stated.
    assert!(
        t1 >= 2 && y_idx + i >= t1 + 2 && y_idx + n <= buf.len(),
        "comb filter batch runs outside its buffer"
    );
    let p = buf.as_mut_ptr();
    while i + 8 <= n {
        let idx = y_idx + i;
        let base = idx - t1; // >= 2 (t1 <= idx-2, and idx >= y_idx >= t1+2)
        let s = _mm256_loadu_ps(p.add(idx));
        let r1 = _mm256_loadu_ps(p.add(base));
        let r1p1 = _mm256_loadu_ps(p.add(base + 1));
        let r1m1 = _mm256_loadu_ps(p.add(base - 1));
        let r1p2 = _mm256_loadu_ps(p.add(base + 2));
        let r1m2 = _mm256_loadu_ps(p.add(base - 2));
        let a = _mm256_add_ps(r1p1, r1m1);
        let b = _mm256_add_ps(r1p2, r1m2);
        // out = ((s + g10*r1) + g11*a) + g12*b  (left-to-right, two-rounding, no FMA)
        let mut out = _mm256_add_ps(s, _mm256_mul_ps(vg10, r1));
        out = _mm256_add_ps(out, _mm256_mul_ps(vg11, a));
        out = _mm256_add_ps(out, _mm256_mul_ps(vg12, b));
        _mm256_storeu_ps(p.add(idx), out);
        i += 8;
    }
    i
}

fn run_prefilter(
    in_buf: &mut [f32],
    prefilter_mem: &mut [f32],
    prefilter_period: usize,
    prefilter_gain: f32,
    prefilter_tapset: i32,
    tapset_decision: i32,
    window: &[f32],
    channels: usize,
    frame_size: usize,
    overlap: usize,

    pre: &mut [f32],
    pitch_buf: &mut [f32],

    analysis: &AnalysisInfo,
    loss_rate: i32,
    nb_available_bytes: i32,
) -> (bool, f32, usize) {
    let max_period = COMBFILTER_MAXPERIOD;
    let min_period = COMBFILTER_MINPERIOD;
    let buf_stride = frame_size + overlap;
    let pre_size = max_period + frame_size;

    for c in 0..channels {
        pre[c * pre_size..c * pre_size + max_period]
            .copy_from_slice(&prefilter_mem[c * max_period..(c + 1) * max_period]);
        pre[c * pre_size + max_period..c * pre_size + pre_size].copy_from_slice(
            &in_buf[c * buf_stride + overlap..c * buf_stride + overlap + frame_size],
        );
    }

    let pitch_buf_len = (max_period + frame_size) >> 1;
    {
        let pre_slices: Vec<&[f32]> = (0..channels)
            .map(|c| &pre[c * pre_size..c * pre_size + pre_size])
            .collect();
        crate::celt::pitch::pitch_downsample(&pre_slices, pitch_buf, pitch_buf_len, channels, 2);
    }

    let search_max = max_period - 3 * min_period;
    let pitch_result = crate::celt::pitch::pitch_search(
        &pitch_buf[max_period >> 1..],
        pitch_buf,
        frame_size,
        search_max,
    );
    let mut pitch_index = (max_period - pitch_result).min(max_period - 2);

    let gain1_raw = crate::celt::pitch::remove_doubling(
        pitch_buf,
        max_period,
        min_period,
        frame_size,
        &mut pitch_index,
        prefilter_period,
        prefilter_gain,
    );
    let mut gain1 = gain1_raw * 0.7;

    // Loss-rate ladder (matches celt_encoder.c: halve >2%, halve again >4%,
    // zero >8%).
    if loss_rate > 2 {
        gain1 *= 0.5;
    }
    if loss_rate > 4 {
        gain1 *= 0.5;
    }
    if loss_rate > 8 {
        gain1 = 0.0;
    }

    // Apply max_pitch_ratio from analysis if available
    if analysis.valid {
        gain1 *= analysis.max_pitch_ratio;
    }

    let mut pf_threshold = 0.2f32;
    if (pitch_index as i32 - prefilter_period as i32).unsigned_abs() as usize * 10 > pitch_index {
        pf_threshold += 0.2;
    }
    // Rate-based bumps (celt_encoder.c): the ~7 pf bits are not worth it on
    // starved frames.
    if nb_available_bytes < 25 {
        pf_threshold += 0.1;
    }
    if nb_available_bytes < 35 {
        pf_threshold += 0.1;
    }
    if prefilter_gain > 0.4 {
        pf_threshold -= 0.1;
    }
    if prefilter_gain > 0.55 {
        pf_threshold -= 0.1;
    }
    pf_threshold = pf_threshold.max(0.2);

    let pf_on;
    if gain1 < pf_threshold {
        gain1 = 0.0;
        pf_on = false;
    } else {
        if (gain1 - prefilter_gain).abs() < 0.1 {
            gain1 = prefilter_gain;
        }
        let qg = ((gain1 * 32.0 / 3.0 + 0.5).floor() as i32 - 1).clamp(0, 7);
        gain1 = 0.09375 * (qg + 1) as f32;
        pf_on = true;
    }

    // Standard Opus modes have shortMdctSize == overlap (120), so C's
    // `offset = mode->shortMdctSize - overlap` is always 0 here.
    let offset = 0usize;
    let prev_period = prefilter_period.clamp(COMBFILTER_MINPERIOD, max_period - 2);

    for c in 0..channels {
        if offset > 0 {
            let pre_c = &pre[c * pre_size..];
            comb_filter(
                in_buf,
                pre_c,
                c * buf_stride + overlap,
                max_period,
                prev_period,
                prev_period,
                offset,
                -prefilter_gain,
                -prefilter_gain,
                prefilter_tapset,
                prefilter_tapset,
                window,
                0,
            );
        }

        {
            let pre_c = &pre[c * pre_size..];
            comb_filter(
                in_buf,
                pre_c,
                c * buf_stride + overlap + offset,
                max_period + offset,
                prev_period,
                pitch_index,
                frame_size - offset,
                -prefilter_gain,
                -gain1,
                prefilter_tapset,
                tapset_decision,
                window,
                overlap,
            );
        }
    }

    for c in 0..channels {
        if frame_size >= max_period {
            prefilter_mem[c * max_period..(c + 1) * max_period].copy_from_slice(
                &pre[c * pre_size + frame_size..c * pre_size + frame_size + max_period],
            );
        } else {
            let shift = max_period - frame_size;
            prefilter_mem.copy_within(
                c * max_period + frame_size..(c + 1) * max_period,
                c * max_period,
            );
            prefilter_mem[c * max_period + shift..(c + 1) * max_period].copy_from_slice(
                &pre[c * pre_size + max_period..c * pre_size + max_period + frame_size],
            );
        }
    }

    (pf_on, gain1, pitch_index)
}

const STRIDE_ACCESS_PAD: usize = crate::celt::pvq::MAX_PVQ_N * 8;

/// libopus celt_encoder.c `compute_vbr` (float build), minus the pieces that
/// need the tonality analysis / surround masking / LFE / temporal-VBR inputs we
/// don't compute (their boosts are quality refinements, not conformance).
/// All quantities in eighth-bits per frame.
fn compute_vbr_target(
    mode: &CeltMode,
    base_target: i32,
    lm: i32,
    last_coded_bands: i32,
    channels: i32,
    intensity: i32,
    constrained_vbr: bool,
    stereo_saving: f32,
    tot_boost: i32,
    tf_estimate: f32,
    max_depth: f32,
) -> i32 {
    let nb_ebands = mode.nb_ebands as i32;
    let e_bands = mode.e_bands;
    let coded_bands = if last_coded_bands != 0 {
        last_coded_bands
    } else {
        nb_ebands
    };
    let mut coded_bins = (e_bands[coded_bands as usize] as i32) << lm;
    if channels == 2 {
        coded_bins += (e_bands[intensity.min(coded_bands) as usize] as i32) << lm;
    }

    let mut target = base_target;

    // Stereo savings.
    if channels == 2 {
        let coded_stereo_bands = intensity.min(coded_bands);
        let coded_stereo_dof =
            ((e_bands[coded_stereo_bands as usize] as i32) << lm) - coded_stereo_bands;
        // Maximum fraction of the bits we could save if the signal were mono.
        let max_frac = 0.8f32 * coded_stereo_dof as f32 / coded_bins as f32;
        let ss = stereo_saving.min(1.0);
        target -= ((max_frac * target as f32) as i32)
            .min(((ss - 0.1) * ((coded_stereo_dof << BITRES) as f32)) as i32);
    }
    // Boost according to dynalloc (minus the average for calibration).
    target += tot_boost - (19 << lm);
    // Transient boost, compensating for the average.
    let tf_calibration = 0.044f32;
    target += (2.0 * (tf_estimate - tf_calibration) * target as f32) as i32;

    // Don't allocate more than 8 bits above the "depth" of the signal.
    {
        let bins = (e_bands[nb_ebands as usize - 2] as i32) << lm;
        let mut floor_depth = (((channels * bins) << BITRES) as f32 * max_depth) as i32;
        floor_depth = floor_depth.max(target >> 2);
        target = target.min(floor_depth);
    }

    // Constrained VBR can't sustain large swings.
    if constrained_vbr {
        target = base_target + (0.67 * (target - base_target) as f32) as i32;
    }

    // Never more than double the base rate.
    target.min(2 * base_target)
}

/// Fix up the spectrum of a zero-stuffed input (`upsample > 1`).
///
/// Inserting zeros to reach 48 kHz keeps the baseband shape but leaves it at
/// `1/upsample` of its amplitude once the mirror image is removed, and that
/// image sits in every band above the input's Nyquist rate. Scaling the coded
/// bands up and zeroing the rest fixes both, and is what libopus does at the end
/// of `compute_mdcts`. Without it a 24 kHz encode decodes at half amplitude.
fn scale_spectrum_for_upsample(
    freq: &mut [f32],
    frame_size: usize,
    channels: usize,
    upsample: usize,
) {
    if upsample == 1 {
        return;
    }
    let bound = frame_size / upsample;
    let scale = upsample as f32;
    for c in 0..channels {
        let base = c * frame_size;
        for v in freq[base + bound..base + frame_size].iter_mut() {
            *v = 0.0;
        }
        for v in freq[base..base + bound].iter_mut() {
            *v *= scale;
        }
    }
}

pub struct CeltEncoder {
    mode: &'static CeltMode,
    channels: usize,
    /// 48 kHz output samples per input sample: 1 at 48 kHz, 2 at 24, 3 at 16,
    /// 4 at 12, 6 at 8. CELT only has the 48 kHz mode, so a lower API rate is
    /// coded by zero-stuffing the input up to 48 kHz and never coding the bands
    /// above the input's Nyquist rate — what libopus does via `st->upsample`.
    upsample: usize,
    pub complexity: i32,
    syn_mem: Vec<f32>,
    old_band_e: Vec<f32>,
    preemph_mem: Vec<f32>,
    tonal_average: i32,
    hf_average: i32,
    tapset_decision: i32,
    spread_decision: i32,
    intensity: i32,
    last_coded_bands: i32,
    /// Input bit depth for the dynalloc noise floors (opus lsb_depth).
    pub lsb_depth: i32,
    /// VBR target in eighth-bits per frame (0 = hard CBR). libopus vbr_rate.
    pub vbr_rate: i32,
    /// Constrained VBR (libopus default): reservoir-limited drift around target.
    pub constrained_vbr: bool,
    /// What SILK coded in the low band of this hybrid frame (libopus `SILKInfo`,
    /// handed over by `CELT_SET_SILK_INFO`). The high band's rate and its
    /// temporal resolution both key off it, so a hybrid frame coded without it
    /// is allocating blind. Ignored outside hybrid, where libopus does not set it.
    pub silk_signal_type: i32,
    /// SILK's quantization offset for this frame. Small means a tonal frame,
    /// which needs more bits in the high band than a noisy one.
    pub silk_offset: i32,
    vbr_reservoir: i32,
    vbr_drift: i32,
    vbr_offset: i32,
    vbr_count: i32,
    prefilter_mem: Vec<f32>,
    prefilter_period: usize,
    prefilter_gain: f32,
    prefilter_tapset: i32,
    old_band_e2: Vec<f32>,
    delayed_intra: f32,

    w_in_buf: Vec<f32>,
    w_freq: Vec<f32>,
    w_band_e: Vec<f32>,
    w_x: Vec<f32>,
    w_band_log_e: Vec<f32>,
    w_band_log_e2: Vec<f32>,
    w_error: Vec<f32>,
    w_tf_res: Vec<i32>,
    w_cap: Vec<i32>,
    w_offsets: Vec<i32>,
    w_pulses: Vec<i32>,
    w_ebits: Vec<i32>,
    w_fine_priority: Vec<i32>,
    w_collapse_masks: Vec<u32>,
    consec_transient: i32,

    w_prefilter_pre: Vec<f32>,
    w_prefilter_pitch_buf: Vec<f32>,

    w_transient_tmp: Vec<f32>,
    w_transient_tmp2: Vec<f32>,

    pub(crate) analysis: AnalysisInfo,
    /// Expected packet loss %, plumbed from `OpusEncoder.packet_loss_perc`
    /// each frame (was never assigned — census 2026-08-07). Drives the
    /// prefilter loss ladder and coarse-energy intra bias.
    pub(crate) loss_rate: i32,
    /// Enable libopus's tonality VBR boost in `compute_vbr_target`
    /// Peak |sample| of the previous frame's overlap tail — the `st->overlap_max`
    /// of celt_encoder.c, needed so silence is only declared once the region the
    /// MDCT folds is silent as well.
    overlap_max: f32,
}

const INTEN_THRESHOLDS: [i32; 21] = [
    1, 2, 3, 4, 5, 6, 7, 8, 16, 24, 36, 44, 50, 56, 62, 67, 72, 79, 88, 106, 134,
];
const INTEN_HYSTERESIS: [i32; 21] = [
    1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 3, 3, 4, 5, 6, 8, 8,
];

fn hysteresis_decision(val: i32, thresholds: &[i32], hysteresis: &[i32], prev: i32) -> i32 {
    let mut i = 0;
    while i < thresholds.len() {
        if val < thresholds[i] {
            break;
        }
        i += 1;
    }
    let mut res = i as i32;
    if res > prev && val < thresholds[prev as usize] + hysteresis[prev as usize] {
        res = prev;
    }
    if res < prev && res > 0 && val > thresholds[prev as usize - 1] - hysteresis[prev as usize - 1]
    {
        res = prev;
    }
    res
}

fn alloc_trim_analysis(
    mode: &CeltMode,
    x: &[f32],
    band_log_e: &[f32],
    end: usize,
    lm: i32,
    channels: usize,
    n0: usize,
    stereo_saving: &mut f32,
    tf_estimate: f32,
    intensity: i32,
    surround_trim: f32,
    equiv_rate: i32,
) -> i32 {
    let mut trim = 5.0f32;
    if equiv_rate < 64000 {
        trim = 4.0;
    } else if equiv_rate < 80000 {
        let frac = (equiv_rate - 64000) as f32 / 1024.0;
        trim = 4.0 + (1.0 / 16.0) * frac;
    }

    if channels == 2 {
        let mut sum = 0.0f32;
        for i in 0..8 {
            let offset = (mode.e_bands[i] as usize) << lm;
            let n = ((mode.e_bands[i + 1] - mode.e_bands[i]) as usize) << lm;
            let mut partial = 0.0f32;
            for j in 0..n {
                partial += x[offset + j] * x[n0 + offset + j];
            }
            sum += partial;
        }
        sum = (sum / 8.0).abs().min(1.0);
        let mut min_xc = sum;
        for i in 8..intensity as usize {
            let offset = (mode.e_bands[i] as usize) << lm;
            let n = ((mode.e_bands[i + 1] - mode.e_bands[i]) as usize) << lm;
            let mut partial = 0.0f32;
            for j in 0..n {
                partial += x[offset + j] * x[n0 + offset + j];
            }
            min_xc = min_xc.min(partial.abs());
        }
        min_xc = min_xc.min(1.0);

        let log_xc = (1.001 - sum * sum).log2();
        let log_xc2 = (log_xc * 0.5).max((1.001 - min_xc * min_xc).log2());

        trim += (-4.0f32).max(0.75 * log_xc);
        *stereo_saving = (*stereo_saving + 0.25).min(-0.5 * log_xc2);
    }

    let mut diff = 0.0f32;
    for c in 0..channels {
        for i in 0..end - 1 {
            diff += band_log_e[c * mode.nb_ebands + i] * (2 + 2 * i as i32 - end as i32) as f32;
        }
    }
    diff /= (channels * (end - 1)) as f32;
    trim -= (-2.0f32).max(2.0f32.min((diff + 1.0) / 6.0));
    trim -= surround_trim;
    trim -= 2.0 * tf_estimate;

    // Stereo-music LF tilt (PEAQ-tuned). Our per-output analysis lands the trim
    // slightly lower than is perceptually ideal for coupled stereo music — tilting
    // a little more toward LF (where our coding is strongest) recovers ~0.03–0.10
    // ODG on stereo music across 64–192 kbps with no regressions (a +2 tilt was
    // stronger at mid rates but starved HF at 64k under VBR rate overlap; +1 is the
    // safe, monotonic choice). Mono is untouched, and trim is transmitted so
    // encoder/decoder stay in sync — fully conformant.
    if channels == 2 {
        trim += 1.0;
    }

    let trim_index = (trim + 0.5).floor() as i32;
    trim_index.clamp(0, 10)
}

#[inline(always)]
fn median3(a: f32, b: f32, c: f32) -> f32 {
    let mut v = [a, b, c];
    v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    v[1]
}

#[inline(always)]
fn median5(v: &[f32]) -> f32 {
    let mut x = [v[0], v[1], v[2], v[3], v[4]];
    x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    x[2]
}

/// Full port of celt_encoder.c dynalloc_analysis: per-band boosts (offsets),
/// the tf importance weights, the spreading-decision SMR weights, and maxDepth
/// (the signal depth over the noise floor, used as the VBR ceiling). Consumes
/// the pre-transient band logs (band_log_e2) and the analysis leak_boost.
fn dynalloc_analysis(
    mode: &CeltMode,
    band_log_e: &[f32],
    band_log_e2: &[f32],
    start: usize,
    end: usize,
    channels: usize,
    offsets: &mut [i32],
    lsb_depth: i32,
    is_transient: bool,
    vbr: bool,
    constrained_vbr: bool,
    lm: usize,
    effective_bytes: usize,
    analysis: &AnalysisInfo,
    importance: &mut [f32],
    spread_weight: &mut [i32],
) -> f32 {
    let nb = mode.nb_ebands;
    offsets.fill(0);

    // Noise floor: eMeans, depth, band width (logN) and the preemphasis tilt
    // (~ square of the bark band index).
    let mut noise_floor = [0.0f32; MAX_NB_EBANDS];
    for i in 0..end {
        noise_floor[i] = 0.0625 * mode.log_n[i] as f32 + 0.5 + (9 - lsb_depth) as f32
            - mode.e_means[i]
            + 0.0062 * ((i + 5) * (i + 5)) as f32;
    }
    let mut max_depth = -31.9f32;
    for c in 0..channels {
        for i in 0..end {
            max_depth = max_depth.max(band_log_e[c * nb + i] - noise_floor[i]);
        }
    }

    // Simple masking model for the spreading decision: ignore fully masked bands.
    {
        let mut mask = [0.0f32; MAX_NB_EBANDS];
        let mut sig = [0.0f32; MAX_NB_EBANDS];
        for i in 0..end {
            mask[i] = band_log_e[i] - noise_floor[i];
        }
        if channels == 2 {
            for i in 0..end {
                mask[i] = mask[i].max(band_log_e[nb + i] - noise_floor[i]);
            }
        }
        sig[..end].copy_from_slice(&mask[..end]);
        for i in 1..end {
            mask[i] = mask[i].max(mask[i - 1] - 2.0);
        }
        for i in (0..end.saturating_sub(1)).rev() {
            mask[i] = mask[i].max(mask[i + 1] - 3.0);
        }
        for i in 0..end {
            // SMR: mask never more than 72 dB below the peak, never below floor.
            let smr = sig[i] - (0.0f32.max(max_depth - 12.0)).max(mask[i]);
            let shift = 5.min(0.max(-((0.5 + smr).floor() as i32)));
            spread_weight[i] = 32 >> shift;
        }
    }

    // Make sure dynamic allocation can't bust the budget.
    if effective_bytes > 50 && lm >= 1 {
        let mut follower = [0.0f32; 2 * MAX_NB_EBANDS];
        let mut last = 0usize;
        for c in 0..channels {
            let base = c * nb;
            follower[base] = band_log_e2[base];
            for i in 1..end {
                // The last band at least .5 dB higher than the previous one is
                // the last we'll consider (band-limited signals).
                if band_log_e2[base + i] > band_log_e2[base + i - 1] + 0.5 {
                    last = i;
                }
                follower[base + i] = (follower[base + i - 1] + 1.5).min(band_log_e2[base + i]);
            }
            for i in (0..last).rev() {
                follower[base + i] = follower[base + i]
                    .min((follower[base + i + 1] + 2.0).min(band_log_e2[base + i]));
            }

            // Median filter so dynalloc doesn't trigger unnecessarily.
            let offset = 1.0f32;
            if end >= 5 {
                for i in 2..end - 2 {
                    follower[base + i] = follower[base + i]
                        .max(median5(&band_log_e2[base + i - 2..base + i + 3]) - offset);
                }
            }
            if end >= 3 {
                let tmp = median3(
                    band_log_e2[base],
                    band_log_e2[base + 1],
                    band_log_e2[base + 2],
                ) - offset;
                follower[base] = follower[base].max(tmp);
                follower[base + 1] = follower[base + 1].max(tmp);
                let tmp = median3(
                    band_log_e2[base + end - 3],
                    band_log_e2[base + end - 2],
                    band_log_e2[base + end - 1],
                ) - offset;
                follower[base + end - 2] = follower[base + end - 2].max(tmp);
                follower[base + end - 1] = follower[base + end - 1].max(tmp);
            }

            for i in 0..end {
                follower[base + i] = follower[base + i].max(noise_floor[i]);
            }
        }
        if channels == 2 {
            for i in start..end {
                // Consider 24 dB "cross-talk".
                follower[nb + i] = follower[nb + i].max(follower[i] - 4.0);
                follower[i] = follower[i].max(follower[nb + i] - 4.0);
                follower[i] = 0.5
                    * ((band_log_e[i] - follower[i]).max(0.0)
                        + (band_log_e[nb + i] - follower[nb + i]).max(0.0));
            }
        } else {
            for i in start..end {
                follower[i] = (band_log_e[i] - follower[i]).max(0.0);
            }
        }
        for i in start..end {
            importance[i] = (0.5 + 13.0 * (follower[i].min(4.0)).exp2()).floor();
        }
        // For non-transient CBR/CVBR frames, halve the dynalloc contribution.
        if (!vbr || constrained_vbr) && !is_transient {
            for f in follower.iter_mut().take(end).skip(start) {
                *f *= 0.5;
            }
        }
        for i in start..end {
            if i < 8 {
                follower[i] *= 2.0;
            }
            if i >= 12 {
                follower[i] *= 0.5;
            }
        }
        if analysis.valid {
            for i in start..end.min(19) {
                follower[i] += analysis.leak_boost[i] as f32 * (1.0 / 64.0);
            }
        }
        let mut tot_boost = 0i32;
        for i in start..end {
            follower[i] = follower[i].min(4.0);

            let width =
                channels as i32 * (mode.e_bands[i + 1] - mode.e_bands[i]) as i32 * (1 << lm);
            let (boost, boost_bits) = if width < 6 {
                let b = follower[i] as i32;
                (b, (b * width) << BITRES)
            } else if width > 48 {
                let b = (follower[i] * 8.0) as i32;
                (b, ((b * width) << BITRES) / 8)
            } else {
                let b = (follower[i] * width as f32 / 6.0) as i32;
                (b, (b * 6) << BITRES)
            };
            // For CBR and non-transient CVBR frames, limit dynalloc to 2/3 of
            // the bits.
            if (!vbr || (constrained_vbr && !is_transient))
                && ((tot_boost + boost_bits) >> BITRES >> 3) > 2 * effective_bytes as i32 / 3
            {
                let cap = (2 * effective_bytes as i32 / 3) << BITRES << 3;
                offsets[i] = cap - tot_boost;
                break;
            } else {
                offsets[i] = boost;
                tot_boost += boost_bits;
            }
        }
    } else {
        for i in start..end {
            importance[i] = 13.0;
        }
    }
    max_depth
}

impl CeltEncoder {
    pub fn new(mode: &'static CeltMode, channels: usize) -> Self {
        let overlap = mode.overlap;
        let channel_mem_size = 2048 + overlap;
        let syn_mem_size = channels * channel_mem_size;
        let nb_ebands = mode.nb_ebands;
        let nb_x_ch = nb_ebands * channels;
        let frame_x_ch = MAX_FRAME_SIZE * channels;
        let bufstride_x_ch = (MAX_FRAME_SIZE + overlap) * channels;
        Self {
            mode,
            channels,
            complexity: 9,
            syn_mem: vec![0.0; syn_mem_size],
            old_band_e: vec![0.0; nb_x_ch],
            upsample: 1,
            preemph_mem: vec![0.0; channels],
            tonal_average: 256,
            hf_average: 0,
            tapset_decision: 0,
            spread_decision: SPREAD_NORMAL,
            intensity: 0,
            last_coded_bands: 0,
            lsb_depth: 24,
            vbr_rate: 0,
            constrained_vbr: true,
            silk_signal_type: 0,
            silk_offset: 0,
            vbr_reservoir: 0,
            vbr_drift: 0,
            vbr_offset: 0,
            vbr_count: 0,
            prefilter_mem: vec![0.0; channels * COMBFILTER_MAXPERIOD],
            prefilter_period: COMBFILTER_MINPERIOD,
            prefilter_gain: 0.0,
            prefilter_tapset: 0,
            old_band_e2: vec![0.0; nb_x_ch],
            delayed_intra: 0.0,

            w_in_buf: vec![0.0; bufstride_x_ch],
            w_freq: vec![0.0; frame_x_ch + 4],
            w_band_e: vec![0.0; nb_x_ch],

            w_x: vec![0.0; frame_x_ch + STRIDE_ACCESS_PAD],
            w_band_log_e: vec![0.0; nb_x_ch],
            w_band_log_e2: vec![0.0; nb_x_ch],
            w_error: vec![0.0; nb_x_ch],
            w_tf_res: vec![0; nb_ebands],
            w_cap: vec![0; nb_ebands],
            w_offsets: vec![0; nb_ebands],
            w_pulses: vec![0; nb_ebands],
            w_ebits: vec![0; nb_x_ch],
            w_fine_priority: vec![0; nb_x_ch],
            w_collapse_masks: vec![0; nb_x_ch],

            w_prefilter_pre: vec![0.0; channels * (COMBFILTER_MAXPERIOD + MAX_FRAME_SIZE)],
            w_prefilter_pitch_buf: vec![0.0; (COMBFILTER_MAXPERIOD + MAX_FRAME_SIZE) >> 1],
            w_transient_tmp: vec![0.0; MAX_TRANSIENT_LEN],
            w_transient_tmp2: vec![0.0; MAX_TRANSIENT_LEN / 2],
            consec_transient: 0,

            analysis: AnalysisInfo::default(),
            loss_rate: 0,
            overlap_max: 0.0,
        }
    }

    /// Set how many 48 kHz samples each input sample stands for: 1 at a
    /// 48 kHz API rate, 2 at 24 kHz, 3 at 16 kHz, 4 at 12 kHz, 6 at 8 kHz.
    /// `frame_size` keeps counting samples at the caller's rate; CELT widens it
    /// internally.
    pub fn set_upsample(&mut self, upsample: usize) {
        self.upsample = upsample.max(1);
    }

    pub fn encode_with_budget(
        &mut self,
        pcm: &[f32],
        frame_size: usize,
        rc: &mut RangeCoder,
        start_band: usize,
        end_band: usize,
        total_bits: i32,
    ) {
        self.encode_impl(pcm, frame_size, rc, start_band, end_band, Some(total_bits))
    }

    fn encode_impl(
        &mut self,
        pcm: &[f32],
        frame_size: usize,
        rc: &mut RangeCoder,
        start_band: usize,
        end_band: usize,
        explicit_total_bits: Option<i32>,
    ) {
        debug_assert!(end_band > start_band && end_band <= self.mode.nb_ebands);
        let mode = self.mode;
        let channels = self.channels;
        let nb_ebands = mode.nb_ebands;

        // ---- Digital-silence detection (celt_encoder.c) ----
        // sample_max spans this frame's non-overlap part PLUS the previous
        // frame's overlap tail, so a frame is only "silent" once the region the
        // MDCT will actually fold is silent too. Coding the silence flag is what
        // keeps digital silence at ~3% of the active-frame rate instead of ~69%.
        // The caller counts samples at its own rate; CELT only has the 48 kHz
        // mode, so widen the frame here and zero-stuff the input into it.
        let upsample = self.upsample;
        let in_len = frame_size;
        let frame_size = frame_size * upsample;
        let silence = {
            let ovl = (mode.overlap / upsample).min(in_len);
            let head = (in_len - ovl) * channels;
            let maxabs = |s: &[f32]| s.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
            let n = (in_len * channels).min(pcm.len());
            let head_max = maxabs(&pcm[..head.min(n)]);
            let tail_max = maxabs(&pcm[head.min(n)..n]);
            let sample_max = self.overlap_max.max(head_max).max(tail_max);
            self.overlap_max = tail_max;
            sample_max <= 1.0 / (1i64 << self.lsb_depth) as f32
        };
        let overlap = mode.overlap;
        // Bits already in the coder at entry (the SILK part in hybrid mode) — used
        // by the VBR min-size guard so shrinking never truncates them.
        let tell0_frac = rc.tell_frac();

        let mut lm = 0;
        while (mode.short_mdct_size << lm) != frame_size {
            lm += 1;
            if lm > mode.max_lm {
                break;
            }
        }
        if (mode.short_mdct_size << lm) != frame_size {
            lm = 0;
        }

        let syn_mem_size = 2048 + overlap;
        for c in 0..channels {
            let channel_offset = c * syn_mem_size;

            self.syn_mem.copy_within(
                channel_offset + frame_size..channel_offset + syn_mem_size,
                channel_offset,
            );

            let mut m = self.preemph_mem[c];
            let coef = mode.preemph[0];
            for i in 0..frame_size {
                // Zero-stuff to 48 kHz: every `upsample`-th slot carries an
                // input sample, the rest are zero. The preemphasis recursion
                // then runs over all 48 kHz samples, as in libopus.
                let x = if i % upsample == 0 {
                    pcm[c * in_len + i / upsample] * 32768.0
                } else {
                    0.0
                };
                let val = x - m;
                self.syn_mem[channel_offset + syn_mem_size - frame_size + i] = val;
                m = x * coef;
            }
            self.preemph_mem[c] = m;
        }

        let buf_stride = frame_size + overlap;
        let in_buf = &mut self.w_in_buf[..buf_stride * channels];
        for c in 0..channels {
            let channel_offset = c * syn_mem_size;
            let in_buf_offset = c * buf_stride;

            let src_start = syn_mem_size - frame_size - overlap;
            in_buf[in_buf_offset..in_buf_offset + buf_stride].copy_from_slice(
                &self.syn_mem[channel_offset + src_start..channel_offset + syn_mem_size],
            );
        }

        // Encoder pitch prefilter (the inverse of the decoder postfilter).
        // Enable gate matches celt_encoder.c: enough bytes to be worth the ~7
        // bits, CELT-only (start_band == 0; the hybrid high band has no pf),
        // complexity >= 5. `CELT_PF_OFF` disables for A/B debugging.
        // (History: default-off until 2026-07-09 — the octave signalling was one
        // low for every pitch_index >= 31, so decoders reconstructed a garbage
        // period; fixed, sine round-trip 7.6 -> 45.7 dB.)
        let nb_available_bytes = (explicit_total_bits.unwrap_or((rc.buf.len() * 8) as i32) >> 3)
            - ((rc.tell() + 4) >> 3);
        let pf_enabled =
            start_band == 0 && self.complexity >= 5 && nb_available_bytes > 12 * channels as i32;
        // Capture the tapset used for THIS frame's comb (C's `prefilter_tapset`
        // local): spreading_decision mutates self.tapset_decision later in the
        // frame, and the value applied+signalled here — not the mutated one —
        // must become next frame's "old" tapset.
        let prefilter_tapset = self.tapset_decision;
        let (pf_on, gain1, pitch_index) = if pf_enabled {
            run_prefilter(
                in_buf,
                &mut self.prefilter_mem,
                self.prefilter_period,
                self.prefilter_gain,
                self.prefilter_tapset,
                prefilter_tapset,
                mode.window,
                channels,
                frame_size,
                overlap,
                &mut self.w_prefilter_pre,
                &mut self.w_prefilter_pitch_buf,
                &self.analysis,
                self.loss_rate,
                nb_available_bytes,
            )
        } else {
            (false, 0.0f32, COMBFILTER_MINPERIOD)
        };

        // Save the prefiltered overlap for the next frame.
        // In libopus, st->in_mem stores the overlap separately and run_prefilter
        // copies it to/from in[]. Here we emulate that by updating syn_mem with
        // the last overlap samples of in_buf (which were prefiltered in place).
        let syn_mem_size = 2048 + overlap;
        for c in 0..channels {
            let channel_offset = c * syn_mem_size;
            let in_buf_offset = c * buf_stride;
            self.syn_mem[channel_offset + syn_mem_size - overlap..channel_offset + syn_mem_size]
                .copy_from_slice(&in_buf[in_buf_offset + frame_size..in_buf_offset + buf_stride]);
        }

        // Transient analysis runs on the PREFILTERED signal (celt_encoder.c
        // order) — the comb removes periodic energy so pitch pulses don't read
        // as transients.
        let mut tf_estimate = 0.0f32;
        let mut tf_chan = 0;
        let mut weak_transient = false;
        // Reduces energy instability on fricatives at low-bitrate hybrid, while
        // still allowing real transients on vowels (which SILK marks with a
        // small quantization offset). Signal type 2 is voiced.
        // libopus has `effectiveBytes` in hand well before transient analysis;
        // here it is computed further down, so take it from the same two places
        // the reference does: the VBR target, or the packet when there is none.
        let early_effective_bytes = if self.vbr_rate > 0 {
            self.vbr_rate >> (3 + BITRES)
        } else {
            explicit_total_bits.unwrap_or_else(|| (rc.buf.len() * 8) as i32) / 8
        };
        let allow_weak_transients =
            start_band != 0 && early_effective_bytes < 15 && self.silk_signal_type != 2;
        let is_transient = if self.complexity >= 1 {
            transient_analysis(
                in_buf,
                buf_stride,
                channels,
                &mut tf_estimate,
                &mut tf_chan,
                allow_weak_transients,
                &mut weak_transient,
                0.0,
                0.0,
                &mut self.w_transient_tmp,
                &mut self.w_transient_tmp2,
            )
        } else {
            false
        };

        let freq = &mut self.w_freq[..frame_size * channels];
        // The first MDCT pass is always LONG blocks: for non-transients it is
        // the coding transform; for transients it feeds bandLogE2 (the
        // pre-transient spectrum dynalloc smooths against, celt_encoder.c
        // secondMdct) and the short re-MDCT below produces the coding one.
        let (shift, b) = (mode.max_lm - lm, 1);
        let n = frame_size / b;

        for c in 0..channels {
            let c_buf_offset = c * buf_stride;

            for i in 0..b {
                mode.mdct.forward(
                    &in_buf[c_buf_offset + i * n..],
                    &mut freq[c * frame_size + i..],
                    mode.window,
                    overlap,
                    shift,
                    b,
                );
            }
        }

        scale_spectrum_for_upsample(freq, frame_size, channels, upsample);

        let band_e = &mut self.w_band_e[..nb_ebands * channels];
        band_e.fill(0.0);
        compute_band_energies(mode, freq, band_e, end_band, channels, lm);

        let x_pad_end = (frame_size * channels + STRIDE_ACCESS_PAD).min(self.w_x.len());
        let x = &mut self.w_x[..x_pad_end];
        normalise_bands(
            mode,
            freq,
            x,
            band_e,
            end_band,
            channels,
            (1 << lm) as usize,
        );

        let mut total_bits = explicit_total_bits.unwrap_or_else(|| (rc.buf.len() * 8) as i32);
        self.w_error[..nb_ebands * channels].fill(0.0);
        let error = &mut self.w_error[..nb_ebands * channels];

        // The silence flag is only in the bitstream when CELT owns a fresh range
        // coder (`tell == 1`, i.e. CELT-only). In hybrid, SILK has already coded
        // into it, so celt_encoder.c forces `silence = 0` rather than sending a
        // flag the decoder will not read — celt_decoder.c reads the flag under
        // the same `tell == 1` condition. Dropping that `else` let a digitally
        // silent hybrid frame take the shortcut below, which shrinks the coder
        // and marks the budget spent, so CELT wrote no layer at all while the
        // decoder still read one: the packet desynchronized from that point.
        let tell = rc.tell();
        let silence = if tell == 1 {
            rc.encode_bit_logp(silence, 15);
            silence
        } else {
            false
        };
        if silence {
            // celt_encoder.c: on a silent frame send only the minimum. Clamp the
            // coder to the bytes already filled + 2, then tell the range coder
            // the rest is spoken for. Every downstream budget check (allocation,
            // prefilter, bands) then has nothing to spend and codes nothing,
            // while the whole pipeline still runs — which is what keeps the
            // encoder in lockstep with the decoder's mirror of this at
            // `rc.nbits_total += total_bits - rc.tell()`.
            //
            // CBR frames keep their full size (the packet length is fixed), so
            // the shrink is VBR-only, exactly as in the C.
            if self.vbr_rate > 0 {
                let filled = (rc.tell() + 7) >> 3;
                let nb_compressed = (total_bits >> 3).min(filled + 2).max(2);
                rc.shrink(nb_compressed as u32);
                total_bits = nb_compressed * 8;
            }
            rc.nbits_total += total_bits - rc.tell();
        }

        if start_band == 0 && !silence && rc.tell() + 16 <= total_bits {
            rc.encode_bit_logp(pf_on, 1);
            if pf_on {
                let qg = (gain1 / 0.09375 - 1.0 + 0.5).floor() as i32;
                let qg = qg.clamp(0, 7);
                let pi = (pitch_index + 1) as u32;
                // octave = EC_ILOG(pi) - 5 (EC_ILOG = 32 - clz, the BIT COUNT of
                // pi, not floor(log2)). The old `31 - clz` was one octave low for
                // every pi >= 32, overflowing the 4+octave residual field -> the
                // decoder reconstructed a garbage period (the prefilter's AM/PM
                // sideband bug). pi >= MINPERIOD+1 = 16 keeps this >= 0.
                let octave = 32 - pi.leading_zeros() - 5;
                rc.enc_uint(octave, 6);
                rc.enc_bits(pi - (16 << octave), 4 + octave);
                rc.enc_bits(qg as u32, 3);
                rc.encode_icdf(prefilter_tapset, &TAPSET_ICDF, 2);
            }
        }

        let mut short_blocks = false;
        if lm > 0 && rc.tell() + 3 <= total_bits {
            rc.encode_bit_logp(is_transient, 3);
            if is_transient {
                short_blocks = true;
            }
        }

        // bandLogE2: the long-MDCT logs + 0.5*LM when we re-MDCT short
        // (celt_encoder.c secondMdct); else a copy of the final logs (set after
        // the final amp2log2 below).
        let mut second_mdct_logs = false;
        if short_blocks && self.complexity >= 8 {
            let band_log_e2 = &mut self.w_band_log_e2[..nb_ebands * channels];
            band_log_e2.fill(-14.0);
            crate::celt::bands::amp2log2(mode, 0, end_band, band_e, band_log_e2, channels);
            for v in band_log_e2.iter_mut() {
                *v += 0.5 * lm as f32;
            }
            second_mdct_logs = true;
        }
        if short_blocks {
            let b = 1 << lm;
            let n = frame_size / b;
            for c in 0..channels {
                let c_offset = c * buf_stride;
                for i in 0..b {
                    mode.mdct.forward(
                        &in_buf[c_offset + i * n..c_offset + buf_stride],
                        &mut freq[c * frame_size + i..],
                        mode.window,
                        overlap,
                        mode.max_lm,
                        b,
                    );
                }
            }

            scale_spectrum_for_upsample(freq, frame_size, channels, upsample);
            compute_band_energies(mode, freq, band_e, end_band, channels, lm);
            normalise_bands(
                mode,
                freq,
                x,
                band_e,
                end_band,
                channels,
                (1 << lm) as usize,
            );
        }

        // Final band logs come AFTER the (possibly short) coding MDCT — C order
        // (celt_encoder.c:1742). C computes real logs for ALL bands below end
        // (amp2Log2 effEnd==end), incl. below start in hybrid: dynalloc's noise
        // floor and the spreading mask read them.
        let band_log_e = &mut self.w_band_log_e[..nb_ebands * channels];
        band_log_e.fill(-14.0);
        crate::celt::bands::amp2log2(mode, 0, end_band, band_e, band_log_e, channels);
        if !second_mdct_logs {
            self.w_band_log_e2[..nb_ebands * channels].copy_from_slice(band_log_e);
        }

        let intra_ener = if self.complexity >= 4 {
            false
        } else {
            self.old_band_e[..nb_ebands * channels]
                .iter()
                .all(|&e| e <= -27.0)
        };
        quant_coarse_energy_advanced(
            mode,
            start_band,
            end_band,
            end_band,
            band_log_e,
            &mut self.old_band_e,
            total_bits as u32,
            error,
            rc,
            channels,
            lm,
            (total_bits / 8) as usize,
            is_transient || intra_ener,
            &mut self.delayed_intra,
            self.complexity >= 4,
            0,
            false,
        );
        // Dynalloc analysis runs BEFORE tf (celt_encoder.c order): its
        // importance[] weights the tf Viterbi costs and spread_weight[] feeds
        // the spreading decision. The boost FLAGS are still written later, in
        // bitstream order.
        let effective_bytes = ((total_bits / 8) as usize).max(1);
        let mut importance = [13.0f32; MAX_NB_EBANDS];
        let mut spread_weight = [32i32; MAX_NB_EBANDS];
        self.w_offsets[..nb_ebands].fill(0);
        let max_depth = {
            let band_log_e2 = &self.w_band_log_e2[..nb_ebands * channels];
            dynalloc_analysis(
                mode,
                band_log_e,
                band_log_e2,
                start_band,
                end_band,
                channels,
                &mut self.w_offsets[..nb_ebands],
                self.lsb_depth,
                is_transient,
                self.vbr_rate > 0,
                self.constrained_vbr,
                lm,
                effective_bytes,
                &self.analysis,
                &mut importance,
                &mut spread_weight,
            )
        };

        self.w_tf_res[..nb_ebands].fill(0);
        let tf_res = &mut self.w_tf_res[..nb_ebands];
        let lambda = 80.max(20480 / effective_bytes + 2) as i32;

        // libopus disables variable TF resolution for hybrid outright, and at
        // very low bitrate (`celt_encoder.c`: `enable_tf_analysis`). Running it
        // in hybrid anyway spends bits on `tf_res` that the reference never
        // codes, and picks a different transform resolution for the high band.
        let hybrid = start_band != 0;
        let tf_select = if effective_bytes >= 15 * channels && !hybrid && self.complexity >= 2 {
            tf_analysis(
                mode,
                end_band,
                is_transient,
                tf_res,
                lambda,
                x,
                frame_size,
                lm as i32,
                tf_estimate,
                tf_chan,
                &importance,
            )
        } else if hybrid && weak_transient {
            // Improving time resolution with TF on a long window is imperfect
            // and will not collapse the energy at low bitrate, so a weak
            // transient is better served by leaving the resolution alone.
            tf_res[..end_band].fill(1);
            0
        } else if hybrid && effective_bytes < 15 && self.silk_signal_type != 2 {
            // Low-bitrate hybrid forces 5 ms temporal resolution, not 2.5 ms.
            tf_res[..end_band].fill(0);
            is_transient as i32
        } else {
            tf_res[..end_band].fill(is_transient as i32);
            0
        };
        tf_encode(
            start_band,
            end_band,
            is_transient,
            tf_res,
            lm as i32,
            tf_select,
            rc,
        );

        let mut dual_stereo_val = if channels == 2 {
            stereo_analysis(mode, x, lm as i32, frame_size) as i32
        } else {
            0
        };

        let mut stereo_saving = 0.0f32;
        let equiv_rate = (total_bits * 48000) / frame_size as i32;
        if channels == 2 {
            self.intensity = hysteresis_decision(
                equiv_rate / 1000,
                &INTEN_THRESHOLDS,
                &INTEN_HYSTERESIS,
                self.intensity,
            );
            // Clamp to [start, end], NOT [0, nb_ebands] (celt_encoder.c:2034).
            // clt_compute_allocation codes `intensity - start` in a field of
            // width `end + 1 - start`; a value below start (which happens in
            // stereo HYBRID, start_band = 17) underflowed that field and
            // desynced the range coder on the first stereo-hybrid frame.
            self.intensity = self.intensity.clamp(start_band as i32, end_band as i32);
        }

        if self.complexity == 0 {
            self.spread_decision = SPREAD_NONE;
            if rc.tell() + 4 <= total_bits {
                rc.encode_icdf(self.spread_decision, &SPREAD_ICDF, 5);
            }
        } else if rc.tell() + 4 <= total_bits {
            if is_transient || self.complexity < 3 || effective_bytes < 10 * channels {
                self.spread_decision = SPREAD_NORMAL;
            } else {
                let update_hf = lm == mode.max_lm;
                self.spread_decision = spreading_decision(
                    mode,
                    x,
                    &mut self.tonal_average,
                    self.spread_decision,
                    &mut self.hf_average,
                    &mut self.tapset_decision,
                    update_hf,
                    end_band,
                    channels,
                    (1 << lm) as usize,
                    &spread_weight,
                );
            }
            rc.encode_icdf(self.spread_decision, &SPREAD_ICDF, 5);
        } else {
            self.spread_decision = SPREAD_NORMAL;
        }

        self.w_cap[..nb_ebands].fill(0);
        let cap = &mut self.w_cap[..nb_ebands];
        for (i, cap_i) in cap.iter_mut().enumerate() {
            let n = (mode.e_bands[i + 1] - mode.e_bands[i]) << lm;
            *cap_i = ((mode.cache.caps[nb_ebands * (2 * lm + channels - 1) + i] as i32 + 64)
                * channels as i32
                * n as i32)
                >> 2;
        }

        let offsets = &mut self.w_offsets[..nb_ebands];

        let mut dynalloc_logp = 6i32;
        let total_bits_bitres = total_bits << BITRES;
        let mut total_boost = 0i32;
        let mut tell_frac = rc.tell_frac();

        for i in start_band..end_band {
            let width =
                channels as i32 * (mode.e_bands[i + 1] - mode.e_bands[i]) as i32 * (1 << lm);
            let quanta = (width << BITRES).min((6 << BITRES).max(width));
            let mut dynalloc_loop_logp = dynalloc_logp;
            let mut boost = 0i32;
            let mut j = 0i32;

            while tell_frac + (dynalloc_loop_logp << BITRES) < total_bits_bitres - total_boost
                && boost < cap[i]
            {
                let flag = j < offsets[i];
                rc.encode_bit_logp(flag, dynalloc_loop_logp as u32);
                tell_frac = rc.tell_frac();
                if !flag {
                    break;
                }
                boost += quanta;
                total_boost += quanta;
                dynalloc_loop_logp = 1;
                j += 1;
            }

            if j > 0 {
                dynalloc_logp = 2.max(dynalloc_logp - 1);
            }
            offsets[i] = boost;
        }

        let alloc_trim = alloc_trim_analysis(
            mode,
            x,
            band_log_e,
            end_band,
            lm as i32,
            channels,
            frame_size,
            &mut stereo_saving,
            tf_estimate,
            self.intensity,
            0.0,
            equiv_rate,
        );
        // libopus celt_encoder.c: alloc_trim is 5 UNLESS there is room to code the
        // analysis value — the decoder falls back to 5 when the trim isn't coded,
        // so the encoder MUST use 5 in the allocation math too. Keeping the
        // analysis trim here made trim_offset (hence the allocation) differ from
        // every conformant decoder on tight-budget frames (e.g. 24 kbps hybrid),
        // desyncing the range coder on ~1% of packets.
        let alloc_trim = if rc.tell_frac() + (6 << BITRES) <= total_bits_bitres - total_boost {
            rc.encode_icdf(alloc_trim, &TRIM_ICDF, 7);
            alloc_trim
        } else {
            5
        };

        // ---- VBR: pick this frame's size and shrink the coder to it ----
        // (libopus celt_encoder.c `if (vbr_rate>0)`; runs between the trim and the
        // allocation so the allocator sees the final budget.)
        let total_bits = if self.vbr_rate > 0 {
            let hybrid = start_band != 0;
            let lm_diff = mode.max_lm as i32 - lm as i32;
            let vbr_rate = self.vbr_rate;
            let mut base_target = if hybrid {
                0.max(vbr_rate - ((9 * channels as i32 + 4) << BITRES))
            } else {
                vbr_rate - ((40 * channels as i32 + 20) << BITRES)
            };
            if self.constrained_vbr {
                base_target += self.vbr_offset >> lm_diff;
            }
            let mut target = if hybrid {
                let mut t = base_target;
                // Tonal frames (a small SILK quantization offset) need more bits
                // in the high band than noisy ones do.
                if self.silk_offset < 100 {
                    t += 12 << BITRES >> (3 - lm as i32);
                }
                if self.silk_offset > 100 {
                    t -= 18 << BITRES >> (3 - lm as i32);
                }
                t += ((tf_estimate - 0.25) * (50 << BITRES) as f32) as i32;
                if tf_estimate > 0.7 {
                    t = t.max(50 << BITRES);
                }
                t
            } else {
                compute_vbr_target(
                    mode,
                    base_target,
                    lm as i32,
                    self.last_coded_bands,
                    channels as i32,
                    self.intensity,
                    self.constrained_vbr,
                    stereo_saving,
                    total_boost,
                    tf_estimate,
                    max_depth,
                )
            };
            let tell = rc.tell_frac();
            target += tell;
            // Never shrink below what's already coded (+2 bytes of margin); in
            // hybrid, keep >=37 bits after the SILK part so the redundancy
            // signalling space assumed by every decoder still exists.
            let mut min_allowed =
                ((tell + total_boost + (1 << (BITRES + 3)) - 1) >> (BITRES + 3)) + 2;
            if hybrid {
                min_allowed = min_allowed.max(
                    (tell0_frac + (37 << BITRES) + total_boost + (1 << (BITRES + 3)) - 1)
                        >> (BITRES + 3),
                );
            }
            let cap_bytes = (total_bits / 8).min(1275 >> (3 - lm as i32));
            let mut nb_available = (target + (1 << (BITRES + 2))) >> (BITRES + 3);
            nb_available = nb_available.max(min_allowed).min(cap_bytes);

            // Reservoir/drift tracking (constrained VBR).
            let delta = target - vbr_rate;
            let target_q = nb_available << (BITRES + 3);
            if self.vbr_count < 970 {
                self.vbr_count += 1;
            }
            let alpha = if self.vbr_count < 970 {
                1.0f32 / (self.vbr_count as f32 + 20.0)
            } else {
                0.001f32
            };
            if self.constrained_vbr {
                self.vbr_reservoir += target_q - vbr_rate;
                self.vbr_drift += (alpha
                    * ((delta * (1 << lm_diff)) - self.vbr_offset - self.vbr_drift) as f32)
                    as i32;
                self.vbr_offset = -self.vbr_drift;
                if self.vbr_reservoir < 0 {
                    let adjust = (-self.vbr_reservoir) / (8 << BITRES);
                    nb_available += adjust;
                    self.vbr_reservoir = 0;
                }
            }
            let nb_compressed = cap_bytes.min(nb_available).max(2);
            rc.shrink(nb_compressed as u32);
            nb_compressed * 8
        } else {
            total_bits
        };

        let mut intensity = self.intensity;
        self.w_pulses[..nb_ebands].fill(0);
        let pulses = &mut self.w_pulses[..nb_ebands];

        let stereo = channels > 1;
        let ebands_stereo = if stereo {
            nb_ebands * channels
        } else {
            nb_ebands
        };
        self.w_fine_priority[..ebands_stereo].fill(0);
        let fine_priority = &mut self.w_fine_priority[..ebands_stereo];
        self.w_ebits[..ebands_stereo].fill(0);
        let ebits = &mut self.w_ebits[..ebands_stereo];
        let mut balance = 0;

        // The anti-collapse bit reservation must be subtracted from the allocation
        // budget BEFORE compute_allocation (libopus celt_encoder.c: `total_bits -=
        // anti_collapse_rsv` precedes it) — the decoder reserves it there too.
        // Computing it only afterwards (as this code used to) let the encoder
        // allocate 1<<BITRES more than the decoder assumes on transient LM>=2
        // frames -> band budgets differ from band `start` -> range desync on
        // exactly those frames (caught by opus_demo -d's per-packet range check).
        // Same formula as the decoder for exact symmetry.
        let anti_collapse_rsv = if is_transient && lm >= 2 {
            let remaining = (total_bits << BITRES) - rc.tell_frac() - 1;
            if remaining >= ((lm as i32 + 2) << BITRES) {
                1i32 << BITRES
            } else {
                0
            }
        } else {
            0
        };

        // signalBandwidth: end-1 by CHOICE (C uses the analysis bandwidth,
        // celt_encoder.c:2174, to let the allocator skip top bands — but that
        // narrowing loses ~0.7 ODG on music even with leak_boost live, and
        // libopus's own narrowed scores lose to our full-band ones). PEAQ-gated
        // out twice; do not re-enable without a corpus win.
        let signal_bandwidth = end_band as i32 - 1;

        self.last_coded_bands = clt_compute_allocation(
            mode,
            start_band,
            end_band,
            offsets,
            cap,
            alloc_trim,
            &mut intensity,
            &mut dual_stereo_val,
            (total_bits << BITRES) - rc.tell_frac() - 1 - anti_collapse_rsv,
            &mut balance,
            pulses,
            ebits,
            fine_priority,
            channels as i32,
            lm as i32,
            rc,
            true,
            0,
            signal_bandwidth,
        );

        quant_fine_energy(
            mode,
            start_band,
            end_band,
            &mut self.old_band_e,
            error,
            ebits,
            rc,
            channels,
        );

        self.w_collapse_masks[..nb_ebands * channels].fill(0);
        let collapse_masks = &mut self.w_collapse_masks[..nb_ebands * channels];
        let (x_split, y_split) = x.split_at_mut(frame_size);
        let y_opt = if channels == 2 { Some(y_split) } else { None };

        let mut dual_stereo = dual_stereo_val != 0;

        let theta_rdo = channels == 2 && !dual_stereo && self.complexity >= 8;
        let resynth = theta_rdo;

        quant_all_bands(
            true,
            mode,
            start_band,
            end_band,
            x_split,
            y_opt,
            collapse_masks,
            band_e,
            pulses,
            short_blocks,
            self.spread_decision,
            &mut dual_stereo,
            intensity as usize,
            tf_res,
            (total_bits << BITRES) - anti_collapse_rsv,
            &mut balance,
            rc,
            lm as i32,
            self.last_coded_bands,
            resynth,
            false,
            &mut 0u32,
        );

        if anti_collapse_rsv > 0 {
            let anti_collapse_on = if self.consec_transient < 2 {
                1u32
            } else {
                0u32
            };
            rc.enc_bits(anti_collapse_on, 1);
        }

        quant_energy_finalise(
            mode,
            start_band,
            end_band,
            &mut self.old_band_e,
            error,
            ebits,
            fine_priority,
            total_bits - rc.tell(),
            rc,
            channels,
        );

        // libopus resynthesises the encoded frame here to keep an encoder-side
        // decode buffer in step. This port never read that buffer back, so the
        // synthesis was pure dead computation and has been removed. `resynth`
        // itself stays live: quant_all_bands() above uses it, and it does affect
        // the bitstream.

        if !is_transient {
            self.old_band_e2.copy_from_slice(&self.old_band_e);
        } else {
            for i in 0..channels * nb_ebands {
                self.old_band_e2[i] = self.old_band_e2[i].min(self.old_band_e[i]);
            }
        }

        // "In case start or end were to change" (celt_encoder.c:2301): zero the
        // coarse-energy state outside [start, end) and floor the log history —
        // the decoder does the same every frame, and a later frame with a wider
        // end must predict those bands from the SAME (zeroed) base.
        for c in 0..channels {
            for i in 0..start_band {
                self.old_band_e[c * nb_ebands + i] = 0.0;
                self.old_band_e2[c * nb_ebands + i] = -28.0;
            }
            for i in end_band..nb_ebands {
                self.old_band_e[c * nb_ebands + i] = 0.0;
                self.old_band_e2[c * nb_ebands + i] = -28.0;
            }
        }

        rc.pad_to_bits(total_bits);

        if pf_on {
            self.prefilter_period = pitch_index;
            self.prefilter_gain = gain1;
        } else {
            self.prefilter_period = COMBFILTER_MINPERIOD;
            self.prefilter_gain = 0.0;
        }
        self.prefilter_tapset = prefilter_tapset;

        if is_transient {
            self.consec_transient += 1;
        } else {
            self.consec_transient = 0;
        }
    }
}

pub struct CeltDecoder {
    mode: &'static CeltMode,
    channels: usize,
    /// 48 kHz samples per output sample — the mirror of the encoder's
    /// `upsample`. CELT always synthesises at 48 kHz; a lower API rate emits
    /// every `downsample`-th sample. The deemphasis recursion still runs over
    /// every 48 kHz sample, only the output is decimated (libopus `deemphasis`).
    downsample: usize,
    // Bitstream (coded) channels C; normally == channels (CC). A mono packet in a
    // stereo decoder sets this to 1 (C=1, CC=2) so the CELT inter-frame state stays
    // one continuous chain across mono<->stereo switches, matching libopus.
    stream_channels: usize,
    decode_mem: Vec<f32>,
    old_band_e: Vec<f32>,
    preemph_mem: Vec<f32>,
    prefilter_mem: Vec<f32>,
    prefilter_period: usize,
    prefilter_period_old: usize,
    prefilter_gain: f32,
    prefilter_gain_old: f32,
    prefilter_tapset: i32,
    prefilter_tapset_old: i32,
    old_band_e2: Vec<f32>,
    old_band_e3: Vec<f32>,
    rng: u32,
    /// How long the decoder has been concealing, in units of 2.5 ms, saturating
    /// at 10000 (celt_decode_lost's `loss_duration`). Zero on any decoded frame.
    /// The energy prediction on the first frame back is made safe in proportion
    /// to it, and the noise floor is allowed to rise by the whole gap at once.
    loss_duration: i32,
    /// The same count, but *not* cleared by a single good frame — only by the
    /// two consecutive ones that clear `skip_plc`. It is what decides when a
    /// burst stops being pitch-extrapolated and becomes noise.
    plc_duration: i32,
    /// Stay on the noise branch until two consecutive packets have arrived. A
    /// single good frame in the middle of a bad stretch is not enough history to
    /// extrapolate a pitch period from.
    skip_plc: bool,
    /// One of the `FRAME_*` constants: what the previous frame was.
    last_frame_type: i32,
    /// The concealed frame left its MDCT overlap un-folded, so the next frame
    /// must prefilter and TDAC-fold it before its own synthesis overlaps onto it.
    prefilter_and_fold: bool,
    /// Per-band noise floor, the level concealment decays *towards* rather than
    /// through (celt_decoder.c:764). Without it a long burst fades to silence
    /// instead of to the background the stream actually sat in.
    background_log_e: Vec<f32>,
    /// Pitch lag from the first lost frame, reused across a loss burst.
    last_pitch_index: i32,
    /// LPC coefficients (per channel, PLC_LPC_ORDER) computed at the first loss
    /// and reused for the rest of the burst (pitch-based PLC).
    plc_lpc: Vec<f32>,

    w_tf_res: Vec<i32>,
    w_cap: Vec<i32>,
    w_offsets: Vec<i32>,
    w_pulses: Vec<i32>,
    w_ebits: Vec<i32>,
    w_fine_priority: Vec<i32>,
    w_x: Vec<f32>,
    w_collapse_masks: Vec<u32>,
    w_freq: Vec<f32>,
    w_band_amp: Vec<f32>,
    w_pcm_frame: Vec<f32>,
    w_post: Vec<f32>,
    w_etmp: Vec<f32>,
}

impl CeltDecoder {
    pub fn new(mode: &'static CeltMode, channels: usize) -> Self {
        let overlap = mode.overlap;
        let nb_ebands = mode.nb_ebands;
        // The band-energy state is two channels wide whatever the decoder is,
        // exactly as libopus allocates it (celt_decoder.c:202). A mono decoder
        // is not wasting the second half: it snapshots each decoded frame's
        // coarse energy there and merges it back on the next frame, which is how
        // the energy prediction survives a concealed frame having decayed it.
        let nb_x_ch = nb_ebands * 2;
        // The spectrum is C channels wide, and C is the *bitstream's* channel
        // count, which a mono decoder does not bound: a stereo packet decoded to
        // mono output is C=2, CC=1. Size for C like the band-energy state above,
        // not for CC.
        let dec_frame_x_ch = DECODE_BUFFER_SIZE * 2;
        Self {
            mode,
            channels,
            stream_channels: channels,
            decode_mem: vec![0.0; channels * (DECODE_BUFFER_SIZE + overlap)],
            // libopus: oldBandE inits to 0 (OPUS_CLEAR); only oldLogE/oldLogE2 get
            // the -28 "very quiet" floor. Do NOT init old_band_e to -28 (it is the
            // coarse-energy prediction state; -28 makes the first frames too quiet).
            old_band_e: vec![0.0; nb_x_ch],
            downsample: 1,
            preemph_mem: vec![0.0; channels],
            prefilter_mem: vec![0.0; channels * COMBFILTER_MAXPERIOD],
            prefilter_period: COMBFILTER_MINPERIOD,
            prefilter_period_old: COMBFILTER_MINPERIOD,
            prefilter_gain: 0.0,
            prefilter_gain_old: 0.0,
            prefilter_tapset: 0,
            prefilter_tapset_old: 0,
            // oldLogE / oldLogE2 in libopus: init -QCONST16(28,DB_SHIFT).
            old_band_e2: vec![-28.0; nb_x_ch],
            old_band_e3: vec![-28.0; nb_x_ch],
            rng: 0,
            loss_duration: 0,
            plc_duration: 0,
            // libopus resets a fresh decoder through OPUS_RESET_STATE, which
            // sets `skip_plc` (celt_decoder.c:1807): until two packets have been
            // decoded back to back there is no pitch history worth extrapolating.
            skip_plc: true,
            last_frame_type: FRAME_NONE,
            prefilter_and_fold: false,
            // backgroundLogE is zero-initialised, unlike oldLogE/oldLogE2.
            background_log_e: vec![0.0; nb_x_ch],
            last_pitch_index: 0,
            plc_lpc: vec![0.0; channels * PLC_LPC_ORDER],

            w_tf_res: vec![0; nb_ebands],
            w_cap: vec![0; nb_ebands],
            w_offsets: vec![0; nb_ebands],
            w_pulses: vec![0; nb_ebands],
            w_ebits: vec![0; nb_x_ch],
            w_fine_priority: vec![0; nb_x_ch],

            w_x: vec![0.0; dec_frame_x_ch + STRIDE_ACCESS_PAD],
            w_collapse_masks: vec![0; nb_x_ch],
            w_freq: vec![0.0; dec_frame_x_ch + 4], // +4: NEON backward pre-rotation reads up to 3 elements past n2
            w_band_amp: vec![0.0; nb_x_ch],
            w_pcm_frame: vec![0.0; DECODE_BUFFER_SIZE],
            w_post: vec![0.0; DECODE_BUFFER_SIZE + COMBFILTER_MAXPERIOD],
            w_etmp: vec![0.0; overlap],
        }
    }

    /// Channels coded in the next packet's bitstream. Independent of the
    /// decoder's own channel count: a mono packet in a stereo decoder is C=1/
    /// CC=2, and a stereo packet in a mono decoder is C=2/CC=1. Both keep the
    /// 2-channel inter-frame state continuous across mono<->stereo switches.
    pub fn set_stream_channels(&mut self, sc: usize) {
        self.stream_channels = sc.clamp(1, 2);
    }

    /// libopus OPUS_RESET_STATE for the decoder: clear everything from rng onward,
    /// then oldLogE/oldLogE2 = -28 (oldBandE stays 0).
    pub fn reset(&mut self) {
        self.decode_mem.fill(0.0);
        self.old_band_e.fill(0.0);
        self.old_band_e2.fill(-28.0);
        self.old_band_e3.fill(-28.0);
        self.preemph_mem.fill(0.0);
        self.prefilter_mem.fill(0.0);
        self.prefilter_period = COMBFILTER_MINPERIOD;
        self.prefilter_period_old = COMBFILTER_MINPERIOD;
        self.prefilter_gain = 0.0;
        self.prefilter_gain_old = 0.0;
        self.prefilter_tapset = 0;
        self.prefilter_tapset_old = 0;
        self.rng = 0;
        self.background_log_e.fill(0.0);
        self.loss_duration = 0;
        self.plc_duration = 0;
        self.prefilter_and_fold = false;
        self.last_pitch_index = 0;
        self.plc_lpc.fill(0.0);
        // OPUS_RESET_STATE ends by re-arming these two (celt_decoder.c:1807).
        self.skip_plc = true;
        self.last_frame_type = FRAME_NONE;
    }

    /// Mirror of [`CeltEncoder::set_upsample`]: `frame_size` counts samples at
    /// the caller's rate, and CELT decimates its 48 kHz synthesis into it.
    pub fn set_downsample(&mut self, downsample: usize) {
        self.downsample = downsample.max(1);
    }

    /// Decode one CELT frame from `rc` into `pcm`, which receives
    /// `frame_size * channels` samples **interleaved** — the same layout the
    /// caller hands to its own caller, and the same one
    /// [`CeltDecoder::conceal_lost_bands`] writes, so a decoded frame and a
    /// concealed one can be summed or faded together without a repack.
    pub fn decode_from_range_coder_with_band_range(
        &mut self,
        rc: &mut RangeCoder,
        total_bits: i32,
        frame_size: usize,
        pcm: &mut [f32],
        start_band: usize,
        end_band: usize,
    ) -> usize {
        self.decode_impl_from_rc(rc, total_bits, frame_size, pcm, start_band, end_band)
    }

    fn decode_impl_from_rc(
        &mut self,
        rc: &mut RangeCoder,
        total_bits: i32,
        frame_size: usize,
        pcm: &mut [f32],
        start_band: usize,
        end_band: usize,
    ) -> usize {
        // The caller counts samples at its own rate; CELT synthesises at 48 kHz
        // and decimates on the way out.
        let frame_size = frame_size * self.downsample;
        let mode = self.mode;
        // CC = state/output channels; C (=`channels`) = channels coded in the
        // bitstream. The two are independent: energy/allocation/bands/denormalise
        // all use C, and synthesis renders C into CC outputs. Mono packet in a
        // stereo decoder (C=1, CC=2) writes the single decoded channel to both;
        // stereo packet in a mono decoder (C=2, CC=1) sums the two in the
        // frequency domain before the single inverse MDCT.
        let cc = self.channels;
        let channels = self.stream_channels.clamp(1, 2);
        let nb_ebands = mode.nb_ebands;
        let end_band = end_band.min(nb_ebands).max(start_band);
        let overlap = mode.overlap;
        let mem_size = DECODE_BUFFER_SIZE + overlap;

        // Two consecutive packets have to arrive before the pitch-based
        // concealment is trusted again (celt_decoder.c:1297): this frame follows
        // a decoded one only if the loss counter was already clear on entry.
        if self.loss_duration == 0 {
            self.skip_plc = false;
        }

        let mut lm = 0;
        while (mode.short_mdct_size << lm) != frame_size {
            lm += 1;
            if lm > mode.max_lm {
                break;
            }
        }
        if (mode.short_mdct_size << lm) != frame_size {
            lm = 0;
        }

        // libopus celt_decoder.c:953: `if (C==1) oldBandE[i]=MAX(oldBandE[i],
        // oldBandE[nbEBands+i])` before the coarse-energy decode — a mono packet in
        // a stereo decoder predicts its single channel from the MAX of both
        // channels' previous energy. (Only meaningful on the first mono frame after
        // stereo; after every mono frame ch0 is replicated to ch1 at frame end.)
        if channels == 1 {
            for i in 0..nb_ebands {
                self.old_band_e[i] = self.old_band_e[i].max(self.old_band_e[nb_ebands + i]);
            }
        }

        let tell = rc.tell();
        let mut silence = false;
        if tell >= total_bits {
            silence = true;
        } else if tell == 1 {
            silence = rc.decode_bit_logp(15);
        }
        if silence {
            // libopus: "Pretend we've read all the remaining bits" — every
            // downstream budget check then skips its entropy reads naturally, the
            // whole pipeline still runs (decode_mem shift, overlap fade-out via a
            // zeroed spectrum, postfilter/deemph, frame-end energy bookkeeping).
            // The old early-return left the decoder state one frame stale and the
            // energy prediction hot -> the next loud frame decoded ~2^15 too loud
            // and railed the output.
            rc.nbits_total += total_bits - rc.tell();
        }

        let mut pf_on = false;
        let mut pitch_index = COMBFILTER_MINPERIOD;
        let mut gain1 = 0.0f32;
        let mut prefilter_tapset = 0;

        if start_band == 0 && !silence && rc.tell() + 16 <= total_bits {
            pf_on = rc.decode_bit_logp(1);
            if pf_on {
                let octave = rc.dec_uint(6);
                pitch_index = ((16 << octave) + rc.dec_bits(4 + octave)) as usize - 1;
                let qg = rc.dec_bits(3);
                if rc.tell() + 2 <= total_bits {
                    prefilter_tapset = rc.decode_icdf(&TAPSET_ICDF, 2) as usize;
                }
                gain1 = 0.09375 * (qg as f32 + 1.0);
            }
        }
        if start_band != 0 {
            self.prefilter_gain = 0.0;
        }

        let mut is_transient = false;
        if lm > 0 && rc.tell() + 3 <= total_bits {
            is_transient = rc.decode_bit_logp(3);
        }
        let short_blocks = is_transient;

        let intra_ener = if rc.tell() + 3 <= total_bits {
            rc.decode_bit_logp(3)
        } else {
            false
        };

        // Coming back from a loss, the coarse energy is predicted from state the
        // concealment invented, so a band that was on its way down can be
        // predicted back up and arrive as a loud artefact. libopus makes the
        // prediction safe in proportion to how long the gap was
        // (celt_decoder.c:1362): a band already falling keeps falling along its
        // own slope, and one that is not falling is held to the quietest of the
        // last two frames.
        if !intra_ener && self.loss_duration != 0 {
            let missing = 10.min(self.loss_duration >> lm) as f32;
            let safety = match lm {
                0 => 1.5f32,
                1 => 0.5,
                _ => 0.0,
            };
            for c in 0..2 {
                for i in start_band..end_band {
                    let k = c * nb_ebands + i;
                    let (e1, e2) = (self.old_band_e2[k], self.old_band_e3[k]);
                    let mut e0 = self.old_band_e[k];
                    if e0 < e1.max(e2) {
                        let slope = (e1 - e0).max(0.5 * (e2 - e0)).min(2.0);
                        e0 -= ((1.0 + missing) * slope).max(0.0);
                        self.old_band_e[k] = e0.max(-20.0);
                    } else {
                        self.old_band_e[k] = e0.min(e1).min(e2);
                    }
                    // Shorter frames fluctuate more naturally — play it safe.
                    self.old_band_e[k] -= safety;
                }
            }
        }

        unquant_coarse_energy(
            mode,
            start_band,
            end_band,
            &mut self.old_band_e,
            intra_ener,
            rc,
            channels,
            lm,
        );
        self.w_tf_res[..nb_ebands].fill(0);
        let tf_res = &mut self.w_tf_res[..nb_ebands];
        tf_decode(start_band, end_band, is_transient, tf_res, lm as i32, rc);

        let spread_decision = if rc.tell() + 4 <= total_bits {
            rc.decode_icdf(&SPREAD_ICDF, 5)
        } else {
            SPREAD_NORMAL
        };

        self.w_cap[..nb_ebands].fill(0);
        let cap = &mut self.w_cap[..nb_ebands];
        for (i, cap_i) in cap.iter_mut().enumerate() {
            let n = (mode.e_bands[i + 1] - mode.e_bands[i]) << lm;
            *cap_i = ((mode.cache.caps[nb_ebands * (2 * lm + channels - 1) + i] as i32 + 64)
                * channels as i32
                * n as i32)
                >> 2;
        }

        self.w_offsets[..nb_ebands].fill(0);
        let offsets = &mut self.w_offsets[..nb_ebands];
        let mut dynalloc_logp = 6i32;
        let mut total_bits_bitres = total_bits << BITRES;
        let mut tell_frac = rc.tell_frac();
        for i in start_band..end_band {
            let width =
                channels as i32 * (mode.e_bands[i + 1] - mode.e_bands[i]) as i32 * (1 << lm);
            let quanta = (width << BITRES).min((6i32 << BITRES).max(width));
            let mut dynalloc_loop_logp = dynalloc_logp;
            let mut boost = 0i32;
            while tell_frac + (dynalloc_loop_logp << BITRES) < total_bits_bitres && boost < cap[i] {
                let flag = rc.decode_bit_logp(dynalloc_loop_logp as u32);
                tell_frac = rc.tell_frac();
                if !flag {
                    break;
                }
                boost += quanta;
                total_bits_bitres -= quanta;
                dynalloc_loop_logp = 1;
            }
            offsets[i] = boost;
            if boost > 0 {
                dynalloc_logp = dynalloc_logp.max(2) - 1;
                dynalloc_logp = dynalloc_logp.max(2);
            }
        }

        let alloc_trim = if rc.tell_frac() + (6 << BITRES) <= total_bits_bitres {
            rc.decode_icdf(&TRIM_ICDF, 7)
        } else {
            5
        };
        let anti_collapse_rsv = if is_transient && lm >= 2 {
            let remaining = (total_bits << BITRES) - rc.tell_frac() - 1;
            if remaining >= ((lm as i32 + 2) << BITRES) {
                1i32 << BITRES
            } else {
                0
            }
        } else {
            0
        };

        let mut intensity = 0;
        let mut dual_stereo_val = if channels == 2 { 1 } else { 0 };
        let mut balance = 0;
        self.w_pulses[..nb_ebands].fill(0);
        let pulses = &mut self.w_pulses[..nb_ebands];

        let ebands_stereo = if channels > 1 {
            nb_ebands * channels
        } else {
            nb_ebands
        };
        self.w_fine_priority[..ebands_stereo].fill(0);
        let fine_priority = &mut self.w_fine_priority[..ebands_stereo];
        self.w_ebits[..ebands_stereo].fill(0);
        let ebits = &mut self.w_ebits[..ebands_stereo];

        let alloc_bits = (total_bits << BITRES) - rc.tell_frac() - 1 - anti_collapse_rsv;
        let coded_bands = clt_compute_allocation(
            mode,
            start_band,
            end_band,
            offsets,
            cap,
            alloc_trim,
            &mut intensity,
            &mut dual_stereo_val,
            alloc_bits,
            &mut balance,
            pulses,
            ebits,
            fine_priority,
            channels as i32,
            lm as i32,
            rc,
            false,
            0,
            end_band as i32 - 1,
        );

        unquant_fine_energy(
            mode,
            start_band,
            end_band,
            &mut self.old_band_e,
            ebits,
            rc,
            channels,
        );

        if frame_size > DECODE_BUFFER_SIZE + overlap {
            return 0;
        }

        // Slide every channel's decode buffer, then fold the MDCT overlap a
        // concealed frame left raw (celt_decoder.c:1482 and :1531). The fold has
        // to see the buffer already slid, and both have to happen before any
        // channel's synthesis overlap-adds onto that tail.
        for c in 0..cc {
            let base = c * mem_size;
            self.decode_mem
                .copy_within(base + frame_size..base + mem_size, base);
        }
        if self.prefilter_and_fold {
            prefilter_and_fold(
                &mut self.decode_mem,
                &mut self.w_etmp,
                cc,
                mem_size,
                frame_size,
                mode.window,
                overlap,
                self.prefilter_period_old,
                self.prefilter_period,
                self.prefilter_gain_old,
                self.prefilter_gain,
                self.prefilter_tapset_old,
                self.prefilter_tapset,
            );
        }

        self.w_x[..frame_size * channels].fill(0.0);

        let x_pad_end = (frame_size * channels + STRIDE_ACCESS_PAD).min(self.w_x.len());
        let x = &mut self.w_x[..x_pad_end];
        self.w_collapse_masks[..nb_ebands * channels].fill(0);
        let collapse_masks = &mut self.w_collapse_masks[..nb_ebands * channels];

        let (x_split, y_split) = x.split_at_mut(frame_size);
        let y_opt = if channels == 2 { Some(y_split) } else { None };

        let mut dual_stereo = dual_stereo_val != 0;
        self.w_band_amp[..nb_ebands * channels].fill(0.0);
        let band_amp = &mut self.w_band_amp[..nb_ebands * channels];
        log2amp(mode, nb_ebands, band_amp, &self.old_band_e, channels);
        quant_all_bands(
            false,
            mode,
            start_band,
            end_band,
            x_split,
            y_opt,
            collapse_masks,
            band_amp,
            pulses,
            short_blocks,
            spread_decision,
            &mut dual_stereo,
            intensity as usize,
            tf_res,
            (total_bits << BITRES) - anti_collapse_rsv,
            &mut balance,
            rc,
            lm as i32,
            coded_bands,
            true,
            false,
            &mut self.rng,
        );
        // Trace X values for comparison with C decoder
        let mut anti_collapse_on = false;
        if anti_collapse_rsv > 0 {
            anti_collapse_on = rc.dec_bits(1) != 0;
        }

        unquant_energy_finalise(
            mode,
            start_band,
            end_band,
            &mut self.old_band_e,
            ebits,
            fine_priority,
            total_bits - rc.tell(),
            rc,
            channels,
        );
        if anti_collapse_on {
            // libopus passes `end`, not nbEBands: for narrower bandwidths (e.g.
            // SWB end=19) anti-collapsing the uncoded bands would burn PRNG draws
            // and desync the noise-fill seed for every subsequent frame.
            self.rng = crate::celt::bands::anti_collapse(
                mode,
                x,
                collapse_masks,
                lm as i32,
                channels,
                frame_size,
                start_band,
                end_band,
                &self.old_band_e,
                &self.old_band_e2,
                &self.old_band_e3,
                pulses,
                self.rng,
            );
        }

        // libopus celt_decoder.c:1107: silence floors the coded channels' energy to
        // -28 (so the next frame's inter prediction starts from "very quiet") and
        // renders a zero spectrum — the frame's output is just the MDCT overlap
        // fade-out of the previous frame.
        if silence {
            for i in 0..channels * nb_ebands {
                self.old_band_e[i] = -28.0;
            }
        }

        // Recompute band_amp after unquant_energy_finalise, which adjusts old_band_e.
        // (Mirrors the encoder's resynth path: log2amp is called after quant_energy_finalise.)
        log2amp(mode, nb_ebands, band_amp, &self.old_band_e, channels);
        self.w_freq[..frame_size * channels].fill(0.0);
        let freq = &mut self.w_freq[..frame_size * channels];
        if !silence {
            denormalise_bands(
                mode,
                x,
                freq,
                band_amp,
                start_band,
                end_band,
                channels,
                (1 << lm) as usize,
            );
        }
        // Downmixing a stereo stream to a mono output (C=2, CC=1). libopus sums
        // the two denormalised spectra and runs a *single* inverse MDCT over the
        // sum (celt_decoder.c, `celt_synthesis`), so the decoder keeps one
        // overlap-add, prefilter and preemphasis history rather than averaging
        // two independently synthesised channels afterwards. The `fc` clamp in
        // the synthesis loop below then reads this summed channel 0.
        if cc == 1 && channels == 2 {
            let (ch0, ch1) = freq.split_at_mut(frame_size);
            for (a, b) in ch0.iter_mut().zip(ch1.iter()) {
                *a = 0.5 * *a + 0.5 * *b;
            }
        }
        // Always trace freq and band_amp for comparison

        let (shift, b) = if short_blocks {
            (mode.max_lm, 1 << lm)
        } else {
            (mode.max_lm - lm, 1)
        };
        let n = frame_size / b;

        let out_syn_idx = DECODE_BUFFER_SIZE - frame_size;
        for c in 0..cc {
            // A mono packet (C=1) in a stereo decoder (CC=2) renders its single
            // decoded channel into both outputs: re-run the iMDCT reading channel
            // 0's freq (fc clamps to C-1). Re-synthesis (not a decode_mem copy) is
            // required so the per-channel postfilter/deemph below run exactly once
            // each; denormalise_bands leaves freq unmodified so this is exact.
            let fc = c.min(channels - 1);
            let channel_mem_offset = c * mem_size;

            for i in 0..b {
                let block_freq_idx = fc * frame_size + i;
                // Stride between short-block MDCT outputs is short_mdct_size (not n).
                // In libopus: out_syn[c] + NB*b, where NB = mode->shortMdctSize.
                // For non-transient b=1, i*n == 0 either way.
                let block_stride = if short_blocks {
                    mode.short_mdct_size
                } else {
                    n
                };
                let block_out_idx = channel_mem_offset + out_syn_idx + i * block_stride;
                let available_len = self.decode_mem.len() - block_out_idx;
                if available_len < n + overlap {
                    panic!(
                        "MDCT backward buffer too small: need {}, have {} (out_syn_idx={}, n={}, overlap={})",
                        n + overlap,
                        available_len,
                        out_syn_idx,
                        n,
                        overlap
                    );
                }
                self.mode.mdct.backward(
                    &freq[block_freq_idx..],
                    &mut self.decode_mem[block_out_idx..],
                    mode.window,
                    overlap,
                    shift,
                    b,
                );
            }

            // `celt_decoder.c` saturates the synthesis here, but `SATURATE` is
            // the identity in a float build (`arch.h`), so there is nothing to
            // transcribe: the clamp was a read-modify-write pass over the whole
            // frame that could not change a sample.
            let pcm_frame = &mut self.w_pcm_frame[..frame_size];
            pcm_frame.copy_from_slice(
                &self.decode_mem[channel_mem_offset + out_syn_idx
                    ..channel_mem_offset + out_syn_idx + frame_size],
            );
            if pf_on || self.prefilter_gain > 0.0 || self.prefilter_gain_old > 0.0 {
                // Set up w_post = [prefilter_mem | pcm_frame] for history access.
                // We apply combfilter in-place on w_post[COMBFILTER_MAXPERIOD..] so that
                // later samples can reference already-filtered earlier samples, matching C's
                // in-place comb_filter behavior.
                self.w_post[..COMBFILTER_MAXPERIOD].copy_from_slice(
                    &self.prefilter_mem[c * COMBFILTER_MAXPERIOD..(c + 1) * COMBFILTER_MAXPERIOD],
                );
                self.w_post[COMBFILTER_MAXPERIOD..COMBFILTER_MAXPERIOD + frame_size]
                    .copy_from_slice(pcm_frame);

                let short_n = mode.short_mdct_size;
                // Call 1: first short_n samples, transition old→current params
                // Apply in-place on w_post[COMBFILTER_MAXPERIOD..], output overwrites input
                comb_filter_inplace(
                    &mut self.w_post,
                    COMBFILTER_MAXPERIOD,
                    self.prefilter_period_old,
                    self.prefilter_period,
                    short_n,
                    self.prefilter_gain_old,
                    self.prefilter_gain,
                    self.prefilter_tapset_old,
                    self.prefilter_tapset,
                    mode.window,
                    overlap,
                );
                if lm != 0 {
                    // Call 2: remaining N-short_n samples, transition current→new params
                    comb_filter_inplace(
                        &mut self.w_post,
                        COMBFILTER_MAXPERIOD + short_n,
                        self.prefilter_period,
                        pitch_index,
                        frame_size - short_n,
                        self.prefilter_gain,
                        gain1,
                        self.prefilter_tapset,
                        prefilter_tapset as i32,
                        mode.window,
                        overlap,
                    );
                }

                pcm_frame.copy_from_slice(
                    &self.w_post[COMBFILTER_MAXPERIOD..COMBFILTER_MAXPERIOD + frame_size],
                );

                self.decode_mem[channel_mem_offset + out_syn_idx
                    ..channel_mem_offset + out_syn_idx + frame_size]
                    .copy_from_slice(pcm_frame);
            }
            // The postfilter's history window slides: the tail of the old
            // history moves down and this frame lands on top of it. Done in
            // place, because staging it through a temporary meant zeroing a
            // buffer that was then overwritten in full.
            let base = c * COMBFILTER_MAXPERIOD;
            if frame_size >= COMBFILTER_MAXPERIOD {
                self.prefilter_mem[base..base + COMBFILTER_MAXPERIOD]
                    .copy_from_slice(&pcm_frame[frame_size - COMBFILTER_MAXPERIOD..]);
            } else {
                self.prefilter_mem
                    .copy_within(base + frame_size..base + COMBFILTER_MAXPERIOD, base);
                self.prefilter_mem
                    [base + COMBFILTER_MAXPERIOD - frame_size..base + COMBFILTER_MAXPERIOD]
                    .copy_from_slice(pcm_frame);
            }
        }

        // De-emphasis, once the whole frame exists for every channel. The
        // reference reaches this point the same way, with `out_syn[c]` pointing
        // into the decode buffer, writes the caller's buffer interleaved, and
        // takes the stereo case in one pass; see `deemphasis_stereo` for why
        // that is worth a separate routine.
        let downsample = self.downsample;
        let out_len = frame_size / downsample;
        let syn = out_syn_idx;
        if cc == 2 && downsample == 1 {
            let (mem_a, mem_b) = self.decode_mem.split_at(mem_size);
            deemphasis_stereo(
                &mem_a[syn..syn + frame_size],
                &mem_b[syn..syn + frame_size],
                &mut pcm[..2 * out_len],
                mode.preemph[0],
                &mut self.preemph_mem,
            );
        } else {
            for c in 0..cc {
                let src = c * mem_size + syn;
                self.preemph_mem[c] = deemphasis(
                    &self.decode_mem[src..src + frame_size],
                    &mut pcm[c..],
                    cc,
                    downsample,
                    mode.preemph[0],
                    self.preemph_mem[c],
                );
            }
        }

        self.prefilter_period_old = self.prefilter_period;
        self.prefilter_gain_old = self.prefilter_gain;
        self.prefilter_tapset_old = self.prefilter_tapset;

        if pf_on {
            self.prefilter_period = pitch_index;
            self.prefilter_gain = gain1;
            self.prefilter_tapset = prefilter_tapset as i32;
        } else {
            self.prefilter_period = COMBFILTER_MINPERIOD;
            self.prefilter_gain = 0.0;
            self.prefilter_tapset = 0;
        }

        if lm > 0 {
            self.prefilter_period_old = self.prefilter_period;
            self.prefilter_gain_old = self.prefilter_gain;
            self.prefilter_tapset_old = self.prefilter_tapset;
        }

        // libopus celt_decoder.c:1140: after a mono frame in a stereo decoder,
        // replicate channel 0's coarse energy to channel 1 — this keeps ch1's
        // prediction state current through mono runs (and is what makes the
        // pre-decode MAX-merge a first-frame-only event).
        if channels == 1 {
            let (ch0, ch1) = self.old_band_e.split_at_mut(nb_ebands);
            ch1[..nb_ebands].copy_from_slice(&ch0[..nb_ebands]);
        }

        // oldLogE/oldLogE2 updates run over ALL state channels (2*nbEBands in
        // libopus), not just the coded ones.
        if !is_transient {
            self.old_band_e3.copy_from_slice(&self.old_band_e2);
            self.old_band_e2.copy_from_slice(&self.old_band_e);
        } else {
            for i in 0..2 * nb_ebands {
                self.old_band_e2[i] = self.old_band_e2[i].min(self.old_band_e[i]);
            }
        }

        // The noise floor concealment decays towards. It may rise by only 2.4 dB
        // per second normally, but a gap gets the whole gap's worth at once, so a
        // stream that comes back from DTX does not conceal towards a floor that
        // belongs to what was playing before the silence.
        let max_background_increase = (160.min(self.loss_duration + (1 << lm)) as f32) * 0.001;
        for i in 0..2 * nb_ebands {
            self.background_log_e[i] =
                (self.background_log_e[i] + max_background_increase).min(self.old_band_e[i]);
        }

        // "In case start or end were to change" (celt_decoder.c:1162-1174): zero
        // the coarse energy outside [start, end) and floor the log history, for
        // BOTH state channels. Matters for hybrid (start=17) and narrower
        // bandwidths (end<21) mixing with full-band frames in one stream.
        for c in 0..2 {
            for i in 0..start_band {
                self.old_band_e[c * nb_ebands + i] = 0.0;
                self.old_band_e2[c * nb_ebands + i] = -28.0;
                self.old_band_e3[c * nb_ebands + i] = -28.0;
            }
            for i in end_band..nb_ebands {
                self.old_band_e[c * nb_ebands + i] = 0.0;
                self.old_band_e2[c * nb_ebands + i] = -28.0;
                self.old_band_e3[c * nb_ebands + i] = -28.0;
            }
        }

        self.rng = rc.rng;
        self.loss_duration = 0;
        self.plc_duration = 0;
        self.last_frame_type = FRAME_NORMAL;
        self.prefilter_and_fold = false;

        frame_size
    }

    /// Packet-loss concealment for a lost CELT frame — a port of libopus
    /// `celt_decode_lost` (celt_decoder.c). For the first 100 ms of a burst it
    /// uses the pitch-based branch (LPC-whitened excitation extrapolated at the
    /// last pitch period, resynthesized through the LPC filter — good for
    /// tonal/music content); beyond that it falls back to the noise-based branch
    /// (spectrally-shaped, energy-decayed random excitation). Both fill the
    /// decode buffer, then this deemphasises to `pcm` (interleaved, /32768).
    /// Real attenuating audio instead of silence.
    ///
    /// `start_band` / `end_band` are libopus's `st->start` / `st->end`: the
    /// hybrid layer conceals only its own high band (start 17), and
    /// `celt_decode_lost` forces the noise-based branch whenever `start != 0`,
    /// because the pitch-based branch extrapolates a full-band excitation it
    /// has no low band for.
    pub fn conceal_lost_bands(
        &mut self,
        frame_size: usize,
        pcm: &mut [f32],
        start_band: usize,
        end_band: usize,
    ) {
        let n = frame_size * self.downsample;
        let end_band = end_band.min(self.mode.nb_ebands);
        let lm = self.lm_for(n);
        let noise_based = self.plc_duration >= PLC_NOISE_AFTER || start_band != 0 || self.skip_plc;
        if noise_based {
            self.conceal_fill_noise(n, lm, start_band, end_band);
        } else {
            self.conceal_fill_pitch(n);
        }

        // Deemphasise the concealed frame (decode_mem out_syn) to interleaved pcm.
        let mode = self.mode;
        let c = self.channels;
        let overlap = mode.overlap;
        let mem_size = DECODE_BUFFER_SIZE + overlap;
        let out_syn_idx = DECODE_BUFFER_SIZE - n;
        let coef = mode.preemph[0];
        let downsample = self.downsample;
        for ch in 0..c {
            let src = ch * mem_size + out_syn_idx;
            self.preemph_mem[ch] = deemphasis(
                &self.decode_mem[src..src + n],
                &mut pcm[ch..],
                c,
                downsample,
                coef,
                self.preemph_mem[ch],
            );
        }

        // The postfilter's history. libopus filters in place inside the decode
        // buffer, so a concealed frame becomes history for the next frame's
        // postfilter automatically; this decoder keeps that window as a separate
        // buffer and so has to refresh it here. Leaving it stale made the frame
        // after a loss comb-filter against the last *decoded* frame, which is a
        // frame-length out of place.
        for ch in 0..c {
            let base = ch * mem_size;
            self.prefilter_mem[ch * COMBFILTER_MAXPERIOD..(ch + 1) * COMBFILTER_MAXPERIOD]
                .copy_from_slice(
                    &self.decode_mem[base + DECODE_BUFFER_SIZE - COMBFILTER_MAXPERIOD
                        ..base + DECODE_BUFFER_SIZE],
                );
        }

        // Saturate so a very long silence cannot wrap either counter.
        self.loss_duration = (self.loss_duration + (1 << lm)).min(10_000);
        self.plc_duration = (self.plc_duration + (1 << lm)).min(10_000);
        self.last_frame_type = if noise_based {
            FRAME_PLC_NOISE
        } else {
            FRAME_PLC_PERIODIC
        };
    }

    /// The frame's `LM`: which of the mode's frame sizes `n` is, as a power of
    /// two over the short-MDCT size.
    fn lm_for(&self, n: usize) -> usize {
        let mut lm = 0usize;
        while (self.mode.short_mdct_size << lm) != n && lm < self.mode.max_lm {
            lm += 1;
        }
        lm
    }

    /// Noise-based concealment branch (celt_decode_lost, `noise_based`): fill the
    /// decode buffer's out_syn region with an energy-decayed random spectrum.
    fn conceal_fill_noise(&mut self, n: usize, lm: usize, start: usize, end: usize) {
        let mode = self.mode;
        let nb_ebands = mode.nb_ebands;
        let overlap = mode.overlap;
        let c = self.channels;
        // libopus: effEnd = IMAX(start, IMIN(end, mode->effEBands)).
        let eff_end = end.min(mode.eff_ebands).max(start);
        let mem_size = DECODE_BUFFER_SIZE + overlap;

        // Slide the decode buffer first, then fold whatever a previous
        // concealment left unfolded, so the synthesis below overlap-adds onto a
        // tail that is already in the MDCT's domain.
        for ch in 0..c {
            let base = ch * mem_size;
            self.decode_mem
                .copy_within(base + n..base + DECODE_BUFFER_SIZE + overlap, base);
        }
        if self.prefilter_and_fold {
            prefilter_and_fold(
                &mut self.decode_mem,
                &mut self.w_etmp,
                c,
                mem_size,
                n,
                mode.window,
                overlap,
                self.prefilter_period_old,
                self.prefilter_period,
                self.prefilter_gain_old,
                self.prefilter_gain,
                self.prefilter_tapset_old,
                self.prefilter_tapset,
            );
        }

        let decay = if self.loss_duration == 0 {
            1.5f32
        } else {
            0.5f32
        };
        for ch in 0..c {
            for i in start..end {
                let floor = self.background_log_e[ch * nb_ebands + i];
                let e = &mut self.old_band_e[ch * nb_ebands + i];
                *e = (*e - decay).max(floor);
            }
        }

        let mut seed = self.rng;
        self.w_x[..n * c].fill(0.0);
        for ch in 0..c {
            for i in start..eff_end {
                let boffs = n * ch + ((mode.e_bands[i] as usize) << lm);
                let blen = ((mode.e_bands[i + 1] - mode.e_bands[i]) as usize) << lm;
                for j in 0..blen {
                    seed = crate::celt::bands::celt_lcg_rand(seed);
                    self.w_x[boffs + j] = ((seed as i32) >> 20) as f32;
                }
                crate::celt::bands::renormalise_vector(
                    &mut self.w_x[boffs..boffs + blen],
                    blen,
                    1.0,
                );
            }
        }
        self.rng = seed;

        self.w_band_amp[..nb_ebands * c].fill(0.0);
        let band_amp = &mut self.w_band_amp[..nb_ebands * c];
        log2amp(mode, nb_ebands, band_amp, &self.old_band_e, c);
        self.w_freq[..n * c].fill(0.0);
        let freq = &mut self.w_freq[..n * c];
        denormalise_bands(
            mode,
            &self.w_x,
            freq,
            band_amp,
            start,
            eff_end,
            c,
            1usize << lm,
        );

        let shift = mode.max_lm - lm;
        let out_syn_idx = DECODE_BUFFER_SIZE - n;
        const SIG_SAT: f32 = 536870911.0;
        for ch in 0..c {
            let out = ch * mem_size + out_syn_idx;
            self.mode.mdct.backward(
                &freq[ch * n..],
                &mut self.decode_mem[out..],
                mode.window,
                overlap,
                shift,
                1,
            );
            for i in 0..n {
                let v = &mut self.decode_mem[out + i];
                *v = v.clamp(-SIG_SAT, SIG_SAT);
            }
        }

        // Run the postfilter over the concealed frame with the parameters the
        // last decoded frame left, so the comb filtering does not stop dead at
        // the loss.
        self.prefilter_period = self.prefilter_period.max(COMBFILTER_MINPERIOD);
        self.prefilter_period_old = self.prefilter_period_old.max(COMBFILTER_MINPERIOD);
        let short_n = mode.short_mdct_size;
        for ch in 0..c {
            let out = ch * mem_size + out_syn_idx;
            comb_filter_inplace(
                &mut self.decode_mem,
                out,
                self.prefilter_period_old,
                self.prefilter_period,
                short_n,
                self.prefilter_gain_old,
                self.prefilter_gain,
                self.prefilter_tapset_old,
                self.prefilter_tapset,
                mode.window,
                overlap,
            );
            if lm != 0 {
                comb_filter_inplace(
                    &mut self.decode_mem,
                    out + short_n,
                    self.prefilter_period,
                    self.prefilter_period,
                    n - short_n,
                    self.prefilter_gain,
                    self.prefilter_gain,
                    self.prefilter_tapset,
                    self.prefilter_tapset,
                    mode.window,
                    overlap,
                );
            }
        }
        self.prefilter_period_old = self.prefilter_period;
        self.prefilter_gain_old = self.prefilter_gain;
        self.prefilter_tapset_old = self.prefilter_tapset;

        // The synthesis above already folded its own overlap, and a noise frame
        // is never followed by a pitch-extrapolated one until two good packets
        // have arrived.
        self.prefilter_and_fold = false;
        self.skip_plc = true;
    }

    /// Pitch-based concealment branch (celt_decode_lost, pitch-based): extrapolate
    /// the LPC-whitened excitation at the last pitch period with per-period decay,
    /// resynthesize through the LPC filter, then TDAC-fold the overlap.
    fn conceal_fill_pitch(&mut self, n: usize) {
        let mode = self.mode;
        let overlap = mode.overlap;
        let c = self.channels;
        let mem_size = DECODE_BUFFER_SIZE + overlap;
        const MAX_PERIOD: usize = COMBFILTER_MAXPERIOD;
        let ord = PLC_LPC_ORDER;
        let out_syn_idx = DECODE_BUFFER_SIZE - n;
        const SIG_SAT: f32 = 536870911.0;
        let window = mode.window;

        // Pitch lag: search when this is the first loss of a burst, reuse it
        // for the rest.
        let first = self.last_frame_type != FRAME_PLC_PERIODIC;
        let mut fade = 1.0f32;
        if first {
            let mut lp = vec![0.0f32; DECODE_BUFFER_SIZE >> 1];
            let slices: Vec<&[f32]> = (0..c)
                .map(|ch| &self.decode_mem[ch * mem_size..ch * mem_size + DECODE_BUFFER_SIZE])
                .collect();
            crate::celt::pitch::pitch_downsample(&slices, &mut lp, DECODE_BUFFER_SIZE >> 1, c, 2);
            let pr = crate::celt::pitch::pitch_search(
                &lp[PLC_PITCH_LAG_MAX >> 1..],
                &lp,
                DECODE_BUFFER_SIZE - PLC_PITCH_LAG_MAX,
                PLC_PITCH_LAG_MAX - PLC_PITCH_LAG_MIN,
            );
            self.last_pitch_index = (PLC_PITCH_LAG_MAX - pr) as i32;
        } else {
            fade = 0.8;
        }
        let pitch_index = (self.last_pitch_index.max(1) as usize).min(MAX_PERIOD - 1);
        let exc_length = (2 * pitch_index).min(MAX_PERIOD);

        for ch in 0..c {
            let base = ch * mem_size;
            // exc[k] = exc_buf[ord + k] for k in -ord..MAX_PERIOD.
            let mut exc_buf = vec![0.0f32; MAX_PERIOD + ord];
            for (i, v) in exc_buf.iter_mut().enumerate() {
                *v = self.decode_mem[base + DECODE_BUFFER_SIZE - MAX_PERIOD - ord + i];
            }
            if first {
                let mut ac = vec![0.0f32; ord + 1];
                crate::celt::lpc::autocorr(
                    &exc_buf[ord..ord + MAX_PERIOD],
                    &mut ac,
                    Some(window),
                    overlap,
                    ord,
                    MAX_PERIOD,
                );
                ac[0] *= 1.0001; // -40 dB noise floor
                for i in 1..=ord {
                    // Written as two separate multiplies by `i`, the way the
                    // reference is, so the intermediate rounds identically.
                    ac[i] -= ac[i] * (0.008 * 0.008) * i as f32 * i as f32; // lag windowing
                }
                let mut lc = vec![0.0f32; ord];
                crate::celt::lpc::lpc(&mut lc, &ac, ord);
                self.plc_lpc[ch * ord..ch * ord + ord].copy_from_slice(&lc);
            }
            let lc: Vec<f32> = self.plc_lpc[ch * ord..ch * ord + ord].to_vec();

            // Whiten the last exc_length excitation samples (celt_fir with history
            // — pass the ord preceding samples and read outputs at [ord..]).
            {
                let x = &exc_buf[MAX_PERIOD - exc_length..];
                let mut y = vec![0.0f32; ord + exc_length];
                crate::celt::lpc::celt_fir(x, &lc, &mut y, ord + exc_length, ord);
                for i in 0..exc_length {
                    exc_buf[ord + MAX_PERIOD - exc_length + i] = y[ord + i];
                }
            }

            // Decay factor from the excitation energy ratio (avoid adding energy).
            let decay_length = exc_length >> 1;
            let mut e1 = 1.0f32;
            let mut e2 = 1.0f32;
            for i in 0..decay_length {
                let a = exc_buf[ord + MAX_PERIOD - decay_length + i];
                e1 += a * a;
                let b = exc_buf[ord + MAX_PERIOD - 2 * decay_length + i];
                e2 += b * b;
            }
            e1 = e1.min(e2);
            let decay = (e1 / e2).sqrt();

            // Shift decode buffer one frame left.
            self.decode_mem
                .copy_within(base + n..base + DECODE_BUFFER_SIZE, base);

            // Extrapolate at period `pitch_index`, attenuating each period.
            let extrapolation_offset = MAX_PERIOD - pitch_index;
            let extrapolation_len = n + overlap;
            let mut atten = fade * decay;
            let mut j = 0usize;
            let mut s1 = 0.0f32;
            for i in 0..extrapolation_len {
                if j >= pitch_index {
                    j -= pitch_index;
                    atten *= decay;
                }
                self.decode_mem[base + out_syn_idx + i] =
                    atten * exc_buf[ord + extrapolation_offset + j];
                let tmp = self.decode_mem
                    [base + (DECODE_BUFFER_SIZE - MAX_PERIOD - n) + extrapolation_offset + j];
                s1 += tmp * tmp;
                j += 1;
            }

            // Resynthesize: excitation -> signal through the LPC synthesis filter.
            let mut lpc_mem = [0.0f32; PLC_LPC_ORDER];
            for (i, v) in lpc_mem.iter_mut().enumerate().take(ord) {
                *v = self.decode_mem[base + DECODE_BUFFER_SIZE - n - 1 - i];
            }
            let extrap: Vec<f32> = self.decode_mem
                [base + out_syn_idx..base + out_syn_idx + extrapolation_len]
                .to_vec();
            crate::celt::lpc::celt_iir(
                &extrap,
                &lc,
                &mut self.decode_mem[base + out_syn_idx..base + out_syn_idx + extrapolation_len],
                extrapolation_len,
                ord,
                &mut lpc_mem[..ord],
            );
            for i in 0..extrapolation_len {
                let v = &mut self.decode_mem[base + out_syn_idx + i];
                *v = v.clamp(-SIG_SAT, SIG_SAT);
            }

            // Explosion / NaN guard (the !(S1 > .2*S2) test also catches IIR NaNs).
            let mut s2 = 0.0f32;
            for i in 0..extrapolation_len {
                let t = self.decode_mem[base + out_syn_idx + i];
                s2 += t * t;
            }
            // Reversed so a NaN correlation takes the "no usable pitch" branch.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            let no_pitch = !(s1 > 0.2 * s2);
            if no_pitch {
                for i in 0..extrapolation_len {
                    self.decode_mem[base + out_syn_idx + i] = 0.0;
                }
            } else if s1 < s2 {
                let ratio = ((s1 + 1.0) / (s2 + 1.0)).sqrt();
                for i in 0..overlap {
                    let g = 1.0 - window[i] * (1.0 - ratio);
                    self.decode_mem[base + out_syn_idx + i] *= g;
                }
                for i in overlap..extrapolation_len {
                    self.decode_mem[base + out_syn_idx + i] *= ratio;
                }
            }
        }

        // The MDCT window this wrote runs `overlap` samples past the frame and is
        // left unfolded: whatever decodes next folds it with its own postfilter
        // parameters, which are not known here.
        self.prefilter_and_fold = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::celt::modes;
    use crate::range_coder::RangeCoder;

    // Regression test: directly drive CeltEncoder with an invalid frame_size=48,
    // bypassing the OpusEncoder::encode() validation layer.
    //
    // This reproduces the crash that was reported against opus-rs 0.1.19 when
    // G.729-decoded PCM (8 kHz) reached the 48 kHz Opus encoder without correct
    // resampling, producing a 48-sample frame instead of 480.
    //
    // Root cause: the lm-search in encode_impl finds no valid match for frame_size=48
    // (valid sizes are 120, 240, 480, 960) and silently falls back to lm=0.
    // With lm=0 and shift=max_lm=3: n=1920>>3=240, n2=120, overlap2=60.
    // The in_buf slice has only frame_size+overlap=168 elements, but forward()
    // requires input.len() >= n2+overlap2 = 180, so it panics immediately.
    // In opus-rs 0.1.19 this assertion was absent and the crash reached the MDCT
    // output write: "index out of bounds: the len is 48 but the index is 119".
    //
    // Either way: the call panics, confirming the crash path is real.
    // The fix in OpusEncoder::encode() returns Err before reaching CeltEncoder.
    #[test]
    #[should_panic]
    fn test_celt_frame_size_48_panics_confirms_crash_path() {
        let mode = modes::default_mode();
        let mut enc = CeltEncoder::new(mode, 1);
        // frame_size=48: lm-search fails, falls back to lm=0.
        // forward() will panic — either on the input-size assertion (0.1.21+) or
        // on the output write (0.1.19): "len is 48 but the index is 119".
        let pcm = vec![0.0f32; 48 + mode.overlap]; // supply ≥ frame_size samples
        let mut rc = RangeCoder::new_encoder(100);
        enc.encode_with_budget(&pcm, 48, &mut rc, 0, 21, 800);
    }

    // Prefilter/postfilter inversion, MDCT bypassed: run the real run_prefilter
    // per frame (with the real signalling quantization of gain/period), feed the
    // FILTERED stream straight into the decoder's postfilter sequence (call 1
    // old->current over shortMdctSize, call 2 current->new with the crossfade),
    // honoring the 120-sample MDCT delay. If the encoder applies exactly what it
    // signals with the timing the decoder inverts, the round trip is ~identity.
    #[test]
    fn prefilter_postfilter_inversion() {
        let mode = modes::default_mode();
        let n = 960usize;
        let overlap = mode.overlap; // 120
        let short_n = mode.short_mdct_size; // 120
        let frames = 100usize;
        let max_period = COMBFILTER_MAXPERIOD;

        // Signal designed to TOGGLE the prefilter: alternating strongly periodic
        // stretches (varying pitch) and noise bursts.
        let total = frames * n;
        let mut x = vec![0.0f32; total];
        let mut rng = 0x12345678u32;
        let mut next = || {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            (rng >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        for (t, v) in x.iter_mut().enumerate() {
            let seg = t / (n * 10);
            let phase = t as f32;
            *v = match seg % 4 {
                0 => (phase * std::f32::consts::TAU / 147.0).sin() * 8000.0, // ~326 Hz
                1 => next() * 6000.0,
                2 => {
                    ((phase * std::f32::consts::TAU / 89.0).sin()
                        + 0.5 * (phase * std::f32::consts::TAU / 44.5).sin())
                        * 7000.0
                }
                _ => (phase * std::f32::consts::TAU / 480.0).sin() * 5000.0, // 100 Hz
            };
        }

        // ---- encoder side ----
        let mut pre = vec![0.0f32; max_period + n];
        let mut pitch_buf = vec![0.0f32; (max_period + n) >> 1];
        let mut prefilter_mem = vec![0.0f32; max_period];
        let mut in_mem = vec![0.0f32; overlap];
        let (mut prev_t, mut prev_g) = (COMBFILTER_MINPERIOD, 0.0f32);
        let analysis = AnalysisInfo::default();
        let mut filtered = vec![0.0f32; total];
        let mut params = Vec::new(); // (pf_on, T, g) per frame
        let mut in_buf = vec![0.0f32; n + overlap];
        for k in 0..frames {
            in_buf[..overlap].copy_from_slice(&in_mem);
            in_buf[overlap..].copy_from_slice(&x[k * n..(k + 1) * n]);
            let (pf_on, g1, t1) = run_prefilter(
                &mut in_buf,
                &mut prefilter_mem,
                prev_t,
                prev_g,
                0, // prefilter_tapset (old)
                0, // tapset_decision (new)
                mode.window,
                1,
                n,
                overlap,
                &mut pre,
                &mut pitch_buf,
                &analysis,
                0,
                159,
            );
            filtered[k * n..(k + 1) * n].copy_from_slice(&in_buf[overlap..]);
            in_mem.copy_from_slice(&in_buf[n..]);
            params.push((pf_on, t1, g1));
            // encoder end-of-frame state update
            prev_t = if pf_on { t1 } else { COMBFILTER_MINPERIOD };
            prev_g = if pf_on { g1 } else { 0.0 };
        }

        // ---- decoder side (postfilter only), 120-sample MDCT delay ----
        let mut delayed = vec![0.0f32; total];
        delayed[short_n..].copy_from_slice(&filtered[..total - short_n]);
        let mut w = vec![0.0f32; max_period + n];
        let mut post_mem = vec![0.0f32; max_period];
        let (mut d_t_old, mut d_g_old) = (COMBFILTER_MINPERIOD, 0.0f32);
        let (mut d_t, mut d_g) = (COMBFILTER_MINPERIOD, 0.0f32);
        let mut out = vec![0.0f32; total];
        for k in 0..frames {
            let (pf_on, sig_t, sig_g) = params[k];
            let (gain1, pitch_index) = if pf_on {
                (sig_g, sig_t)
            } else {
                (0.0, COMBFILTER_MINPERIOD)
            };
            w[..max_period].copy_from_slice(&post_mem);
            w[max_period..].copy_from_slice(&delayed[k * n..(k + 1) * n]);
            if pf_on || d_g > 0.0 || d_g_old > 0.0 {
                comb_filter_inplace(
                    &mut w,
                    max_period,
                    d_t_old,
                    d_t,
                    short_n,
                    d_g_old,
                    d_g,
                    0,
                    0,
                    mode.window,
                    overlap,
                );
                comb_filter_inplace(
                    &mut w,
                    max_period + short_n,
                    d_t,
                    pitch_index,
                    n - short_n,
                    d_g,
                    gain1,
                    0,
                    0,
                    mode.window,
                    overlap,
                );
            }
            out[k * n..(k + 1) * n].copy_from_slice(&w[max_period..]);
            post_mem.copy_from_slice(&w[n..]);
            // decoder end-of-frame chain, then the lm > 0 override
            if pf_on {
                d_t = pitch_index;
                d_g = gain1;
            } else {
                d_t = COMBFILTER_MINPERIOD;
                d_g = 0.0;
            }
            d_t_old = d_t;
            d_g_old = d_g;
        }

        // ---- compare out (delayed by short_n) against x ----
        let m = total - 2 * n;
        let mut se = 0.0f64;
        let mut sx = 0.0f64;
        for t in n..m {
            let e = (out[t + short_n] - x[t]) as f64;
            se += e * e;
            sx += (x[t] as f64) * (x[t] as f64);
        }
        let snr = 10.0 * (sx / se.max(1e-30)).log10();
        let engaged = params.iter().filter(|p| p.0).count();
        assert!(
            engaged > frames / 4,
            "prefilter never engaged ({engaged}/{frames}) — test signal too weak"
        );
        assert!(
            snr > 90.0,
            "prefilter/postfilter round trip not transparent: SNR={snr:.1} dB (engaged {engaged}/{frames})"
        );
    }

    /// Deterministic pseudo-random samples in [-1, 1).
    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1 << 23) as f32 - 1.0
            })
            .collect()
    }

    /// The comb filter's SIMD paths must agree with the scalar definition.
    ///
    /// This runs whichever kernel the host actually dispatches to — NEON here,
    /// SSE or AVX on x86 — against `comb_filter_const_scalar`, so the same test
    /// covers a different kernel on each target.
    #[test]
    fn simd_comb_filter_matches_the_scalar_definition() {
        for &t in &[3usize, 15, 64, 200] {
            for &n in &[1usize, 3, 8, 15, 16, 33, 120, 480] {
                let pad = t + 4;
                let x = noise(pad + n + 8, 0x517E_0001 ^ n as u32);
                let x_idx = pad;
                for &(g10, g11, g12) in &[
                    (0.0f32, 0.0f32, 0.0f32),
                    (0.3, 0.2, 0.1),
                    (-0.75, 0.5, -0.25),
                ] {
                    let mut got = vec![0.0f32; n + 4];
                    let mut want = vec![0.0f32; n + 4];
                    comb_filter_const(&mut got, &x, 0, x_idx, t, n, g10, g11, g12);
                    comb_filter_const_scalar(&mut want, &x, 0, x_idx, t, n, g10, g11, g12);
                    for (i, (g, w)) in got.iter().zip(&want).enumerate().take(n) {
                        assert!(
                            (g - w).abs() <= 1e-5 * (1.0 + w.abs()),
                            "comb_filter t={t} n={n} gains=({g10},{g11},{g12}) sample {i}: \
                             {g} vs {w}"
                        );
                    }
                }
            }
        }
    }

    /// `l1_metric` takes a SIMD path only at n >= 16, so the scalar tail and the
    /// vector body have to agree on the boundary sizes either side of it.
    #[test]
    fn simd_l1_metric_matches_the_scalar_definition() {
        for &n in &[1usize, 8, 15, 16, 17, 31, 32, 33, 120, 960] {
            let tmp = noise(n, 0x1111_2222 ^ n as u32);
            for &lm in &[0i32, 1, 2, 3] {
                for &bias in &[0.0f32, 0.05, 0.5] {
                    let l1: f32 = tmp[..n].iter().map(|v| v.abs()).sum();
                    let want = l1 + (lm as f32) * bias * l1;
                    let got = l1_metric(&tmp, n, lm, bias);
                    assert!(
                        (got - want).abs() <= 1e-4 * (1.0 + want.abs()),
                        "l1_metric n={n} lm={lm} bias={bias}: {got} vs {want}"
                    );
                }
            }
        }
    }

    /// Name the vector kernels this build will actually dispatch to.
    ///
    /// Every other `simd_*` test runs whichever path the host CPU selects, so a
    /// pass means the selected path matched its scalar definition — but says
    /// nothing about *which* path that was. On a machine without AVX2 the whole
    /// AVX2 family goes untested and every one of those tests still reports
    /// success, which is the failure mode worth guarding: the suite looks the
    /// same whether it covered those kernels or skipped them.
    ///
    /// So this prints the answer, and turns it into a failure when the caller
    /// states what it expected. Set `OPUS_PURE_REQUIRE_SIMD` to a comma-separated
    /// list of the names below and the test fails if any is missing. CI sets it
    /// per runner, which is what makes "the x86-64 leg covers AVX2" a checked
    /// claim rather than an assumption about the hardware GitHub handed out.
    #[test]
    fn simd_dispatch_reaches_the_expected_kernels() {
        #[allow(unused_mut)]
        let mut available: Vec<&str> = Vec::new();

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            // Each name gates at least one kernel in this crate. The two `-fma`
            // entries go through the crate's own predicates instead of
            // repeating their definitions, so this cannot drift away from what
            // the dispatch sites actually test.
            if std::arch::is_x86_feature_detected!("sse2") {
                available.push("sse2");
            }
            if std::arch::is_x86_feature_detected!("avx") {
                available.push("avx");
            }
            if have_avx_fma() {
                available.push("avx-fma");
            }
            if std::arch::is_x86_feature_detected!("avx2") {
                available.push("avx2");
            }
            if have_avx2_fma() {
                available.push("avx2-fma");
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            // NEON is in the aarch64 baseline, so those kernels are compiled in
            // unconditionally and there is nothing to detect at runtime.
            available.push("neon");
        }

        let summary = if available.is_empty() {
            "scalar only".to_string()
        } else {
            available.join(", ")
        };
        println!("SIMD dispatch on {}: {summary}", std::env::consts::ARCH);

        let Ok(required) = std::env::var("OPUS_PURE_REQUIRE_SIMD") else {
            return;
        };
        for want in required.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            assert!(
                available.contains(&want),
                "OPUS_PURE_REQUIRE_SIMD asked for `{want}`, but this host dispatches to \
                 [{summary}] — nothing here exercised the kernels behind `{want}`"
            );
        }
    }
}

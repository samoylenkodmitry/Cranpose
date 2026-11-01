#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use crate::cpu_features::FeatureCache;
use crate::silk::define::*;
use crate::silk::macros::*;
use crate::silk::sigproc_fix::*;
use crate::silk::structs::*;
use crate::silk::tables::*;

#[derive(Copy, Clone)]
pub struct NSQDelDecStruct {
    pub s_lpc_q14: [i32; MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH],
    pub rand_state: [i32; DECISION_DELAY],
    pub q_q10: [i32; DECISION_DELAY],
    pub xq_q14: [i32; DECISION_DELAY],
    pub pred_q15: [i32; DECISION_DELAY],
    pub shape_q14: [i32; DECISION_DELAY],
    pub s_ar2_q14: [i32; MAX_SHAPE_LPC_ORDER],
    pub lf_ar_q14: i32,
    pub diff_q14: i32,
    pub seed: i32,
    pub seed_init: i32,
    pub rd_q10: i32,
}

impl Default for NSQDelDecStruct {
    fn default() -> Self {
        Self {
            s_lpc_q14: [0; MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH],
            rand_state: [0; DECISION_DELAY],
            q_q10: [0; DECISION_DELAY],
            xq_q14: [0; DECISION_DELAY],
            pred_q15: [0; DECISION_DELAY],
            shape_q14: [0; DECISION_DELAY],
            s_ar2_q14: [0; MAX_SHAPE_LPC_ORDER],
            lf_ar_q14: 0,
            diff_q14: 0,
            seed: 0,
            seed_init: 0,
            rd_q10: 0,
        }
    }
}

#[derive(Copy, Clone, Default)]
pub struct NSQSampleStruct {
    pub q_q10: i32,
    pub rd_q10: i32,
    pub xq_q14: i32,
    pub lf_ar_q14: i32,
    pub diff_q14: i32,
    pub s_ltp_shp_q14: i32,
    pub lpc_exc_q14: i32,
}

#[inline]
fn silk_nsq_del_dec_scale_states(
    ps_enc_c: &SilkEncoderStateCommon,
    nsq: &mut SilkNSQState,
    ps_del_dec: &mut [NSQDelDecStruct],
    x16: &[i16],
    x_sc_q10: &mut [i32],
    s_ltp: &[i16],
    s_ltp_q15: &mut [i32],
    subfr: usize,
    n_states_delayed_decision: i32,
    ltp_scale_q14: i32,
    gains_q16: &[i32],
    pitch_l: &[i32],
    signal_type: i32,
    decision_delay: i32,
) {
    let lag = pitch_l[subfr] as usize;
    let inv_gain_q31 = silk_inverse32_varq(gains_q16[subfr].max(1), 47);

    let inv_gain_q26 = silk_rshift_round(inv_gain_q31, 5);
    let n = ps_enc_c.subfr_length as usize;
    for (x_out, &x_in) in x_sc_q10[..n].iter_mut().zip(x16[..n].iter()) {
        *x_out = silk_smulww(x_in as i32, inv_gain_q26);
    }

    if nsq.rewhite_flag != 0 {
        let mut inv_gain_q31_scaled = inv_gain_q31;
        if subfr == 0 {
            inv_gain_q31_scaled = silk_lshift(silk_smulwb(inv_gain_q31, ltp_scale_q14), 2);
        }
        for i in (nsq.s_ltp_buf_idx as usize - lag - LTP_ORDER / 2)..(nsq.s_ltp_buf_idx as usize) {
            s_ltp_q15[i] = silk_smulwb(inv_gain_q31_scaled, s_ltp[i] as i32);
        }
    }

    if gains_q16[subfr] != nsq.prev_gain_q16 {
        let gain_adj_q16 = silk_div32_varq(nsq.prev_gain_q16, gains_q16[subfr], 16);

        for i in (nsq.s_ltp_shp_buf_idx as usize - ps_enc_c.ltp_mem_length as usize)
            ..(nsq.s_ltp_shp_buf_idx as usize)
        {
            nsq.s_ltp_shp_q14[i] = silk_smulww(gain_adj_q16, nsq.s_ltp_shp_q14[i]);
        }

        if signal_type == TYPE_VOICED && nsq.rewhite_flag == 0 {
            let ltp_start = nsq.s_ltp_buf_idx as usize - lag - LTP_ORDER / 2;
            let ltp_end = nsq.s_ltp_buf_idx as usize - decision_delay as usize;
            for v in s_ltp_q15[ltp_start..ltp_end].iter_mut() {
                *v = silk_smulww(gain_adj_q16, *v);
            }
        }

        let n_states = (n_states_delayed_decision as usize).min(NSQ_MAX_STATES_OPERATING);
        for k in 0..n_states {
            let ps_dd = &mut ps_del_dec[k];
            ps_dd.lf_ar_q14 = silk_smulww(gain_adj_q16, ps_dd.lf_ar_q14);
            ps_dd.diff_q14 = silk_smulww(gain_adj_q16, ps_dd.diff_q14);
            for i in 0..NSQ_LPC_BUF_LENGTH {
                ps_dd.s_lpc_q14[i] = silk_smulww(gain_adj_q16, ps_dd.s_lpc_q14[i]);
            }
            for i in 0..MAX_SHAPE_LPC_ORDER {
                ps_dd.s_ar2_q14[i] = silk_smulww(gain_adj_q16, ps_dd.s_ar2_q14[i]);
            }
            for i in 0..DECISION_DELAY {
                ps_dd.pred_q15[i] = silk_smulww(gain_adj_q16, ps_dd.pred_q15[i]);
                ps_dd.shape_q14[i] = silk_smulww(gain_adj_q16, ps_dd.shape_q14[i]);
            }
        }
        nsq.prev_gain_q16 = gains_q16[subfr];
    }
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn silk_lpc_prediction_neon(
    ps_lpc_q14: &[i32],
    idx: usize,
    a_q12: &[i16],
    predict_lpc_order: i32,
) -> i32 {
    use std::arch::aarch64::*;

    let order = predict_lpc_order as usize;
    let lpc_base = ps_lpc_q14.as_ptr().add(idx);
    let a_ptr = a_q12.as_ptr();

    let mut acc = vdupq_n_s32(0i32);
    let neon_taps = order & !3;

    let mut j = 0usize;
    while j < neon_taps {
        let lpc_asc = vld1q_s32(lpc_base.sub(j + 3));

        let coef_narrow = vld1_s16(a_ptr.add(j));
        let coef_wide = vmovl_s16(coef_narrow);

        let coef_rev = vrev64q_s32(vcombine_s32(
            vget_high_s32(coef_wide),
            vget_low_s32(coef_wide),
        ));

        let prod_lo = vmull_s32(vget_low_s32(lpc_asc), vget_low_s32(coef_rev));
        let prod_hi = vmull_s32(vget_high_s32(lpc_asc), vget_high_s32(coef_rev));

        let shr_lo = vshrn_n_s64::<16>(prod_lo); // int32x2
        let shr_hi = vshrn_n_s64::<16>(prod_hi); // int32x2

        acc = vaddq_s32(acc, vcombine_s32(shr_lo, shr_hi));
        j += 4;
    }

    let mut out = silk_rshift(predict_lpc_order, 1) + vaddvq_s32(acc);

    while j < order {
        out = silk_smlawb(out, ps_lpc_q14[idx - j], a_q12[j] as i32);
        j += 1;
    }

    out
}

#[inline]
/// AVX2 twin of the 16/10-tap LPC short-prediction dot product (S1c).
///
/// Byte-identical to the scalar: each per-tap product is
/// `(lpc[idx-j] as i64 * a[j] as i64) >> 16` narrowed to i32, and i32
/// wrapping-addition is associative so the lane reduction matches the sequential
/// sum exactly. AVX2 has no signed 64-bit shift, so `>>16` is emulated
/// (logical shift + sign fill). Processes 8 taps/iteration; scalar tail.
///
/// SAFETY: caller guarantees `idx >= predict_lpc_order - 1` (SILK frame sizing,
/// same precondition the scalar path relies on) so the 8 loads at
/// `lpc[idx-j-7 ..= idx-j]` are in bounds; AVX2 availability is checked by the
/// caller via `is_x86_feature_detected!`.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn silk_lpc_prediction_avx2(
    ps_lpc_q14: &[i32],
    idx: usize,
    a_q12: &[i16],
    predict_lpc_order: i32,
) -> i32 {
    use core::arch::x86_64::*;

    let order = predict_lpc_order as usize;
    let lpc = ps_lpc_q14.as_ptr();
    // Each per-tap product `(lpc*a)>>16` fits in i32 (|lpc|<2^31, |a|<2^15 →
    // |product|<2^46, >>16 <2^31), so accumulating the products as i64 lanes and
    // truncating to i32 once at the end equals the scalar's i32 wrapping sum.
    let mut acc_e = _mm256_setzero_si256();
    let mut acc_o = _mm256_setzero_si256();
    // Arithmetic >>16 of packed i64 (0<16<32): logical shift + sign fill.
    let asr16 = |x: __m256i| -> __m256i {
        let sign = _mm256_cmpgt_epi64(_mm256_setzero_si256(), x); // -1 where x<0
        let fill = _mm256_slli_epi64(sign, 64 - 16);
        _mm256_or_si256(_mm256_srli_epi64(x, 16), fill)
    };

    let mut j = 0usize;
    let main = order & !7; // multiple of 8
    while j < main {
        // L[k] = lpc[idx-j-7+k] (k=0..7); product_t = lpc[idx-j-t]*a[j+t]
        //      = L[7-t]*C[t]. Order of the sum is irrelevant (associative), so
        //      pair L[k] with a_q12[j+7-k] by loading coefs reversed.
        let l = _mm256_loadu_si256(lpc.add(idx - j - 7) as *const __m256i);
        let c16 = _mm_loadu_si128(a_q12.as_ptr().add(j) as *const __m128i);
        let c = _mm256_cvtepi16_epi32(c16); // [a[j]..a[j+7]] i32
        let crev = _mm256_permutevar8x32_epi32(c, _mm256_setr_epi32(7, 6, 5, 4, 3, 2, 1, 0));

        // Even 32-bit lanes -> 4 i64 products; odd lanes via a dword shuffle.
        let pe = _mm256_mul_epi32(l, crev);
        let lo = _mm256_shuffle_epi32(l, 0b11_11_01_01);
        let co = _mm256_shuffle_epi32(crev, 0b11_11_01_01);
        let po = _mm256_mul_epi32(lo, co);

        acc_e = _mm256_add_epi64(acc_e, asr16(pe));
        acc_o = _mm256_add_epi64(acc_o, asr16(po));
        j += 8;
    }

    // Sum the 8 i64 lanes; only the low 32 bits matter (== i32 wrapping sum).
    let s = _mm256_add_epi64(acc_e, acc_o); // 4 i64
    let s2 = _mm_add_epi64(_mm256_castsi256_si128(s), _mm256_extracti128_si256(s, 1)); // 2 i64
    let s3 = _mm_add_epi64(s2, _mm_unpackhi_epi64(s2, s2)); // 1 i64 in low lane
    let mut out = silk_rshift(predict_lpc_order, 1).wrapping_add(_mm_cvtsi128_si32(s3));

    while j < order {
        out = silk_smlawb(out, ps_lpc_q14[idx - j], a_q12[j] as i32);
        j += 1;
    }
    out
}

/// Cached AVX2 dispatch decision: `is_x86_feature_detected!("avx2")` unless the
/// `RUSTY_OPUS_NO_AVX2` env var is set (for interleaved A/B against the scalar
/// twin). Cached so the hot path pays no per-call feature-detect cost.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn lpc_avx2_enabled() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| is_x86_feature_detected!("avx2"))
}

#[inline(always)]
fn silk_lpc_prediction_scalar(
    ps_lpc_q14: &[i32],
    idx: usize,
    a_q12: &[i16],
    predict_lpc_order: i32,
) -> i32 {
    let mut out = silk_rshift(predict_lpc_order, 1);
    for j in 0..predict_lpc_order as usize {
        out = silk_smlawb(out, ps_lpc_q14[idx - j], a_q12[j] as i32);
    }
    out
}

pub(crate) fn silk_noise_shape_quantizer_short_prediction(
    ps_lpc_q14: &[i32],
    idx: usize,
    a_q12: &[i16],
    predict_lpc_order: i32,
) -> i32 {
    #[cfg(target_arch = "aarch64")]
    if idx + 1 >= predict_lpc_order as usize {
        // SAFETY: aarch64 always has NEON; idx precondition guarantees the loads.
        return unsafe { silk_lpc_prediction_neon(ps_lpc_q14, idx, a_q12, predict_lpc_order) };
    }
    #[cfg(target_arch = "x86_64")]
    {
        // Runtime-dispatched; the scalar twin stays the oracle/fallback and the
        // feature detect is cached once. `avx2_matches_scalar` asserts they agree.
        if lpc_avx2_enabled() && idx + 1 >= predict_lpc_order as usize {
            // SAFETY: avx2 verified at runtime; idx precondition guarantees the loads.
            return unsafe { silk_lpc_prediction_avx2(ps_lpc_q14, idx, a_q12, predict_lpc_order) };
        }
    }
    #[allow(unreachable_code)]
    silk_lpc_prediction_scalar(ps_lpc_q14, idx, a_q12, predict_lpc_order)
}

/// Cached AVX2 dispatch for the cross-state NSQ shaping filter (S1d/Path 2).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn nsq_shape_avx2_enabled() -> bool {
    static CACHE: FeatureCache = FeatureCache::new();
    CACHE.get(|| is_x86_feature_detected!("avx2"))
}

/// Cross-state warped shaping AR filter over a state-minor SoA buffer
/// `sar[tap][state]` (the 4 del-dec states are the lanes). Serial in the tap
/// dimension; scalar twin = the oracle for the AVX2 version below.
#[inline(always)]
fn nsq_shape_filter_soa_scalar(
    sar: &mut [[i32; 4]],
    diff: &[i32; 4],
    warp: i32,
    ar_shp_q13: &[i16],
    order: usize,
    base: i32,
    n_ar: &mut [i32; 4],
) {
    for k in 0..4 {
        let mut tmp2 = silk_smlawb(diff[k], sar[0][k], warp);
        let mut tmp1 = silk_smlawb(sar[0][k], silk_sub32_ovflw(sar[1][k], tmp2), warp);
        sar[0][k] = tmp2;
        let mut acc = silk_smlawb(base, tmp2, ar_shp_q13[0] as i32);
        let mut j = 2;
        while j < order {
            tmp2 = silk_smlawb(sar[j - 1][k], silk_sub32_ovflw(sar[j][k], tmp1), warp);
            sar[j - 1][k] = tmp1;
            acc = silk_smlawb(acc, tmp1, ar_shp_q13[j - 1] as i32);
            tmp1 = silk_smlawb(sar[j][k], silk_sub32_ovflw(sar[j + 1][k], tmp2), warp);
            sar[j][k] = tmp2;
            acc = silk_smlawb(acc, tmp2, ar_shp_q13[j] as i32);
            j += 2;
        }
        sar[order - 1][k] = tmp1;
        n_ar[k] = silk_smlawb(acc, tmp1, ar_shp_q13[order - 1] as i32);
    }
}

/// Hand-AVX2 twin: the 4 states run as 4 **i64 lanes** of a `__m256i` throughout
/// the recurrence (values stay i32-range) — the key over the reverted S1d attempt
/// is that nothing narrows/permutes per op; the narrow-to-i32 happens once, on
/// store. Byte-identical to the scalar twin (`silk_sub32_ovflw` never wraps here:
/// the shaping states are bounded Q14 values, verified by the oracle + unit test).
/// Micro-benchmarked at ~1.56× the 4-chain scalar.
///
/// SAFETY: AVX2 verified by the caller; `sar`/`n_ar` sized ≥ order/4.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn nsq_shape_filter_soa_avx2(
    sar: &mut [[i32; 4]],
    diff: &[i32; 4],
    warp: i32,
    ar_shp_q13: &[i16],
    order: usize,
    base: i32,
    n_ar: &mut [i32; 4],
) {
    use core::arch::x86_64::*;
    let wb = _mm256_set1_epi64x(warp as i64);
    let asr16 = |x: __m256i| {
        let s = _mm256_cmpgt_epi64(_mm256_setzero_si256(), x);
        _mm256_or_si256(_mm256_srli_epi64(x, 16), _mm256_slli_epi64(s, 48))
    };
    let smlawb_v =
        |a: __m256i, b: __m256i, cb: __m256i| _mm256_add_epi64(a, asr16(_mm256_mul_epi32(b, cb)));
    let ldv = |p: &[i32; 4]| _mm256_cvtepi32_epi64(_mm_loadu_si128(p.as_ptr() as *const __m128i));
    let stv = |p: &mut [i32; 4], v: __m256i| {
        let g = _mm256_permutevar8x32_epi32(v, _mm256_setr_epi32(0, 2, 4, 6, 0, 2, 4, 6));
        _mm_storeu_si128(p.as_mut_ptr() as *mut __m128i, _mm256_castsi256_si128(g));
    };
    let cbv = |c: i32| _mm256_set1_epi64x(c as i16 as i64);

    // Load the state column-vectors once (i64 lanes); operate in-register.
    let mut vsar = [_mm256_setzero_si256(); MAX_SHAPE_LPC_ORDER];
    for j in 0..order {
        vsar[j] = ldv(&sar[j]);
    }
    let vdiff = ldv(diff);
    let mut tmp2 = smlawb_v(vdiff, vsar[0], wb);
    let mut tmp1 = smlawb_v(vsar[0], _mm256_sub_epi64(vsar[1], tmp2), wb);
    vsar[0] = tmp2;
    let mut acc = smlawb_v(
        _mm256_set1_epi64x(base as i64),
        tmp2,
        cbv(ar_shp_q13[0] as i32),
    );
    let mut j = 2;
    while j < order {
        tmp2 = smlawb_v(vsar[j - 1], _mm256_sub_epi64(vsar[j], tmp1), wb);
        vsar[j - 1] = tmp1;
        acc = smlawb_v(acc, tmp1, cbv(ar_shp_q13[j - 1] as i32));
        tmp1 = smlawb_v(vsar[j], _mm256_sub_epi64(vsar[j + 1], tmp2), wb);
        vsar[j] = tmp2;
        acc = smlawb_v(acc, tmp2, cbv(ar_shp_q13[j] as i32));
        j += 2;
    }
    vsar[order - 1] = tmp1;
    acc = smlawb_v(acc, tmp1, cbv(ar_shp_q13[order - 1] as i32));
    for j in 0..order {
        stv(&mut sar[j], vsar[j]);
    }
    stv(n_ar, acc);
}

#[inline(always)]
fn nsq_shape_filter_soa(
    sar: &mut [[i32; 4]],
    diff: &[i32; 4],
    warp: i32,
    ar_shp_q13: &[i16],
    order: usize,
    base: i32,
    n_ar: &mut [i32; 4],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if nsq_shape_avx2_enabled() {
            // SAFETY: avx2 checked; sar/n_ar sized ≥ order/4.
            unsafe { nsq_shape_filter_soa_avx2(sar, diff, warp, ar_shp_q13, order, base, n_ar) };
            return;
        }
    }
    nsq_shape_filter_soa_scalar(sar, diff, warp, ar_shp_q13, order, base, n_ar);
}

#[cfg(all(test, target_arch = "x86_64"))]
mod lpc_pred_avx2_tests {
    use super::*;

    /// AVX2 twin must be BYTE-IDENTICAL to the scalar oracle over random inputs
    /// at every valid LPC order.
    #[test]
    fn avx2_matches_scalar() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }
        // xorshift for deterministic pseudo-random coverage.
        let mut s: u64 = 0x1234_5678_9abc_def1;
        let mut rng = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for order in [10usize, 12, 14, 16] {
            for _ in 0..50_000 {
                // buffer big enough for idx and the order-1 look-back
                let mut lpc = [0i32; 64];
                for v in lpc.iter_mut() {
                    // Q14-ish magnitudes, full sign range.
                    *v = (rng() as i32) >> (rng() as u32 % 12);
                }
                let mut a = [0i16; 16];
                for v in a.iter_mut().take(order) {
                    *v = (rng() as i16) >> (rng() as u32 % 3);
                }
                let idx = 32 + (rng() as usize % 16);
                let got = unsafe { silk_lpc_prediction_avx2(&lpc, idx, &a, order as i32) };
                let want = silk_lpc_prediction_scalar(&lpc, idx, &a, order as i32);
                assert_eq!(got, want, "order={order} idx={idx}");
            }
        }
    }

    /// Cross-state shaping filter (Path 2): the i64-lane AVX2 must match the
    /// scalar SoA twin over random states, incl. large magnitudes (stresses the
    /// `silk_sub32_ovflw` i32-wrap assumption — the shaping states are bounded in
    /// practice, but this covers well beyond the realistic range).
    #[test]
    fn nsq_shape_filter_avx2_matches_scalar() {
        if !is_x86_feature_detected!("avx2") {
            return;
        }
        let mut s: u64 = 0xC0FF_EE00_1234_5678;
        let mut rng = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for order in [16usize, 24] {
            for _ in 0..20_000 {
                let sh = 6 + (rng() % 20) as u32;
                let mut sar_a = vec![[0i32; 4]; order];
                for row in sar_a.iter_mut() {
                    for v in row.iter_mut() {
                        *v = (rng() as i32) >> sh;
                    }
                }
                let mut sar_b = sar_a.clone();
                let diff = [
                    (rng() as i32) >> sh,
                    (rng() as i32) >> sh,
                    (rng() as i32) >> sh,
                    (rng() as i32) >> sh,
                ];
                let ar: Vec<i16> = (0..order).map(|_| (rng() >> 8) as i16).collect();
                let warp = 13421;
                let base = (order as i32) >> 1;
                let mut na = [0i32; 4];
                let mut nb = [0i32; 4];
                nsq_shape_filter_soa_scalar(&mut sar_a, &diff, warp, &ar, order, base, &mut na);
                unsafe {
                    nsq_shape_filter_soa_avx2(&mut sar_b, &diff, warp, &ar, order, base, &mut nb)
                };
                assert_eq!(na, nb, "n_ar mismatch order={order} sh={sh}");
                assert_eq!(sar_a, sar_b, "sar mismatch order={order} sh={sh}");
            }
        }
    }
}

pub fn silk_noise_shape_quantizer_del_dec(
    nsq: &mut SilkNSQState,
    ps_del_dec: &mut [NSQDelDecStruct],
    signal_type: i32,
    x_q10: &[i32],
    pulses: &mut [i8],
    pulses_offset: i32,
    xq_ptr: i32,
    s_ltp_q15: &mut [i32],
    delayed_ga_q10: &mut [i32],
    a_q12: &[i16],
    b_q14: &[i16],
    ar_shp_q13: &[i16],
    lag: i32,
    harm_shape_fir_packed_q14: i32,
    tilt_q14: i32,
    lf_shp_q14: i32,
    gain_q16: i32,
    lambda_q10: i32,
    offset_q10: i32,
    length: i32,
    subfr: i32,
    shaping_lpc_order: i32,
    predict_lpc_order: i32,
    warping_q16: i32,
    n_states_delayed_decision: i32,
    smpl_buf_idx: &mut i32,
    decision_delay: i32,
    _frame_counter: i32,
) {
    let mut ps_sample_state = [[NSQSampleStruct::default(); 2]; NSQ_MAX_STATES_OPERATING];
    let gain_q10 = silk_rshift(gain_q16, 6);

    let n_states = (n_states_delayed_decision as usize).min(NSQ_MAX_STATES_OPERATING);

    let pred_lag_ptr_base = nsq.s_ltp_buf_idx - lag + LTP_ORDER as i32 / 2;
    let shp_lag_ptr_base = nsq.s_ltp_shp_buf_idx - lag + HARM_SHAPE_FIR_TAPS as i32 / 2;

    // Persistent cross-state SoA for the warped-shaping state `s_ar2` (Path 2):
    // transposed AoS→SoA once here and back at the end (amortised over `length`
    // samples), so the per-sample shaping filter runs cross-state (4 states = 4
    // lanes) with no per-sample transpose. Within the sample loop, `s_ar2` lives
    // ONLY in `sar_soa` (the shaping filter + the RD state-swap update it there).
    let shp_ord = shaping_lpc_order as usize;
    let shp_base = silk_rshift(shaping_lpc_order, 1);
    let mut sar_soa = [[0i32; 4]; MAX_SHAPE_LPC_ORDER];
    for k in 0..n_states {
        for j in 0..shp_ord {
            sar_soa[j][k] = ps_del_dec[k].s_ar2_q14[j];
        }
    }

    for i in 0..length {
        let idx = i as usize;
        let mut ltp_pred_q14 = 0;
        if signal_type == TYPE_VOICED {
            let pred_lag_idx_calc = pred_lag_ptr_base + i;
            if pred_lag_idx_calc >= LTP_ORDER as i32 && pred_lag_idx_calc < s_ltp_q15.len() as i32 {
                let pred_lag_idx = pred_lag_idx_calc as usize;
                ltp_pred_q14 = 2;
                for j in 0..LTP_ORDER {
                    ltp_pred_q14 =
                        silk_smlawb(ltp_pred_q14, s_ltp_q15[pred_lag_idx - j], b_q14[j] as i32);
                }
                ltp_pred_q14 = silk_lshift(ltp_pred_q14, 1);
            }
        }

        let mut n_ltp_q14 = 0;
        if lag > 0 {
            let shp_lag_idx_calc = shp_lag_ptr_base + i;
            if shp_lag_idx_calc >= HARM_SHAPE_FIR_TAPS as i32
                && shp_lag_idx_calc < nsq.s_ltp_shp_q14.len() as i32
            {
                let shp_lag_idx = shp_lag_idx_calc as usize;
                n_ltp_q14 = silk_smulwb(
                    silk_add_sat32(
                        nsq.s_ltp_shp_q14[shp_lag_idx],
                        nsq.s_ltp_shp_q14[shp_lag_idx - 2],
                    ),
                    harm_shape_fir_packed_q14,
                );
                n_ltp_q14 = silk_smlawt(
                    n_ltp_q14,
                    nsq.s_ltp_shp_q14[shp_lag_idx - 1],
                    harm_shape_fir_packed_q14,
                );
                n_ltp_q14 = silk_sub_lshift32(ltp_pred_q14, n_ltp_q14, 2);
            }
        }

        // Shaping pre-pass (Path 2): seed + LPC prediction (per state), then the
        // warped shaping AR filter run CROSS-STATE over `sar_soa` (4 states = 4
        // i64 lanes). Byte-identical to the per-state scalar; ~1.56× the kernel.
        // The states are independent within a sample, so this hoist is exact.
        let mut lpc_pred_arr = [0i32; NSQ_MAX_STATES_OPERATING];
        let mut n_ar_arr = [0i32; NSQ_MAX_STATES_OPERATING];
        let mut n_lf_arr = [0i32; NSQ_MAX_STATES_OPERATING];
        let mut diff_arr = [0i32; 4];
        for k in 0..n_states {
            let ps_dd = &mut ps_del_dec[k];
            ps_dd.seed = silk_rand(ps_dd.seed);
            let ps_lpc_q14_idx = NSQ_LPC_BUF_LENGTH - 1 + idx;
            lpc_pred_arr[k] = silk_lshift(
                silk_noise_shape_quantizer_short_prediction(
                    &ps_dd.s_lpc_q14,
                    ps_lpc_q14_idx,
                    a_q12,
                    predict_lpc_order,
                ),
                4,
            );
            diff_arr[k] = ps_dd.diff_q14;
        }
        let mut n_ar_raw = [0i32; 4];
        nsq_shape_filter_soa(
            &mut sar_soa,
            &diff_arr,
            warping_q16,
            ar_shp_q13,
            shp_ord,
            shp_base,
            &mut n_ar_raw,
        );
        let smpl_idx = (*smpl_buf_idx as usize).min(DECISION_DELAY - 1);
        for k in 0..n_states {
            let ps_dd = &ps_del_dec[k];
            let mut n_ar_q14 = silk_lshift(n_ar_raw[k], 1);
            n_ar_q14 = silk_smlawb(n_ar_q14, ps_dd.lf_ar_q14, tilt_q14);
            n_ar_q14 = silk_lshift(n_ar_q14, 2);
            n_ar_arr[k] = n_ar_q14;
            let mut n_lf_q14 = silk_smulwb(ps_dd.shape_q14[smpl_idx], lf_shp_q14);
            n_lf_q14 = silk_smlawt(n_lf_q14, ps_dd.lf_ar_q14, lf_shp_q14);
            n_lf_q14 = silk_lshift(n_lf_q14, 2);
            n_lf_arr[k] = n_lf_q14;
        }

        // RD decision pass (per state; branchy — scalar).
        for k in 0..n_states {
            let ps_dd = &mut ps_del_dec[k];
            let ps_ss = &mut ps_sample_state[k];
            let lpc_pred_q14 = lpc_pred_arr[k];
            let n_ar_q14 = n_ar_arr[k];
            let n_lf_q14 = n_lf_arr[k];

            let tmp1_val = silk_sub_sat32(
                silk_add32_ovflw(n_ltp_q14, lpc_pred_q14),
                silk_add_sat32(n_ar_q14, n_lf_q14),
            );
            let r_q10 = x_q10[idx] - silk_rshift_round(tmp1_val, 4);

            let r_q10_signed = if ps_dd.seed < 0 { -r_q10 } else { r_q10 };
            let r_q10_signed = silk_limit_32(r_q10_signed, -(31 << 10), 30 << 10);

            let q1_q10_in = r_q10_signed - offset_q10;
            let mut q1_q0 = silk_rshift(q1_q10_in, 10);
            if lambda_q10 > 2048 {
                let rdo_offset = lambda_q10 / 2 - 512;
                if q1_q10_in > rdo_offset {
                    q1_q0 = silk_rshift(q1_q10_in - rdo_offset, 10);
                } else if q1_q10_in < -rdo_offset {
                    q1_q0 = silk_rshift(q1_q10_in + rdo_offset, 10);
                } else if q1_q10_in < 0 {
                    q1_q0 = -1;
                } else {
                    q1_q0 = 0;
                }
            }

            let (rd1_q10, rd2_q10, q1_q10_val, q2_q10_val);
            if q1_q0 > 0 {
                q1_q10_val =
                    silk_sub32(silk_lshift(q1_q0, 10), QUANT_LEVEL_ADJUST_Q10) + offset_q10;
                q2_q10_val = q1_q10_val + 1024;
                rd1_q10 = silk_smulbb(q1_q10_val, lambda_q10);
                rd2_q10 = silk_smulbb(q2_q10_val, lambda_q10);
            } else if q1_q0 == 0 {
                q1_q10_val = offset_q10;
                q2_q10_val = q1_q10_val + 1024 - QUANT_LEVEL_ADJUST_Q10;
                rd1_q10 = silk_smulbb(q1_q10_val, lambda_q10);
                rd2_q10 = silk_smulbb(q2_q10_val, lambda_q10);
            } else if q1_q0 == -1 {
                q2_q10_val = offset_q10;
                q1_q10_val = q2_q10_val - (1024 - QUANT_LEVEL_ADJUST_Q10);
                rd1_q10 = silk_smulbb(-q1_q10_val, lambda_q10);
                rd2_q10 = silk_smulbb(q2_q10_val, lambda_q10);
            } else {
                q1_q10_val =
                    silk_add32(silk_lshift(q1_q0, 10), QUANT_LEVEL_ADJUST_Q10) + offset_q10;
                q2_q10_val = q1_q10_val + 1024;
                rd1_q10 = silk_smulbb(-q1_q10_val, lambda_q10);
                rd2_q10 = silk_smulbb(-q2_q10_val, lambda_q10);
            }

            let mut rr_q10 = r_q10_signed - q1_q10_val;
            let rd1_q10_final = silk_rshift(silk_smlabb(rd1_q10, rr_q10, rr_q10), 10);
            rr_q10 = r_q10_signed - q2_q10_val;
            let rd2_q10_final = silk_rshift(silk_smlabb(rd2_q10, rr_q10, rr_q10), 10);

            if rd1_q10_final < rd2_q10_final {
                ps_ss[0].rd_q10 = ps_dd.rd_q10 + rd1_q10_final;
                ps_ss[1].rd_q10 = ps_dd.rd_q10 + rd2_q10_final;
                ps_ss[0].q_q10 = q1_q10_val;
                ps_ss[1].q_q10 = q2_q10_val;
            } else {
                ps_ss[0].rd_q10 = ps_dd.rd_q10 + rd2_q10_final;
                ps_ss[1].rd_q10 = ps_dd.rd_q10 + rd1_q10_final;
                ps_ss[0].q_q10 = q2_q10_val;
                ps_ss[1].q_q10 = q1_q10_val;
            }

            for j in 0..2 {
                let mut exc_q14 = silk_lshift(ps_ss[j].q_q10, 4);
                if ps_dd.seed < 0 {
                    exc_q14 = -exc_q14;
                }
                let lpc_exc_q14 = silk_add32(exc_q14, ltp_pred_q14);
                let xq_q14 = silk_add32_ovflw(lpc_exc_q14, lpc_pred_q14);
                ps_ss[j].diff_q14 = silk_sub32_ovflw(xq_q14, silk_lshift(x_q10[idx], 4));
                let s_lf_ar_shp_q14 = silk_sub32_ovflw(ps_ss[j].diff_q14, n_ar_q14);
                ps_ss[j].s_ltp_shp_q14 = silk_sub_sat32(s_lf_ar_shp_q14, n_lf_q14);
                ps_ss[j].lf_ar_q14 = s_lf_ar_shp_q14;
                ps_ss[j].lpc_exc_q14 = lpc_exc_q14;
                ps_ss[j].xq_q14 = xq_q14;
            }
        }

        *smpl_buf_idx = (*smpl_buf_idx - 1 + DECISION_DELAY as i32) % DECISION_DELAY as i32;
        let last_smple_idx = ((*smpl_buf_idx + decision_delay) % DECISION_DELAY as i32) as usize;

        let mut winner_ind = 0;
        let mut rd_min_q10 = ps_sample_state[0][0].rd_q10;
        for k in 1..n_states {
            if ps_sample_state[k][0].rd_q10 < rd_min_q10 {
                rd_min_q10 = ps_sample_state[k][0].rd_q10;
                winner_ind = k;
            }
        }

        let winner_rand_state = ps_del_dec[winner_ind].rand_state[last_smple_idx];
        for k in 0..n_states {
            if ps_del_dec[k].rand_state[last_smple_idx] != winner_rand_state {
                ps_sample_state[k][0].rd_q10 =
                    ps_sample_state[k][0].rd_q10.saturating_add(i32::MAX >> 4);
                ps_sample_state[k][1].rd_q10 =
                    ps_sample_state[k][1].rd_q10.saturating_add(i32::MAX >> 4);
            }
        }

        let mut rd_max_q10 = ps_sample_state[0][0].rd_q10;
        let mut rd_max_ind = 0;
        let mut rd_min_q10_2 = ps_sample_state[0][1].rd_q10;
        let mut rd_min_ind = 0;
        for k in 1..n_states {
            if ps_sample_state[k][0].rd_q10 > rd_max_q10 {
                rd_max_q10 = ps_sample_state[k][0].rd_q10;
                rd_max_ind = k;
            }
            if ps_sample_state[k][1].rd_q10 < rd_min_q10_2 {
                rd_min_q10_2 = ps_sample_state[k][1].rd_q10;
                rd_min_ind = k;
            }
        }

        if rd_min_q10_2 < rd_max_q10 {
            if rd_min_ind != rd_max_ind {
                let (min_state, max_state) = if rd_min_ind < rd_max_ind {
                    let (left, right) = ps_del_dec.split_at_mut(rd_max_ind);
                    (&left[rd_min_ind], &mut right[0])
                } else {
                    let (left, right) = ps_del_dec.split_at_mut(rd_min_ind);
                    (&right[0], &mut left[rd_max_ind])
                };
                max_state.s_lpc_q14[idx..].copy_from_slice(&min_state.s_lpc_q14[idx..]);
                max_state.rand_state = min_state.rand_state;
                max_state.q_q10 = min_state.q_q10;
                max_state.xq_q14 = min_state.xq_q14;
                max_state.pred_q15 = min_state.pred_q15;
                max_state.shape_q14 = min_state.shape_q14;
                max_state.lf_ar_q14 = min_state.lf_ar_q14;
                max_state.diff_q14 = min_state.diff_q14;
                max_state.seed = min_state.seed;
                max_state.seed_init = min_state.seed_init;
                max_state.rd_q10 = min_state.rd_q10;
                // s_ar2 lives in the SoA buffer during the loop — swap its column.
                for j in 0..shp_ord {
                    sar_soa[j][rd_max_ind] = sar_soa[j][rd_min_ind];
                }
            }

            ps_sample_state[rd_max_ind][0] = ps_sample_state[rd_min_ind][1];
        }

        let ps_dd = &ps_del_dec[winner_ind];
        if subfr > 0 || i >= decision_delay {
            let pulse_idx = (pulses_offset + i - decision_delay) as isize;
            let xq_idx = (xq_ptr + i - decision_delay) as isize;
            let shp_idx = (nsq.s_ltp_shp_buf_idx - decision_delay) as isize;
            let ltp_idx = (nsq.s_ltp_buf_idx - decision_delay) as isize;

            if pulse_idx >= 0 && pulse_idx < pulses.len() as isize {
                pulses[pulse_idx as usize] =
                    silk_rshift_round(ps_dd.q_q10[last_smple_idx], 10) as i8;
            }
            if xq_idx >= 0 && xq_idx < nsq.xq.len() as isize {
                nsq.xq[xq_idx as usize] = silk_sat16(silk_rshift_round(
                    silk_smulww(ps_dd.xq_q14[last_smple_idx], delayed_ga_q10[last_smple_idx]),
                    8,
                )) as i16;
            }
            if shp_idx >= 0 && shp_idx < nsq.s_ltp_shp_q14.len() as isize {
                nsq.s_ltp_shp_q14[shp_idx as usize] = ps_dd.shape_q14[last_smple_idx];
            }
            if ltp_idx >= 0 && ltp_idx < s_ltp_q15.len() as isize {
                s_ltp_q15[ltp_idx as usize] = ps_dd.pred_q15[last_smple_idx];
            }
        }
        nsq.s_ltp_shp_buf_idx += 1;
        nsq.s_ltp_buf_idx += 1;

        for k in 0..n_states {
            let ps_ss = &ps_sample_state[k][0];
            let ps_dd = &mut ps_del_dec[k];
            ps_dd.lf_ar_q14 = ps_ss.lf_ar_q14;
            ps_dd.diff_q14 = ps_ss.diff_q14;

            let lpc_idx = NSQ_LPC_BUF_LENGTH + idx;
            if lpc_idx < ps_dd.s_lpc_q14.len() {
                ps_dd.s_lpc_q14[lpc_idx] = ps_ss.xq_q14;
            }
            let smpl_idx = (*smpl_buf_idx as usize).min(DECISION_DELAY - 1);
            ps_dd.xq_q14[smpl_idx] = ps_ss.xq_q14;
            ps_dd.q_q10[smpl_idx] = ps_ss.q_q10;
            ps_dd.pred_q15[smpl_idx] = silk_lshift(ps_ss.lpc_exc_q14, 1);
            ps_dd.shape_q14[smpl_idx] = ps_ss.s_ltp_shp_q14;
            ps_dd.seed = silk_add32_ovflw(ps_dd.seed, silk_rshift_round(ps_ss.q_q10, 10));
            ps_dd.rand_state[smpl_idx] = ps_dd.seed;
            ps_dd.rd_q10 = ps_ss.rd_q10;
        }
        let smpl_idx = (*smpl_buf_idx as usize).min(DECISION_DELAY - 1);
        delayed_ga_q10[smpl_idx] = gain_q10;
    }
    // Transpose the cross-state shaping state back to the AoS structs (once), so
    // the next subframe's scale_states and the final copy see the updated s_ar2.
    for k in 0..n_states {
        for j in 0..shp_ord {
            ps_del_dec[k].s_ar2_q14[j] = sar_soa[j][k];
        }
    }
    for k in 0..n_states {
        let ps_dd = &mut ps_del_dec[k];
        let mut tmp = [0i32; NSQ_LPC_BUF_LENGTH];
        tmp.copy_from_slice(
            &ps_dd.s_lpc_q14[length as usize..length as usize + NSQ_LPC_BUF_LENGTH],
        );
        ps_dd.s_lpc_q14[..NSQ_LPC_BUF_LENGTH].copy_from_slice(&tmp);
    }
}

pub fn silk_nsq_del_dec(
    ps_common: &SilkEncoderStateCommon,
    ps_nsq: &mut SilkNSQState,
    ps_indices: &SideInfoIndices,

    x16: &[i16],
    pulses: &mut [i8],
    pred_coef_q12: &[i16],
    ltp_coef_q14: &[i16],
    ar_q13: &[i16],
    harm_shape_gain_q14: &[i32],
    tilt_q14: &[i32],
    lf_shp_q14: &[i32],
    gains_q16: &[i32],
    pitch_l: &[i32],
    lambda_q10: i32,
    ltp_scale_q14: i32,
) -> i32 {
    let mut x_sc_q10 = [0i32; MAX_SUB_FRAME_LENGTH];
    let mut delayed_ga_q10 = [0i32; DECISION_DELAY];
    let mut s_ltp_q15 = [0i32; LTP_MEM_LENGTH_MS * MAX_FS_KHZ + MAX_FRAME_LENGTH];
    let mut s_ltp = [0i16; LTP_MEM_LENGTH_MS * MAX_FS_KHZ + MAX_FRAME_LENGTH];
    let mut ps_del_dec = [NSQDelDecStruct::default(); NSQ_MAX_STATES_OPERATING];

    let mut lag = ps_nsq.lag_prev;

    let n_states = (ps_common.n_states_delayed_decision as usize).min(NSQ_MAX_STATES_OPERATING);

    for k in 0..n_states {
        let ps_dd = &mut ps_del_dec[k];
        ps_dd.seed = (k as i32 + ps_indices.seed as i32) & 3;
        ps_dd.seed_init = ps_dd.seed;
        ps_dd.rd_q10 = 0;
        ps_dd.lf_ar_q14 = ps_nsq.s_lf_ar_q14;
        ps_dd.diff_q14 = ps_nsq.s_diff_shp_q14;

        if ps_common.ltp_mem_length > 0 {
            ps_dd.shape_q14[0] = ps_nsq.s_ltp_shp_q14[ps_common.ltp_mem_length as usize - 1];
        }
        ps_dd.s_lpc_q14[..NSQ_LPC_BUF_LENGTH]
            .copy_from_slice(&ps_nsq.s_lpc_q14[..NSQ_LPC_BUF_LENGTH]);
        ps_dd.s_ar2_q14.copy_from_slice(&ps_nsq.s_ar2_q14);
    }

    let offset_q10 = SILK_QUANT_OFFSETS_Q10[(ps_indices.signal_type >> 1) as usize]
        [ps_indices.quant_offset_type as usize] as i32;
    let mut smpl_buf_idx = 0i32;
    let mut decision_delay = (DECISION_DELAY as i32).min(ps_common.subfr_length);

    if ps_indices.signal_type as i32 == TYPE_VOICED {
        for k in 0..ps_common.nb_subfr as usize {
            let pitch_constraint = pitch_l[k] - LTP_ORDER as i32 / 2 - 1;
            if pitch_constraint > 0 {
                decision_delay = decision_delay.min(pitch_constraint);
            }
        }
    } else if lag > 0 {
        let lag_constraint = lag - LTP_ORDER as i32 / 2 - 1;
        if lag_constraint > 0 {
            decision_delay = decision_delay.min(lag_constraint);
        }
    }

    let lsf_interpolation_flag = if ps_indices.nlsf_interp_coef_q2 == 4 {
        0
    } else {
        1
    };

    ps_nsq.s_ltp_shp_buf_idx = ps_common.ltp_mem_length;
    ps_nsq.s_ltp_buf_idx = ps_common.ltp_mem_length;

    let mut x_ptr = 0;
    let mut pulses_ptr = 0;
    let mut xq_ptr = ps_common.ltp_mem_length as usize;
    let mut subfr_nsq = 0;

    for k in 0..ps_common.nb_subfr as usize {
        let a_q12 =
            &pred_coef_q12[((k >> 1) | (1 - lsf_interpolation_flag as usize)) * MAX_LPC_ORDER..];
        let b_q14 = &ltp_coef_q14[k * LTP_ORDER..];
        let ar_shp_q13 = &ar_q13[k * MAX_SHAPE_LPC_ORDER..];

        let harm_shape_gain = harm_shape_gain_q14[k];
        let mut harm_shape_fir_packed_q14 = silk_rshift(harm_shape_gain, 2);
        harm_shape_fir_packed_q14 |= silk_lshift(silk_rshift(harm_shape_gain, 1), 16);

        ps_nsq.rewhite_flag = 0;
        if ps_indices.signal_type as i32 == TYPE_VOICED {
            lag = pitch_l[k];
            if (k & (3 - silk_lshift(lsf_interpolation_flag, 1) as usize)) == 0 {
                if k == 2 {
                    let mut rd_min_q10 = ps_del_dec[0].rd_q10;
                    let mut winner_ind = 0;
                    for i in 1..n_states {
                        if ps_del_dec[i].rd_q10 < rd_min_q10 {
                            rd_min_q10 = ps_del_dec[i].rd_q10;
                            winner_ind = i;
                        }
                    }
                    for i in 0..n_states {
                        if i != winner_ind {
                            ps_del_dec[i].rd_q10 =
                                ps_del_dec[i].rd_q10.saturating_add(i32::MAX >> 4);
                        }
                    }

                    let ps_dd = &ps_del_dec[winner_ind];
                    let mut last_smple_idx =
                        (smpl_buf_idx + decision_delay) % DECISION_DELAY as i32;
                    for i in 0..decision_delay {
                        last_smple_idx =
                            (last_smple_idx - 1 + DECISION_DELAY as i32) % DECISION_DELAY as i32;
                        let pulse_idx = (pulses_ptr as i32 + i - decision_delay) as isize;
                        let xq_idx = (xq_ptr as i32 + i - decision_delay) as isize;
                        let shp_idx = (ps_nsq.s_ltp_shp_buf_idx + i - decision_delay) as isize;
                        if pulse_idx >= 0 && xq_idx >= 0 && shp_idx >= 0 {
                            pulses[pulse_idx as usize] =
                                silk_rshift_round(ps_dd.q_q10[last_smple_idx as usize], 10) as i8;
                            ps_nsq.xq[xq_idx as usize] = silk_sat16(silk_rshift_round(
                                silk_smulww(ps_dd.xq_q14[last_smple_idx as usize], gains_q16[1]),
                                14,
                            )) as i16;
                            ps_nsq.s_ltp_shp_q14[shp_idx as usize] =
                                ps_dd.shape_q14[last_smple_idx as usize];
                        }
                    }
                    subfr_nsq = 0;
                }

                let start_idx_calc = ps_common.ltp_mem_length
                    - lag
                    - ps_common.predict_lpc_order
                    - LTP_ORDER as i32 / 2;
                if start_idx_calc < 0 {
                    continue;
                }
                let start_idx = start_idx_calc as usize;
                let xq_start = start_idx + k * ps_common.subfr_length as usize;
                let filter_len = ps_common.ltp_mem_length as usize - start_idx;

                if start_idx + filter_len > s_ltp.len() || xq_start + filter_len > ps_nsq.xq.len() {
                    continue;
                }
                silk_lpc_analysis_filter(
                    &mut s_ltp[start_idx..],
                    &ps_nsq.xq[xq_start..],
                    a_q12,
                    filter_len,
                    ps_common.predict_lpc_order as usize,
                    0,
                );
                ps_nsq.s_ltp_buf_idx = ps_common.ltp_mem_length;
                ps_nsq.rewhite_flag = 1;
            }
        }

        silk_nsq_del_dec_scale_states(
            ps_common,
            ps_nsq,
            &mut ps_del_dec,
            &x16[x_ptr..],
            &mut x_sc_q10,
            &s_ltp,
            &mut s_ltp_q15,
            k,
            ps_common.n_states_delayed_decision,
            ltp_scale_q14,
            gains_q16,
            pitch_l,
            ps_indices.signal_type as i32,
            decision_delay,
        );

        silk_noise_shape_quantizer_del_dec(
            ps_nsq,
            &mut ps_del_dec,
            ps_indices.signal_type as i32,
            &x_sc_q10,
            pulses,
            pulses_ptr as i32,
            xq_ptr as i32,
            &mut s_ltp_q15,
            &mut delayed_ga_q10,
            a_q12,
            b_q14,
            ar_shp_q13,
            lag,
            harm_shape_fir_packed_q14,
            tilt_q14[k],
            lf_shp_q14[k],
            gains_q16[k],
            lambda_q10,
            offset_q10,
            ps_common.subfr_length,
            subfr_nsq,
            ps_common.shaping_lpc_order,
            ps_common.predict_lpc_order,
            ps_common.warping_q16,
            ps_common.n_states_delayed_decision,
            &mut smpl_buf_idx,
            decision_delay,
            ps_common.frame_counter,
        );

        x_ptr += ps_common.subfr_length as usize;
        pulses_ptr += ps_common.subfr_length as usize;
        xq_ptr += ps_common.subfr_length as usize;
        subfr_nsq += 1;
    }

    let mut rd_min_q10 = ps_del_dec[0].rd_q10;
    let mut winner_ind = 0;
    for k in 1..n_states {
        if ps_del_dec[k].rd_q10 < rd_min_q10 {
            rd_min_q10 = ps_del_dec[k].rd_q10;
            winner_ind = k;
        }
    }

    let ps_dd = &ps_del_dec[winner_ind];
    ps_nsq.s_lf_ar_q14 = ps_dd.lf_ar_q14;
    ps_nsq.s_diff_shp_q14 = ps_dd.diff_q14;
    ps_nsq.lag_prev = pitch_l[ps_common.nb_subfr as usize - 1];

    let gain_q10_final = silk_rshift(gains_q16[ps_common.nb_subfr as usize - 1], 6);
    let mut last_smple_idx = smpl_buf_idx + decision_delay;
    for i in 0..decision_delay {
        last_smple_idx = (last_smple_idx - 1 + DECISION_DELAY as i32) % DECISION_DELAY as i32;
        let pulse_idx = (pulses_ptr as i32 + i - decision_delay) as isize;
        let xq_idx = (xq_ptr as i32 + i - decision_delay) as isize;
        if pulse_idx >= 0 && (pulse_idx as usize) < pulses.len() {
            pulses[pulse_idx as usize] =
                silk_rshift_round(ps_dd.q_q10[last_smple_idx as usize], 10) as i8;
        }
        if xq_idx >= 0 && (xq_idx as usize) < ps_nsq.xq.len() {
            ps_nsq.xq[xq_idx as usize] = silk_sat16(silk_rshift_round(
                silk_smulww(ps_dd.xq_q14[last_smple_idx as usize], gain_q10_final),
                8,
            )) as i16;
        }
        let shp_idx = (ps_nsq.s_ltp_shp_buf_idx - decision_delay + i) as isize;
        if shp_idx >= 0 && (shp_idx as usize) < ps_nsq.s_ltp_shp_q14.len() {
            ps_nsq.s_ltp_shp_q14[shp_idx as usize] = ps_dd.shape_q14[last_smple_idx as usize];
        }
    }

    let subfr_len = ps_common.subfr_length as usize;
    ps_nsq.s_lpc_q14[..NSQ_LPC_BUF_LENGTH]
        .copy_from_slice(&ps_dd.s_lpc_q14[subfr_len..subfr_len + NSQ_LPC_BUF_LENGTH]);
    ps_nsq.s_ar2_q14.copy_from_slice(&ps_dd.s_ar2_q14);

    let ltp_mem_len = ps_common.ltp_mem_length as usize;
    let frame_len = ps_common.frame_length as usize;
    ps_nsq.xq.copy_within(frame_len..frame_len + ltp_mem_len, 0);
    ps_nsq
        .s_ltp_shp_q14
        .copy_within(frame_len..frame_len + ltp_mem_len, 0);

    ps_del_dec[winner_ind].seed_init
}

/// The aarch64 twin of [`lpc_pred_avx2_tests::avx2_matches_scalar`]. SILK is
/// fixed point, so the vectorised prediction has to be byte-identical to the
/// scalar oracle, not merely close.
#[cfg(all(test, target_arch = "aarch64"))]
mod lpc_pred_neon_tests {
    use super::*;

    #[test]
    fn neon_matches_scalar() {
        let mut s: u64 = 0x1234_5678_9abc_def1;
        let mut rng = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for order in [10usize, 12, 14, 16] {
            for _ in 0..50_000 {
                let mut lpc = [0i32; 64];
                for v in lpc.iter_mut() {
                    *v = (rng() as i32) >> (rng() as u32 % 12);
                }
                let mut a = [0i16; 16];
                for v in a.iter_mut().take(order) {
                    *v = (rng() as i16) >> (rng() as u32 % 3);
                }
                let idx = 32 + (rng() as usize % 16);
                let got = unsafe { silk_lpc_prediction_neon(&lpc, idx, &a, order as i32) };
                let want = silk_lpc_prediction_scalar(&lpc, idx, &a, order as i32);
                assert_eq!(got, want, "order={order} idx={idx}");
            }
        }
    }
}

/// The shaping filter's dispatcher, on whatever path this target takes.
#[cfg(test)]
mod shape_filter_tests {
    use super::*;

    #[test]
    fn shape_filter_dispatch_matches_the_scalar_definition() {
        let mut s: u64 = 0x0fed_cba9_8765_4321;
        let mut rng = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for order in [8usize, 12, 16, 20, 24] {
            for _ in 0..2_000 {
                let mut sar = vec![[0i32; 4]; order + 4];
                for lane in sar.iter_mut() {
                    for v in lane.iter_mut() {
                        *v = (rng() as i32) >> (rng() as u32 % 10);
                    }
                }
                let mut diff = [0i32; 4];
                for v in diff.iter_mut() {
                    *v = (rng() as i32) >> (rng() as u32 % 10);
                }
                let warp = (rng() as i32) >> 18;
                let mut ar = vec![0i16; order];
                for v in ar.iter_mut() {
                    *v = (rng() as i16) >> (rng() as u32 % 3);
                }
                let base = (rng() as i32) >> 12;

                let mut sar_got = sar.clone();
                let mut got = [0i32; 4];
                nsq_shape_filter_soa(&mut sar_got, &diff, warp, &ar, order, base, &mut got);

                let mut sar_want = sar.clone();
                let mut want = [0i32; 4];
                nsq_shape_filter_soa_scalar(
                    &mut sar_want,
                    &diff,
                    warp,
                    &ar,
                    order,
                    base,
                    &mut want,
                );

                assert_eq!(got, want, "n_ar mismatch at order={order}");
                assert_eq!(sar_got, sar_want, "state mismatch at order={order}");
            }
        }
    }
}

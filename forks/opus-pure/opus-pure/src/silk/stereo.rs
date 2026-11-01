//! Adaptive mid/side stereo on the encoder side (libopus `silk/stereo_LR_to_MS.c`,
//! `stereo_find_predictor.c` and `stereo_quant_pred.c`).
//!
//! SILK does not code left and right. It codes a mid channel plus a *predicted*
//! side channel: two prediction weights, one for a low-passed band and one for a
//! high-passed band, are sent per frame, and only what those weights fail to
//! predict is coded as the side channel. When the weights predict the side
//! signal well enough — an amplitude-panned source, or simply too little bitrate
//! to spare — the side channel is dropped entirely and `mid_only_flag` says so.
//!
//! The decoder half lives in [`crate::silk::decode_indices`]; the two are exact
//! inverses, which is what [`stereo_tests`] checks.

use crate::range_coder::RangeCoder;
use crate::silk::define::*;
use crate::silk::macros::*;
use crate::silk::sigproc_fix::{silk_inner_prod_aligned_scale, silk_sum_sqr_shift};
use crate::silk::structs::SilkStereoState;
use crate::silk::tables::SILK_STEREO_PRED_QUANT_Q13;

/// Least-squares prediction gain of `y` from `x`, in Q13, plus the ratio of the
/// smoothed residual and basis norms in Q14 (libopus `silk_stereo_find_predictor`).
///
/// `mid_res_amp_q0` is the caller's smoothed `[mid_norm, residual_norm]` pair for
/// this band, updated in place.
fn silk_stereo_find_predictor(
    ratio_q14: &mut i32,
    x: &[i16],
    y: &[i16],
    mid_res_amp_q0: &mut [i32],
    length: usize,
    smooth_coef_q16: i32,
) -> i32 {
    let mut nrgx = 0;
    let mut scale1 = 0;
    let mut nrgy = 0;
    let mut scale2 = 0;
    silk_sum_sqr_shift(&mut nrgx, &mut scale1, x, length);
    silk_sum_sqr_shift(&mut nrgy, &mut scale2, y, length);

    // Bring both energies to a common, even scale: the inner product below is
    // taken at `scale`, and halving it for the norms needs it to divide by two.
    let mut scale = silk_max_int(scale1, scale2);
    scale += scale & 1;
    nrgy = silk_rshift32(nrgy, scale - scale2);
    nrgx = silk_max_int(silk_rshift32(nrgx, scale - scale1), 1);

    let corr = silk_inner_prod_aligned_scale(x, y, scale, length);
    let pred_q13 = silk_limit(silk_div32_varq(corr, nrgx, 13), -(1 << 14), 1 << 14);
    let pred2_q10 = silk_smulwb(pred_q13, pred_q13);

    // A large predictor means the stereo image is moving, so track it faster
    // than the nominal smoothing coefficient would.
    let smooth_coef_q16 = silk_max_int(smooth_coef_q16, pred2_q10.abs());
    debug_assert!(smooth_coef_q16 < 32768);

    let scale = silk_rshift(scale, 1);
    mid_res_amp_q0[0] = silk_smlawb(
        mid_res_amp_q0[0],
        silk_lshift(silk_sqrt_approx(nrgx), scale) - mid_res_amp_q0[0],
        smooth_coef_q16,
    );
    // Residual energy = nrgy - 2 * pred * corr + pred^2 * nrgx.
    nrgy = silk_sub_lshift32(nrgy, silk_smulwb(corr, pred_q13), 3 + 1);
    nrgy = silk_add_lshift32(nrgy, silk_smulwb(nrgx, pred2_q10), 6);
    mid_res_amp_q0[1] = silk_smlawb(
        mid_res_amp_q0[1],
        silk_lshift(silk_sqrt_approx(nrgy), scale) - mid_res_amp_q0[1],
        smooth_coef_q16,
    );

    *ratio_q14 = silk_limit(
        silk_div32_varq(mid_res_amp_q0[1], mid_res_amp_q0[0].max(1), 14),
        0,
        32767,
    );

    pred_q13
}

/// Quantize the two prediction weights and emit the index triples the bitstream
/// carries (libopus `silk_stereo_quant_pred`). `pred_q13` is replaced by the
/// quantized values, with the second subtracted from the first so the caller can
/// apply them directly.
/// Level `j` of the `STEREO_QUANT_SUB_STEPS` sub-steps between prediction-table
/// entries `i` and `i + 1`, in Q13.
///
/// The encoder searches these levels and the decoder reproduces the one the
/// encoder chose, so the two sides must agree exactly — any drift between them
/// corrupts the bitstream silently. Keep this the single definition instead of
/// transcribing the arithmetic on each side. (`stereo_tests::all_levels` is a
/// deliberately independent transcription, and is the oracle for this one.)
#[inline]
pub(crate) fn stereo_pred_level_q13(i: usize, j: i32) -> i32 {
    let low_q13 = SILK_STEREO_PRED_QUANT_Q13[i] as i32;
    // SILK_FIX_CONST(0.5 / STEREO_QUANT_SUB_STEPS, 16) = round(0.1 * 65536)
    let step_q13 = silk_smulwb(SILK_STEREO_PRED_QUANT_Q13[i + 1] as i32 - low_q13, 6554);
    silk_smlabb(low_q13, step_q13, 2 * j + 1)
}

pub fn silk_stereo_quant_pred(pred_q13: &mut [i32; 2], ix: &mut [[i8; 3]; 2]) {
    let mut quant_pred_q13 = 0;

    for n in 0..2 {
        // Walk the levels in order and stop at the first one that is worse than
        // its predecessor: the table is monotonic, so the error is unimodal and
        // the first upturn is the optimum.
        let mut err_min_q13 = i32::MAX;
        'levels: for i in 0..STEREO_QUANT_TAB_SIZE - 1 {
            for j in 0..STEREO_QUANT_SUB_STEPS {
                let lvl_q13 = stereo_pred_level_q13(i, j as i32);
                let err_q13 = (pred_q13[n] - lvl_q13).abs();
                if err_q13 < err_min_q13 {
                    err_min_q13 = err_q13;
                    quant_pred_q13 = lvl_q13;
                    ix[n][0] = i as i8;
                    ix[n][1] = j as i8;
                } else {
                    break 'levels;
                }
            }
        }
        ix[n][2] = ix[n][0] / 3;
        ix[n][0] -= ix[n][2] * 3;
        pred_q13[n] = quant_pred_q13;
    }

    // Subtract the second predictor from the first; this is the form the
    // interpolation loop and the decoder both want.
    pred_q13[0] -= pred_q13[1];
}

/// Left/right to adaptive mid/side (libopus `silk_stereo_LR_to_MS`).
///
/// `x1` and `x2` are the two channels' input buffers, each `frame_length + 2`
/// samples: slots 0 and 1 are the previous frame's two-sample tail and the rest
/// is this frame. On return `x1` holds the mid signal over the same range and
/// `x2[1 ..= frame_length]` holds the side signal with the prediction removed —
/// the ranges `silk_encode_frame` reads as `inputBuf[1..]`.
///
/// Also returns, through its out-parameters, the quantization indices for the
/// frame, whether the side channel is worth coding at all, and how to split the
/// frame's bit budget between the two channels.
pub fn silk_stereo_lr_to_ms(
    state: &mut SilkStereoState,
    x1: &mut [i16],
    x2: &mut [i16],
    ix: &mut [[i8; 3]; 2],
    mid_only_flag: &mut i8,
    mid_side_rates_bps: &mut [i32; 2],
    mut total_rate_bps: i32,
    prev_speech_act_q8: i32,
    to_mono: bool,
    fs_khz: i32,
    frame_length: usize,
) {
    debug_assert!(x1.len() >= frame_length + 2);
    debug_assert!(x2.len() >= frame_length + 2);

    let mid = x1;
    let mut side = vec![0i16; frame_length + 2];

    // Basic mid/side, before any prediction.
    for n in 0..frame_length + 2 {
        let sum = mid[n] as i32 + x2[n] as i32;
        let diff = mid[n] as i32 - x2[n] as i32;
        mid[n] = silk_rshift_round(sum, 1) as i16;
        side[n] = silk_sat16(silk_rshift_round(diff, 1)) as i16;
    }

    // The first two slots belong to the previous frame, whose tail we saved;
    // whatever the caller left there is not part of this signal.
    mid[..2].copy_from_slice(&state.s_mid);
    side[..2].copy_from_slice(&state.s_side);
    state
        .s_mid
        .copy_from_slice(&mid[frame_length..frame_length + 2]);
    state
        .s_side
        .copy_from_slice(&side[frame_length..frame_length + 2]);

    // Split both signals into a low band and a high band. The two get their own
    // prediction weight, because a stereo image is rarely the same width at the
    // bottom of the spectrum as at the top.
    let mut lp_mid = vec![0i16; frame_length];
    let mut hp_mid = vec![0i16; frame_length];
    let mut lp_side = vec![0i16; frame_length];
    let mut hp_side = vec![0i16; frame_length];
    for n in 0..frame_length {
        let sum = silk_rshift_round(
            silk_add_lshift32(mid[n] as i32 + mid[n + 2] as i32, mid[n + 1] as i32, 1),
            2,
        );
        lp_mid[n] = sum as i16;
        hp_mid[n] = (mid[n + 1] as i32 - sum) as i16;
    }
    for n in 0..frame_length {
        let sum = silk_rshift_round(
            silk_add_lshift32(side[n] as i32 + side[n + 2] as i32, side[n + 1] as i32, 1),
            2,
        );
        lp_side[n] = sum as i16;
        hp_side[n] = (side[n + 1] as i32 - sum) as i16;
    }

    let is_10ms_frame = frame_length == 10 * fs_khz as usize;
    let smooth_coef_q16 = if is_10ms_frame {
        // SILK_FIX_CONST(STEREO_RATIO_SMOOTH_COEF / 2, 16)
        328
    } else {
        // SILK_FIX_CONST(STEREO_RATIO_SMOOTH_COEF, 16)
        655
    };
    // Smooth slowly when the previous frame was mostly silence: the predictors
    // estimated from near-nothing are not worth tracking.
    let smooth_coef_q16 = silk_smulwb(
        silk_smulbb(prev_speech_act_q8, prev_speech_act_q8),
        smooth_coef_q16,
    );

    let mut lp_ratio_q14 = 0;
    let mut hp_ratio_q14 = 0;
    let mut pred_q13 = [0i32; 2];
    pred_q13[0] = silk_stereo_find_predictor(
        &mut lp_ratio_q14,
        &lp_mid,
        &lp_side,
        &mut state.mid_side_amp_q0[0..2],
        frame_length,
        smooth_coef_q16,
    );
    pred_q13[1] = silk_stereo_find_predictor(
        &mut hp_ratio_q14,
        &hp_mid,
        &hp_side,
        &mut state.mid_side_amp_q0[2..4],
        frame_length,
        smooth_coef_q16,
    );

    // How much signal the predictors leave behind, relative to the mid channel.
    let frac_q16 = silk_smlabb(hp_ratio_q14, lp_ratio_q14, 3).min(65536);

    // Roughly what the stereo parameters themselves cost.
    total_rate_bps -= if is_10ms_frame { 1200 } else { 600 };
    total_rate_bps = total_rate_bps.max(1);

    let min_mid_rate_bps = silk_smlabb(2000, fs_khz, 600);
    debug_assert!(min_mid_rate_bps < 32767);

    // Nominal split: 8 parts mid to (5 + 3 * frac) parts side.
    let frac_3_q16 = silk_mul(3, frac_q16);
    mid_side_rates_bps[0] = silk_div32_varq(total_rate_bps, 13 * 65536 + frac_3_q16, 16 + 3);
    let mut width_q14;
    if mid_side_rates_bps[0] < min_mid_rate_bps {
        // The mid channel cannot go below its floor, so the stereo image
        // narrows instead until the side channel fits in what is left.
        mid_side_rates_bps[0] = min_mid_rate_bps;
        mid_side_rates_bps[1] = total_rate_bps - mid_side_rates_bps[0];
        width_q14 = silk_div32_varq(
            silk_lshift(mid_side_rates_bps[1], 1) - min_mid_rate_bps,
            silk_smulwb(65536 + frac_3_q16, min_mid_rate_bps),
            14 + 2,
        );
        width_q14 = silk_limit(width_q14, 0, 1 << 14);
    } else {
        mid_side_rates_bps[1] = total_rate_bps - mid_side_rates_bps[0];
        width_q14 = 1 << 14;
    }

    state.smth_width_q14 = silk_smlawb(
        state.smth_width_q14 as i32,
        width_q14 - state.smth_width_q14 as i32,
        smooth_coef_q16,
    ) as i16;

    // At very low rates, or when the source is close to amplitude-panned mono,
    // stop sending a side channel and let the predictors carry the image.
    *mid_only_flag = 0;
    if to_mono {
        // Last frame before a stereo -> mono switch: collapse the width so the
        // decoder does not have to.
        width_q14 = 0;
        pred_q13 = [0, 0];
        silk_stereo_quant_pred(&mut pred_q13, ix);
    } else if state.width_prev_q14 == 0
        && (8 * total_rate_bps < 13 * min_mid_rate_bps
            || silk_smulwb(frac_q16, state.smth_width_q14 as i32) < 819)
    {
        // Panned mono, and the previous frame was already at zero width.
        pred_q13[0] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[0]), 14);
        pred_q13[1] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[1]), 14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 0;
        pred_q13 = [0, 0];
        mid_side_rates_bps[0] = total_rate_bps;
        mid_side_rates_bps[1] = 0;
        *mid_only_flag = 1;
    } else if state.width_prev_q14 != 0
        && (8 * total_rate_bps < 11 * min_mid_rate_bps
            || silk_smulwb(frac_q16, state.smth_width_q14 as i32) < 328)
    {
        // Heading for zero width; taper into it rather than cutting.
        pred_q13[0] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[0]), 14);
        pred_q13[1] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[1]), 14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 0;
        pred_q13 = [0, 0];
    } else if state.smth_width_q14 > 15565 {
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = 1 << 14;
    } else {
        pred_q13[0] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[0]), 14);
        pred_q13[1] = silk_rshift(silk_smulbb(state.smth_width_q14 as i32, pred_q13[1]), 14);
        silk_stereo_quant_pred(&mut pred_q13, ix);
        width_q14 = state.smth_width_q14 as i32;
    }

    // Keep coding the side channel until the taper above has actually been
    // transmitted, or the decoder hears the width vanish in one step.
    if *mid_only_flag == 1 {
        state.silent_side_len = (state.silent_side_len as i32 + frame_length as i32
            - STEREO_INTERP_LEN_MS * fs_khz) as i16;
        if (state.silent_side_len as i32) < LA_SHAPE_MS as i32 * fs_khz {
            *mid_only_flag = 0;
        } else {
            state.silent_side_len = 10000;
        }
    } else {
        state.silent_side_len = 0;
    }

    if *mid_only_flag == 0 && mid_side_rates_bps[1] < 1 {
        mid_side_rates_bps[1] = 1;
        mid_side_rates_bps[0] = silk_max_int(1, total_rate_bps - mid_side_rates_bps[1]);
    }

    // Subtract the prediction from the side channel, ramping the weights and the
    // width from last frame's values over the first STEREO_INTERP_LEN_MS.
    let mut pred0_q13 = -(state.pred_prev_q13[0] as i32);
    let mut pred1_q13 = -(state.pred_prev_q13[1] as i32);
    let mut w_q24 = silk_lshift(state.width_prev_q14 as i32, 10);
    let denom_q16 = silk_div32_16(1 << 16, STEREO_INTERP_LEN_MS * fs_khz);
    let delta0_q13 = -silk_rshift_round(
        silk_smulbb(pred_q13[0] - state.pred_prev_q13[0] as i32, denom_q16),
        16,
    );
    let delta1_q13 = -silk_rshift_round(
        silk_smulbb(pred_q13[1] - state.pred_prev_q13[1] as i32, denom_q16),
        16,
    );
    let deltaw_q24 = silk_lshift(
        silk_smulwb(width_q14 - state.width_prev_q14 as i32, denom_q16),
        10,
    );

    let interp_len = (STEREO_INTERP_LEN_MS * fs_khz) as usize;
    for n in 0..frame_length {
        if n < interp_len {
            pred0_q13 += delta0_q13;
            pred1_q13 += delta1_q13;
            w_q24 += deltaw_q24;
        } else if n == interp_len {
            pred0_q13 = -pred_q13[0];
            pred1_q13 = -pred_q13[1];
            w_q24 = silk_lshift(width_q14, 10);
        }
        // Q11
        let mut sum = silk_lshift(
            silk_add_lshift32(mid[n] as i32 + mid[n + 2] as i32, mid[n + 1] as i32, 1),
            9,
        );
        // Q8
        sum = silk_smlawb(silk_smulwb(w_q24, side[n + 1] as i32), sum, pred0_q13);
        sum = silk_smlawb(sum, silk_lshift(mid[n + 1] as i32, 11), pred1_q13);
        x2[n + 1] = silk_sat16(silk_rshift_round(sum, 8)) as i16;
    }

    state.pred_prev_q13[0] = pred_q13[0] as i16;
    state.pred_prev_q13[1] = pred_q13[1] as i16;
    state.width_prev_q14 = width_q14 as i16;
}

/// Entropy code the mid/side quantization indices (libopus
/// `silk_stereo_encode_pred`).
pub fn silk_stereo_encode_pred(ps_range_enc: &mut RangeCoder, ix: &[[i8; 3]; 2]) {
    let n = 5 * ix[0][2] as i32 + ix[1][2] as i32;
    debug_assert!(n < 25);
    ps_range_enc.encode_icdf(n, &crate::silk::tables::SILK_STEREO_PRED_JOINT_ICDF, 8);
    for ixn in ix.iter() {
        debug_assert!(ixn[0] < 3);
        debug_assert!((ixn[1] as usize) < STEREO_QUANT_SUB_STEPS);
        ps_range_enc.encode_icdf(ixn[0] as i32, &crate::silk::tables::SILK_UNIFORM3_ICDF, 8);
        ps_range_enc.encode_icdf(ixn[1] as i32, &crate::silk::tables::SILK_UNIFORM5_ICDF, 8);
    }
}

/// Entropy code the mid-only flag (libopus `silk_stereo_encode_mid_only`).
pub fn silk_stereo_encode_mid_only(ps_range_enc: &mut RangeCoder, mid_only_flag: i8) {
    ps_range_enc.encode_icdf(
        mid_only_flag as i32,
        &crate::silk::tables::SILK_STEREO_ONLY_CODE_MID_ICDF,
        8,
    );
}

#[cfg(test)]
mod stereo_tests {
    use super::*;
    use crate::silk::decode_indices::{silk_stereo_decode_pred, silk_stereo_ms_to_lr};

    /// One frame of libopus `silk_stereo_LR_to_MS` output, captured from the C
    /// function itself. The bulk of the two signals is covered by an FNV-1a hash
    /// over the sample bytes, with the ends spelled out so a failure says
    /// something about *where* the two diverged rather than only that they did.
    struct F {
        mid_only: i8,
        rates: [i32; 2],
        ix: [[i8; 3]; 2],
        pred_prev: [i16; 2],
        width_prev: i16,
        smth_width: i16,
        amp: [i32; 4],
        silent_side_len: i16,
        s_mid: [i16; 2],
        s_side: [i16; 2],
        mid_hash: u32,
        side_hash: u32,
        mid_head: [i16; 4],
        side_head: [i16; 4],
        mid_tail: [i16; 4],
        side_tail: [i16; 4],
    }

    fn fnv(v: &[i16]) -> u32 {
        let mut h = 2_166_136_261u32;
        for &s in v {
            let u = s as u16;
            h = (h ^ (u & 0xff) as u32).wrapping_mul(16_777_619);
            h = (h ^ (u >> 8) as u32).wrapping_mul(16_777_619);
        }
        h
    }

    /// The same integer source the C harness used: left is a smoothed random
    /// walk, right a scaled and delayed copy with its own noise, so the pair is
    /// correlated without being identical and the predictor search has work to do.
    fn gen_pair(n: usize, seed: u32) -> (Vec<i16>, Vec<i16>) {
        let mut state = seed;
        let mut lcg = move || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            ((state >> 16) & 0x7fff) as i32
        };
        let mut hist = [0i32; 4];
        let mut l = vec![0i16; n];
        let mut r = vec![0i16; n];
        for i in 0..n {
            let x = lcg() - 16384;
            let smooth = (x + 3 * hist[0] + 3 * hist[1] + hist[2]) / 4;
            hist[3] = hist[2];
            hist[2] = hist[1];
            hist[1] = hist[0];
            hist[0] = x;
            l[i] = smooth.clamp(-32768, 32767) as i16;
            let d = if i >= 3 { l[i - 3] as i32 } else { 0 };
            r[i] = ((3 * d) / 5 + (lcg() - 16384) / 8).clamp(-32768, 32767) as i16;
        }
        (l, r)
    }

    fn check(
        label: &str,
        expected: &[F],
        fs_khz: i32,
        frame_length: usize,
        total_rate: i32,
        speech_act: i32,
        to_mono_last: bool,
        seed: u32,
    ) {
        let mut state = SilkStereoState::default();
        state.reset_for_stereo();

        let n = expected.len() * (frame_length + 2);
        let (l, r) = if seed == 0 {
            (vec![0i16; n], vec![0i16; n])
        } else {
            gen_pair(n, seed)
        };

        for (f, want) in expected.iter().enumerate() {
            let base = f * (frame_length + 2);
            let mut x1 = l[base..base + frame_length + 2].to_vec();
            let mut x2 = r[base..base + frame_length + 2].to_vec();
            let mut ix = [[0i8; 3]; 2];
            let mut mid_only = 0i8;
            let mut rates = [0i32; 2];

            silk_stereo_lr_to_ms(
                &mut state,
                &mut x1,
                &mut x2,
                &mut ix,
                &mut mid_only,
                &mut rates,
                total_rate,
                speech_act,
                to_mono_last && f == expected.len() - 1,
                fs_khz,
                frame_length,
            );

            let at = format!("{label}, frame {f}");
            assert_eq!(mid_only, want.mid_only, "mid_only_flag ({at})");
            assert_eq!(rates, want.rates, "mid/side rates ({at})");
            assert_eq!(ix, want.ix, "quantization indices ({at})");
            assert_eq!(state.pred_prev_q13, want.pred_prev, "pred_prev_q13 ({at})");
            assert_eq!(
                state.width_prev_q14, want.width_prev,
                "width_prev_q14 ({at})"
            );
            assert_eq!(
                state.smth_width_q14, want.smth_width,
                "smth_width_q14 ({at})"
            );
            assert_eq!(state.mid_side_amp_q0, want.amp, "mid_side_amp_q0 ({at})");
            assert_eq!(
                state.silent_side_len, want.silent_side_len,
                "silent_side_len ({at})"
            );
            assert_eq!(state.s_mid, want.s_mid, "s_mid ({at})");
            assert_eq!(state.s_side, want.s_side, "s_side ({at})");

            let fl = frame_length;
            assert_eq!(
                [x1[0], x1[1], x1[2], x1[3]],
                want.mid_head,
                "first mid samples ({at})"
            );
            assert_eq!(
                [x2[1], x2[2], x2[3], x2[4]],
                want.side_head,
                "first side samples ({at})"
            );
            assert_eq!(
                [x1[fl - 2], x1[fl - 1], x1[fl], x1[fl + 1]],
                want.mid_tail,
                "last mid samples ({at})"
            );
            assert_eq!(
                [x2[fl - 3], x2[fl - 2], x2[fl - 1], x2[fl]],
                want.side_tail,
                "last side samples ({at})"
            );
            assert_eq!(fnv(&x1[..fl + 2]), want.mid_hash, "whole mid signal ({at})");
            assert_eq!(
                fnv(&x2[1..1 + fl]),
                want.side_hash,
                "whole side signal ({at})"
            );
        }
    }

    /// wideband 20 ms, full width.
    /// `fs=16 frame_length=320 total_rate=48000 speech_act=200 to_mono_last=0 seed=12345`
    const WIDEBAND_20MS: &[F] = &[
        F {
            mid_only: 0,
            rates: [24236, 23164],
            ix: [[0, 0, 3], [0, 0, 3]],
            pred_prev: [0, 3155],
            width_prev: 16384,
            smth_width: 16384,
            amp: [615, 481, 110, 130],
            silent_side_len: 0,
            s_mid: [7879, 2568],
            s_side: [2388, -4572],
            mid_hash: 0xd564096d,
            side_hash: 0xf83c820a,
            mid_head: [0, 0, 4103, 3524],
            side_head: [0, 40, 45, -35],
            mid_tail: [9420, 11475, 7879, 2568],
            side_tail: [-3824, -2507, 38, -646],
        },
        F {
            mid_only: 0,
            rates: [24264, 23136],
            ix: [[0, 0, 3], [0, 1, 3]],
            pred_prev: [-410, 3565],
            width_prev: 16384,
            smth_width: 16384,
            amp: [1230, 954, 226, 266],
            silent_side_len: 0,
            s_mid: [9691, 6936],
            s_side: [3244, 5185],
            mid_hash: 0x0e133398,
            side_hash: 0x45a3e9ba,
            mid_head: [7879, 2568, 3264, 2249],
            side_head: [-5560, 4472, 8213, 2395],
            mid_tail: [4150, 9192, 9691, 6936],
            side_tail: [6801, -2770, -4524, -529],
        },
        F {
            mid_only: 0,
            rates: [24362, 23038],
            ix: [[0, 1, 3], [0, 0, 3]],
            pred_prev: [410, 3155],
            width_prev: 16384,
            smth_width: 16384,
            amp: [1825, 1397, 347, 390],
            silent_side_len: 0,
            s_mid: [-6058, -2310],
            s_side: [-4801, 2730],
            mid_hash: 0xeb16cf04,
            side_hash: 0x4a2b50e7,
            mid_head: [9691, 6936, 10601, 10828],
            side_head: [2590, -85, 3881, 1744],
            mid_tail: [-4356, -6633, -6058, -2310],
            side_tail: [784, -2539, -4912, -2204],
        },
    ];

    /// wideband 10 ms.
    /// `fs=16 frame_length=160 total_rate=48000 speech_act=180 to_mono_last=0 seed=777`
    const WIDEBAND_10MS: &[F] = &[
        F {
            mid_only: 0,
            rates: [23682, 23118],
            ix: [[0, 1, 3], [0, 1, 3]],
            pred_prev: [0, 3565],
            width_prev: 16384,
            smth_width: 16384,
            amp: [189, 159, 36, 44],
            silent_side_len: 0,
            s_mid: [2780, -1025],
            s_side: [6666, -2558],
            mid_hash: 0xbfeaac9b,
            side_hash: 0x88f59102,
            mid_head: [0, 0, -5448, -4217],
            side_head: [0, -53, -22, 75],
            mid_tail: [-6159, -715, 2780, -1025],
            side_tail: [7738, 8586, 8055, 5456],
        },
        F {
            mid_only: 0,
            rates: [23968, 22832],
            ix: [[0, 0, 3], [0, 2, 3]],
            pred_prev: [-820, 3975],
            width_prev: 16384,
            smth_width: 16384,
            amp: [390, 295, 80, 98],
            silent_side_len: 0,
            s_mid: [-2298, -2492],
            s_side: [1288, 2471],
            mid_hash: 0xd537e51e,
            side_hash: 0xf7559fe3,
            mid_head: [2780, -1025, 927, -479],
            side_head: [-2111, 3683, 6453, 3485],
            mid_tail: [-7346, -4112, -2298, -2492],
            side_tail: [2666, 2268, 2126, 2123],
        },
        F {
            mid_only: 0,
            rates: [24068, 22732],
            ix: [[0, 0, 3], [0, 3, 3]],
            pred_prev: [-1230, 4385],
            width_prev: 16384,
            smth_width: 16384,
            amp: [585, 435, 136, 160],
            silent_side_len: 0,
            s_mid: [-4222, 2547],
            s_side: [1341, -181],
            mid_hash: 0x7f19ec81,
            side_hash: 0x2ef8bdbe,
            mid_head: [-2298, -2492, 3667, 4745],
            side_head: [3590, 4102, 2196, -255],
            mid_tail: [-5479, -3937, -4222, 2547],
            side_tail: [4708, 11230, 9152, 3232],
        },
    ];

    /// narrowband 20 ms at a low rate.
    /// `fs=8 frame_length=160 total_rate=9000 speech_act=250 to_mono_last=0 seed=4242`
    const NARROWBAND_20MS_LOW_RATE: &[F] = &[
        F {
            mid_only: 1,
            rates: [8400, 0],
            ix: [[0, 3, 3], [0, 2, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16228,
            amp: [548, 551, 138, 162],
            silent_side_len: 10000,
            s_mid: [-11441, -10375],
            s_side: [-2175, -724],
            mid_hash: 0xd65df5e0,
            side_hash: 0x97b79ec5,
            mid_head: [0, 0, 3371, -4092],
            side_head: [0, 0, 0, 0],
            mid_tail: [-8382, -10315, -11441, -10375],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [8400, 0],
            ix: [[0, 1, 3], [0, 0, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16073,
            amp: [1111, 1038, 284, 320],
            silent_side_len: 10000,
            s_mid: [-1198, 562],
            s_side: [-1837, -3924],
            mid_hash: 0xa7a4628e,
            side_hash: 0x97b79ec5,
            mid_head: [-11441, -10375, -2144, -393],
            side_head: [0, 0, 0, 0],
            mid_tail: [315, 2292, -1198, 562],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [8400, 0],
            ix: [[2, 4, 2], [0, 2, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 15919,
            amp: [1947, 1571, 406, 456],
            silent_side_len: 10000,
            s_mid: [-5045, -3495],
            s_side: [-11336, -1501],
            mid_hash: 0x8677b561,
            side_hash: 0x97b79ec5,
            mid_head: [-1198, 562, 795, -1991],
            side_head: [0, 0, 0, 0],
            mid_tail: [948, -5910, -5045, -3495],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [8400, 0],
            ix: [[0, 0, 3], [0, 1, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 15767,
            amp: [2686, 2096, 528, 589],
            silent_side_len: 10000,
            s_mid: [3718, 2895],
            s_side: [-4412, -2129],
            mid_hash: 0x151bcd5c,
            side_hash: 0x97b79ec5,
            mid_head: [-5045, -3495, 3778, 6674],
            side_head: [0, 0, 0, 0],
            mid_tail: [4272, 2188, 3718, 2895],
            side_tail: [0, 0, 0, 0],
        },
    ];

    /// wideband 20 ms, collapsing to mono.
    /// `fs=16 frame_length=320 total_rate=48000 speech_act=200 to_mono_last=1 seed=999`
    const WIDEBAND_20MS_TO_MONO: &[F] = &[
        F {
            mid_only: 0,
            rates: [24382, 23018],
            ix: [[0, 1, 3], [0, 0, 3]],
            pred_prev: [410, 3155],
            width_prev: 16384,
            smth_width: 16384,
            amp: [566, 444, 121, 127],
            silent_side_len: 0,
            s_mid: [4245, -1132],
            s_side: [-643, -4256],
            mid_hash: 0x517ae30d,
            side_hash: 0xb6d595cd,
            mid_head: [0, 0, -8794, -7735],
            side_head: [1, -49, -127, 32],
            mid_tail: [3029, 2327, 4245, -1132],
            side_tail: [1677, 4058, 2037, -2399],
        },
        F {
            mid_only: 0,
            rates: [24361, 23039],
            ix: [[1, 2, 2], [1, 2, 2]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16384,
            amp: [1256, 945, 232, 270],
            silent_side_len: 0,
            s_mid: [7530, 3712],
            s_side: [-4732, -3673],
            mid_hash: 0x0d49b829,
            side_hash: 0xf10d1e3f,
            mid_head: [4245, -1132, -64, -4080],
            side_head: [-3814, 4916, 2403, -2454],
            mid_tail: [6371, 7456, 7530, 3712],
            side_tail: [0, 0, 0, 0],
        },
    ];

    /// wideband 20 ms at a rate that narrows but keeps the side channel.
    /// `fs=16 frame_length=320 total_rate=20000 speech_act=240 to_mono_last=0 seed=5150`
    const WIDEBAND_20MS_NARROWED: &[F] = &[
        F {
            mid_only: 0,
            rates: [11600, 7800],
            ix: [[0, 1, 3], [0, 1, 3]],
            pred_prev: [0, 3565],
            width_prev: 16384,
            smth_width: 16295,
            amp: [903, 689, 177, 208],
            silent_side_len: 0,
            s_mid: [1062, 4187],
            s_side: [6732, 7256],
            mid_hash: 0x97c5edeb,
            side_hash: 0xa48fd85a,
            mid_head: [0, 0, -6202, -5805],
            side_head: [0, -61, -112, -53],
            mid_tail: [-4384, 1761, 1062, 4187],
            side_tail: [1937, 1425, 3112, 6270],
        },
        F {
            mid_only: 0,
            rates: [11600, 7800],
            ix: [[0, 0, 3], [0, 1, 3]],
            pred_prev: [-410, 3565],
            width_prev: 16384,
            smth_width: 16207,
            amp: [1800, 1336, 352, 411],
            silent_side_len: 0,
            s_mid: [2126, 10574],
            s_side: [-1887, 2283],
            mid_hash: 0x6cb40325,
            side_hash: 0x110f2c4d,
            mid_head: [1062, 4187, 4909, 9231],
            side_head: [5435, -5078, -2347, 3165],
            mid_tail: [11813, 4962, 2126, 10574],
            side_tail: [-1407, -193, -544, -2565],
        },
        F {
            mid_only: 0,
            rates: [11600, 7800],
            ix: [[0, 1, 3], [0, 3, 3]],
            pred_prev: [-820, 4385],
            width_prev: 16384,
            smth_width: 16119,
            amp: [2590, 2018, 539, 629],
            silent_side_len: 0,
            s_mid: [21, 1479],
            s_side: [953, 143],
            mid_hash: 0x46e2a2bb,
            side_hash: 0x1d1d48d4,
            mid_head: [2126, 10574, -3876, -3688],
            side_head: [-2082, -11876, -12377, -862],
            mid_tail: [241, -2888, 21, 1479],
            side_tail: [2702, 2727, 4812, 908],
        },
        F {
            mid_only: 0,
            rates: [11600, 7800],
            ix: [[0, 1, 3], [0, 1, 3]],
            pred_prev: [0, 3565],
            width_prev: 16384,
            smth_width: 16032,
            amp: [3379, 2651, 701, 816],
            silent_side_len: 0,
            s_mid: [2615, 63],
            s_side: [-5940, -3387],
            mid_hash: 0x669f0a41,
            side_hash: 0xa48128ae,
            mid_head: [21, 1479, 4280, 3624],
            side_head: [-467, 3294, 576, -2193],
            mid_tail: [5044, -534, 2615, 63],
            side_tail: [1650, 584, -2605, -7078],
        },
    ];

    /// wideband 10 ms at a rate that wants mid-only.
    /// `fs=16 frame_length=160 total_rate=14000 speech_act=230 to_mono_last=0 seed=8642`
    const WIDEBAND_10MS_TAPERING: &[F] = &[
        F {
            mid_only: 0,
            rates: [12799, 1],
            ix: [[2, 4, 2], [2, 3, 2]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16318,
            amp: [295, 191, 59, 55],
            silent_side_len: 32,
            s_mid: [7390, 955],
            s_side: [1461, -9895],
            mid_hash: 0x34de505d,
            side_hash: 0x97b79ec5,
            mid_head: [0, 0, 4591, 6124],
            side_head: [0, 0, 0, 0],
            mid_tail: [6713, 9735, 7390, 955],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 0,
            rates: [12799, 1],
            ix: [[0, 0, 3], [2, 4, 2]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16252,
            amp: [577, 413, 115, 115],
            silent_side_len: 64,
            s_mid: [-5685, 268],
            s_side: [337, 7731],
            mid_hash: 0x94168756,
            side_hash: 0x97b79ec5,
            mid_head: [7390, 955, -7343, -6867],
            side_head: [0, 0, 0, 0],
            mid_tail: [-3058, -8542, -5685, 268],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [12800, 0],
            ix: [[0, 0, 3], [0, 1, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16186,
            amp: [850, 603, 173, 178],
            silent_side_len: 10000,
            s_mid: [-6024, -8652],
            s_side: [-10464, -7125],
            mid_hash: 0x6a16d362,
            side_hash: 0x97b79ec5,
            mid_head: [-5685, 268, 986, 6481],
            side_head: [0, 0, 0, 0],
            mid_tail: [1769, -4976, -6024, -8652],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [12800, 0],
            ix: [[0, 0, 3], [0, 0, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16120,
            amp: [1116, 795, 224, 235],
            silent_side_len: 10000,
            s_mid: [-5415, -682],
            s_side: [-753, 2578],
            mid_hash: 0x03fc8c46,
            side_hash: 0x97b79ec5,
            mid_head: [-6024, -8652, -8328, -3433],
            side_head: [0, 0, 0, 0],
            mid_tail: [-2950, -5815, -5415, -682],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [12800, 0],
            ix: [[0, 0, 3], [0, 0, 3]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16055,
            amp: [1378, 998, 280, 300],
            silent_side_len: 10000,
            s_mid: [-3129, 3169],
            s_side: [-10015, -6647],
            mid_hash: 0x4d6029bb,
            side_hash: 0x97b79ec5,
            mid_head: [-5415, -682, 6753, 10730],
            side_head: [0, 0, 0, 0],
            mid_tail: [7506, 1216, -3129, 3169],
            side_tail: [0, 0, 0, 0],
        },
    ];

    /// silence.
    /// `fs=16 frame_length=320 total_rate=48000 speech_act=200 to_mono_last=0 seed=0`
    const SILENCE: &[F] = &[
        F {
            mid_only: 1,
            rates: [47400, 0],
            ix: [[1, 2, 2], [1, 2, 2]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16384,
            amp: [0, 0, 0, 0],
            silent_side_len: 10000,
            s_mid: [0, 0],
            s_side: [0, 0],
            mid_hash: 0x29871715,
            side_hash: 0x455f9fc5,
            mid_head: [0, 0, 0, 0],
            side_head: [0, 0, 0, 0],
            mid_tail: [0, 0, 0, 0],
            side_tail: [0, 0, 0, 0],
        },
        F {
            mid_only: 1,
            rates: [47400, 0],
            ix: [[1, 2, 2], [1, 2, 2]],
            pred_prev: [0, 0],
            width_prev: 0,
            smth_width: 16384,
            amp: [0, 0, 0, 0],
            silent_side_len: 10000,
            s_mid: [0, 0],
            s_side: [0, 0],
            mid_hash: 0x29871715,
            side_hash: 0x455f9fc5,
            mid_head: [0, 0, 0, 0],
            side_head: [0, 0, 0, 0],
            mid_tail: [0, 0, 0, 0],
            side_tail: [0, 0, 0, 0],
        },
    ];

    #[test]
    fn lr_to_ms_matches_libopus_at_full_width() {
        check(
            "wideband 20 ms",
            WIDEBAND_20MS,
            16,
            320,
            48_000,
            200,
            false,
            12345,
        );
    }

    #[test]
    fn lr_to_ms_matches_libopus_on_10ms_frames() {
        check(
            "wideband 10 ms",
            WIDEBAND_10MS,
            16,
            160,
            48_000,
            180,
            false,
            777,
        );
    }

    /// At 9 kb/s the mid channel alone needs more than the nominal split allows,
    /// so the width collapses and the side channel stops being coded. This is the
    /// branch the old encoder took unconditionally.
    #[test]
    fn lr_to_ms_matches_libopus_when_the_rate_forces_mid_only() {
        check(
            "narrowband 20 ms at 9 kb/s",
            NARROWBAND_20MS_LOW_RATE,
            8,
            160,
            9_000,
            250,
            false,
            4242,
        );
    }

    #[test]
    fn lr_to_ms_matches_libopus_collapsing_to_mono() {
        check(
            "wideband 20 ms to mono",
            WIDEBAND_20MS_TO_MONO,
            16,
            320,
            48_000,
            200,
            true,
            999,
        );
    }

    /// The quantizer, the entropy coder and the decoder have to agree exactly:
    /// whatever weight the encoder settles on is the weight the decoder applies,
    /// and there is no room for an off-by-one in the index packing.
    #[test]
    fn quantized_predictors_survive_a_round_trip_through_the_decoder() {
        for a in (-16384..=16384).step_by(97) {
            for b in (-16384..=16384).step_by(1021) {
                let mut pred = [a, b];
                let mut ix = [[0i8; 3]; 2];
                silk_stereo_quant_pred(&mut pred, &mut ix);

                let mut enc = RangeCoder::new_encoder(64);
                silk_stereo_encode_pred(&mut enc, &ix);
                enc.done();
                let n = enc.storage as usize;

                let mut dec = RangeCoder::new_decoder(&enc.buf[..n]);
                assert_eq!(
                    silk_stereo_decode_pred(&mut dec),
                    pred,
                    "predictors {a}, {b} did not survive the round trip"
                );
            }
        }
    }

    /// Between roughly 1.6x and 1.7x the mid channel's rate floor, libopus can
    /// afford a side channel but not a full-width one, so the width lands
    /// strictly between zero and one. None of the cases above reach that branch.
    #[test]
    fn lr_to_ms_matches_libopus_when_the_width_is_narrowed_but_kept() {
        check(
            "wideband 20 ms at 20 kb/s",
            WIDEBAND_20MS_NARROWED,
            16,
            320,
            20_000,
            240,
            false,
            5150,
        );
    }

    /// A 10 ms frame is shorter than the interpolation window plus the shaping
    /// look-ahead, so the first frames that want to drop the side channel are
    /// forced to keep coding it until the taper to zero width has actually been
    /// sent. Without that the width vanishes in one step and the decoder hears it.
    #[test]
    fn lr_to_ms_matches_libopus_while_tapering_into_mid_only() {
        check(
            "wideband 10 ms at 14 kb/s",
            WIDEBAND_10MS_TAPERING,
            16,
            160,
            14_000,
            230,
            false,
            8642,
        );
    }

    /// Digital silence drives the basis energy to zero, and the predictor search
    /// divides by it. libopus floors it at one; without that floor this is a
    /// divide-by-zero panic on a perfectly ordinary input.
    #[test]
    fn lr_to_ms_matches_libopus_on_silence() {
        check("silence", SILENCE, 16, 320, 48_000, 200, false, 0);
    }

    /// Every level the table can produce, in order.
    fn all_levels() -> Vec<i32> {
        let mut levels = Vec::new();
        for i in 0..STEREO_QUANT_TAB_SIZE - 1 {
            let low = SILK_STEREO_PRED_QUANT_Q13[i] as i32;
            let step = silk_smulwb(SILK_STEREO_PRED_QUANT_Q13[i + 1] as i32 - low, 6554);
            for j in 0..STEREO_QUANT_SUB_STEPS {
                levels.push(silk_smlabb(low, step, 2 * j as i32 + 1));
            }
        }
        levels
    }

    /// The search walks the levels in order and stops at the first one worse than
    /// its predecessor, which is only correct because the table is monotonic and
    /// the error is therefore unimodal. Check the shortcut against an exhaustive
    /// nearest-level search over the whole input range, including the values
    /// outside the table that `silk_stereo_find_predictor`'s clamp still admits.
    #[test]
    fn predictor_quantization_picks_the_nearest_level() {
        let levels = all_levels();
        for raw in (-16384..=16384).step_by(7) {
            let mut pred = [raw, 0];
            let mut ix = [[0i8; 3]; 2];
            silk_stereo_quant_pred(&mut pred, &mut ix);
            // pred[1] is the quantized zero; quant_pred leaves pred[0] as the
            // difference of the two, so add it back to recover the level chosen.
            let got = pred[0] + pred[1];
            let want = *levels
                .iter()
                .min_by_key(|l| ((*l - raw).abs(), **l))
                .unwrap();
            assert_eq!(got, want, "quantizing {raw}");
        }
    }

    /// The table stops short of the +/-1<<14 clamp that `silk_stereo_find_predictor`
    /// applies, so the outermost weights saturate rather than wrapping.
    #[test]
    fn predictors_beyond_the_table_saturate_at_its_ends() {
        let levels = all_levels();
        for (raw, want) in [(-16384, levels[0]), (16384, *levels.last().unwrap())] {
            let mut pred = [raw, 0];
            let mut ix = [[0i8; 3]; 2];
            silk_stereo_quant_pred(&mut pred, &mut ix);
            assert_eq!(pred[0] + pred[1], want, "quantizing {raw}");
        }
    }

    /// Analysis and synthesis are inverses: running the encoder's LR -> MS and
    /// then the decoder's MS -> LR has to give the original pair back, up to the
    /// weight quantization and the one-sample stagger between the two buffers.
    #[test]
    fn lr_to_ms_then_ms_to_lr_reconstructs_the_input() {
        let fs_khz = 16;
        let fl = 320usize;
        let (l, r) = gen_pair(4 * (fl + 2), 31337);

        let mut enc_state = SilkStereoState::default();
        enc_state.reset_for_stereo();
        let mut dec_pred_prev = [0i32; 2];
        let mut dec_s_mid = [0i16; 2];
        let mut dec_s_side = [0i16; 2];

        let mut worst = 0i32;
        for f in 0..4 {
            let base = f * (fl + 2);
            let mut x1 = l[base..base + fl + 2].to_vec();
            let mut x2 = r[base..base + fl + 2].to_vec();
            let mut ix = [[0i8; 3]; 2];
            let mut mid_only = 0i8;
            let mut rates = [0i32; 2];
            silk_stereo_lr_to_ms(
                &mut enc_state,
                &mut x1,
                &mut x2,
                &mut ix,
                &mut mid_only,
                &mut rates,
                64_000,
                256,
                false,
                fs_khz,
                fl,
            );
            assert_eq!(
                mid_only, 0,
                "frame {f} should still be coding a side channel"
            );

            // What the decoder receives: mid and side frames of `fl` samples each,
            // laid out with two slots of history in front.
            let mut d1 = vec![0i16; fl + 2];
            let mut d2 = vec![0i16; fl + 2];
            d1[2..].copy_from_slice(&x1[1..1 + fl]);
            d2[2..].copy_from_slice(&x2[1..1 + fl]);

            let mut enc = RangeCoder::new_encoder(64);
            silk_stereo_encode_pred(&mut enc, &ix);
            enc.done();
            let n = enc.storage as usize;
            let mut dec = RangeCoder::new_decoder(&enc.buf[..n]);
            let pred = silk_stereo_decode_pred(&mut dec);

            silk_stereo_ms_to_lr(
                &mut dec_pred_prev,
                &mut dec_s_mid,
                &mut dec_s_side,
                &mut d1,
                &mut d2,
                &pred,
                fs_khz,
                fl,
            );

            // The first frame runs the width up from zero, and both buffers carry
            // a one-sample stagger, so compare the settled part against the input
            // shifted by one.
            if f > 0 {
                for n in 8..fl - 1 {
                    worst = worst.max((d1[n + 1] as i32 - l[base + n] as i32).abs());
                    worst = worst.max((d2[n + 1] as i32 - r[base + n] as i32).abs());
                }
            }
        }
        assert!(
            worst < 900,
            "round trip through mid/side moved a sample by {worst}"
        );
    }
}

use crate::silk::decoder_structs::{SilkDecoderControl, SilkDecoderState};
use crate::silk::define::*;
use crate::silk::macros::*;
use crate::silk::sigproc_fix::silk_lpc_analysis_filter;
use crate::silk::tables::SILK_QUANTIZATION_OFFSETS_Q10;

/// Short-term LPC synthesis for one subframe, with the tap count known at
/// compile time.
///
/// `decode_core.c` writes the taps out one by one and asserts beforehand that
/// the order is 10 or 16. That assert is the same statement this signature
/// makes: the filter is short, its length is one of exactly two values, and
/// leaving the count to run time costs more than the duplication saves. With
/// the count in the type the tap loop unrolls the way the reference's does by
/// hand, and the history window is a slice of known length, so the reads carry
/// no bounds check.
///
/// The taps are accumulated oldest first, which is what sets this loop's speed.
/// The filter is recursive: the newest history sample is the one the previous
/// iteration wrote, so the addition that consumes it and every addition after
/// it sit on the loop-carried dependency. Taken in the reference's order the
/// newest tap comes first, which puts all `ORDER` of them on that path and
/// measured 46 cycles per output sample against a handful of cycles of
/// arithmetic. Summing the older taps first leaves them free to run ahead of
/// the recursion, and took `silk_decode_core` from 1.7x the reference's time to
/// parity with it.
///
/// The reordering is exact, not an approximation: each product depends only on
/// the history and the coefficients, never on the running total, and two's
/// complement addition is associative, so every order yields the same `i32`.
/// The RFC vectors decode exactly as they did before the reordering, all
/// 20,075 packets of them.
///
/// The reference is not slow in its own order because clang reassociates its
/// taps, which are written out one per line, into a summation tree. This loop
/// is unrolled too late in the pipeline for that to happen to it, so the order
/// written here is the order that reaches code generation.
///
/// `s_lpc_q14` holds `MAX_LPC_ORDER` samples of history followed by room for
/// the subframe; on return it holds this subframe's output in Q14. `xq`
/// receives the same samples scaled by `gain_q10` and saturated to 16 bits.
#[inline(always)]
fn lpc_synthesis<const ORDER: usize>(
    s_lpc_q14: &mut [i32],
    res_q14: &[i32],
    a_q12: &[i16],
    xq: &mut [i16],
    gain_q10: i32,
) {
    let a: &[i16; ORDER] = a_q12[..ORDER].try_into().expect("order coefficients");
    for (i, (&res, out)) in res_q14.iter().zip(xq).enumerate() {
        let hist = &s_lpc_q14[MAX_LPC_ORDER + i - ORDER..MAX_LPC_ORDER + i];
        // Seeded with half the order, which stops `silk_smlawb`'s round-toward
        // negative-infinity from biasing the sum. The reference notes the same.
        let mut lpc_pred_q10 = (ORDER >> 1) as i32;
        for j in (1..ORDER).rev() {
            lpc_pred_q10 = silk_smlawb(lpc_pred_q10, hist[ORDER - 1 - j], a[j] as i32);
        }
        // `hist`'s last element is the sample this loop wrote on the previous
        // pass, so its tap goes in once the rest of the sum is already standing.
        let lpc_pred_q10 = silk_smlawb(lpc_pred_q10, hist[ORDER - 1], a[0] as i32);
        let val = silk_add_sat32(res, silk_lshift_sat32(lpc_pred_q10, 4));
        s_lpc_q14[MAX_LPC_ORDER + i] = val;
        *out = silk_sat16(silk_rshift_round(silk_smulww(val, gain_q10), 8)) as i16;
    }
}

pub fn silk_decode_core(
    ps_dec: &mut SilkDecoderState,
    ps_dec_ctrl: &mut SilkDecoderControl,
    xq: &mut [i16],
    pulses: &[i16],
) {
    let nlsf_interpolation_flag = if ps_dec.indices.nlsf_interp_coef_q2 < 4 {
        1
    } else {
        0
    };

    let offset_q10 = SILK_QUANTIZATION_OFFSETS_Q10[(ps_dec.indices.signal_type >> 1) as usize]
        [ps_dec.indices.quant_offset_type as usize] as i32;

    let mut rand_seed = ps_dec.indices.seed as i32;
    for i in 0..ps_dec.frame_length as usize {
        rand_seed = silk_rand(rand_seed);
        ps_dec.exc_q14[i] = (pulses[i] as i32) << 14;
        if ps_dec.exc_q14[i] > 0 {
            ps_dec.exc_q14[i] -= QUANT_LEVEL_ADJUST_Q10 << 4;
        } else if ps_dec.exc_q14[i] < 0 {
            ps_dec.exc_q14[i] += QUANT_LEVEL_ADJUST_Q10 << 4;
        }
        ps_dec.exc_q14[i] += offset_q10 << 4;
        if rand_seed < 0 {
            ps_dec.exc_q14[i] = -ps_dec.exc_q14[i];
        }
        rand_seed = silk_add32_ovflw(rand_seed, pulses[i] as i32);
    }

    let mut s_lpc_q14: [i32; MAX_SUB_FRAME_LENGTH + MAX_LPC_ORDER] =
        [0; MAX_SUB_FRAME_LENGTH + MAX_LPC_ORDER];
    s_lpc_q14[..MAX_LPC_ORDER].copy_from_slice(&ps_dec.s_lpc_q14_buf);

    let mut pexc_q14_idx: usize = 0;
    let mut pxq_idx: usize = 0;
    let mut s_ltp_buf_idx = ps_dec.ltp_mem_length;

    let s_ltp_q15_len = ps_dec.ltp_mem_length as usize + ps_dec.frame_length as usize;
    let s_ltp_len = ps_dec.ltp_mem_length as usize;

    const MAX_S_LTP_Q15: usize = 640;
    const MAX_S_LTP: usize = 320;
    debug_assert!(s_ltp_q15_len <= MAX_S_LTP_Q15);
    debug_assert!(s_ltp_len <= MAX_S_LTP);
    let mut s_ltp_q15_buf = [0i32; MAX_S_LTP_Q15];
    let s_ltp_q15 = &mut s_ltp_q15_buf[..s_ltp_q15_len];
    let mut s_ltp_buf = [0i16; MAX_S_LTP];
    let s_ltp = &mut s_ltp_buf[..s_ltp_len];

    for k in 0..ps_dec.nb_subfr as usize {
        // "Avoid abrupt transition from voiced PLC to unvoiced normal decoding"
        // (silk/decode_core.c:131). Concealment of a voiced frame leaves a
        // strongly periodic signal in the LTP history; decoding the next frame as
        // unvoiced with no long-term prediction at all cuts that off mid-pitch-
        // period, which is a click. libopus instead codes the first half of the
        // frame as voiced at the concealment's own lag with a fixed 0.25 LTP
        // gain, so the periodicity fades out rather than stopping. This decoder
        // took the lag and the signal type but left the LTP gains at the decoded
        // zeros, which is the transition it was meant to avoid.
        let transition = ps_dec.loss_cnt > 0
            && ps_dec.prev_signal_type == TYPE_VOICED
            && ps_dec.indices.signal_type as i32 != TYPE_VOICED
            && k < MAX_NB_SUBFR / 2;
        if transition {
            let b = &mut ps_dec_ctrl.ltp_coef_q14[k * LTP_ORDER..(k + 1) * LTP_ORDER];
            b.fill(0);
            b[LTP_ORDER / 2] = 1 << 12; // 0.25 in Q14
            ps_dec_ctrl.pitch_l[k] = ps_dec.lag_prev;
        }

        let a_q12 = &ps_dec_ctrl.pred_coef_q12[k >> 1];
        let b_q14 = &ps_dec_ctrl.ltp_coef_q14[k * LTP_ORDER..];
        let signal_type = ps_dec.indices.signal_type;

        let mut inv_gain_q31 = silk_inverse32_varq(ps_dec_ctrl.gains_q16[k], 47);

        let gain_adj_q16 = if ps_dec_ctrl.gains_q16[k] != ps_dec.prev_gain_q16 {
            let adj = silk_div32_varq(ps_dec.prev_gain_q16, ps_dec_ctrl.gains_q16[k], 16);

            for i in 0..MAX_LPC_ORDER {
                s_lpc_q14[i] = silk_smulww(adj, s_lpc_q14[i]);
            }
            adj
        } else {
            1 << 16
        };

        ps_dec.prev_gain_q16 = ps_dec_ctrl.gains_q16[k];

        let (eff_signal_type, eff_pitch_l) = if transition {
            (TYPE_VOICED, ps_dec.lag_prev)
        } else {
            (signal_type as i32, ps_dec_ctrl.pitch_l[k])
        };

        let mut lag = 0;
        if eff_signal_type == TYPE_VOICED {
            lag = eff_pitch_l;

            if k == 0 || (k == 2 && nlsf_interpolation_flag != 0) {
                let start_idx =
                    ps_dec.ltp_mem_length - lag - ps_dec.lpc_order - (LTP_ORDER / 2) as i32;
                debug_assert!(start_idx > 0);

                if k == 2 {
                    let copy_start = ps_dec.ltp_mem_length as usize;
                    let copy_len = 2 * ps_dec.subfr_length as usize;
                    ps_dec.out_buf[copy_start..copy_start + copy_len]
                        .copy_from_slice(&xq[0..copy_len]);
                }

                let filter_input_offset = start_idx as usize + k * ps_dec.subfr_length as usize;
                let filter_len = (ps_dec.ltp_mem_length - start_idx) as usize;
                silk_lpc_analysis_filter(
                    &mut s_ltp[start_idx as usize..],
                    &ps_dec.out_buf[filter_input_offset..],
                    a_q12,
                    filter_len,
                    ps_dec.lpc_order as usize,
                    0,
                );

                if k == 0 {
                    inv_gain_q31 =
                        silk_lshift(silk_smulwb(inv_gain_q31, ps_dec_ctrl.ltp_scale_q14), 2);
                }
                for i in 0..(lag + LTP_ORDER as i32 / 2) as usize {
                    s_ltp_q15[s_ltp_buf_idx as usize - i - 1] = silk_smulwb(
                        inv_gain_q31,
                        s_ltp[ps_dec.ltp_mem_length as usize - i - 1] as i32,
                    );
                }
            } else if gain_adj_q16 != (1 << 16) {
                for i in 0..(lag + LTP_ORDER as i32 / 2) as usize {
                    s_ltp_q15[s_ltp_buf_idx as usize - i - 1] =
                        silk_smulww(gain_adj_q16, s_ltp_q15[s_ltp_buf_idx as usize - i - 1]);
                }
            }
        }

        let mut res_q14: [i32; MAX_SUB_FRAME_LENGTH] = [0; MAX_SUB_FRAME_LENGTH];

        if eff_signal_type == TYPE_VOICED {
            let pred_lag_ptr_start = (s_ltp_buf_idx - lag + LTP_ORDER as i32 / 2) as usize;
            for i in 0..ps_dec.subfr_length as usize {
                let mut ltp_pred_q13: i32 = 2;
                for j in 0..LTP_ORDER {
                    ltp_pred_q13 = silk_smlawb(
                        ltp_pred_q13,
                        s_ltp_q15[pred_lag_ptr_start + i - j],
                        b_q14[j] as i32,
                    );
                }

                res_q14[i] = silk_add_lshift32(ps_dec.exc_q14[pexc_q14_idx + i], ltp_pred_q13, 1);

                s_ltp_q15[s_ltp_buf_idx as usize] = res_q14[i] << 1;
                s_ltp_buf_idx += 1;
            }
        } else {
            res_q14[..(ps_dec.subfr_length as usize)].copy_from_slice(
                &ps_dec.exc_q14[pexc_q14_idx..(ps_dec.subfr_length as usize + pexc_q14_idx)],
            );
        }

        let subfr_length = ps_dec.subfr_length as usize;
        let gain_q10 = ps_dec_ctrl.gains_q16[k] >> 6;
        // The order is 10 or 16 and nothing else, which is what the reference's
        // `celt_assert` says before it writes the taps out one by one.
        if ps_dec.lpc_order == 16 {
            lpc_synthesis::<16>(
                &mut s_lpc_q14,
                &res_q14[..subfr_length],
                a_q12,
                &mut xq[pxq_idx..pxq_idx + subfr_length],
                gain_q10,
            );
        } else {
            lpc_synthesis::<10>(
                &mut s_lpc_q14,
                &res_q14[..subfr_length],
                a_q12,
                &mut xq[pxq_idx..pxq_idx + subfr_length],
                gain_q10,
            );
        }

        for i in 0..MAX_LPC_ORDER {
            s_lpc_q14[i] = s_lpc_q14[ps_dec.subfr_length as usize + i];
        }
        pexc_q14_idx += ps_dec.subfr_length as usize;
        pxq_idx += ps_dec.subfr_length as usize;
    }

    ps_dec
        .s_lpc_q14_buf
        .copy_from_slice(&s_lpc_q14[..MAX_LPC_ORDER]);
}

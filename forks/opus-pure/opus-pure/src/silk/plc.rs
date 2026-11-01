//! Port of libopus `silk/PLC.c` — SILK packet-loss concealment. When a frame
//! is lost the decoder extrapolates it from the previous frame's LPC + LTP
//! state plus attenuated random excitation (silk_PLC_conceal); on a good frame
//! it refreshes the concealment state (silk_PLC_update); silk_PLC_glue_frames
//! energy-matches the first good frame after a loss.

use crate::silk::decoder_structs::{SilkDecoderControl, SilkDecoderState};
use crate::silk::define::{LTP_ORDER, MAX_LPC_ORDER, MAX_NB_SUBFR, TYPE_VOICED};
use crate::silk::lpc_analysis::silk_lpc_inverse_pred_gain;
use crate::silk::macros::*;
use crate::silk::sigproc_fix::{silk_bwexpander, silk_lpc_analysis_filter, silk_sum_sqr_shift};

const BWE_COEF_Q16: i32 = 64881; // 0.99 in Q16
const V_PITCH_GAIN_START_MIN_Q14: i32 = 11469; // 0.7
const V_PITCH_GAIN_START_MAX_Q14: i32 = 15565; // 0.95
const MAX_PITCH_LAG_MS: i32 = 18;
const RAND_BUF_SIZE: i32 = 128;
const RAND_BUF_MASK: i32 = RAND_BUF_SIZE - 1;
const LOG2_INV_LPC_GAIN_HIGH_THRES: i32 = 3;
const LOG2_INV_LPC_GAIN_LOW_THRES: i32 = 8;
const PITCH_DRIFT_FAC_Q16: i32 = 655; // 0.01
const NB_ATT: usize = 2;
const HARM_ATT_Q15: [i16; NB_ATT] = [32440, 31130]; // 0.99, 0.95
const PLC_RAND_ATTENUATE_V_Q15: [i16; NB_ATT] = [31130, 26214]; // 0.95, 0.8
const PLC_RAND_ATTENUATE_UV_Q15: [i16; NB_ATT] = [32440, 29491]; // 0.99, 0.9

pub fn silk_plc_reset(ps_dec: &mut SilkDecoderState) {
    ps_dec.s_plc.pitch_l_q8 = ps_dec.frame_length << 7;
    ps_dec.s_plc.prev_gain_q16 = [1 << 16, 1 << 16];
    ps_dec.s_plc.subfr_length = 20;
    ps_dec.s_plc.nb_subfr = 2;
}

/// silk_PLC: control entry. `lost` != 0 → conceal, else refresh state.
pub fn silk_plc(
    ps_dec: &mut SilkDecoderState,
    ps_dec_ctrl: &mut SilkDecoderControl,
    frame: &mut [i16],
    lost: i32,
) {
    if ps_dec.fs_khz != ps_dec.s_plc.fs_khz {
        silk_plc_reset(ps_dec);
        ps_dec.s_plc.fs_khz = ps_dec.fs_khz;
    }
    if lost != 0 {
        silk_plc_conceal(ps_dec, ps_dec_ctrl, frame);
        ps_dec.loss_cnt += 1;
    } else {
        silk_plc_update(ps_dec, ps_dec_ctrl);
    }
}

fn silk_plc_update(ps_dec: &mut SilkDecoderState, ps_dec_ctrl: &SilkDecoderControl) {
    let nb_subfr = ps_dec.nb_subfr as usize;
    let subfr_length = ps_dec.subfr_length;
    ps_dec.prev_signal_type = ps_dec.indices.signal_type as i32;

    let mut ltp_gain_q14 = 0i32;
    let mut plc_ltp = [0i16; LTP_ORDER];
    let mut plc_pitch_l_q8 = ps_dec.s_plc.pitch_l_q8;
    if ps_dec.indices.signal_type as i32 == TYPE_VOICED {
        // The last subframe containing a pitch pulse is the last we consider.
        let mut j = 0usize;
        while (j as i32) * subfr_length < ps_dec_ctrl.pitch_l[nb_subfr - 1] {
            if j == nb_subfr {
                break;
            }
            let mut tmp = 0i32;
            for i in 0..LTP_ORDER {
                tmp += ps_dec_ctrl.ltp_coef_q14[(nb_subfr - 1 - j) * LTP_ORDER + i] as i32;
            }
            if tmp > ltp_gain_q14 {
                ltp_gain_q14 = tmp;
                let base = (nb_subfr - 1 - j) * LTP_ORDER;
                plc_ltp.copy_from_slice(&ps_dec_ctrl.ltp_coef_q14[base..base + LTP_ORDER]);
                plc_pitch_l_q8 = ps_dec_ctrl.pitch_l[nb_subfr - 1 - j] << 8;
            }
            j += 1;
        }

        plc_ltp.fill(0);
        plc_ltp[LTP_ORDER / 2] = ltp_gain_q14 as i16;

        if ltp_gain_q14 < V_PITCH_GAIN_START_MIN_Q14 {
            let tmp = V_PITCH_GAIN_START_MIN_Q14 << 10;
            let scale_q10 = silk_div32(tmp, ltp_gain_q14.max(1));
            for c in plc_ltp.iter_mut() {
                *c = silk_rshift(silk_smulbb(*c as i32, scale_q10), 10) as i16;
            }
        } else if ltp_gain_q14 > V_PITCH_GAIN_START_MAX_Q14 {
            let tmp = V_PITCH_GAIN_START_MAX_Q14 << 14;
            let scale_q14 = silk_div32(tmp, ltp_gain_q14.max(1));
            for c in plc_ltp.iter_mut() {
                *c = silk_rshift(silk_smulbb(*c as i32, scale_q14), 14) as i16;
            }
        }
    } else {
        plc_pitch_l_q8 = silk_smulbb(ps_dec.fs_khz, 18) << 8;
        plc_ltp.fill(0);
    }
    ps_dec.s_plc.pitch_l_q8 = plc_pitch_l_q8;
    ps_dec.s_plc.ltp_coef_q14 = plc_ltp;

    // Save LPC coefficients (second half), LTP scale, last two gains.
    ps_dec.s_plc.prev_lpc_q12[..ps_dec.lpc_order as usize]
        .copy_from_slice(&ps_dec_ctrl.pred_coef_q12[1][..ps_dec.lpc_order as usize]);
    ps_dec.s_plc.prev_ltp_scale_q14 = ps_dec_ctrl.ltp_scale_q14 as i16;
    ps_dec.s_plc.prev_gain_q16[0] = ps_dec_ctrl.gains_q16[nb_subfr - 2];
    ps_dec.s_plc.prev_gain_q16[1] = ps_dec_ctrl.gains_q16[nb_subfr - 1];
    ps_dec.s_plc.subfr_length = ps_dec.subfr_length;
    ps_dec.s_plc.nb_subfr = ps_dec.nb_subfr;
}

fn silk_plc_energy(
    exc_q14: &[i32],
    prev_gain_q10: &[i32; 2],
    subfr_length: usize,
    nb_subfr: usize,
) -> (i32, i32, i32, i32) {
    let mut exc_buf = vec![0i16; 2 * subfr_length];
    for k in 0..2 {
        for i in 0..subfr_length {
            exc_buf[k * subfr_length + i] = silk_sat16(silk_rshift(
                silk_smulww(
                    exc_q14[i + (k + nb_subfr - 2) * subfr_length],
                    prev_gain_q10[k],
                ),
                8,
            )) as i16;
        }
    }
    let (mut e1, mut s1, mut e2, mut s2) = (0i32, 0i32, 0i32, 0i32);
    silk_sum_sqr_shift(&mut e1, &mut s1, &exc_buf[..subfr_length], subfr_length);
    silk_sum_sqr_shift(&mut e2, &mut s2, &exc_buf[subfr_length..], subfr_length);
    (e1, s1, e2, s2)
}

fn silk_plc_conceal(
    ps_dec: &mut SilkDecoderState,
    ps_dec_ctrl: &mut SilkDecoderControl,
    frame: &mut [i16],
) {
    let lpc_order = ps_dec.lpc_order as usize;
    let subfr_length = ps_dec.subfr_length as usize;
    let nb_subfr = ps_dec.nb_subfr as usize;
    let ltp_mem_length = ps_dec.ltp_mem_length as usize;
    let frame_length = ps_dec.frame_length as usize;

    let mut s_ltp_q14 = vec![0i32; ltp_mem_length + frame_length];
    let mut s_ltp = vec![0i16; ltp_mem_length];

    let prev_gain_q10 = [
        silk_rshift(ps_dec.s_plc.prev_gain_q16[0], 6),
        silk_rshift(ps_dec.s_plc.prev_gain_q16[1], 6),
    ];

    if ps_dec.first_frame_after_reset != 0 {
        ps_dec.s_plc.prev_lpc_q12 = [0; MAX_LPC_ORDER];
    }

    let (energy1, shift1, energy2, shift2) =
        silk_plc_energy(&ps_dec.exc_q14, &prev_gain_q10, subfr_length, nb_subfr);

    let plc_nb_subfr = ps_dec.s_plc.nb_subfr as usize;
    let plc_subfr_length = ps_dec.s_plc.subfr_length as usize;
    let rand_base = if silk_rshift(energy1, shift2) < silk_rshift(energy2, shift1) {
        (0.max((plc_nb_subfr as i32 - 1) * plc_subfr_length as i32 - RAND_BUF_SIZE)) as usize
    } else {
        (0.max(plc_nb_subfr as i32 * plc_subfr_length as i32 - RAND_BUF_SIZE)) as usize
    };

    let mut b_q14 = ps_dec.s_plc.ltp_coef_q14;
    let mut rand_scale_q14 = ps_dec.s_plc.rand_scale_q14;

    let loss_idx = (NB_ATT - 1).min(ps_dec.loss_cnt.max(0) as usize);
    let harm_gain_q15 = HARM_ATT_Q15[loss_idx] as i32;
    let mut rand_gain_q15 = if ps_dec.prev_signal_type == TYPE_VOICED {
        PLC_RAND_ATTENUATE_V_Q15[loss_idx] as i32
    } else {
        PLC_RAND_ATTENUATE_UV_Q15[loss_idx] as i32
    };

    // BWE on the previous LPC, then preload to a stack array.
    silk_bwexpander(
        &mut ps_dec.s_plc.prev_lpc_q12[..lpc_order],
        lpc_order,
        BWE_COEF_Q16,
    );
    let mut a_q12 = [0i16; MAX_LPC_ORDER];
    a_q12[..lpc_order].copy_from_slice(&ps_dec.s_plc.prev_lpc_q12[..lpc_order]);

    if ps_dec.loss_cnt == 0 {
        rand_scale_q14 = 1 << 14;
        if ps_dec.prev_signal_type == TYPE_VOICED {
            let mut rs = rand_scale_q14 as i32;
            for &c in b_q14.iter() {
                rs -= c as i32;
            }
            rs = rs.max(3277); // 0.2
            rand_scale_q14 =
                silk_rshift(silk_smulbb(rs, ps_dec.s_plc.prev_ltp_scale_q14 as i32), 14) as i16;
        } else {
            let inv_gain_q30 = silk_lpc_inverse_pred_gain(&ps_dec.s_plc.prev_lpc_q12, lpc_order);
            let mut down_scale_q30 = silk_min_32(
                silk_rshift(1 << 30, LOG2_INV_LPC_GAIN_HIGH_THRES),
                inv_gain_q30,
            );
            down_scale_q30 = silk_max_32(
                silk_rshift(1 << 30, LOG2_INV_LPC_GAIN_LOW_THRES),
                down_scale_q30,
            );
            down_scale_q30 <<= LOG2_INV_LPC_GAIN_HIGH_THRES;
            rand_gain_q15 = silk_rshift(silk_smulwb(down_scale_q30, rand_gain_q15), 14);
        }
    }

    let mut rand_seed = ps_dec.s_plc.rand_seed;
    let mut lag = silk_rshift_round(ps_dec.s_plc.pitch_l_q8, 8);
    let mut s_ltp_buf_idx = ltp_mem_length;

    // Rewhiten LTP state.
    let idx = ltp_mem_length as i32 - lag - lpc_order as i32 - (LTP_ORDER as i32) / 2;
    debug_assert!(idx > 0);
    let idx = idx as usize;
    silk_lpc_analysis_filter(
        &mut s_ltp[idx..],
        &ps_dec.out_buf[idx..],
        &a_q12[..lpc_order],
        ltp_mem_length - idx,
        lpc_order,
        0,
    );
    let mut inv_gain_q30 = silk_inverse32_varq(ps_dec.s_plc.prev_gain_q16[1], 46);
    inv_gain_q30 = inv_gain_q30.min(i32::MAX >> 1);
    for i in (idx + lpc_order)..ltp_mem_length {
        s_ltp_q14[i] = silk_smulwb(inv_gain_q30, s_ltp[i] as i32);
    }

    // LTP synthesis filtering.
    let mut plc_pitch_l_q8 = ps_dec.s_plc.pitch_l_q8;
    for _k in 0..nb_subfr {
        let mut pred_lag = s_ltp_buf_idx as i32 - lag + (LTP_ORDER as i32) / 2;
        // `pred_lag` is a read index into the LTP history that advances with the
        // output, not a loop counter — rewriting it as the loop's iterator
        // (clippy's suggestion) obscures that it also indexes *backwards* by up
        // to LTP_ORDER taps.
        #[allow(clippy::explicit_counter_loop)]
        for _ in 0..subfr_length {
            let p = pred_lag as usize;
            let mut ltp_pred_q12 = 2i32;
            ltp_pred_q12 = silk_smlawb(ltp_pred_q12, s_ltp_q14[p], b_q14[0] as i32);
            ltp_pred_q12 = silk_smlawb(ltp_pred_q12, s_ltp_q14[p - 1], b_q14[1] as i32);
            ltp_pred_q12 = silk_smlawb(ltp_pred_q12, s_ltp_q14[p - 2], b_q14[2] as i32);
            ltp_pred_q12 = silk_smlawb(ltp_pred_q12, s_ltp_q14[p - 3], b_q14[3] as i32);
            ltp_pred_q12 = silk_smlawb(ltp_pred_q12, s_ltp_q14[p - 4], b_q14[4] as i32);
            pred_lag += 1;

            rand_seed = silk_rand(rand_seed);
            let ridx = (silk_rshift(rand_seed, 25) & RAND_BUF_MASK) as usize;
            s_ltp_q14[s_ltp_buf_idx] = silk_lshift(
                silk_smlawb(
                    ltp_pred_q12,
                    ps_dec.exc_q14[rand_base + ridx],
                    rand_scale_q14 as i32,
                ),
                2,
            );
            s_ltp_buf_idx += 1;
        }

        // Gradually reduce LTP gain + excitation gain, drift pitch.
        for c in b_q14.iter_mut() {
            *c = silk_rshift(silk_smulbb(harm_gain_q15, *c as i32), 15) as i16;
        }
        rand_scale_q14 = silk_rshift(silk_smulbb(rand_scale_q14 as i32, rand_gain_q15), 15) as i16;
        plc_pitch_l_q8 = silk_smlawb(plc_pitch_l_q8, plc_pitch_l_q8, PITCH_DRIFT_FAC_Q16);
        plc_pitch_l_q8 = silk_min_32(
            plc_pitch_l_q8,
            silk_smulbb(MAX_PITCH_LAG_MS, ps_dec.fs_khz) << 8,
        );
        lag = silk_rshift_round(plc_pitch_l_q8, 8);
    }

    // LPC synthesis filtering (sLPC region overlaps sLTP_Q14 at ltp_mem_length-MAX_LPC_ORDER).
    let base = ltp_mem_length - MAX_LPC_ORDER;
    s_ltp_q14[base..base + MAX_LPC_ORDER].copy_from_slice(&ps_dec.s_lpc_q14_buf[..MAX_LPC_ORDER]);
    debug_assert!(lpc_order >= 10);
    for i in 0..frame_length {
        let o = base + MAX_LPC_ORDER + i;
        let mut lpc_pred_q10 = silk_rshift(lpc_order as i32, 1);
        for (j, &a) in a_q12.iter().enumerate().take(lpc_order) {
            lpc_pred_q10 = silk_smlawb(lpc_pred_q10, s_ltp_q14[o - 1 - j], a as i32);
        }
        s_ltp_q14[o] = silk_add_sat32(s_ltp_q14[o], silk_lshift_sat32(lpc_pred_q10, 4));
        frame[i] = silk_sat16(silk_sat16(silk_rshift_round(
            silk_smulww(s_ltp_q14[o], prev_gain_q10[1]),
            8,
        ))) as i16;
    }

    // Save LPC state.
    for i in 0..MAX_LPC_ORDER {
        ps_dec.s_lpc_q14_buf[i] = s_ltp_q14[base + frame_length + i];
    }

    ps_dec.s_plc.rand_seed = rand_seed;
    ps_dec.s_plc.rand_scale_q14 = rand_scale_q14;
    ps_dec.s_plc.pitch_l_q8 = plc_pitch_l_q8;
    ps_dec.s_plc.ltp_coef_q14 = b_q14;
    for i in 0..MAX_NB_SUBFR {
        ps_dec_ctrl.pitch_l[i] = lag;
    }
}

/// silk_PLC_glue_frames: energy-match the first good frame after a loss.
pub fn silk_plc_glue_frames(ps_dec: &mut SilkDecoderState, frame: &mut [i16], length: usize) {
    if ps_dec.loss_cnt != 0 {
        let (mut e, mut s) = (0i32, 0i32);
        silk_sum_sqr_shift(&mut e, &mut s, frame, length);
        ps_dec.s_plc.conc_energy = e;
        ps_dec.s_plc.conc_energy_shift = s;
        ps_dec.s_plc.last_frame_lost = 1;
    } else {
        if ps_dec.s_plc.last_frame_lost != 0 {
            let (mut energy, mut energy_shift) = (0i32, 0i32);
            silk_sum_sqr_shift(&mut energy, &mut energy_shift, frame, length);
            let mut conc_energy = ps_dec.s_plc.conc_energy;
            let conc_shift = ps_dec.s_plc.conc_energy_shift;
            if energy_shift > conc_shift {
                conc_energy = silk_rshift(conc_energy, energy_shift - conc_shift);
            } else if energy_shift < conc_shift {
                energy = silk_rshift(energy, conc_shift - energy_shift);
            }
            if energy > conc_energy {
                let mut lz = silk_clz32(conc_energy) - 1;
                conc_energy <<= lz;
                energy = silk_rshift(energy, silk_max_32(24 - lz, 0));
                let frac_q24 = silk_div32(conc_energy, energy.max(1));
                let mut gain_q16 = silk_lshift(silk_sqrt_approx(frac_q24), 4);
                let mut slope_q16 = silk_div32_16((1 << 16) - gain_q16, length as i32);
                slope_q16 <<= 2;
                let _ = &mut lz;
                for f in frame.iter_mut().take(length) {
                    *f = silk_smulwb(gain_q16, *f as i32) as i16;
                    gain_q16 += slope_q16;
                    if gain_q16 > (1 << 16) {
                        break;
                    }
                }
            }
        }
        ps_dec.s_plc.last_frame_lost = 0;
    }
}

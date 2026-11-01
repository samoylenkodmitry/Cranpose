//! SILK comfort-noise generation — port of `silk/CNG.c`.
//!
//! Called once per decoded frame. On active silence frames
//! (`prevSignalType == NO_VOICE_ACTIVITY`) it tracks the background-noise
//! parameters (smoothed NLSF, gain, and an excitation buffer). On lost frames
//! it synthesizes comfort noise from those tracked parameters and *adds* it to
//! the PLC output, so DTX/packet-loss silence sounds like natural background
//! noise rather than dead air.

use crate::silk::decoder_structs::{SilkDecoderControl, SilkDecoderState};
use crate::silk::define::{MAX_LPC_ORDER, TYPE_NO_VOICE_ACTIVITY};
use crate::silk::macros::{
    silk_add_sat16, silk_add_sat32, silk_lshift_sat32, silk_rand, silk_rshift_round, silk_sat16,
    silk_smlawb, silk_smulwb, silk_smulww, silk_sqrt_approx,
};
use crate::silk::nlsf::silk_nlsf2a;

const CNG_BUF_MASK_MAX: i32 = 255;
const CNG_GAIN_SMTH_Q16: i32 = 4634; // 0.25^(1/4)
const CNG_GAIN_SMTH_THRESHOLD_Q16: i32 = 46396; // -3 dB
const CNG_NLSF_SMTH_Q16: i32 = 16348; // 0.25

#[inline]
fn silk_smultt(a: i32, b: i32) -> i32 {
    (a >> 16).wrapping_mul(b >> 16)
}

/// Reset the CNG state (init smoothed NLSFs to a flat spectrum, gain 0, seed).
pub fn silk_cng_reset(ps_dec: &mut SilkDecoderState) {
    let order = ps_dec.lpc_order;
    let nlsf_step_q15 = 32767 / (order + 1); // silk_int16_MAX / (LPC_order + 1)
    let mut nlsf_acc_q15 = 0i32;
    for i in 0..order as usize {
        nlsf_acc_q15 += nlsf_step_q15;
        ps_dec.s_cng.cng_smth_nlsf_q15[i] = nlsf_acc_q15 as i16;
    }
    ps_dec.s_cng.cng_smth_gain_q16 = 0;
    ps_dec.s_cng.rand_seed = 3176576;
}

/// Comfort-noise generation for one frame (silk_CNG).
pub fn silk_cng(
    ps_dec: &mut SilkDecoderState,
    ps_dec_ctrl: &SilkDecoderControl,
    frame: &mut [i16],
    length: usize,
) {
    let lpc_order = ps_dec.lpc_order as usize;
    let nb_subfr = ps_dec.nb_subfr as usize;
    let subfr_length = ps_dec.subfr_length as usize;

    if ps_dec.fs_khz != ps_dec.s_cng.fs_khz {
        silk_cng_reset(ps_dec);
        ps_dec.s_cng.fs_khz = ps_dec.fs_khz;
    }

    // Update CNG parameters from an active silence frame.
    if ps_dec.loss_cnt == 0 && ps_dec.prev_signal_type == TYPE_NO_VOICE_ACTIVITY {
        for i in 0..lpc_order {
            let d = ps_dec.prev_nlsf_q15[i] as i32 - ps_dec.s_cng.cng_smth_nlsf_q15[i] as i32;
            ps_dec.s_cng.cng_smth_nlsf_q15[i] = (ps_dec.s_cng.cng_smth_nlsf_q15[i] as i32
                + silk_smulwb(d, CNG_NLSF_SMTH_Q16))
                as i16;
        }
        // Find the subframe with the highest gain and buffer its excitation.
        let mut max_gain_q16 = 0i32;
        let mut subfr = 0usize;
        for i in 0..nb_subfr {
            if ps_dec_ctrl.gains_q16[i] > max_gain_q16 {
                max_gain_q16 = ps_dec_ctrl.gains_q16[i];
                subfr = i;
            }
        }
        ps_dec
            .s_cng
            .cng_exc_buf_q14
            .copy_within(0..(nb_subfr - 1) * subfr_length, subfr_length);
        ps_dec.s_cng.cng_exc_buf_q14[..subfr_length].copy_from_slice(
            &ps_dec.exc_q14[subfr * subfr_length..subfr * subfr_length + subfr_length],
        );
        for i in 0..nb_subfr {
            let g = ps_dec_ctrl.gains_q16[i];
            ps_dec.s_cng.cng_smth_gain_q16 +=
                silk_smulwb(g - ps_dec.s_cng.cng_smth_gain_q16, CNG_GAIN_SMTH_Q16);
            if silk_smulww(ps_dec.s_cng.cng_smth_gain_q16, CNG_GAIN_SMTH_THRESHOLD_Q16) > g {
                ps_dec.s_cng.cng_smth_gain_q16 = g;
            }
        }
    }

    if ps_dec.loss_cnt != 0 {
        let mut cng_sig_q14 = vec![0i32; length + MAX_LPC_ORDER];

        // Comfort-noise gain from the PLC scale and the smoothed background gain.
        let mut gain_q16 = silk_smulww(
            ps_dec.s_plc.rand_scale_q14 as i32,
            ps_dec.s_plc.prev_gain_q16[1],
        );
        let smth = ps_dec.s_cng.cng_smth_gain_q16;
        if gain_q16 >= (1 << 21) || smth > (1 << 23) {
            gain_q16 = silk_smultt(gain_q16, gain_q16);
            gain_q16 = (silk_smultt(smth, smth)).wrapping_sub(gain_q16 << 5);
            gain_q16 = silk_sqrt_approx(gain_q16) << 16;
        } else {
            gain_q16 = silk_smulww(gain_q16, gain_q16);
            gain_q16 = (silk_smulww(smth, smth)).wrapping_sub(gain_q16 << 5);
            gain_q16 = silk_sqrt_approx(gain_q16) << 8;
        }
        let gain_q10 = gain_q16 >> 6;

        // Random excitation from the buffered background noise.
        let mut exc_mask = CNG_BUF_MASK_MAX;
        while exc_mask > length as i32 {
            exc_mask >>= 1;
        }
        let mut seed = ps_dec.s_cng.rand_seed;
        for i in 0..length {
            seed = silk_rand(seed);
            let idx = ((seed >> 24) & exc_mask) as usize;
            cng_sig_q14[MAX_LPC_ORDER + i] = ps_dec.s_cng.cng_exc_buf_q14[idx];
        }
        ps_dec.s_cng.rand_seed = seed;

        // LPC synthesis of the comfort noise, added to the (PLC) output frame.
        let mut a_q12 = [0i16; MAX_LPC_ORDER];
        silk_nlsf2a(&mut a_q12, &ps_dec.s_cng.cng_smth_nlsf_q15, lpc_order);
        cng_sig_q14[..MAX_LPC_ORDER].copy_from_slice(&ps_dec.s_cng.cng_synth_state);
        for i in 0..length {
            let idx = MAX_LPC_ORDER + i;
            let mut lpc_pred_q10 = (lpc_order >> 1) as i32;
            for (j, &a) in a_q12.iter().enumerate().take(lpc_order) {
                lpc_pred_q10 = silk_smlawb(lpc_pred_q10, cng_sig_q14[idx - 1 - j], a as i32);
            }
            cng_sig_q14[idx] = silk_add_sat32(cng_sig_q14[idx], silk_lshift_sat32(lpc_pred_q10, 4));
            frame[i] = silk_add_sat16(
                frame[i],
                silk_sat16(silk_rshift_round(
                    silk_smulww(cng_sig_q14[idx], gain_q10),
                    8,
                )) as i16,
            );
        }
        ps_dec
            .s_cng
            .cng_synth_state
            .copy_from_slice(&cng_sig_q14[length..length + MAX_LPC_ORDER]);
    } else {
        ps_dec.s_cng.cng_synth_state[..lpc_order].fill(0);
    }
}

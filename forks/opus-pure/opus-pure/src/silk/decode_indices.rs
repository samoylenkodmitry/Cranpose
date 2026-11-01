use crate::range_coder::RangeCoder;
use crate::silk::decoder_structs::SilkDecoderState;
use crate::silk::define::*;
use crate::silk::macros::{
    silk_div32_16, silk_lshift, silk_rshift_round, silk_sat16, silk_smlawb, silk_smulbb,
};
use crate::silk::nlsf_unpack::silk_nlsf_unpack;
use crate::silk::stereo::stereo_pred_level_q13;
use crate::silk::tables::*;

pub fn silk_decode_indices(
    ps_dec: &mut SilkDecoderState,
    ps_range_dec: &mut RangeCoder,
    frame_index: i32,
    decode_lbrr: i32,
    cond_coding: i32,
) {
    let mut ix: i32;

    if decode_lbrr != 0 || ps_dec.vad_flags[frame_index as usize] != 0 {
        ix = ps_range_dec.decode_icdf(&SILK_TYPE_OFFSET_VAD_ICDF, 8) + 2;
    } else {
        ix = ps_range_dec.decode_icdf(&SILK_TYPE_OFFSET_NO_VAD_ICDF, 8);
    }
    ps_dec.indices.signal_type = (ix >> 1) as i8;
    ps_dec.indices.quant_offset_type = (ix & 1) as i8;

    if cond_coding == CODE_CONDITIONALLY {
        ps_dec.indices.gains_indices[0] = ps_range_dec.decode_icdf(&SILK_DELTA_GAIN_ICDF, 8) as i8;
    } else {
        ps_dec.indices.gains_indices[0] = (ps_range_dec
            .decode_icdf(&SILK_GAIN_ICDF[ps_dec.indices.signal_type as usize], 8)
            << 3) as i8;
        ps_dec.indices.gains_indices[0] += ps_range_dec.decode_icdf(&SILK_UNIFORM8_ICDF, 8) as i8;
    }

    for i in 1..ps_dec.nb_subfr as usize {
        ps_dec.indices.gains_indices[i] = ps_range_dec.decode_icdf(&SILK_DELTA_GAIN_ICDF, 8) as i8;
    }

    let nlsf_cb = ps_dec.ps_nlsf_cb.unwrap();
    ps_dec.indices.nlsf_indices[0] = ps_range_dec.decode_icdf(
        &nlsf_cb.cb1_icdf
            [((ps_dec.indices.signal_type >> 1) as usize) * (nlsf_cb.n_vectors as usize)..],
        8,
    ) as i8;

    let mut ec_ix: [i16; MAX_LPC_ORDER] = [0; MAX_LPC_ORDER];
    let mut pred_q8: [u8; MAX_LPC_ORDER] = [0; MAX_LPC_ORDER];
    silk_nlsf_unpack(
        &mut ec_ix,
        &mut pred_q8,
        nlsf_cb,
        ps_dec.indices.nlsf_indices[0] as usize,
    );

    for i in 0..(nlsf_cb.order as usize) {
        ix = ps_range_dec.decode_icdf(&nlsf_cb.ec_icdf[ec_ix[i] as usize..], 8);
        if ix == 0 {
            ix -= ps_range_dec.decode_icdf(&SILK_NLSF_EXT_ICDF, 8);
        } else if ix == 2 * NLSF_QUANT_MAX_AMPLITUDE {
            ix += ps_range_dec.decode_icdf(&SILK_NLSF_EXT_ICDF, 8);
        }
        ps_dec.indices.nlsf_indices[i + 1] = (ix - NLSF_QUANT_MAX_AMPLITUDE) as i8;
    }

    if ps_dec.nb_subfr == MAX_NB_SUBFR as i32 {
        ps_dec.indices.nlsf_interp_coef_q2 =
            ps_range_dec.decode_icdf(&SILK_NLSF_INTERPOLATION_FACTOR_ICDF, 8) as i8;
    } else {
        ps_dec.indices.nlsf_interp_coef_q2 = 4;
    }

    if ps_dec.indices.signal_type == TYPE_VOICED as i8 {
        let mut decode_absolute_lag_index = 1;

        if cond_coding == CODE_CONDITIONALLY && ps_dec.ec_prev_signal_type == TYPE_VOICED {
            let delta_lag_index = ps_range_dec.decode_icdf(&SILK_PITCH_DELTA_ICDF, 8) as i16;
            if delta_lag_index > 0 {
                ps_dec.indices.lag_index = ps_dec.ec_prev_lag_index + delta_lag_index - 9;
                decode_absolute_lag_index = 0;
            }
        }
        if decode_absolute_lag_index != 0 {
            ps_dec.indices.lag_index =
                ((ps_range_dec.decode_icdf(&SILK_PITCH_LAG_ICDF, 8)) * (ps_dec.fs_khz >> 1)) as i16;
            ps_dec.indices.lag_index +=
                ps_range_dec.decode_icdf(ps_dec.pitch_lag_low_bits_icdf, 8) as i16;
        }
        ps_dec.ec_prev_lag_index = ps_dec.indices.lag_index;

        ps_dec.indices.contour_index = ps_range_dec.decode_icdf(ps_dec.pitch_contour_icdf, 8) as i8;

        ps_dec.indices.per_index = ps_range_dec.decode_icdf(&SILK_LTP_PER_INDEX_ICDF, 8) as i8;

        for k in 0..ps_dec.nb_subfr as usize {
            ps_dec.indices.ltp_index[k] = ps_range_dec.decode_icdf(
                SILK_LTP_GAIN_ICDF_PTRS[ps_dec.indices.per_index as usize],
                8,
            ) as i8;
        }

        if cond_coding == CODE_INDEPENDENTLY {
            ps_dec.indices.ltp_scale_index = ps_range_dec.decode_icdf(&SILK_LTPSCALE_ICDF, 8) as i8;
        } else {
            ps_dec.indices.ltp_scale_index = 0;
        }
    }
    ps_dec.ec_prev_signal_type = ps_dec.indices.signal_type as i32;

    ps_dec.indices.seed = ps_range_dec.decode_icdf(&SILK_UNIFORM4_ICDF, 8) as i8;
}

/// Decode the mid/side stereo predictors (RFC 6716 §3; libopus
/// silk_stereo_decode_pred). Returns `pred_Q13[2]` (Q13). Previously this only
/// consumed the bits and discarded the predictors — needed for the stereo
/// MS->LR reconstruction.
pub fn silk_stereo_decode_pred(ps_range_dec: &mut RangeCoder) -> [i32; 2] {
    let mut ix = [[0i32; 3]; 2];
    let n = ps_range_dec.decode_icdf(&SILK_STEREO_PRED_JOINT_ICDF, 8);
    ix[0][2] = n / 5;
    ix[1][2] = n - 5 * ix[0][2];
    for j in 0..2 {
        ix[j][0] = ps_range_dec.decode_icdf(&SILK_UNIFORM3_ICDF, 8);
        ix[j][1] = ps_range_dec.decode_icdf(&SILK_UNIFORM5_ICDF, 8);
    }
    // Dequantize
    let mut pred_q13 = [0i32; 2];
    for j in 0..2 {
        ix[j][0] += 3 * ix[j][2];
        pred_q13[j] = stereo_pred_level_q13(ix[j][0] as usize, ix[j][1]);
    }
    // Subtract second predictor from the first (helps when applying)
    pred_q13[0] -= pred_q13[1];
    pred_q13
}

/// Adaptive Mid/Side -> Left/Right (libopus silk_stereo_MS_to_LR). `x1` is the
/// mid (becomes left), `x2` the side (becomes right); both have 2 samples of
/// history at the front (indices 0,1) and `frame_length` decoded samples after.
/// `state` carries `pred_prev_q13[2]`, `s_mid[2]`, `s_side[2]` across frames.
pub fn silk_stereo_ms_to_lr(
    pred_prev_q13: &mut [i32; 2],
    s_mid: &mut [i16; 2],
    s_side: &mut [i16; 2],
    x1: &mut [i16],
    x2: &mut [i16],
    pred_q13: &[i32; 2],
    fs_khz: i32,
    frame_length: usize,
) {
    // Buffer the 2-sample history and stash this frame's tail for next time.
    x1[0] = s_mid[0];
    x1[1] = s_mid[1];
    x2[0] = s_side[0];
    x2[1] = s_side[1];
    s_mid[0] = x1[frame_length];
    s_mid[1] = x1[frame_length + 1];
    s_side[0] = x2[frame_length];
    s_side[1] = x2[frame_length + 1];

    let interp_len = (STEREO_INTERP_LEN_MS * fs_khz) as usize;
    let denom_q16 = silk_div32_16(1 << 16, STEREO_INTERP_LEN_MS * fs_khz);
    let mut pred0_q13 = pred_prev_q13[0];
    let mut pred1_q13 = pred_prev_q13[1];
    let delta0_q13 = silk_rshift_round(silk_smulbb(pred_q13[0] - pred_prev_q13[0], denom_q16), 16);
    let delta1_q13 = silk_rshift_round(silk_smulbb(pred_q13[1] - pred_prev_q13[1], denom_q16), 16);

    for n in 0..frame_length {
        if n < interp_len {
            pred0_q13 += delta0_q13;
            pred1_q13 += delta1_q13;
        } else {
            pred0_q13 = pred_q13[0];
            pred1_q13 = pred_q13[1];
        }
        // sum = LSHIFT( ADD_LSHIFT(x1[n]+x1[n+2], x1[n+1], 1), 9 )  Q11
        let mut sum = silk_lshift(
            (x1[n] as i32 + x1[n + 2] as i32) + silk_lshift(x1[n + 1] as i32, 1),
            9,
        );
        sum = silk_smlawb(silk_lshift(x2[n + 1] as i32, 8), sum, pred0_q13); // Q8
        sum = silk_smlawb(sum, silk_lshift(x1[n + 1] as i32, 11), pred1_q13); // Q8
        x2[n + 1] = silk_sat16(silk_rshift_round(sum, 8)) as i16;
    }
    pred_prev_q13[0] = pred_q13[0];
    pred_prev_q13[1] = pred_q13[1];

    // Convert to left/right
    for n in 0..frame_length {
        let sum = x1[n + 1] as i32 + x2[n + 1] as i32;
        let diff = x1[n + 1] as i32 - x2[n + 1] as i32;
        x1[n + 1] = silk_sat16(sum) as i16;
        x2[n + 1] = silk_sat16(diff) as i16;
    }
}

pub fn silk_stereo_decode_mid_only(ps_range_dec: &mut RangeCoder) -> bool {
    ps_range_dec.decode_icdf(&SILK_STEREO_ONLY_CODE_MID_ICDF, 8) != 0
}

#[cfg(test)]
mod stereo_tests {
    use super::*;

    /// Bit-exact vs libopus silk_stereo_MS_to_LR (reference output captured from
    /// the actual C function with this exact input).
    #[test]
    fn ms_to_lr_matches_libopus() {
        let mut pred_prev = [500i32, -300];
        let mut s_mid = [10i16, -20];
        let mut s_side = [5i16, -8];
        let fl = 160usize;
        let fs = 16;
        let mut x1 = [0i16; 164];
        let mut x2 = [0i16; 164];
        for i in 0..fl {
            x1[i + 2] = (37 * (i as i32 + 1) % 1000 - 400) as i16;
            x2[i + 2] = ((i as i32 * 13) % 500 - 250) as i16;
        }
        let pred = [1200i32, -700];
        silk_stereo_ms_to_lr(
            &mut pred_prev,
            &mut s_mid,
            &mut s_side,
            &mut x1,
            &mut x2,
            &pred,
            fs,
            fl,
        );
        let idx = [0usize, 1, 2, 127, 128, 129, 158, 159];
        let l: Vec<i16> = idx.iter().map(|&i| x1[i + 1]).collect();
        let r: Vec<i16> = idx.iter().map(|&i| x2[i + 1]).collect();
        assert_eq!(l, vec![-33, -616, -571, 204, 258, 310, 264, 316]);
        assert_eq!(r, vec![-7, -110, -81, 394, 414, 436, 628, 650]);
        assert_eq!(s_mid, [483, 520]);
        assert_eq!(s_side, [-196, -183]);
    }
}

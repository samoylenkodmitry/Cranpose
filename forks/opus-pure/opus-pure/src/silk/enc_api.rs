use crate::range_coder::RangeCoder;
use crate::silk::control_fixed::*;
use crate::silk::control_snr::silk_control_snr;
use crate::silk::define::*;
use crate::silk::encode_indices::*;
use crate::silk::encode_pulses::*;
use crate::silk::gain_quant::{silk_gains_dequant, silk_gains_id, silk_gains_quant};
use crate::silk::hp_variable_cutoff::silk_hp_variable_cutoff;
use crate::silk::macros::*;
use crate::silk::noise_shape_analysis::*;
use crate::silk::nsq::*;
use crate::silk::nsq_del_dec::*;
use crate::silk::pitch_analysis::*;
use crate::silk::stereo::{
    silk_stereo_encode_mid_only, silk_stereo_encode_pred, silk_stereo_lr_to_ms,
};
use crate::silk::structs::*;
use crate::silk::tuning_parameters::{BITRESERVOIR_DECAY_TIME_MS, LBRR_SPEECH_ACTIVITY_THRES};
use crate::silk::vad::silk_vad_get_sa_q8;

pub fn silk_encode_do_vad(ps_enc: &mut SilkEncoderState, input: &[i16], activity: i32) {
    let activity_threshold = SPEECH_ACTIVITY_DTX_THRES_Q8;

    let frame_length = ps_enc.s_cmn.frame_length as usize;
    silk_vad_get_sa_q8(ps_enc, input, frame_length);

    if activity == 0 && ps_enc.s_cmn.speech_activity_q8 >= activity_threshold {
        ps_enc.s_cmn.speech_activity_q8 = activity_threshold - 1;
    }

    if ps_enc.s_cmn.speech_activity_q8 < activity_threshold {
        ps_enc.s_cmn.indices.signal_type = TYPE_NO_VOICE_ACTIVITY as i8;
        ps_enc.s_cmn.no_speech_counter += 1;
        if ps_enc.s_cmn.no_speech_counter > MAX_CONSECUTIVE_DTX + NB_SPEECH_FRAMES_BEFORE_DTX {
            ps_enc.s_cmn.no_speech_counter = NB_SPEECH_FRAMES_BEFORE_DTX;
        }
        ps_enc.s_cmn.vad_flags[ps_enc.s_cmn.n_frames_encoded as usize] = 0;
    } else {
        ps_enc.s_cmn.no_speech_counter = 0;
        ps_enc.s_cmn.indices.signal_type = TYPE_UNVOICED as i8;
        ps_enc.s_cmn.vad_flags[ps_enc.s_cmn.n_frames_encoded as usize] = 1;
    }
}

pub fn silk_encode_prefill(
    ps_enc: &mut SilkEncoderState,
    stereo: &mut SilkStereoState,
    samples: &[i16],
    _activity: i32,
) {
    let fs_khz = ps_enc.s_cmn.fs_khz as usize;

    if fs_khz != 8 && fs_khz != 12 && fs_khz != 16 {
        return;
    }

    let prefill_frame_length = fs_khz * 10;

    if samples.len() < prefill_frame_length {
        return;
    }

    let real_frame_length = ps_enc.s_cmn.frame_length as usize;
    let real_nb_subfr = ps_enc.s_cmn.nb_subfr;
    let real_subfr_length = ps_enc.s_cmn.subfr_length;

    ps_enc.s_cmn.frame_length = prefill_frame_length as i32;
    ps_enc.s_cmn.nb_subfr = 2;
    ps_enc.s_cmn.subfr_length = (prefill_frame_length / 2) as i32;

    let ltp_mem_length = ps_enc.s_cmn.ltp_mem_length as usize;
    let la_shape_ms_samples = 5 * fs_khz;

    let n = prefill_frame_length.min(samples.len());

    let mut input_buf = [0i16; super::define::MAX_FRAME_LENGTH + 2];
    input_buf[0] = stereo.s_mid[0];
    input_buf[1] = stereo.s_mid[1];
    input_buf[2..2 + n].copy_from_slice(&samples[..n]);
    stereo.s_mid[0] = input_buf[prefill_frame_length];
    stereo.s_mid[1] = input_buf[prefill_frame_length + 1];

    // libopus calls silk_LP_variable_cutoff() here to run the low-pass
    // bandwidth-transition filter. This port does not implement it: the filter
    // is driven by `silk_LP_state.mode`, which only `silk_control_audio_bandwidth`
    // ever sets, and that function was never ported. The filter was therefore
    // dead code — an unconditional no-op — so it has been removed rather than
    // left in place looking active. Porting WB/SWB LP transitions means
    // restoring both halves together.

    let x_frame_idx = ltp_mem_length;
    let dst = x_frame_idx + la_shape_ms_samples;

    if dst + prefill_frame_length <= ps_enc.s_cmn.x_buf.len() {
        ps_enc.s_cmn.x_buf[dst..dst + prefill_frame_length]
            .copy_from_slice(&input_buf[1..1 + prefill_frame_length]);
    }

    let move_len = ltp_mem_length + la_shape_ms_samples;
    if prefill_frame_length + move_len <= ps_enc.s_cmn.x_buf.len() {
        ps_enc
            .s_cmn
            .x_buf
            .copy_within(prefill_frame_length..prefill_frame_length + move_len, 0);
    }

    ps_enc.s_cmn.frame_length = real_frame_length as i32;
    ps_enc.s_cmn.nb_subfr = real_nb_subfr;
    ps_enc.s_cmn.subfr_length = real_subfr_length;
}

/// Port of libopus `silk_LBRR_encode_FIX` (silk/fixed/encode_frame_FIX.c).
///
/// In-band FEC works by putting a cheap second copy of frame N into packet
/// N+1, so a receiver that loses packet N can still recover it. The copy is
/// made cheap by raising the first subframe's gain index, which coarsens the
/// quantizer: fewer pulses survive, so the frame costs fewer bits.
///
/// The gains therefore differ from the primary frame's, and the excitation has
/// to be requantized against them — reusing the primary frame's pulses would
/// scale them by a gain they were never quantized for. libopus runs a second
/// NSQ pass over a *copy* of the quantizer state so the primary frame's
/// encoding is unaffected.
fn silk_lbrr_encode(
    ps_enc: &mut SilkEncoderState,
    s_enc_ctrl: &SilkEncoderControl,
    x_frame_idx: usize,
    cond_coding: i32,
) {
    // SILK_FIX_CONST(LBRR_SPEECH_ACTIVITY_THRES, 8)
    const LBRR_SPEECH_ACTIVITY_THRES_Q8: i32 = (LBRR_SPEECH_ACTIVITY_THRES * 256.0 + 0.5) as i32;

    if ps_enc.s_cmn.lbrr_enabled == 0
        || ps_enc.s_cmn.speech_activity_q8 <= LBRR_SPEECH_ACTIVITY_THRES_Q8
    {
        return;
    }
    let fi = ps_enc.s_cmn.n_frames_encoded as usize;
    if fi >= MAX_FRAMES_PER_PACKET {
        return;
    }
    ps_enc.s_cmn.lbrr_flags[fi] = 1;

    // Start from the primary frame's decision — same LSFs, same pitch, same
    // signal type — and change only the gains.
    let mut indices_lbrr = ps_enc.s_cmn.indices;
    let nb_subfr = ps_enc.s_cmn.nb_subfr as usize;

    if fi == 0 || ps_enc.s_cmn.lbrr_flags[fi - 1] == 0 {
        // First LBRR frame of this packet: start its gain chain from where the
        // primary encoder's has reached, and raise the first subframe.
        ps_enc.s_cmn.lbrr_prev_last_gain_index = ps_enc.s_shape.last_gain_index;
        indices_lbrr.gains_indices[0] = silk_min_int(
            indices_lbrr.gains_indices[0] as i32 + ps_enc.s_cmn.lbrr_gain_increases,
            N_LEVELS_QGAIN - 1,
        ) as i8;
    }

    // Dequantize so the encoder quantizes against exactly the gains the decoder
    // will reconstruct.
    let mut gains_q16 = [0i32; MAX_NB_SUBFR];
    let mut prev_ind = ps_enc.s_cmn.lbrr_prev_last_gain_index;
    silk_gains_dequant(
        &mut gains_q16,
        &indices_lbrr.gains_indices,
        &mut prev_ind,
        if cond_coding == CODE_CONDITIONALLY {
            1
        } else {
            0
        },
        nb_subfr,
    );
    ps_enc.s_cmn.lbrr_prev_last_gain_index = prev_ind;

    let mut pred_coef_q12_flat = [0i16; 2 * MAX_LPC_ORDER];
    pred_coef_q12_flat[..MAX_LPC_ORDER].copy_from_slice(&s_enc_ctrl.pred_coef_q12[0]);
    pred_coef_q12_flat[MAX_LPC_ORDER..].copy_from_slice(&s_enc_ctrl.pred_coef_q12[1]);

    // A copy of the quantizer state: the LBRR pass must not disturb the primary
    // stream's noise-shaping memory.
    let mut nsq_lbrr = ps_enc.s_nsq;
    let mut pulses_lbrr = [0i8; MAX_FRAME_LENGTH];
    if ps_enc.s_cmn.n_states_delayed_decision > 1 {
        let winner_seed = silk_nsq_del_dec(
            &ps_enc.s_cmn,
            &mut nsq_lbrr,
            &indices_lbrr,
            &ps_enc.s_cmn.x_buf[x_frame_idx..],
            &mut pulses_lbrr,
            &pred_coef_q12_flat,
            &s_enc_ctrl.ltp_coef_q14,
            &s_enc_ctrl.ar_q13,
            &s_enc_ctrl.harm_shape_gain_q14,
            &s_enc_ctrl.tilt_q14,
            &s_enc_ctrl.lf_shp_q14,
            &gains_q16,
            &s_enc_ctrl.pitch_l,
            s_enc_ctrl.lambda_q10,
            s_enc_ctrl.ltp_scale_q14,
        );
        indices_lbrr.seed = winner_seed as i8;
    } else {
        silk_nsq(
            &ps_enc.s_cmn,
            &mut nsq_lbrr,
            &indices_lbrr,
            &ps_enc.s_cmn.x_buf[x_frame_idx..],
            &mut pulses_lbrr,
            &pred_coef_q12_flat,
            &s_enc_ctrl.ltp_coef_q14,
            &s_enc_ctrl.ar_q13,
            &s_enc_ctrl.harm_shape_gain_q14,
            &s_enc_ctrl.tilt_q14,
            &s_enc_ctrl.lf_shp_q14,
            &gains_q16,
            &s_enc_ctrl.pitch_l,
            s_enc_ctrl.lambda_q10,
            s_enc_ctrl.ltp_scale_q14,
        );
    }

    ps_enc.s_cmn.indices_lbrr[fi] = indices_lbrr;
    ps_enc.s_cmn.pulses_lbrr[fi] = pulses_lbrr;
}

pub fn silk_encode_frame(
    ps_enc: &mut SilkEncoderState,
    input: &[i16],
    rc: &mut RangeCoder,
    pn_bytes_out: &mut i32,
    cond_coding: i32,
    max_bits: i32,
    use_cbr: i32,
) -> i32 {
    let mut s_enc_ctrl = SilkEncoderControl::default();

    ps_enc.s_cmn.indices.seed = (ps_enc.s_cmn.frame_counter & 3) as i8;
    ps_enc.s_cmn.frame_counter += 1;

    let frame_length = ps_enc.s_cmn.frame_length as usize;
    let ltp_mem_length = ps_enc.s_cmn.ltp_mem_length as usize;
    let la_shape = ps_enc.s_cmn.la_shape as usize;

    let x_frame_idx = ltp_mem_length;

    let la_shape_max = 5 * ps_enc.s_cmn.fs_khz as usize;
    let new_samples_idx = x_frame_idx + la_shape_max;
    ps_enc.s_cmn.x_buf[new_samples_idx..new_samples_idx + frame_length]
        .copy_from_slice(&input[..frame_length]);

    let x_buf_copy = ps_enc.s_cmn.x_buf;

    let mut res_pitch = [0i16; LA_PITCH_MAX + MAX_FRAME_LENGTH + LTP_MEM_LENGTH_MS * MAX_FS_KHZ];
    let res_pitch_frame_idx = ltp_mem_length;

    silk_find_pitch_lags_fix(ps_enc, &mut s_enc_ctrl, &mut res_pitch, &x_buf_copy, 0);

    let x_tmp = &x_buf_copy[x_frame_idx - la_shape..];
    silk_noise_shape_analysis_fix(
        ps_enc,
        &mut s_enc_ctrl,
        &res_pitch[res_pitch_frame_idx..],
        x_tmp,
    );

    let predict_lpc_order = ps_enc.s_cmn.predict_lpc_order as usize;
    let x_tmp_frame = &x_buf_copy[x_frame_idx - predict_lpc_order..];
    silk_find_pred_coefs_fix(
        ps_enc,
        &mut s_enc_ctrl,
        &res_pitch,
        res_pitch_frame_idx,
        x_tmp_frame,
        &x_buf_copy,
        cond_coding,
    );

    silk_process_gains_fix(ps_enc, &mut s_enc_ctrl, cond_coding);

    // Low-bitrate redundancy for this frame, carried in the NEXT packet.
    silk_lbrr_encode(ps_enc, &s_enc_ctrl, x_frame_idx, cond_coding);

    let max_iter = 6;
    let mut gain_mult_q8: i32 = 256;
    let mut found_lower = false;
    let mut found_upper = false;
    #[allow(unused_assignments)]
    let mut n_bits: i32 = 0;
    let mut n_bits_lower: i32 = 0;
    let mut n_bits_upper: i32 = 0;
    let mut gain_mult_lower: i32 = 0;
    let mut gain_mult_upper: i32 = 0;
    let mut gains_id: i32 =
        silk_gains_id(&ps_enc.s_cmn.indices.gains_indices, ps_enc.s_cmn.nb_subfr);
    let mut gains_id_lower: i32 = -1;
    let mut gains_id_upper: i32 = -1;

    let bits_margin = if use_cbr != 0 { 5 } else { max_bits / 4 };

    let rc_copy = rc.clone();
    let nsq_copy = ps_enc.s_nsq;
    let seed_copy = ps_enc.s_cmn.indices.seed;
    let ec_prev_lag_index_copy = ps_enc.s_cmn.ec_prev_lag_index;
    let ec_prev_signal_type_copy = ps_enc.s_cmn.ec_prev_signal_type;
    let mut rc_copy2: Option<RangeCoder> = None;
    let mut nsq_copy2: Option<SilkNSQState> = None;
    let mut ec_buf_copy = [0u8; 1275];
    let mut last_gain_index_copy2: i8 = 0;

    let mut gain_lock = [false; MAX_NB_SUBFR];
    let mut best_gain_mult = [256i32; MAX_NB_SUBFR];
    let mut best_sum = [i32::MAX; MAX_NB_SUBFR];

    for iter in 0..=max_iter {
        if gains_id == gains_id_lower {
            n_bits = n_bits_lower;
        } else if gains_id == gains_id_upper {
            n_bits = n_bits_upper;
        } else {
            if iter > 0 {
                *rc = rc_copy.clone();
                ps_enc.s_nsq = nsq_copy;
                ps_enc.s_cmn.indices.seed = seed_copy;
                ps_enc.s_cmn.ec_prev_lag_index = ec_prev_lag_index_copy;
                ps_enc.s_cmn.ec_prev_signal_type = ec_prev_signal_type_copy;
            }

            let mut pred_coef_q12_flat = [0i16; 2 * MAX_LPC_ORDER];
            pred_coef_q12_flat[..MAX_LPC_ORDER].copy_from_slice(&s_enc_ctrl.pred_coef_q12[0]);
            pred_coef_q12_flat[MAX_LPC_ORDER..].copy_from_slice(&s_enc_ctrl.pred_coef_q12[1]);

            if ps_enc.s_cmn.n_states_delayed_decision > 1 {
                let winner_seed = silk_nsq_del_dec(
                    &ps_enc.s_cmn,
                    &mut ps_enc.s_nsq,
                    &ps_enc.s_cmn.indices,
                    &ps_enc.s_cmn.x_buf[x_frame_idx..],
                    &mut ps_enc.pulses,
                    &pred_coef_q12_flat,
                    &s_enc_ctrl.ltp_coef_q14,
                    &s_enc_ctrl.ar_q13,
                    &s_enc_ctrl.harm_shape_gain_q14,
                    &s_enc_ctrl.tilt_q14,
                    &s_enc_ctrl.lf_shp_q14,
                    &s_enc_ctrl.gains_q16,
                    &s_enc_ctrl.pitch_l,
                    s_enc_ctrl.lambda_q10,
                    s_enc_ctrl.ltp_scale_q14,
                );

                ps_enc.s_cmn.indices.seed = winner_seed as i8;
            } else {
                silk_nsq(
                    &ps_enc.s_cmn,
                    &mut ps_enc.s_nsq,
                    &ps_enc.s_cmn.indices,
                    &ps_enc.s_cmn.x_buf[x_frame_idx..],
                    &mut ps_enc.pulses,
                    &pred_coef_q12_flat,
                    &s_enc_ctrl.ltp_coef_q14,
                    &s_enc_ctrl.ar_q13,
                    &s_enc_ctrl.harm_shape_gain_q14,
                    &s_enc_ctrl.tilt_q14,
                    &s_enc_ctrl.lf_shp_q14,
                    &s_enc_ctrl.gains_q16,
                    &s_enc_ctrl.pitch_l,
                    s_enc_ctrl.lambda_q10,
                    s_enc_ctrl.ltp_scale_q14,
                );
            }

            if iter == max_iter && !found_lower {
                rc_copy2 = Some(rc.clone());
            }

            silk_encode_indices(
                ps_enc,
                rc,
                ps_enc.s_cmn.n_frames_encoded as usize,
                false,
                cond_coding,
            );

            silk_encode_pulses(
                rc,
                ps_enc.s_cmn.indices.signal_type as i32,
                ps_enc.s_cmn.indices.quant_offset_type as i32,
                &ps_enc.pulses,
                ps_enc.s_cmn.frame_length as usize,
            );

            n_bits = rc.tell();

            if iter == max_iter && !found_lower && n_bits > max_bits {
                if let Some(rc_c2) = &rc_copy2 {
                    *rc = rc_c2.clone();
                }

                ps_enc.s_shape.last_gain_index = s_enc_ctrl.last_gain_index_prev;
                for i in 0..ps_enc.s_cmn.nb_subfr as usize {
                    ps_enc.s_cmn.indices.gains_indices[i] = 4;
                }
                if cond_coding != CODE_CONDITIONALLY {
                    ps_enc.s_cmn.indices.gains_indices[0] = s_enc_ctrl.last_gain_index_prev;
                }
                ps_enc.s_cmn.ec_prev_lag_index = ec_prev_lag_index_copy;
                ps_enc.s_cmn.ec_prev_signal_type = ec_prev_signal_type_copy;

                ps_enc.pulses.fill(0);

                silk_encode_indices(
                    ps_enc,
                    rc,
                    ps_enc.s_cmn.n_frames_encoded as usize,
                    false,
                    cond_coding,
                );
                silk_encode_pulses(
                    rc,
                    ps_enc.s_cmn.indices.signal_type as i32,
                    ps_enc.s_cmn.indices.quant_offset_type as i32,
                    &ps_enc.pulses,
                    ps_enc.s_cmn.frame_length as usize,
                );

                n_bits = rc.tell();
            }

            if use_cbr == 0 && iter == 0 && n_bits <= max_bits {
                break;
            }
        }

        if iter == max_iter {
            if found_lower && (gains_id == gains_id_lower || n_bits > max_bits) {
                if let Some(rc_c2) = &rc_copy2 {
                    *rc = rc_c2.clone();
                    let offs = rc.offs as usize;
                    rc.buf_mut()[..offs].copy_from_slice(&ec_buf_copy[..offs]);
                }
                if let Some(nsq_c2) = &nsq_copy2 {
                    ps_enc.s_nsq = *nsq_c2;
                }
                ps_enc.s_shape.last_gain_index = last_gain_index_copy2;
            }
            break;
        }

        if n_bits > max_bits {
            if !found_lower && iter >= 2 {
                s_enc_ctrl.lambda_q10 =
                    silk_add_rshift32(s_enc_ctrl.lambda_q10, s_enc_ctrl.lambda_q10, 1);
                found_upper = false;
                gains_id_upper = -1;
            } else {
                found_upper = true;
                n_bits_upper = n_bits;
                gain_mult_upper = gain_mult_q8;
                gains_id_upper = gains_id;
            }
        } else if n_bits < max_bits - bits_margin {
            found_lower = true;
            n_bits_lower = n_bits;
            gain_mult_lower = gain_mult_q8;
            if gains_id != gains_id_lower {
                gains_id_lower = gains_id;

                rc_copy2 = Some(rc.clone());
                let offs = rc.offs as usize;
                ec_buf_copy[..offs].copy_from_slice(&rc.buf[..offs]);
                nsq_copy2 = Some(ps_enc.s_nsq);
                last_gain_index_copy2 = ps_enc.s_shape.last_gain_index;
            }
        } else {
            break;
        }

        if !found_lower && n_bits > max_bits {
            let subfr_length = ps_enc.s_cmn.subfr_length as usize;
            for i in 0..ps_enc.s_cmn.nb_subfr as usize {
                let mut sum: i32 = 0;
                for j in (i * subfr_length)..((i + 1) * subfr_length) {
                    sum += ps_enc.pulses[j].abs() as i32;
                }
                if iter == 0 || (sum < best_sum[i] && !gain_lock[i]) {
                    best_sum[i] = sum;
                    best_gain_mult[i] = gain_mult_q8;
                } else {
                    gain_lock[i] = true;
                }
            }
        }

        if !(found_lower && found_upper) {
            if n_bits > max_bits {
                gain_mult_q8 = silk_min_32(1024, (gain_mult_q8 * 3) / 2);
            } else {
                gain_mult_q8 = silk_max_32(64, (gain_mult_q8 * 4) / 5);
            }
        } else {
            let delta = gain_mult_upper - gain_mult_lower;
            gain_mult_q8 = gain_mult_lower
                + silk_div32_16(
                    (gain_mult_upper - gain_mult_lower) * (max_bits - n_bits_lower),
                    n_bits_upper - n_bits_lower,
                );

            let lower_limit = silk_add_rshift32(gain_mult_lower, delta, 2);
            let upper_limit = silk_sub_rshift32(gain_mult_upper, delta, 2);
            if gain_mult_q8 > lower_limit {
                gain_mult_q8 = lower_limit;
            } else if gain_mult_q8 < upper_limit {
                gain_mult_q8 = upper_limit;
            }
        }

        for i in 0..ps_enc.s_cmn.nb_subfr as usize {
            let tmp = if gain_lock[i] {
                best_gain_mult[i]
            } else {
                gain_mult_q8
            };
            s_enc_ctrl.gains_q16[i] =
                silk_lshift_sat32(silk_smulwb(s_enc_ctrl.gains_unq_q16[i], tmp), 8);
        }

        ps_enc.s_shape.last_gain_index = s_enc_ctrl.last_gain_index_prev;
        silk_gains_quant(
            &mut ps_enc.s_cmn.indices.gains_indices,
            &mut s_enc_ctrl.gains_q16,
            &mut ps_enc.s_shape.last_gain_index,
            if cond_coding == CODE_CONDITIONALLY {
                1
            } else {
                0
            },
            ps_enc.s_cmn.nb_subfr as usize,
        );

        gains_id = silk_gains_id(&ps_enc.s_cmn.indices.gains_indices, ps_enc.s_cmn.nb_subfr);
    }

    let move_len = ltp_mem_length + 5 * ps_enc.s_cmn.fs_khz as usize;
    ps_enc
        .s_cmn
        .x_buf
        .copy_within(frame_length..frame_length + move_len, 0);

    ps_enc.s_cmn.prev_lag = s_enc_ctrl.pitch_l[ps_enc.s_cmn.nb_subfr as usize - 1];
    ps_enc.s_cmn.prev_signal_type = ps_enc.s_cmn.indices.signal_type as i32;
    ps_enc.s_cmn.first_frame_after_reset = 0;

    *pn_bytes_out = (rc.tell() + 7) >> 3;

    0
}

/// Encode one packet (libopus `silk_Encode`).
///
/// `samples_l` and `samples_r` are at SILK's internal sample rate. In mono only
/// `samples_l` is read; in stereo the two are converted to an adaptive mid/side
/// pair here rather than by the caller, because the conversion depends on the
/// frame's bit budget and on state that has to advance in step with the coder.
pub fn silk_encode(
    ps_enc: &mut SilkEncoder,
    samples_l: &[i16],
    samples_r: &[i16],
    rc: &mut RangeCoder,
    n_bytes_out: &mut i32,
    target_rate_bps: i32,
    max_bits: i32,
    use_cbr: i32,
    activity: i32,
) -> i32 {
    let n_channels = ps_enc.n_channels_internal.clamp(1, 2) as usize;
    let n_frames_per_packet = ps_enc.state[0].s_cmn.n_frames_per_packet;
    let frame_length = ps_enc.state[0].s_cmn.frame_length as usize;
    let packet_size_ms = ps_enc.state[0].s_cmn.packet_size_ms;
    let fs_khz = ps_enc.state[0].s_cmn.fs_khz;
    let n_samples_in = samples_l.len();

    for ch in 0..n_channels {
        ps_enc.state[ch].s_cmn.n_frames_encoded = 0;
    }

    // Pending redundancy that no longer matches the layout it will be coded in
    // is dropped rather than mis-coded (libopus enc_API.c `transition`). It
    // outlives the frame that produced it by one packet, so a change of packet
    // duration or channel count in between would have it coded as a frame of a
    // different length: 20 ms of stored audio written out as a 10 ms frame is
    // not the audio that was lost, and a receiver recovering from it would be
    // worse off than concealing.
    let layout_changed =
        ps_enc.lbrr_packet_size_ms != packet_size_ms || ps_enc.lbrr_n_channels != n_channels as i32;
    for ch in 0..n_channels {
        if layout_changed || ps_enc.state[ch].s_cmn.first_frame_after_reset != 0 {
            ps_enc.state[ch].s_cmn.lbrr_flags = [0; MAX_FRAMES_PER_PACKET];
        }
    }
    ps_enc.lbrr_packet_size_ms = packet_size_ms;
    ps_enc.lbrr_n_channels = n_channels as i32;

    let n_blocks_of_10ms = (100 * n_samples_in as i32) / (fs_khz * 1000);
    let tot_blocks = if n_blocks_of_10ms > 1 {
        n_blocks_of_10ms >> 1
    } else {
        1
    };

    let packet_bits_total = target_rate_bps * packet_size_ms / 1000;

    // Which frames of the *previous* packet were stored for in-band FEC, per
    // channel: `lbrr_flags` was set by `silk_lbrr_encode` as those frames were
    // coded. The flags, and then the redundant frames themselves, go out at the
    // head of this packet.
    let mut lbrr_symbol = [0i32; 2];
    for ch in 0..n_channels {
        for i in 0..n_frames_per_packet as usize {
            lbrr_symbol[ch] |= ps_enc.state[ch].s_cmn.lbrr_flags[i] << i;
        }
        ps_enc.state[ch].s_cmn.lbrr_flag = if lbrr_symbol[ch] > 0 { 1 } else { 0 };
    }

    let mut sample_offset = 0usize;
    // Two channels' worth of input, each with the two-sample history slot in
    // front that the mid/side conversion and the shaping filters both need.
    let mut input_buf = [[0i16; MAX_FRAME_LENGTH + 2]; 2];

    for frame_idx in 0..n_frames_per_packet {
        let fi = frame_idx as usize;
        if frame_idx == 0 {
            silk_hp_variable_cutoff(&mut ps_enc.state[0].s_cmn);
        }

        let fs_in_khz = fs_khz as usize;
        let frame_end = (sample_offset + frame_length).min(n_samples_in);
        if frame_end.saturating_sub(sample_offset) < fs_in_khz {
            sample_offset += frame_length;
            continue;
        }
        let n = frame_end - sample_offset;
        if n > MAX_FRAME_LENGTH {
            sample_offset += frame_length;
            continue;
        }

        // The caller hands us audio that is already at SILK's internal rate and
        // already carries the resampler's delay (`SilkEncoderResampler`), so
        // there is nothing to do here but place it. This used to run a second
        // delay buffer of its own, which was right only when the API rate
        // happened to equal the internal rate and put every other rate 10
        // samples of internal rate late.
        for ch in 0..n_channels {
            let raw_frame = if ch == 0 {
                &samples_l[sample_offset..frame_end]
            } else {
                &samples_r[sample_offset..frame_end]
            };
            input_buf[ch][2..2 + n].copy_from_slice(&raw_frame[..n]);
        }

        let mut curr_lbrr_bits = 0;
        if frame_idx == 0 {
            // Reserve the VAD and FEC flags at the head of the packet; they are
            // patched in once every frame has been coded.
            let n_flag_bits = ((n_frames_per_packet + 1) * n_channels as i32) as u32;
            let icdf_val = (256i32 - (256i32 >> n_flag_bits)) as u8;
            let icdf = [icdf_val, 0u8];
            rc.encode_icdf(0, &icdf, 8);
            curr_lbrr_bits = rc.tell();

            for ch in 0..n_channels {
                if lbrr_symbol[ch] > 0 && n_frames_per_packet > 1 {
                    let lbrr_icdf = match n_frames_per_packet {
                        3 => &crate::silk::tables::SILK_LBRR_FLAGS_3_ICDF[..],
                        _ => &crate::silk::tables::SILK_LBRR_FLAGS_2_ICDF[..],
                    };
                    rc.encode_icdf(lbrr_symbol[ch] - 1, lbrr_icdf, 8);
                }
            }

            for i in 0..n_frames_per_packet as usize {
                for ch in 0..n_channels {
                    if ps_enc.state[ch].s_cmn.lbrr_flags[i] == 0 {
                        continue;
                    }
                    // The stereo side info belongs to the frame, not to the
                    // channel, so it precedes the mid channel's payload; the
                    // mid-only flag is redundant when the side channel has
                    // LBRR data of its own.
                    if n_channels == 2 && ch == 0 {
                        silk_stereo_encode_pred(rc, &ps_enc.stereo.pred_ix[i]);
                        if ps_enc.state[1].s_cmn.lbrr_flags[i] == 0 {
                            silk_stereo_encode_mid_only(rc, ps_enc.stereo.mid_only_flags[i]);
                        }
                    }
                    // CODE_INDEPENDENTLY, not ..._NO_LTP_SCALING: on a voiced
                    // frame the two differ by one LTP-scale symbol, which the
                    // decoder reads unconditionally. Omitting it desynchronized
                    // the range coder for the rest of the LBRR payload.
                    let lbrr_cond = if i > 0 && ps_enc.state[ch].s_cmn.lbrr_flags[i - 1] != 0 {
                        CODE_CONDITIONALLY
                    } else {
                        CODE_INDEPENDENTLY
                    };
                    silk_encode_indices(&mut ps_enc.state[ch], rc, i, true, lbrr_cond);
                    silk_encode_pulses(
                        rc,
                        ps_enc.state[ch].s_cmn.indices_lbrr[i].signal_type as i32,
                        ps_enc.state[ch].s_cmn.indices_lbrr[i].quant_offset_type as i32,
                        &ps_enc.state[ch].s_cmn.pulses_lbrr[i],
                        frame_length,
                    );
                }
            }
            for ch in 0..n_channels {
                ps_enc.state[ch].s_cmn.lbrr_flags = [0; MAX_FRAMES_PER_PACKET];
            }
            curr_lbrr_bits = rc.tell() - curr_lbrr_bits;
        }

        // What this frame may spend. libopus reads the target through a bit
        // reservoir: whatever previous packets overspent is subtracted here
        // until it drains, and within a multi-frame packet the running balance
        // is subtracted too. Without it a VBR stream holds its per-frame target
        // but drifts well above its average rate.
        if curr_lbrr_bits < 10 {
            ps_enc.n_bits_used_lbrr = 0;
        } else if ps_enc.n_bits_used_lbrr < 10 {
            ps_enc.n_bits_used_lbrr = curr_lbrr_bits;
        } else {
            ps_enc.n_bits_used_lbrr = (ps_enc.n_bits_used_lbrr + curr_lbrr_bits) / 2;
        }
        let n_bits_per_frame = (packet_bits_total - ps_enc.n_bits_used_lbrr) / n_frames_per_packet;
        let mut frame_rate_bps = if packet_size_ms == 10 {
            n_bits_per_frame * 100
        } else {
            n_bits_per_frame * 50
        };
        frame_rate_bps -= ps_enc.n_bits_exceeded * 1000 / BITRESERVOIR_DECAY_TIME_MS;
        if frame_idx > 0 {
            let bits_balance = rc.tell() - ps_enc.n_bits_used_lbrr - n_bits_per_frame * frame_idx;
            frame_rate_bps -= bits_balance * 1000 / BITRESERVOIR_DECAY_TIME_MS;
        }
        // libopus writes this as silk_LIMIT(rate, bitRate, 5000), whose macro
        // swaps its bounds when they arrive in the wrong order; the effect is a
        // clamp to whatever range the requested rate and 5 kb/s span.
        let frame_rate_bps = if target_rate_bps > 5000 {
            frame_rate_bps.clamp(5000, target_rate_bps)
        } else {
            frame_rate_bps.clamp(target_rate_bps, 5000)
        };

        let mut ms_rates_bps = [0i32; 2];
        if n_channels == 2 {
            let (buf0, buf1) = input_buf.split_at_mut(1);
            let mut ix = [[0i8; 3]; 2];
            let mut mid_only_flag = 0i8;
            silk_stereo_lr_to_ms(
                &mut ps_enc.stereo,
                &mut buf0[0][..frame_length + 2],
                &mut buf1[0][..frame_length + 2],
                &mut ix,
                &mut mid_only_flag,
                &mut ms_rates_bps,
                frame_rate_bps,
                // The previous frame's speech activity: this frame's has not
                // been measured yet, and libopus reads it in the same order.
                ps_enc.state[0].s_cmn.speech_activity_q8,
                false,
                fs_khz,
                frame_length,
            );
            ps_enc.stereo.pred_ix[fi] = ix;
            ps_enc.stereo.mid_only_flags[fi] = mid_only_flag;

            if mid_only_flag == 0 {
                if ps_enc.prev_decode_only_middle == 1 {
                    reset_side_encoder(&mut ps_enc.state[1]);
                }
                let vad_frame = input_buf[1][1..1 + frame_length].to_vec();
                silk_encode_do_vad(&mut ps_enc.state[1], &vad_frame, activity);
            } else {
                ps_enc.state[1].s_cmn.vad_flags[fi] = 0;
            }

            silk_stereo_encode_pred(rc, &ps_enc.stereo.pred_ix[fi]);
            if ps_enc.state[1].s_cmn.vad_flags[fi] == 0 {
                silk_stereo_encode_mid_only(rc, mid_only_flag);
            }
        } else {
            // Mono keeps the same two-sample history by hand; in stereo the
            // mid/side conversion above has already done it.
            input_buf[0][0] = ps_enc.stereo.s_mid[0];
            input_buf[0][1] = ps_enc.stereo.s_mid[1];
            ps_enc.stereo.s_mid[0] = input_buf[0][frame_length];
            ps_enc.stereo.s_mid[1] = input_buf[0][frame_length + 1];
        }

        let vad_frame = input_buf[0][1..1 + frame_length].to_vec();
        silk_encode_do_vad(&mut ps_enc.state[0], &vad_frame, activity);

        for ch in 0..n_channels {
            let mut ch_max_bits = max_bits;
            if tot_blocks == 2 && frame_idx == 0 {
                ch_max_bits = ch_max_bits * 3 / 5;
            } else if tot_blocks == 3 {
                if frame_idx == 0 {
                    ch_max_bits = ch_max_bits * 2 / 5;
                } else if frame_idx == 1 {
                    ch_max_bits = ch_max_bits * 3 / 4;
                }
            }
            let mut ch_use_cbr = use_cbr != 0 && frame_idx == n_frames_per_packet - 1;

            let channel_rate_bps = if n_channels == 1 {
                frame_rate_bps
            } else {
                let rate = ms_rates_bps[ch];
                if ch == 0 && ms_rates_bps[1] > 0 {
                    // The side channel still has to fit, so the mid channel
                    // cannot be allowed to spend the whole frame.
                    ch_use_cbr = false;
                    ch_max_bits -= max_bits / (tot_blocks * 2);
                }
                rate
            };
            if channel_rate_bps <= 0 {
                ps_enc.state[ch].s_cmn.n_frames_encoded += 1;
                continue;
            }

            silk_control_snr(&mut ps_enc.state[ch].s_cmn, channel_rate_bps);

            let cond_coding = if frame_idx == 0 {
                CODE_INDEPENDENTLY
            } else if ch > 0 && ps_enc.prev_decode_only_middle != 0 {
                // The side channel was skipped last frame, so there is no stale
                // LTP history to rescale — but also no history to predict from.
                CODE_INDEPENDENTLY_NO_LTP_SCALING
            } else {
                CODE_CONDITIONALLY
            };

            let frame_samples = input_buf[ch][1..1 + frame_length].to_vec();

            let mut frame_bytes = 0i32;
            let ret = silk_encode_frame(
                &mut ps_enc.state[ch],
                &frame_samples,
                rc,
                &mut frame_bytes,
                cond_coding,
                ch_max_bits,
                if ch_use_cbr { 1 } else { 0 },
            );
            if ret != 0 {
                return ret;
            }

            ps_enc.state[ch].s_cmn.n_frames_encoded += 1;
        }

        ps_enc.prev_decode_only_middle = ps_enc.stereo.mid_only_flags[fi] as i32;
        sample_offset += frame_length;
    }

    // Patch the reserved flags: each channel's per-frame VAD bits followed by
    // its FEC bit, most significant channel first.
    let n_flag_bits = ((n_frames_per_packet + 1) * n_channels as i32) as u32;
    let mut flags = 0u32;
    for ch in 0..n_channels {
        for i in 0..n_frames_per_packet as usize {
            flags <<= 1;
            flags |= ps_enc.state[ch].s_cmn.vad_flags[i] as u32;
        }
        flags <<= 1;
        flags |= ps_enc.state[ch].s_cmn.lbrr_flag as u32;
    }
    rc.patch_initial_bits(flags, n_flag_bits);

    *n_bytes_out = (rc.tell() + 7) >> 3;

    if *n_bytes_out > 0 {
        ps_enc.n_bits_exceeded += *n_bytes_out * 8 - packet_bits_total;
        ps_enc.n_bits_exceeded = ps_enc.n_bits_exceeded.clamp(0, 10000);
    }

    0
}

/// Clear the side channel's predictive state (libopus `silk_Encode`).
///
/// The side channel goes quiet whenever the stereo image collapses, and it can
/// stay quiet for many frames. Whatever pitch lag, gain and shaping history it
/// held then describes a signal the decoder no longer has, so predicting from it
/// on the first frame back produces a burst rather than a stereo image.
fn reset_side_encoder(st: &mut SilkEncoderState) {
    st.s_shape = SilkShapeState::default();
    st.s_nsq = SilkNSQState::default();
    st.s_cmn.prev_nlsf_q15 = [0; MAX_LPC_ORDER];
    st.s_cmn.prev_lag = 100;
    st.s_nsq.lag_prev = 100;
    st.s_shape.last_gain_index = 10;
    st.s_cmn.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
    st.s_nsq.prev_gain_q16 = 65536;
    st.s_cmn.first_frame_after_reset = 1;
}

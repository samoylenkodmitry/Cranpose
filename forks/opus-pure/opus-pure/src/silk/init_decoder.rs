use crate::silk::decoder_structs::SilkDecoderState;
use crate::silk::define::*;
use crate::silk::tables_nlsf::*;

pub fn silk_decoder_set_fs(dec: &mut SilkDecoderState, fs_khz: i32, fs_api_hz: i32) -> i32 {
    if fs_khz != 8 && fs_khz != 12 && fs_khz != 16 {
        return -1;
    }

    let new_subfr_length = (SUB_FRAME_LENGTH_MS as i32) * fs_khz;
    // libopus keys frame_length AND the pitch-contour table off the CURRENT
    // nb_subfr (set by the caller before this call): 10 ms frames use nb_subfr=2
    // with the 10 ms contour CDF, 20 ms use nb_subfr=4. Overwriting nb_subfr to
    // MAX here forced the 20 ms contour on every 10 ms frame -> wrong bit count
    // -> range-coder desync on the first 10 ms packet.
    let new_frame_length = new_subfr_length * dec.nb_subfr;
    // libopus resets the decoder's sample-history state ONLY when the internal
    // sampling rate (bandwidth) changes — see decoder_set_fs.c, where the outBuf/
    // sLPC_Q14_buf memset lives inside `if( psDec->fs_kHz != fs_kHz )`, NOT the
    // outer `|| frame_length != psDec->frame_length` block. A pure frame-length
    // switch (e.g. 20 ms -> 10 ms at the same NB/MB/WB rate) must PRESERVE the
    // LPC/output history; zeroing it there makes the first 10 ms frame synthesize
    // from silence (~1/4 magnitude, wrong sign) even though the range coder stays
    // perfectly in sync. Only pitch-contour/frame_length update on a length change.
    let fs_changed = dec.fs_khz != fs_khz;

    dec.fs_khz = fs_khz;
    dec.fs_api_hz = fs_api_hz;

    dec.subfr_length = new_subfr_length;
    dec.frame_length = new_frame_length;
    dec.ltp_mem_length = (LTP_MEM_LENGTH_MS as i32) * fs_khz;
    // NB (8k) and MB (12k) use a 10th-order LPC (the NB_MB NLSF codebook);
    // only WB (16k) is 16th-order. Previously MB got order 16, so it read
    // order-16 NLSF from the order-10 codebook -> garbage LPC on every MB frame.
    dec.lpc_order = if fs_khz == 8 || fs_khz == 12 {
        MIN_LPC_ORDER as i32
    } else {
        MAX_LPC_ORDER as i32
    };

    if fs_khz == 8 {
        if dec.nb_subfr == MAX_NB_SUBFR as i32 {
            dec.pitch_contour_icdf = &crate::silk::tables::SILK_PITCH_CONTOUR_NB_ICDF;
        } else {
            dec.pitch_contour_icdf = &crate::silk::tables::SILK_PITCH_CONTOUR_10_MS_NB_ICDF;
        }
    } else if dec.nb_subfr == MAX_NB_SUBFR as i32 {
        dec.pitch_contour_icdf = &crate::silk::tables::SILK_PITCH_CONTOUR_ICDF;
    } else {
        dec.pitch_contour_icdf = &crate::silk::tables::SILK_PITCH_CONTOUR_10_MS_ICDF;
    }

    dec.pitch_lag_low_bits_icdf = match fs_khz {
        8 => &crate::silk::tables::SILK_UNIFORM4_ICDF,
        12 => &crate::silk::tables::SILK_UNIFORM6_ICDF,
        16 => &crate::silk::tables::SILK_UNIFORM8_ICDF,
        _ => &crate::silk::tables::SILK_UNIFORM8_ICDF,
    };

    dec.ps_nlsf_cb = match fs_khz {
        8 => Some(&SILK_NLSF_CB_NB_MB),
        12 => Some(&SILK_NLSF_CB_NB_MB),
        _ => Some(&SILK_NLSF_CB_WB),
    };

    if fs_changed {
        dec.first_frame_after_reset = 1;
        dec.lag_prev = 100;
        dec.last_gain_index = 10;
        dec.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
        dec.out_buf.fill(0);
        dec.s_lpc_q14_buf.fill(0);
    }

    0
}

pub fn silk_reset_decoder(ps_dec: &mut SilkDecoderState) -> i32 {
    ps_dec.prev_gain_q16 = 1 << 16;
    ps_dec.exc_q14.fill(0);
    ps_dec.s_lpc_q14_buf.fill(0);
    ps_dec.out_buf.fill(0);
    ps_dec.lag_prev = 100;
    ps_dec.last_gain_index = 10;
    ps_dec.first_frame_after_reset = 1;
    ps_dec.ec_prev_signal_type = 0;
    ps_dec.ec_prev_lag_index = 0;
    ps_dec.vad_flags.fill(0);
    ps_dec.lbrr_flag = 0;
    ps_dec.lbrr_flags.fill(0);
    ps_dec.prev_nlsf_q15.fill(0);
    ps_dec.loss_cnt = 0;
    ps_dec.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
    ps_dec.indices = Default::default();
    // silk_decoder_set_fs relies on nb_subfr already holding a valid value (the
    // caller sets it per-packet); seed it so the very first set_fs computes the
    // right frame_length / pitch-contour table.
    ps_dec.nb_subfr = MAX_NB_SUBFR as i32;

    0
}

pub fn silk_init_decoder(ps_dec: &mut SilkDecoderState) -> i32 {
    silk_reset_decoder(ps_dec);

    ps_dec.s_cng = Default::default();
    crate::silk::cng::silk_cng_reset(ps_dec);

    ps_dec.s_plc = Default::default();

    0
}

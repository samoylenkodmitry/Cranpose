use crate::silk::define::*;

/// Encoder-side adaptive mid/side state (libopus `stereo_enc_state`).
///
/// All-zero is the correct reset state, matching libopus's `silk_memset` in
/// `silk_InitEncoder`; [`SilkStereoState::reset_for_stereo`] applies the
/// different initial values libopus uses when a stream turns stereo.
#[derive(Clone, Default)]
pub struct SilkStereoState {
    /// Last frame's quantized prediction weights, Q13, for interpolation.
    pub pred_prev_q13: [i16; 2],
    /// Two-sample tail of the previous frame's mid signal.
    pub s_mid: [i16; 2],
    /// Two-sample tail of the previous frame's side signal.
    pub s_side: [i16; 2],
    /// Smoothed `[mid_norm, residual_norm]` for the low band then the high band.
    pub mid_side_amp_q0: [i32; 4],
    /// Smoothed stereo width, Q14.
    pub smth_width_q14: i16,
    /// Last frame's applied width, Q14.
    pub width_prev_q14: i16,
    /// How long the side channel has been silent, in samples, so a taper to
    /// zero width is always transmitted before the side channel is dropped.
    pub silent_side_len: i16,
    /// Per-frame quantization indices, held until the packet is written.
    pub pred_ix: [[[i8; 3]; 2]; MAX_FRAMES_PER_PACKET],
    /// Per-frame "no side channel coded" flags.
    pub mid_only_flags: [i8; MAX_FRAMES_PER_PACKET],
}

impl SilkStereoState {
    /// Initial state for a mono -> stereo transition (libopus `silk_Encode`).
    /// The norms start at 0/1 rather than 0/0 so the first ratio is a division
    /// by one rather than by zero, and the width starts wide so a genuinely
    /// stereo first frame is not tapered down before it is heard.
    pub fn reset_for_stereo(&mut self) {
        self.pred_prev_q13 = [0; 2];
        self.s_side = [0; 2];
        self.mid_side_amp_q0 = [0, 1, 0, 1];
        self.width_prev_q14 = 0;
        self.smth_width_q14 = 1 << 14;
    }
}

#[derive(Clone, Copy)]
pub struct NLSFCodebook {
    pub n_vectors: i16,
    pub order: i16,
    pub quant_step_size_q16: i32,
    pub inv_quant_step_size_q6: i16,
    pub cb1_nlsf_q8: &'static [u8],
    pub cb1_wght_q9: &'static [i16],
    pub cb1_icdf: &'static [u8],
    pub pred_q8: &'static [u8],
    pub ec_sel: &'static [u8],
    pub ec_icdf: &'static [u8],
    pub ec_rates_q5: &'static [u8],
    pub delta_min_q15: &'static [i16],
}

#[derive(Clone, Copy)]
pub struct SideInfoIndices {
    pub gains_indices: [i8; MAX_NB_SUBFR],
    pub ltp_index: [i8; MAX_NB_SUBFR],
    pub nlsf_indices: [i8; MAX_LPC_ORDER + 1],
    pub lag_index: i16,
    pub contour_index: i8,
    pub signal_type: i8,
    pub quant_offset_type: i8,
    pub nlsf_interp_coef_q2: i8,
    pub per_index: i8,
    pub ltp_scale_index: i8,
    pub seed: i8,
}

impl Default for SideInfoIndices {
    fn default() -> Self {
        Self {
            gains_indices: [0; MAX_NB_SUBFR],
            ltp_index: [0; MAX_NB_SUBFR],
            nlsf_indices: [0; MAX_LPC_ORDER + 1],
            lag_index: 0,
            contour_index: 0,
            signal_type: 0,
            quant_offset_type: 0,
            nlsf_interp_coef_q2: 4,
            per_index: 0,
            ltp_scale_index: 0,
            seed: 0,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct SilkShapeState {
    pub last_gain_index: i8,
    pub harm_shape_gain_smth_q16: i32,
    pub tilt_smth_q16: i32,
}

#[derive(Clone, Copy)]
pub struct SilkVADState {
    pub ana_state: [i32; 2],
    pub ana_state1: [i32; 2],
    pub ana_state2: [i32; 2],
    pub xnrg_subfr: [i32; VAD_N_BANDS],
    pub nrg_ratio_smth_q8: [i32; VAD_N_BANDS],
    pub hp_state: i16,
    pub nl: [i32; VAD_N_BANDS],
    pub inv_nl: [i32; VAD_N_BANDS],
    pub noise_level_bias: [i32; VAD_N_BANDS],
    pub counter: i32,
}

impl Default for SilkVADState {
    fn default() -> Self {
        Self {
            ana_state: [0; 2],
            ana_state1: [0; 2],
            ana_state2: [0; 2],
            xnrg_subfr: [0; VAD_N_BANDS],
            nrg_ratio_smth_q8: [0; VAD_N_BANDS],
            hp_state: 0,
            nl: [0; VAD_N_BANDS],
            inv_nl: [0; VAD_N_BANDS],
            noise_level_bias: [0; VAD_N_BANDS],
            counter: 0,
        }
    }
}

pub struct SilkEncoderStateCommon {
    pub indices: SideInfoIndices,
    pub snr_db_q7: i32,
    pub input_quality_bands_q15: [i32; VAD_N_BANDS],
    pub speech_activity_q8: i32,
    pub use_cbr: i32,
    pub fs_khz: i32,
    pub nb_subfr: i32,
    pub warping_q16: i32,
    pub la_shape: i32,
    pub shape_win_length: i32,
    pub shaping_lpc_order: i32,
    pub predict_lpc_order: i32,
    pub subfr_length: i32,
    pub la_pitch: i32,
    pub frame_length: i32,
    pub ltp_mem_length: i32,
    pub pitch_lpc_win_length: i32,
    pub pitch_estimation_complexity: i32,
    pub pitch_estimation_threshold_q16: i32,
    pub first_frame_after_reset: i32,
    pub prev_signal_type: i32,
    pub input_tilt_q15: i32,
    pub n_states_delayed_decision: i32,
    pub prev_lag: i32,
    pub x_buf: [i16; 2 * MAX_FRAME_LENGTH + LA_SHAPE_MAX],
    pub prev_nlsf_q15: [i16; MAX_LPC_ORDER],
    pub n_nlsf_survivors: i32,
    pub variable_hp_smth1_q15: i32,
    pub variable_hp_smth2_q15: i32,
    pub s_vad: SilkVADState,
    pub lbrr_enabled: i32,
    pub indices_lbrr: [SideInfoIndices; MAX_FRAMES_PER_PACKET],
    pub pulses_lbrr: [[i8; MAX_FRAME_LENGTH]; MAX_FRAMES_PER_PACKET],
    pub n_frames_encoded: i32,
    pub n_frames_per_packet: i32,
    pub packet_size_ms: i32,
    pub complexity: i32,
    pub sum_log_gain_q7: i32,
    pub packet_loss_perc: i32,
    pub lbrr_flag: i8,
    pub no_speech_counter: i32,
    pub vad_flags: [i32; MAX_FRAMES_PER_PACKET],

    pub frame_counter: i32,
    pub ec_prev_signal_type: i32,
    pub ec_prev_lag_index: i16,
    pub use_interpolated_nlsfs: i32,
    pub lbrr_gain_increases: i32,
    pub lbrr_flags: [i32; MAX_FRAMES_PER_PACKET],
    /// Gain-index continuity for the LBRR frames alone. LBRR raises the first
    /// subframe's gain, so its indices dequantize along a different chain than
    /// the primary frame's and cannot share `s_shape.last_gain_index`.
    pub lbrr_prev_last_gain_index: i8,
}

impl Default for SilkEncoderStateCommon {
    fn default() -> Self {
        Self {
            indices: SideInfoIndices::default(),
            snr_db_q7: 0,
            input_quality_bands_q15: [0; VAD_N_BANDS],
            speech_activity_q8: 0,
            use_cbr: 0,
            fs_khz: 0,
            nb_subfr: 0,
            warping_q16: 0,
            la_shape: 0,
            shape_win_length: 0,
            shaping_lpc_order: 0,
            predict_lpc_order: 0,
            subfr_length: 0,
            la_pitch: 0,
            frame_length: 0,
            ltp_mem_length: 0,
            pitch_lpc_win_length: 0,
            pitch_estimation_complexity: 0,
            pitch_estimation_threshold_q16: 0,
            first_frame_after_reset: 0,
            prev_signal_type: 0,
            input_tilt_q15: 0,
            n_states_delayed_decision: 0,
            prev_lag: 0,
            x_buf: [0; 2 * MAX_FRAME_LENGTH + LA_SHAPE_MAX],
            prev_nlsf_q15: [0; MAX_LPC_ORDER],
            n_nlsf_survivors: 0,
            variable_hp_smth1_q15: 0,
            variable_hp_smth2_q15: 0,
            s_vad: SilkVADState::default(),
            lbrr_enabled: 0,
            indices_lbrr: [SideInfoIndices::default(); MAX_FRAMES_PER_PACKET],
            pulses_lbrr: [[0; MAX_FRAME_LENGTH]; MAX_FRAMES_PER_PACKET],
            n_frames_encoded: 0,
            n_frames_per_packet: 0,
            packet_size_ms: 0,
            complexity: 0,
            sum_log_gain_q7: 0,
            packet_loss_perc: 0,
            lbrr_flag: 0,
            no_speech_counter: 0,
            vad_flags: [0; MAX_FRAMES_PER_PACKET],
            frame_counter: 0,
            ec_prev_signal_type: 0,
            ec_prev_lag_index: 0,
            use_interpolated_nlsfs: 0,
            lbrr_gain_increases: 0,
            lbrr_flags: [0; MAX_FRAMES_PER_PACKET],
            lbrr_prev_last_gain_index: 0,
        }
    }
}

pub struct SilkEncoderState {
    pub s_cmn: SilkEncoderStateCommon,
    pub s_shape: SilkShapeState,
    pub pulses: [i8; MAX_FRAME_LENGTH],
    pub s_nsq: SilkNSQState,
    pub ltp_corr_q15: i32,
    pub pitch_estimation_lpc_order: i32,
    pub ps_nlsf_cb: Option<&'static NLSFCodebook>,
}

/// The SILK encoder as a whole (libopus `silk_encoder`): one coder state per
/// internal channel plus the cross-channel mid/side state.
///
/// `state[1]` codes the *side* channel, and only when there is one: at low
/// rates, or on material close to amplitude-panned mono, SILK sends prediction
/// weights alone and the second state sits idle.
pub struct SilkEncoder {
    pub state: [SilkEncoderState; 2],
    pub stereo: SilkStereoState,
    /// Channels SILK is coding internally, 1 or 2.
    pub n_channels_internal: i32,
    /// Whether the last frame coded dropped its side channel. Decides both the
    /// side encoder's reset and the next frame's conditional coding.
    pub prev_decode_only_middle: i32,
    /// Bits spent above the requested rate on previous packets. The next
    /// packet's target is pulled down until this drains, which is what holds a
    /// VBR stream to its average rate rather than to its per-frame one.
    pub n_bits_exceeded: i32,
    /// A running estimate of what in-band FEC costs per packet, taken off the
    /// target so the redundant copy does not push the primary stream over.
    pub n_bits_used_lbrr: i32,
    /// The packet layout in force when the pending LBRR frames were stored.
    /// Redundancy travels in the *next* packet, so a change of packet duration
    /// or channel count in between would have it coded against a layout that no
    /// longer describes the audio it holds; the flags are dropped instead
    /// (libopus enc_API.c `transition`).
    pub lbrr_packet_size_ms: i32,
    pub lbrr_n_channels: i32,
}

impl Default for SilkEncoder {
    fn default() -> Self {
        Self {
            state: [SilkEncoderState::default(), SilkEncoderState::default()],
            stereo: SilkStereoState::default(),
            n_channels_internal: 1,
            prev_decode_only_middle: 0,
            n_bits_exceeded: 0,
            n_bits_used_lbrr: 0,
            lbrr_packet_size_ms: 0,
            lbrr_n_channels: 0,
        }
    }
}

impl Default for SilkEncoderState {
    fn default() -> Self {
        Self {
            s_cmn: SilkEncoderStateCommon::default(),
            s_shape: SilkShapeState::default(),
            pulses: [0; MAX_FRAME_LENGTH],
            s_nsq: SilkNSQState::default(),
            ltp_corr_q15: 0,
            pitch_estimation_lpc_order: 0,
            ps_nlsf_cb: None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct SilkNSQState {
    pub xq: [i16; 2 * MAX_FRAME_LENGTH],
    pub s_ltp_shp_q14: [i32; 2 * MAX_FRAME_LENGTH],
    pub s_lpc_q14: [i32; MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH],
    pub s_ar2_q14: [i32; MAX_SHAPE_LPC_ORDER],
    pub s_lf_ar_q14: i32,
    pub s_diff_shp_q14: i32,
    pub lag_prev: i32,
    pub s_ltp_buf_idx: i32,
    pub s_ltp_shp_buf_idx: i32,
    pub rand_seed: i32,
    pub prev_gain_q16: i32,
    pub rewhite_flag: i32,
}

impl Default for SilkNSQState {
    fn default() -> Self {
        Self {
            xq: [0; 2 * MAX_FRAME_LENGTH],
            s_ltp_shp_q14: [0; 2 * MAX_FRAME_LENGTH],
            s_lpc_q14: [0; MAX_SUB_FRAME_LENGTH + NSQ_LPC_BUF_LENGTH],
            s_ar2_q14: [0; MAX_SHAPE_LPC_ORDER],
            s_lf_ar_q14: 0,
            s_diff_shp_q14: 0,
            lag_prev: 0,
            s_ltp_buf_idx: 0,
            s_ltp_shp_buf_idx: 0,
            rand_seed: 0,
            prev_gain_q16: 0,
            rewhite_flag: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct SilkEncoderControl {
    pub input_quality_q14: i32,
    pub coding_quality_q14: i32,
    pub pitch_l: [i32; MAX_NB_SUBFR],
    pub gains_q16: [i32; MAX_NB_SUBFR],
    pub gains_unq_q16: [i32; MAX_NB_SUBFR],
    pub pred_coef_q12: [[i16; MAX_LPC_ORDER]; 2],
    pub ltp_coef_q14: [i16; MAX_NB_SUBFR * LTP_ORDER],
    pub ltp_scale_q14: i32,
    pub ar_q13: [i16; MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER],
    pub lf_shp_q14: [i32; MAX_NB_SUBFR],
    pub tilt_q14: [i32; MAX_NB_SUBFR],
    pub harm_shape_gain_q14: [i32; MAX_NB_SUBFR],
    pub lambda_q10: i32,
    pub pred_gain_q16: i32,
    pub ltp_red_cod_gain_q7: i32,
    pub res_nrg: [i32; MAX_NB_SUBFR],
    pub res_nrg_q: [i32; MAX_NB_SUBFR],
    pub last_gain_index_prev: i8,
}

impl Default for SilkEncoderControl {
    fn default() -> Self {
        Self {
            input_quality_q14: 0,
            coding_quality_q14: 0,
            pitch_l: [0; MAX_NB_SUBFR],
            gains_q16: [0; MAX_NB_SUBFR],
            gains_unq_q16: [0; MAX_NB_SUBFR],
            pred_coef_q12: [[0; MAX_LPC_ORDER]; 2],
            ltp_coef_q14: [0; MAX_NB_SUBFR * LTP_ORDER],
            ltp_scale_q14: 0,
            ar_q13: [0; MAX_NB_SUBFR * MAX_SHAPE_LPC_ORDER],
            lf_shp_q14: [0; MAX_NB_SUBFR],
            tilt_q14: [0; MAX_NB_SUBFR],
            harm_shape_gain_q14: [0; MAX_NB_SUBFR],
            lambda_q10: 0,
            pred_gain_q16: 0,
            ltp_red_cod_gain_q7: 0,
            res_nrg: [0; MAX_NB_SUBFR],
            res_nrg_q: [0; MAX_NB_SUBFR],
            last_gain_index_prev: 0,
        }
    }
}

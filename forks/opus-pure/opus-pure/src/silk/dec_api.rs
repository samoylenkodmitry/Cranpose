use crate::range_coder::RangeCoder;
use crate::silk::decode_frame::{
    FLAG_DECODE_LBRR, FLAG_DECODE_NORMAL, FLAG_PACKET_LOST, silk_decode_frame,
};
use crate::silk::decode_indices::{
    silk_decode_indices, silk_stereo_decode_mid_only, silk_stereo_decode_pred, silk_stereo_ms_to_lr,
};
use crate::silk::decode_pulses::silk_decode_pulses;
use crate::silk::decoder_structs::SilkDecoderState;
use crate::silk::define::*;
use crate::silk::init_decoder::{silk_decoder_set_fs, silk_init_decoder};
use crate::silk::tables::{SILK_LBRR_FLAGS_2_ICDF, SILK_LBRR_FLAGS_3_ICDF};

pub struct SilkDecoder {
    pub channel_state: [SilkDecoderState; 2],

    pub n_channels_api: i32,

    pub n_channels_internal: i32,

    pub prev_decode_only_middle: i32,

    // Stereo MS->LR reconstruction state (libopus stereo_dec_state).
    pub s_stereo_pred_prev_q13: [i32; 2],
    pub s_stereo_mid: [i16; 2],
    pub s_stereo_side: [i16; 2],

    // When set, a stereo decode reconstructs L/R via silk_stereo_ms_to_lr and
    // publishes them (1-sample-delay-line layout, ready to resample) in l_out/
    // r_out. When clear, stereo decodes emit only the mid (mono downmix) as
    // before. lib.rs sets this for the pure-SILK stereo path.
    pub produce_lr: bool,
    pub l_out: [i16; MAX_FRAME_LENGTH],
    pub r_out: [i16; MAX_FRAME_LENGTH],
}

impl Default for SilkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SilkDecoder {
    pub fn new() -> Self {
        let mut dec = Self {
            channel_state: [SilkDecoderState::default(), SilkDecoderState::default()],
            n_channels_api: 1,
            n_channels_internal: 1,
            prev_decode_only_middle: 0,
            s_stereo_pred_prev_q13: [0; 2],
            s_stereo_mid: [0; 2],
            s_stereo_side: [0; 2],
            produce_lr: false,
            l_out: [0; MAX_FRAME_LENGTH],
            r_out: [0; MAX_FRAME_LENGTH],
        };
        silk_init_decoder(&mut dec.channel_state[0]);
        silk_init_decoder(&mut dec.channel_state[1]);
        dec
    }

    pub fn init(&mut self, sample_rate_hz: i32, channels: i32) -> i32 {
        let fs_khz = sample_rate_hz / 1000;
        let ret = silk_decoder_set_fs(&mut self.channel_state[0], fs_khz, sample_rate_hz);
        if ret < 0 {
            return ret;
        }
        if channels == 2 {
            let ret = silk_decoder_set_fs(&mut self.channel_state[1], fs_khz, sample_rate_hz);
            if ret < 0 {
                return ret;
            }
        }

        self.channel_state[0].n_frames_per_packet = 1;
        self.n_channels_api = channels;
        self.n_channels_internal = channels;
        ret
    }

    pub fn decode(
        &mut self,
        range_dec: &mut RangeCoder,
        output: &mut [i16],
        lost_flag: i32,
        new_packet: bool,
        payload_size_ms: i32,
        internal_sample_rate: i32,
    ) -> i32 {
        if new_packet {
            self.channel_state[0].n_frames_decoded = 0;
            self.channel_state[1].n_frames_decoded = 0;
        }

        if self.channel_state[0].n_frames_decoded == 0 {
            let fs_khz_dec = (internal_sample_rate >> 10) + 1;
            if fs_khz_dec != 8 && fs_khz_dec != 12 && fs_khz_dec != 16 {
                return -1;
            }
            let api_sample_rate = self.channel_state[0].fs_api_hz;
            // libopus configures EVERY internal channel here, not just ch0 — the
            // side channel needs its frame_length/fs set or a stereo side decode
            // consumes the wrong number of bits and desyncs the range coder.
            for n in 0..self.n_channels_internal as usize {
                match payload_size_ms {
                    0 | 10 => {
                        self.channel_state[n].n_frames_per_packet = 1;
                        self.channel_state[n].nb_subfr = 2;
                    }
                    20 => {
                        self.channel_state[n].n_frames_per_packet = 1;
                        self.channel_state[n].nb_subfr = MAX_NB_SUBFR as i32;
                    }
                    40 => {
                        self.channel_state[n].n_frames_per_packet = 2;
                        self.channel_state[n].nb_subfr = MAX_NB_SUBFR as i32;
                    }
                    60 => {
                        self.channel_state[n].n_frames_per_packet = 3;
                        self.channel_state[n].nb_subfr = MAX_NB_SUBFR as i32;
                    }
                    _ => return -1,
                }
                let ret =
                    silk_decoder_set_fs(&mut self.channel_state[n], fs_khz_dec, api_sample_rate);
                if ret < 0 {
                    return ret;
                }
                if payload_size_ms == 10 {
                    self.channel_state[n].nb_subfr = 2;
                    self.channel_state[n].frame_length = self.channel_state[n].subfr_length * 2;
                }
            }
        }

        if lost_flag != FLAG_PACKET_LOST && self.channel_state[0].n_frames_decoded == 0 {
            let n_frames_per_packet = self.channel_state[0].n_frames_per_packet.max(1);
            let n_channels = self.n_channels_internal as usize;

            for n in 0..n_channels {
                for i in 0..n_frames_per_packet as usize {
                    let vad = range_dec.decode_bit_logp(1);
                    self.channel_state[n].vad_flags[i] = if vad { 1 } else { 0 };
                }
                let lbrr = range_dec.decode_bit_logp(1);
                self.channel_state[n].lbrr_flag = if lbrr { 1 } else { 0 };
            }

            for n in 0..n_channels {
                self.channel_state[n].lbrr_flags.fill(0);
                if self.channel_state[n].lbrr_flag != 0 {
                    if n_frames_per_packet == 1 {
                        self.channel_state[n].lbrr_flags[0] = 1;
                    } else {
                        let lbrr_icdf = match n_frames_per_packet {
                            2 => &SILK_LBRR_FLAGS_2_ICDF[..],
                            3 => &SILK_LBRR_FLAGS_3_ICDF[..],
                            _ => &SILK_LBRR_FLAGS_2_ICDF[..],
                        };
                        let lbrr_symbol = range_dec.decode_icdf(lbrr_icdf, 8) + 1;
                        for i in 0..n_frames_per_packet as usize {
                            self.channel_state[n].lbrr_flags[i] = (lbrr_symbol >> i) & 1;
                        }
                    }
                }
            }

            // Skip LBRR data for all frames/channels
            if lost_flag == FLAG_DECODE_NORMAL {
                for i in 0..n_frames_per_packet as usize {
                    for n in 0..n_channels {
                        if self.channel_state[n].lbrr_flags[i] != 0 {
                            if n_channels == 2 && n == 0 {
                                let _ = silk_stereo_decode_pred(range_dec);
                                if self.channel_state[1].lbrr_flags[i] == 0 {
                                    silk_stereo_decode_mid_only(range_dec);
                                }
                            }
                            let cond_coding =
                                if i > 0 && self.channel_state[n].lbrr_flags[i - 1] != 0 {
                                    CODE_CONDITIONALLY
                                } else {
                                    CODE_INDEPENDENTLY
                                };
                            silk_decode_indices(
                                &mut self.channel_state[n],
                                range_dec,
                                i as i32,
                                1,
                                cond_coding,
                            );
                            let mut pulses = [0i16; MAX_FRAME_LENGTH];
                            silk_decode_pulses(
                                range_dec,
                                &mut pulses,
                                self.channel_state[n].indices.signal_type as i32,
                                self.channel_state[n].indices.quant_offset_type as i32,
                                self.channel_state[n].frame_length,
                            );
                        }
                    }
                }
            }
        }

        let frame_index = self.channel_state[0].n_frames_decoded as usize;
        let mut decode_only_middle = 0i32;
        let mut ms_pred_q13 = [0i32; 2];
        if self.n_channels_internal == 2 {
            // A redundant frame carries its own stereo side info, so the
            // predictors are read for FEC exactly as for a normal frame; the
            // bits must be consumed either way to keep the range coder in sync.
            let coded = lost_flag == FLAG_DECODE_NORMAL
                || (lost_flag == FLAG_DECODE_LBRR
                    && self.channel_state[0].lbrr_flags[frame_index] == 1);
            if coded {
                ms_pred_q13 = silk_stereo_decode_pred(range_dec);
                // The mid-only flag is redundant when the side channel has
                // content of its own: VAD says so on a normal frame, the side's
                // own LBRR flag on a redundant one.
                let side_silent = if lost_flag == FLAG_DECODE_NORMAL {
                    self.channel_state[1].vad_flags[frame_index] == 0
                } else {
                    self.channel_state[1].lbrr_flags[frame_index] == 0
                };
                if side_silent {
                    decode_only_middle = if silk_stereo_decode_mid_only(range_dec) {
                        1
                    } else {
                        0
                    };
                }
            } else {
                // Nothing to read: hold the last frame's predictors so a
                // concealed frame keeps the stereo image it had rather than
                // collapsing it towards the centre.
                ms_pred_q13 = self.s_stereo_pred_prev_q13;
            }
        }

        // Reset the side channel's prediction memory for the first frame that
        // codes side after a run of mid-only frames (libopus dec_API.c:249-256).
        if self.n_channels_internal == 2
            && decode_only_middle == 0
            && self.prev_decode_only_middle == 1
        {
            let ch1 = &mut self.channel_state[1];
            ch1.out_buf.fill(0);
            ch1.s_lpc_q14_buf.fill(0);
            ch1.lag_prev = 100;
            ch1.last_gain_index = 10;
            ch1.prev_signal_type = TYPE_NO_VOICE_ACTIVITY;
            ch1.first_frame_after_reset = 1;
        }

        // Whether this frame carries a side channel at all. On a concealed or
        // FEC frame there is no mid-only flag to read, so libopus falls back to
        // what the last real frame did: a side channel that was being coded
        // keeps being concealed, and one that was absent stays absent.
        let has_side = if lost_flag == FLAG_DECODE_NORMAL {
            decode_only_middle == 0
        } else {
            self.prev_decode_only_middle == 0
                || (lost_flag == FLAG_DECODE_LBRR
                    && self.channel_state[1].lbrr_flags
                        [self.channel_state[1].n_frames_decoded as usize]
                        == 1)
        };

        let cond_coding = if frame_index == 0 {
            CODE_INDEPENDENTLY
        } else if lost_flag == FLAG_DECODE_LBRR {
            if self.channel_state[0].lbrr_flags[frame_index - 1] == 1 {
                CODE_CONDITIONALLY
            } else {
                CODE_INDEPENDENTLY
            }
        } else {
            CODE_CONDITIONALLY
        };

        let mut n_samples_out: i32 = 0;
        let ret = silk_decode_frame(
            &mut self.channel_state[0],
            range_dec,
            output,
            &mut n_samples_out,
            lost_flag,
            cond_coding,
        );
        self.channel_state[0].n_frames_decoded += 1;

        let mut side_samples = [0i16; MAX_FRAME_LENGTH];
        let mut side_decoded = false;

        // The side channel. Its bits MUST be consumed even when the output is a
        // mono downmix, or the range coder desyncs for the next internal frame of
        // a multi-frame packet and the mid of frames 2/3 decodes from garbage.
        // On a concealed frame there are no bits, but the side still has to be
        // extrapolated: skipping it leaves the stereo image collapsing to mono
        // for the length of the loss.
        if self.n_channels_internal == 2 && has_side {
            // libopus FrameIndex for the side (n=1) = channel_state[0].nFramesDecoded - 1,
            // evaluated AFTER ch0's own increment, which equals the original
            // frame_index. (Using frame_index-1 wrongly forced INDEP on the 2nd
            // internal frame -> wrong bit count -> desync.)
            let cond1 = if frame_index == 0 {
                CODE_INDEPENDENTLY
            } else if lost_flag == FLAG_DECODE_LBRR {
                if self.channel_state[1].lbrr_flags[frame_index - 1] == 1 {
                    CODE_CONDITIONALLY
                } else {
                    CODE_INDEPENDENTLY
                }
            } else if self.prev_decode_only_middle == 1 {
                CODE_INDEPENDENTLY_NO_LTP_SCALING
            } else {
                CODE_CONDITIONALLY
            };
            let mut ns_side: i32 = 0;
            let ret1 = silk_decode_frame(
                &mut self.channel_state[1],
                range_dec,
                &mut side_samples,
                &mut ns_side,
                lost_flag,
                cond1,
            );
            if ret1 < 0 {
                return ret1;
            }
            side_decoded = true;
        }

        // Reconstruct L/R for the pure-SILK stereo path. mid is in `output`
        // ([0..n]); side in side_samples ([0..n], zero if mid-only). ms_to_lr
        // wants x1/x2 laid out as [hist0, hist1, samples...] and writes L to
        // x1[1..1+n], R to x2[1..1+n] — exactly the 1-sample delay line the
        // resampler consumes.
        if self.produce_lr && self.n_channels_internal == 2 && n_samples_out > 0 {
            let n = n_samples_out as usize;
            let mut mid_buf = [0i16; MAX_FRAME_LENGTH + 2];
            let mut side_buf = [0i16; MAX_FRAME_LENGTH + 2];
            mid_buf[2..2 + n].copy_from_slice(&output[..n]);
            if side_decoded {
                side_buf[2..2 + n].copy_from_slice(&side_samples[..n]);
            }
            silk_stereo_ms_to_lr(
                &mut self.s_stereo_pred_prev_q13,
                &mut self.s_stereo_mid,
                &mut self.s_stereo_side,
                &mut mid_buf,
                &mut side_buf,
                &ms_pred_q13,
                self.channel_state[0].fs_khz,
                n,
            );
            self.l_out[..n].copy_from_slice(&mid_buf[1..1 + n]);
            self.r_out[..n].copy_from_slice(&side_buf[1..1 + n]);
        } else if n_samples_out >= 2 {
            // Mono frame: buffer the last two mid samples so the NEXT stereo
            // frame's ms_to_lr starts from the correct 2-sample history (libopus
            // dec_API.c:312-313 — sStereo.sMid is updated even on mono frames).
            // Without this the first stereo frame after a mono run reconstructs
            // from stale history (~1/4-magnitude glitch at the switch).
            let n = n_samples_out as usize;
            self.s_stereo_mid[0] = output[n - 2];
            self.s_stereo_mid[1] = output[n - 1];
        }

        // libopus increments channel_state[1].nFramesDecoded on EVERY internal
        // frame (even mid-only). The side's silk_decode_frame keys its VAD/
        // signal-type lookup off this counter; leaving it at 0 made every side
        // frame read vad_flags[0] -> wrong signal type on frame 2+ -> desync.
        if self.n_channels_internal == 2 {
            self.channel_state[1].n_frames_decoded += 1;
        }

        // After a concealed packet libopus re-anchors the gain dequantizer
        // (dec_API.c: `LastGainIndex = 10`) and leaves `prev_decode_only_middle`
        // alone. The first is not cosmetic: an independently coded gain is
        // clamped to `LastGainIndex - 16` in `silk_gains_dequant`, so a stale
        // index from before the loss holds the first good frame's gain up when
        // the signal was on its way down — the "energy bounce back" the comment
        // there names. Nothing else resets that index, so the level stayed wrong
        // for as long as the gains kept tracking, which is the whole reason the
        // frame *after* a loss disagreed with the reference while the concealed
        // frame itself matched it bit for bit.
        if lost_flag == FLAG_PACKET_LOST {
            for ch in self.channel_state[..self.n_channels_internal as usize].iter_mut() {
                ch.last_gain_index = 10;
            }
        } else {
            self.prev_decode_only_middle = decode_only_middle;
        }

        if ret < 0 { ret } else { n_samples_out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl SilkDecoder {
        fn frame_length(&self) -> i32 {
            self.channel_state[0].frame_length
        }

        fn sample_rate(&self) -> i32 {
            self.channel_state[0].fs_khz * 1000
        }
    }

    #[test]
    fn test_decoder_creation() {
        let dec = SilkDecoder::new();
        assert_eq!(dec.n_channels_api, 1);
        assert_eq!(dec.n_channels_internal, 1);
    }

    #[test]
    fn test_decoder_init() {
        let mut dec = SilkDecoder::new();

        let ret = dec.init(16000, 1);
        assert_eq!(ret, 0);
        assert_eq!(dec.sample_rate(), 16000);
    }

    #[test]
    fn test_decoder_16khz() {
        let mut dec = SilkDecoder::new();
        let ret = dec.init(16000, 1);
        assert_eq!(ret, 0);
        assert_eq!(dec.frame_length(), 320);
    }
}

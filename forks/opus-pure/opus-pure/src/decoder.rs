//! The Opus decoder.

use crate::celt::{self, CeltDecoder};
use crate::config::{Bandwidth, OpusMode};
use crate::range_coder::RangeCoder;
use crate::repacketizer;
use crate::silk;
use crate::soft_clip::{SoftClip, float_to_i16};
use crate::toc::{
    bandwidth_from_toc, celt_endband_for_bandwidth, channels_from_toc, frame_duration_ms_from_toc,
    mode_from_toc,
};
use crate::{Error, Result};

/// An Opus decoder: Opus packets in, PCM out.
///
/// One decoder handles one stream, and almost everything it needs is carried
/// between packets rather than contained in them: filter histories, the
/// overlap-add buffer, the resampler, and which layer coded the previous frame.
/// So the same instance has to be fed the whole stream in order, and handing a
/// packet to a fresh decoder does not produce the same audio as decoding it in
/// sequence.
///
/// A packet says for itself how long it is and which bandwidth and layer it
/// used, so a decoder needs no configuration beyond the rate and channel count
/// the caller wants back. It follows the stream wherever the encoder went,
/// including mode and bandwidth changes mid-stream.
///
/// Missing packets are expected rather than exceptional. Call
/// [`decode`](Self::decode) with an empty slice to conceal a loss, or
/// [`decode_fec`](Self::decode_fec) on the *following* packet to recover the
/// gap from a redundant copy if the encoder coded one.
///
/// ```
/// use opus_pure::{Application, OpusDecoder, OpusEncoder};
///
/// let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
/// let mut decoder = OpusDecoder::new(48_000, 2)?;
///
/// let mut packet = vec![0u8; 4000];
/// let n = encoder.encode(&vec![0.0f32; 960 * 2], 960, &mut packet)?;
///
/// let mut pcm = vec![0.0f32; 960 * 2];
/// assert_eq!(decoder.decode(&packet[..n], 960, &mut pcm)?, 960);
/// assert_eq!(decoder.decode(&[], 960, &mut pcm)?, 960);   // conceal a loss
/// # Ok::<(), opus_pure::Error>(())
/// ```
pub struct OpusDecoder {
    /// Bits the SILK layer consumed in the last hybrid packet.
    ///
    /// Not part of the public API: this exists so `reference/speed/split` can
    /// measure how a hybrid packet divides between its two layers, which is not
    /// otherwise observable from outside. Off unless the `probe` feature is on,
    /// and what it exposes can change or disappear without a version bump.
    #[cfg(feature = "probe")]
    pub probe_silk_bits: i32,
    /// The last hybrid packet's total bits, the denominator for
    /// [`probe_silk_bits`](Self::probe_silk_bits). Not public API, on the same
    /// terms.
    #[cfg(feature = "probe")]
    pub probe_total_bits: i32,
    celt_dec: CeltDecoder,
    silk_dec: silk::dec_api::SilkDecoder,
    sampling_rate: i32,
    channels: usize,

    prev_mode: Option<OpusMode>,
    frame_size: usize,

    stream_channels: usize,

    silk_resampler: silk::resampler::SilkResampler,
    // Second resampler for the SILK stereo right channel (L uses silk_resampler).
    silk_resampler_r: silk::resampler::SilkResampler,

    prev_internal_rate: i32,

    /// Carries the soft-clipping curve between packets for the 16-bit output
    /// path. libopus keeps the same state on its decoder (`softclip_mem`).
    softclip: SoftClip,
    /// Float scratch the 16-bit entry points decode into before converting.
    w_pcm_f32: Vec<f32>,

    w_pcm_i16: Vec<i16>,
    w_silk_out: Vec<f32>,
    w_pcm_resampled: Vec<i16>,
    w_celt_out: Vec<f32>,

    // SILK per-frame history: libopus prepends the previous frame's last two
    // decoded samples (`sStereo.sMid`) and feeds the resampler from offset 1, a
    // 1-internal-sample delay line. Replicated here so our SILK output aligns
    // with the reference across every bandwidth (was leading by 1 internal
    // sample = 3/4/6 output samples at WB/MB/NB).
    silk_s_mid: [i16; 2],

    /// Range-coder state left by the last decoded frame; read through
    /// [`final_range`](Self::final_range).
    last_range: u32,

    /// Output gain, in Q8 dB. Applied to every decoded sample; 0 is unity.
    ///
    /// This is libopus's `OPUS_SET_GAIN`, and the reason it exists is
    /// [`OpusHead::output_gain_q8`](crate::OpusHead::output_gain_q8): RFC 7845
    /// §5.1 puts a gain in the Ogg header and says players SHOULD apply it, but
    /// nothing in a container can reach inside a decoder to do so. Copy it
    /// across after reading the header and the stream plays at the loudness its
    /// author asked for.
    ///
    /// Applied before the soft clip on the 16-bit path, as libopus does, so a
    /// gain that pushes the signal past full scale is clipped rather than
    /// wrapped.
    pub gain_q8: i32,

    // libopus st->prev_redundancy: the previous frame carried a SILK->CELT
    // redundant frame (redundancy && !celt_to_silk). Suppresses the CELT reset on
    // the following mode change (the redundant frame already primed CELT state).
    prev_redundancy: bool,

    /// libopus `st->DecControl.internalSampleRate`: the rate the SILK layer ran
    /// at in the last frame that carried one. Concealment reuses it instead of
    /// re-deriving it from the packet bandwidth, because libopus only assigns
    /// `internalSampleRate` when it has a packet in hand (opus_decoder.c:423) —
    /// during a mode-switch cross-fade the packet in hand belongs to the *new*
    /// mode and would give the wrong answer.
    silk_internal_rate: i32,
    /// libopus `st->end`: the CELT end band the last real frame set. Concealment
    /// keeps it for the same reason — libopus's `st->bandwidth` is 0 on that
    /// path, so the `CELT_SET_END_BAND` block is skipped (opus_decoder.c:546).
    celt_end_band: usize,
    /// libopus `pcm_transition`: 5 ms of *previous*-mode audio, synthesised by
    /// concealment at a SILK<->CELT switch and cross-faded over the head of the
    /// new frame. Interleaved.
    w_transition: Vec<f32>,
    /// Concealment scratch. The SILK PLC cannot produce a frame shorter than
    /// 10 ms, so a shorter request is concealed here and only its head is kept.
    w_plc: Vec<f32>,
    /// Concealment scratch for the hybrid high band, which is summed onto the
    /// SILK concealment rather than replacing it (libopus `celt_accum`).
    w_plc_celt: Vec<f32>,
}

/// Shows the decoder's configuration and omits its coding state, for the same
/// reason [`OpusEncoder`](crate::OpusEncoder)'s does.
impl std::fmt::Debug for OpusDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpusDecoder")
            .field("sampling_rate", &self.sampling_rate)
            .field("channels", &self.channels)
            .field("final_range", &self.last_range)
            .field("gain_q8", &self.gain_q8)
            .finish_non_exhaustive()
    }
}

impl OpusDecoder {
    /// Create a decoder producing `sampling_rate` Hz and `channels` channels.
    ///
    /// The rate must be one of 8000, 12000, 16000, 24000 or 48000, and the
    /// channel count 1 or 2; anything else is
    /// [`Error::InvalidArgument`].
    ///
    /// Neither has to match how the stream was encoded. These describe the PCM
    /// the caller wants back, and the decoder resamples and mixes to reach it,
    /// so a mono stream decodes to stereo and a 48 kHz one decodes to 16 kHz.
    /// Requesting 48000 avoids a resampling step on the way out.
    ///
    /// For more than two channels, see
    /// [`OpusMSDecoder`](crate::OpusMSDecoder).
    pub fn new(sampling_rate: i32, channels: usize) -> Result<Self> {
        if ![8000, 12000, 16000, 24000, 48000].contains(&sampling_rate) {
            return Err(Error::InvalidArgument("Invalid sampling rate"));
        }
        if ![1, 2].contains(&channels) {
            return Err(Error::InvalidArgument("Invalid number of channels"));
        }

        let mode = celt::modes::default_mode();
        let mut celt_dec = CeltDecoder::new(mode, channels);
        // CELT only has the 48 kHz mode; a lower API rate decimates its output.
        celt_dec.set_downsample((48_000 / sampling_rate) as usize);

        let mut silk_dec = silk::dec_api::SilkDecoder::new();
        silk_dec.init(sampling_rate.min(16000), channels as i32);
        silk_dec.channel_state[0].fs_api_hz = sampling_rate;

        Ok(Self {
            #[cfg(feature = "probe")]
            probe_silk_bits: 0,
            #[cfg(feature = "probe")]
            probe_total_bits: 0,
            celt_dec,
            silk_dec,
            sampling_rate,
            channels,
            prev_mode: None,
            frame_size: 0,
            stream_channels: channels,
            silk_resampler: silk::resampler::SilkResampler::default(),
            silk_resampler_r: silk::resampler::SilkResampler::default(),
            prev_internal_rate: 0,

            // SILK internal scratch: max frame is 60 ms at the 16 kHz WB internal
            // rate (960 samples/ch), i.e. 1920 stereo. Sized like the sibling
            // buffers below for headroom — the old fixed 640 overflowed on any
            // 60 ms SILK frame (panic decoding valid streams).
            w_pcm_i16: vec![0i16; 5760 * channels],

            w_silk_out: vec![0.0f32; 5760 * channels],
            w_pcm_resampled: vec![0i16; 5760 * channels],
            softclip: SoftClip::new(channels),
            w_pcm_f32: Vec::new(),
            w_celt_out: vec![0.0f32; 5760 * channels],
            silk_s_mid: [0; 2],
            last_range: 0,
            gain_q8: 0,
            prev_redundancy: false,
            silk_internal_rate: 16_000,
            celt_end_band: 21,
            w_transition: Vec::new(),
            w_plc: Vec::new(),
            w_plc_celt: Vec::new(),
        })
    }

    /// Write one decoded SILK frame into `output`, resampling to the API rate.
    ///
    /// Always through the resampler, even where the API rate already equals
    /// SILK's internal one. Its copy path is not a plain copy: it still carries
    /// the `delay_matrix_dec` input delay, 4, 9 and 12 samples at 8, 12 and
    /// 16 kHz, and `silk_Decode` calls `silk_resampler` unconditionally for
    /// exactly that reason. Short-circuiting it put every SILK-only stream that
    /// many samples ahead of the reference decoder. `base` is an index into `output` in
    /// interleaved samples; the return is output samples per channel.
    ///
    /// `stereo` selects the true L/R low band that `dec_api` reconstructs from
    /// mid/side. A mono frame in a stereo stream is duplicated to both channels,
    /// but is still pushed through the right-channel resampler as well, so that
    /// resampler's state stays continuous for the next stereo packet.
    fn render_silk_frame(
        &mut self,
        output: &mut [f32],
        base: usize,
        decoded_samples: usize,
        stereo: bool,
        internal_rate: i32,
    ) -> usize {
        let ratio = self.sampling_rate as f64 / internal_rate as f64;
        let out_len = (decoded_samples as f64 * ratio) as usize;
        debug_assert!(out_len <= self.w_pcm_resampled.len());
        let channels = self.channels;

        // How many whole output samples actually fit, decided once. These loops
        // are the entire SILK output path, and testing the bound inside them
        // cost both the branch and any chance of vectorising the conversion:
        // profiled against libopus, this function was eight times the
        // reference's cost for work the reference does in a single pass.
        // Callers size `output` in whole frames, so `fit` is `out_len` in
        // practice and the clamp only reproduces the old truncation.
        let fit = out_len.min(output.len().saturating_sub(base) / channels);
        let out = &mut output[base..base + fit * channels];

        if stereo {
            self.silk_resampler.process(
                &mut self.w_pcm_resampled[..out_len],
                &self.silk_dec.l_out[..decoded_samples],
                decoded_samples as i32,
            );
            for (frame, &v) in out
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(&self.w_pcm_resampled[..fit])
            {
                frame[0] = v as f32 / 32768.0;
            }
            // Right (reuse the scratch)
            self.silk_resampler_r.process(
                &mut self.w_pcm_resampled[..out_len],
                &self.silk_dec.r_out[..decoded_samples],
                decoded_samples as i32,
            );
            for (frame, &v) in out
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(&self.w_pcm_resampled[..fit])
            {
                frame[1] = v as f32 / 32768.0;
            }
        } else {
            {
                let (silk_res, pcm_i16, pcm_out) = (
                    &mut self.silk_resampler,
                    &self.w_pcm_i16,
                    &mut self.w_pcm_resampled,
                );
                silk_res.process(
                    &mut pcm_out[..out_len],
                    &pcm_i16[1..1 + decoded_samples],
                    decoded_samples as i32,
                );
            }
            if channels == 1 {
                for (o, &v) in out.iter_mut().zip(&self.w_pcm_resampled[..fit]) {
                    *o = v as f32 / 32768.0;
                }
            } else {
                for (frame, &v) in out
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(&self.w_pcm_resampled[..fit])
                {
                    let f = v as f32 / 32768.0;
                    frame[0] = f;
                    frame[1] = f;
                }
                // Stereo output, mono packet: also run the mono signal
                // through the RIGHT-channel resampler so its state stays
                // continuous for the next stereo packet (libopus
                // dec_API.c:351-355). Its output overwrites channel 1,
                // which is numerically ~identical to the left here.
                self.silk_resampler_r.process(
                    &mut self.w_pcm_resampled[..out_len],
                    &self.w_pcm_i16[1..1 + decoded_samples],
                    decoded_samples as i32,
                );
                for (frame, &v) in out
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(&self.w_pcm_resampled[..fit])
                {
                    frame[1] = v as f32 / 32768.0;
                }
            }
        }
        out_len
    }

    /// Packet-loss concealment for a lost frame (empty/None packet). Runs the
    /// SILK PLC (LTP+LPC extrapolation) for the last-known SILK/hybrid mode,
    /// resamples to the output rate, and sums the CELT high band's own
    /// concealment on top when that mode was hybrid. Mono conceal is duplicated
    /// to both channels on a stereo output.
    ///
    /// Also drives the cross-fade at a SILK<->CELT mode switch, where libopus
    /// calls it for 5 ms of *previous*-mode audio (see [`Self::fill_transition`]).
    fn decode_plc(&mut self, frame_size: usize, output: &mut [f32]) -> Result<usize> {
        // "Avoids trying to run the PLC on sizes other than 2.5 (CELT), 5
        // (CELT), 10, or 20" (opus_decoder.c:344): a request longer than 20 ms
        // is concealed in 20 ms pieces, because 20 ms is the longest frame CELT
        // has. Its decode buffer holds exactly one, and the pitch branch of
        // concealment indexes `DECODE_BUFFER_SIZE - MAX_PERIOD - n`, which goes
        // negative past that. Concealing a lost 40 or 60 ms packet is an
        // ordinary thing to ask a decoder for, and it reached that subtraction:
        // found by the `decode_stream` fuzz target, pinned in
        // `robustness.rs::concealment_longer_than_a_celt_frame_is_chunked`.
        debug_assert!(output.len() >= frame_size * self.channels);
        let longest = (self.sampling_rate / 50) as usize;
        if frame_size > longest {
            let mut done = 0usize;
            while done < frame_size {
                let take = longest.min(frame_size - done);
                let span = done * self.channels..(done + take) * self.channels;
                self.decode_plc(take, &mut output[span])?;
                done += take;
            }
            return Ok(frame_size);
        }

        let out_samples = frame_size * self.channels;
        for v in output.iter_mut().take(out_samples) {
            *v = 0.0;
        }
        // libopus opus_decoder.c:331 — conceal in the last mode that actually
        // produced audio, which is CELT when the previous frame ended on a
        // SILK->CELT redundant frame.
        //
        // Before any packet has been decoded there is no such mode, and libopus
        // returns the silence above without touching a decoder (`if (mode == 0)`
        // at opus_decoder.c:334). Concealing in a guessed mode instead produces
        // the same silence but leaves SILK's state advanced by a frame — a bumped
        // `lossCnt`, a rotated output history, a stepped PLC seed — so the first
        // real packet decoded its LPC coefficients through the after-loss
        // bandwidth expansion that libopus had no reason to apply.
        let Some(prev_mode) = self.prev_mode else {
            return Ok(frame_size);
        };
        let mode = if self.prev_redundancy {
            OpusMode::CeltOnly
        } else {
            prev_mode
        };
        if mode == OpusMode::CeltOnly {
            // CELT packet-loss concealment (noise-based celt_decode_lost): real
            // attenuating audio instead of silence.
            self.celt_dec
                .conceal_lost_bands(frame_size, output, 0, self.celt_end_band);
            self.prev_mode = Some(mode);
            return Ok(frame_size);
        }

        // "The SILK PLC cannot produce frames of less than 10 ms"
        // (opus_decoder.c:420): a shorter request still runs a 10 ms
        // concealment, and only its head reaches the caller. That happens on
        // every mode-switch cross-fade, which asks for 5 ms.
        let want_ms = (frame_size as i32 * 1000 / self.sampling_rate).max(1);
        let frame_ms = want_ms.max(10);
        let internal_rate = self.silk_internal_rate;
        if internal_rate != self.prev_internal_rate {
            self.silk_resampler.init(internal_rate, self.sampling_rate);
            self.silk_resampler_r
                .init(internal_rate, self.sampling_rate);
            self.prev_internal_rate = internal_rate;
        }
        let n_silk = match frame_ms {
            40 => 2,
            60 => 3,
            _ => 1,
        };
        let internal_frame = (frame_ms * internal_rate / 1000) as usize;
        let internal_sub = internal_frame / n_silk;
        // libopus carries the previous frame's channel configuration through a
        // concealed frame, so a stereo stream conceals both channels and rebuilds
        // L/R the same way a decoded frame does. Forcing mono here instead let the
        // image collapse to the centre for the length of the concealment.
        self.silk_dec.produce_lr = self.channels == 2 && self.silk_dec.n_channels_internal == 2;

        // SILK writes a whole `frame_ms` here; `output` only gets its head.
        let plc_samples = (frame_ms as usize * self.sampling_rate as usize) / 1000;
        let need = plc_samples * self.channels;
        let mut scratch = std::mem::take(&mut self.w_plc);
        if scratch.len() < need {
            scratch.resize(need, 0.0);
        }
        scratch[..need].fill(0.0);
        let conceal = self.conceal_silk(&mut scratch, frame_ms, n_silk, internal_sub);
        if conceal.is_ok() {
            let copy = out_samples.min(need);
            output[..copy].copy_from_slice(&scratch[..copy]);
        }
        self.w_plc = scratch;
        conceal?;

        // Hybrid: the CELT layer conceals its own high band and sums onto the
        // SILK output, exactly as `celt_accum` does on the decode path
        // (opus_decoder.c:599 with data == NULL).
        if mode == OpusMode::Hybrid {
            let mut celt = std::mem::take(&mut self.w_plc_celt);
            if celt.len() < out_samples {
                celt.resize(out_samples, 0.0);
            }
            self.celt_dec.conceal_lost_bands(
                frame_size,
                &mut celt,
                HYBRID_START_BAND,
                self.celt_end_band,
            );
            for (o, c) in output[..out_samples].iter_mut().zip(&celt[..out_samples]) {
                *o += *c;
            }
            self.w_plc_celt = celt;
        }

        self.prev_mode = Some(mode);
        Ok(frame_size)
    }

    /// The SILK half of [`Self::decode_plc`]: `n_silk` concealed internal frames
    /// resampled into `out` (interleaved, `frame_ms` worth).
    fn conceal_silk(
        &mut self,
        out: &mut [f32],
        frame_ms: i32,
        n_silk: usize,
        internal_sub: usize,
    ) -> Result<()> {
        let mut off = 0usize; // output samples/ch written so far
        for sf in 0..n_silk {
            let mut rc = RangeCoder::new_decoder(&[]);
            let n16 = internal_sub;
            if n16 + 2 > self.w_pcm_i16.len() {
                return Err(Error::InvalidPacket("opus PLC: frame exceeds buffer"));
            }
            self.w_pcm_i16[0] = self.silk_s_mid[0];
            self.w_pcm_i16[1] = self.silk_s_mid[1];
            let ret = self.silk_dec.decode(
                &mut rc,
                &mut self.w_pcm_i16[2..n16 + 2],
                silk::decode_frame::FLAG_PACKET_LOST,
                sf == 0,
                frame_ms,
                self.silk_internal_rate,
            );
            if ret < 0 {
                return Err(Error::Internal("SILK PLC failed"));
            }
            let dec = ret as usize;
            if dec >= 2 {
                self.silk_s_mid[0] = self.w_pcm_i16[dec];
                self.silk_s_mid[1] = self.w_pcm_i16[dec + 1];
            }
            // A stereo stream conceals both channels and reconstructs L/R the
            // same way a decoded frame does. Writing the mid to both outputs
            // instead collapses the image for the length of the concealment,
            // which at a mode switch is audible as the stereo field snapping to
            // the centre for one 5 ms window. That reconstruction is exactly
            // what render_silk_frame does, so concealment shares it.
            let stereo = self.channels == 2 && self.silk_dec.produce_lr;
            let base = off * self.channels;
            off += self.render_silk_frame(out, base, dec, stereo, self.silk_internal_rate);
        }
        Ok(())
    }

    /// libopus opus_decoder.c:417 — the SILK layer carries no state across a
    /// CELT-only stretch, so it restarts from scratch when the stream comes
    /// back to it. Without this the first SILK frame after a CELT run predicts
    /// from LPC/LTP history that belongs to whatever came before the CELT run.
    fn reset_silk_after_celt_only(&mut self) {
        if self.prev_mode != Some(OpusMode::CeltOnly) {
            return;
        }
        for ch in 0..2 {
            silk::init_decoder::silk_init_decoder(&mut self.silk_dec.channel_state[ch]);
        }
        self.silk_dec.s_stereo_pred_prev_q13 = [0; 2];
        self.silk_dec.s_stereo_mid = [0; 2];
        self.silk_dec.s_stereo_side = [0; 2];
        self.silk_dec.prev_decode_only_middle = 0;
        self.silk_s_mid = [0; 2];
    }

    /// libopus `pcm_transition` (opus_decoder.c:388 and :540). A SILK<->CELT
    /// switch with no redundancy hands the new layer no history to overlap-add
    /// against, so its first samples start from silence. libopus covers the seam
    /// by concealing 5 ms in the *previous* mode and cross-fading that over the
    /// head of the frame.
    ///
    /// The concealment call is a synthesis side-channel, not a decoded frame, so
    /// `prev_mode` / `prev_redundancy` are restored afterwards. libopus leaves
    /// them equal too: `transition` already implies `prev_redundancy` is clear,
    /// and the recursive call re-stores the same `prev_mode` it read.
    fn fill_transition(&mut self, sub_frame_size: usize, end_band: usize) -> Result<()> {
        let n = ((self.sampling_rate / 200) as usize).min(sub_frame_size);
        let need = n * self.channels;
        let mut buf = std::mem::take(&mut self.w_transition);
        if buf.len() < need {
            buf.resize(need, 0.0);
        }
        let saved_mode = self.prev_mode;
        let saved_redundancy = self.prev_redundancy;
        let saved_end_band = std::mem::replace(&mut self.celt_end_band, end_band);
        let r = self.decode_plc(n, &mut buf);
        self.w_transition = buf;
        self.prev_mode = saved_mode;
        self.prev_redundancy = saved_redundancy;
        self.celt_end_band = saved_end_band;
        r.map(|_| ())
    }

    /// Blend the concealed previous-mode head from [`Self::fill_transition`]
    /// into the start of `region` (opus_decoder.c:660): the first 2.5 ms comes
    /// from the concealment outright, the next 2.5 ms cross-fades into the
    /// decoded frame. A frame shorter than 5 ms has no room for both, so it
    /// gets the cross-fade alone.
    fn apply_transition(&self, region: &mut [f32], sub_frame_size: usize) {
        let f5 = (self.sampling_rate / 200) as usize;
        let f2_5 = f5 / 2;
        let inc = (48_000 / self.sampling_rate) as usize;
        let window = celt::modes::default_mode().window;
        let ch = self.channels;
        let trans = &self.w_transition;
        let head = if sub_frame_size >= f5 {
            region[..f2_5 * ch].copy_from_slice(&trans[..f2_5 * ch]);
            f2_5
        } else {
            0
        };
        for i in 0..f2_5 {
            let w = window[i * inc] * window[i * inc];
            for c in 0..ch {
                let idx = (head + i) * ch + c;
                region[idx] = w * region[idx] + (1.0 - w) * trans[idx];
            }
        }
    }

    /// Forward-error-correction decode: reconstruct a LOST frame from the
    /// low-bitrate redundancy (LBRR) embedded in the NEXT received `packet`.
    ///
    /// The SILK decoder runs in `FLAG_DECODE_LBRR` mode, which self-selects: it
    /// decodes the redundant copy when the packet carries one for this frame,
    /// and extrapolates as for a lost frame when it does not. After this call
    /// the caller decodes `packet` normally for the following frame.
    ///
    /// Redundancy only ever covers one SILK frame, so when `frame_size` is
    /// longer than the packet's own frames the excess ahead of it is concealed
    /// and only the tail is recovered. A CELT-only packet, or one arriving
    /// while the stream is in CELT-only mode, carries no redundancy at all and
    /// falls back to plain concealment.
    /// Per-packet internal channel switch (libopus `dec_API.c:119-166`).
    ///
    /// Returns whether SILK should produce L/R directly for this packet. All
    /// three decode paths (FEC, normal, PLC) need exactly this bookkeeping, so
    /// it lives here once rather than being repeated at each of them.
    fn setup_silk_channels(&mut self, packet_channels: usize) -> bool {
        let silk_lr = self.channels == 2 && packet_channels == 2;
        self.silk_dec.produce_lr = silk_lr;
        let prev_internal_ch = self.silk_dec.n_channels_internal;
        if packet_channels as i32 > prev_internal_ch {
            // mono -> stereo: reset the side channel decoder.
            silk::init_decoder::silk_init_decoder(&mut self.silk_dec.channel_state[1]);
        }
        if silk_lr && prev_internal_ch == 1 {
            // Switching to stereo: clear stereo prediction/side history and
            // seed the right-channel resampler from the (continuous) left.
            self.silk_dec.s_stereo_pred_prev_q13 = [0; 2];
            self.silk_dec.s_stereo_side = [0; 2];
            self.silk_resampler_r = self.silk_resampler.clone();
        }
        self.silk_dec.n_channels_internal = packet_channels as i32;
        silk_lr
    }

    /// The sample rate this decoder was created with, in Hz.
    pub fn sample_rate(&self) -> i32 {
        self.sampling_rate
    }

    /// The channel count this decoder was created with.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Range-coder state left by the last decoded frame (libopus
    /// `OPUS_GET_FINAL_RANGE`).
    ///
    /// A decoder that has read a packet correctly ends in exactly the state the
    /// encoder ended in, so comparing this against
    /// [`OpusEncoder::final_range`](crate::OpusEncoder::final_range) is a cheap
    /// check that the two agree bit for bit. That is what the RFC 6716 test
    /// vectors compare, and what tells a desync apart from a merely
    /// disappointing decode. It is not needed to decode audio.
    pub fn final_range(&self) -> u32 {
        self.last_range
    }

    /// Samples per channel in the last packet decoded from real data (libopus
    /// `OPUS_GET_LAST_PACKET_DURATION`), or 0 before the first one.
    ///
    /// Concealed and FEC-recovered frames do not change it, so after a loss it
    /// still reports the last packet that actually arrived. To ask the same
    /// question of a packet you are holding but have not decoded — which is
    /// what a muxer or a jitter buffer wants — use
    /// [`packet::samples`](crate::packet::samples) instead; it reads the TOC
    /// and needs no decoder at all.
    pub fn last_packet_duration(&self) -> usize {
        self.frame_size
    }

    /// Discard everything the decoder has learned, keeping its settings.
    ///
    /// This is libopus's `OPUS_RESET_STATE`, and the moment to call it is
    /// between two unrelated streams sharing one decoder. Almost everything
    /// interesting in an Opus decoder is carried *between* packets — the LTP
    /// and LPC histories, the overlap-add buffer, the resampler, which layer
    /// coded the previous frame — so a second stream started on a used decoder
    /// begins by blending into the end of the first.
    ///
    /// Equivalent to building a new decoder with the same sample rate and
    /// channel count, and carrying [`gain_q8`](Self::gain_q8) across. It resets
    /// in place and allocates nothing, so a player can call it at every loop
    /// point.
    pub fn reset_state(&mut self) -> Result<()> {
        // Exhaustive, so a field added to the decoder does not compile until
        // this says how it resets. Each is set to what `new` gives it.
        let Self {
            #[cfg(feature = "probe")]
            probe_silk_bits,
            #[cfg(feature = "probe")]
            probe_total_bits,
            celt_dec,
            silk_dec,
            sampling_rate,
            channels,
            prev_mode,
            frame_size,
            stream_channels,
            silk_resampler,
            silk_resampler_r,
            prev_internal_rate,
            softclip,
            w_pcm_f32,
            w_pcm_i16,
            w_silk_out,
            w_pcm_resampled,
            w_celt_out,
            silk_s_mid,
            last_range,
            gain_q8: _,
            prev_redundancy,
            silk_internal_rate,
            celt_end_band,
            w_transition,
            w_plc,
            w_plc_celt,
        } = self;
        #[cfg(feature = "probe")]
        {
            *probe_silk_bits = 0;
            *probe_total_bits = 0;
        }
        // The rate and channel count are settings, as is the CELT downsample
        // factor derived from the rate, which `reset` keeps.
        celt_dec.reset();
        celt_dec.set_stream_channels(*channels);
        // The SILK decoder holds no heap memory, so a new one costs no
        // allocation.
        *silk_dec = silk::dec_api::SilkDecoder::new();
        silk_dec.init((*sampling_rate).min(16000), *channels as i32);
        silk_dec.channel_state[0].fs_api_hz = *sampling_rate;
        *prev_mode = None;
        *frame_size = 0;
        *stream_channels = *channels;
        *silk_resampler = silk::resampler::SilkResampler::default();
        *silk_resampler_r = silk::resampler::SilkResampler::default();
        *prev_internal_rate = 0;
        softclip.reset();
        *silk_s_mid = [0; 2];
        *last_range = 0;
        *prev_redundancy = false;
        *silk_internal_rate = 16_000;
        *celt_end_band = 21;
        // Scratch starts as `new` leaves it, zeroed or empty, with its
        // capacity kept.
        w_pcm_i16.fill(0);
        w_silk_out.fill(0.0);
        w_pcm_resampled.fill(0);
        w_celt_out.fill(0.0);
        w_pcm_f32.clear();
        w_transition.clear();
        w_plc.clear();
        w_plc_celt.clear();
        Ok(())
    }

    /// Decode one packet into `frame_size` samples per channel of float PCM,
    /// returning how many it produced.
    ///
    /// `output` is interleaved and must hold `frame_size * channels` samples.
    /// An empty `input` means a lost packet and runs packet-loss concealment.
    ///
    /// The output is **not** bounded by ±1: the codec rings, and a signal
    /// mastered near full scale comes back slightly over it. libopus behaves
    /// the same way. Convert to integer PCM with
    /// [`decode_s16`](Self::decode_s16), which handles that, or apply
    /// [`SoftClip`] yourself if you need the float and are converting later.
    pub fn decode(&mut self, input: &[u8], frame_size: usize, output: &mut [f32]) -> Result<usize> {
        self.decode_native(input, frame_size, output, false)
    }

    /// Decode one packet into `frame_size` samples per channel of 16-bit PCM,
    /// returning how many it produced.
    ///
    /// Soft-clips before converting, so the result is inside the 16-bit range
    /// without the broadband distortion that saturating there would cause, and
    /// without a step at the packet boundary when a peak straddles one. This is
    /// what libopus's `opus_decode` does and `opus_decode_float` does not, and
    /// it is the reason to prefer this entry point over converting the float
    /// output by hand. See [`SoftClip`] for what the curve is.
    ///
    /// ```
    /// # use opus_pure::{Application, OpusDecoder, OpusEncoder};
    /// # let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
    /// # let mut packet = vec![0u8; 4000];
    /// # let n = encoder.encode_s16(&vec![0i16; 960 * 2], 960, &mut packet)?;
    /// let mut decoder = OpusDecoder::new(48_000, 2)?;
    /// let mut pcm = vec![0i16; 960 * 2];
    /// let samples = decoder.decode_s16(&packet[..n], 960, &mut pcm)?;
    /// # assert_eq!(samples, 960);
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn decode_s16(
        &mut self,
        input: &[u8],
        frame_size: usize,
        output: &mut [i16],
    ) -> Result<usize> {
        self.decode_as_s16(frame_size, output, |d, pcm| {
            d.decode_native(input, frame_size, pcm, true)
        })
    }

    /// Reconstruct the previous packet from this one's in-band FEC, as float.
    ///
    /// Call this when a packet is lost and the packet *after* it has arrived:
    /// SILK and hybrid streams can carry a low-rate copy of the frame before,
    /// which reconstructs it far better than concealment can. Falls back to
    /// concealment when the packet carries no such copy.
    pub fn decode_fec(
        &mut self,
        packet: &[u8],
        frame_size: usize,
        output: &mut [f32],
    ) -> Result<usize> {
        self.decode_fec_native(packet, frame_size, output, false)
    }

    /// [`decode_fec`](Self::decode_fec) into 16-bit PCM, soft-clipped the same
    /// way [`decode_s16`](Self::decode_s16) is.
    pub fn decode_fec_s16(
        &mut self,
        packet: &[u8],
        frame_size: usize,
        output: &mut [i16],
    ) -> Result<usize> {
        self.decode_as_s16(frame_size, output, |d, pcm| {
            d.decode_fec_native(packet, frame_size, pcm, true)
        })
    }

    /// Run `decode` into the float scratch, then convert what it produced.
    ///
    /// The scratch is moved out of `self` and back because the decode needs
    /// `&mut self` and a buffer that lives on it at the same time; it stays
    /// allocated between calls either way.
    fn decode_as_s16(
        &mut self,
        frame_size: usize,
        output: &mut [i16],
        decode: impl FnOnce(&mut Self, &mut [f32]) -> Result<usize>,
    ) -> Result<usize> {
        let capacity = frame_size * self.channels;
        if output.len() < capacity {
            return Err(Error::buffer_too_small(capacity, output.len()));
        }
        let mut pcm = std::mem::take(&mut self.w_pcm_f32);
        pcm.clear();
        pcm.resize(capacity, 0.0);
        let result = decode(self, &mut pcm);
        if let Ok(produced) = result {
            let n = produced * self.channels;
            for (o, &s) in output[..n].iter_mut().zip(&pcm[..n]) {
                *o = float_to_i16(s);
            }
        }
        self.w_pcm_f32 = pcm;
        result
    }

    /// Decode as float, then either apply the soft-clipping curve or clear it.
    ///
    /// Clearing on the float path is deliberate and matches libopus
    /// (`opus_decode_native`): a curve left half-applied by a 16-bit call would
    /// otherwise bend the start of the next frame a caller asked for in float,
    /// where nothing is going to clip it.
    pub(crate) fn decode_native(
        &mut self,
        input: &[u8],
        frame_size: usize,
        output: &mut [f32],
        soft_clip: bool,
    ) -> Result<usize> {
        let produced = self.decode_impl(input, frame_size, output)?;
        self.finish(&mut output[..produced * self.channels], soft_clip);
        Ok(produced)
    }

    /// [`decode_native`](Self::decode_native) for the FEC path.
    fn decode_fec_native(
        &mut self,
        packet: &[u8],
        frame_size: usize,
        output: &mut [f32],
        soft_clip: bool,
    ) -> Result<usize> {
        let produced = self.decode_fec_impl(packet, frame_size, output)?;
        self.finish(&mut output[..produced * self.channels], soft_clip);
        Ok(produced)
    }

    fn finish(&mut self, pcm: &mut [f32], soft_clip: bool) {
        // libopus `opus_decode_native`: the gain goes on before the soft clip,
        // so boosting past full scale is clipped rather than wrapped. The
        // constant converts Q8 dB into an exponent of two —
        // `10^(g/(20*256)) == 2^(g * log2(10) / 5120)`.
        if self.gain_q8 != 0 {
            let gain = (self.gain_q8 as f32 * 6.488_141e-4).exp2();
            for s in pcm.iter_mut() {
                *s *= gain;
            }
        }
        if soft_clip {
            self.softclip.apply(pcm);
        } else {
            self.softclip.reset();
        }
    }

    fn decode_fec_impl(
        &mut self,
        packet: &[u8],
        frame_size: usize,
        output: &mut [f32],
    ) -> Result<usize> {
        let capacity = frame_size * self.channels;
        if output.len() < capacity {
            return Err(Error::buffer_too_small(capacity, output.len()));
        }
        if packet.is_empty() {
            return self.decode_plc(frame_size, output);
        }
        let toc = packet[0];
        let mode = mode_from_toc(toc);
        if mode == OpusMode::CeltOnly || self.prev_mode == Some(OpusMode::CeltOnly) {
            return self.decode_plc(frame_size, output);
        }
        let packet_frame_ms = frame_duration_ms_from_toc(toc);
        let packet_frame_size = (packet_frame_ms * self.sampling_rate / 1000) as usize;
        if packet_frame_size == 0 || frame_size < packet_frame_size {
            return self.decode_plc(frame_size, output);
        }

        // The redundancy lives in the packet's FIRST frame; anything the request
        // covers ahead of it has no redundant copy and is concealed instead.
        let lead = frame_size - packet_frame_size;
        if lead > 0 {
            self.decode_plc(lead, &mut output[..lead * self.channels])?;
        }
        let base = lead * self.channels;
        for v in output[base..capacity].iter_mut() {
            *v = 0.0;
        }

        let (_, frames, _) = crate::repacketizer::parse_packet(packet, false)?;
        let payload = frames[0];

        let bandwidth = bandwidth_from_toc(toc);
        let packet_channels = channels_from_toc(toc);
        let internal_rate = if mode == OpusMode::Hybrid {
            16000
        } else {
            match bandwidth {
                Bandwidth::Narrowband => 8000,
                Bandwidth::Mediumband => 12000,
                _ => 16000,
            }
        };
        if internal_rate != self.prev_internal_rate {
            self.silk_resampler.init(internal_rate, self.sampling_rate);
            self.silk_resampler_r
                .init(internal_rate, self.sampling_rate);
            self.prev_internal_rate = internal_rate;
        }
        self.silk_internal_rate = internal_rate;

        // Same channel bookkeeping as a normal SILK frame: a redundant frame is
        // an ordinary coded frame, so a stereo one has to reconstruct L/R rather
        // than collapse to the mid.
        let silk_lr = self.setup_silk_channels(packet_channels);

        // A 40 or 60 ms packet holds two or three SILK frames, each redundantly
        // coded in its own right; libopus keeps calling the SILK decoder until
        // the requested duration is filled.
        let n_silk = match packet_frame_ms {
            40 => 2,
            60 => 3,
            _ => 1,
        };
        let internal_sub_frame = (packet_frame_ms * internal_rate / 1000) as usize / n_silk;
        let pcm_i16_len = internal_sub_frame * self.channels;
        if pcm_i16_len + 2 > self.w_pcm_i16.len() {
            return Err(Error::InvalidPacket("opus FEC: frame exceeds buffer"));
        }

        let mut rc = RangeCoder::new_decoder(payload);
        let mut written = 0usize;
        for sf in 0..n_silk {
            let s_mid = self.silk_s_mid;
            let ret = {
                let (silk_dec, pcm_i16) = (&mut self.silk_dec, &mut self.w_pcm_i16);
                pcm_i16[0] = s_mid[0];
                pcm_i16[1] = s_mid[1];
                silk_dec.decode(
                    &mut rc,
                    &mut pcm_i16[2..pcm_i16_len + 2],
                    silk::decode_frame::FLAG_DECODE_LBRR,
                    sf == 0,
                    packet_frame_ms,
                    internal_rate,
                )
            };
            if ret < 0 {
                return Err(Error::Internal("SILK FEC failed"));
            }
            let decoded_samples = ret as usize;
            if decoded_samples >= 2 {
                self.silk_s_mid[0] = self.w_pcm_i16[decoded_samples];
                self.silk_s_mid[1] = self.w_pcm_i16[decoded_samples + 1];
            }
            written += self.render_silk_frame(
                output,
                base + written * self.channels,
                decoded_samples,
                silk_lr,
                internal_rate,
            );
        }

        // Hybrid: the redundancy only ever covers the SILK low band, so the CELT
        // high band conceals itself and is summed on top, as it is for a lost
        // frame (opus_decoder.c passes NULL to the CELT decoder here).
        if mode == OpusMode::Hybrid {
            let tail = capacity - base;
            let mut celt = std::mem::take(&mut self.w_plc_celt);
            if celt.len() < tail {
                celt.resize(tail, 0.0);
            }
            self.celt_dec.conceal_lost_bands(
                packet_frame_size,
                &mut celt,
                HYBRID_START_BAND,
                self.celt_end_band,
            );
            for (o, c) in output[base..capacity].iter_mut().zip(&celt[..tail]) {
                *o += *c;
            }
            self.w_plc_celt = celt;
        }

        self.prev_mode = Some(mode);
        self.prev_redundancy = false;
        Ok(frame_size)
    }

    fn decode_impl(
        &mut self,
        input: &[u8],
        frame_size: usize,
        output: &mut [f32],
    ) -> Result<usize> {
        // `frame_size` is the room available in `output`, counted per channel.
        // libopus takes that on trust because C hands it a bare pointer; here
        // the slice knows its own length, so hold the caller to what they
        // declared rather than letting a short buffer truncate the decode.
        let capacity = frame_size * self.channels;
        if output.len() < capacity {
            return Err(Error::buffer_too_small(capacity, output.len()));
        }
        // Lost packet (data==NULL / empty) -> packet-loss concealment.
        if input.is_empty() {
            return self.decode_plc(frame_size, output);
        }

        let toc = input[0];
        let mode = mode_from_toc(toc);
        let packet_channels = channels_from_toc(toc);
        let bandwidth = bandwidth_from_toc(toc);
        let frame_duration_ms = frame_duration_ms_from_toc(toc);

        // Every packet decodes through this one decoder, whatever its channel
        // count. A stream may switch between mono and stereo at any packet, and
        // libopus keeps a single decoder across those switches so that the SILK
        // stereo/resampler state and the CELT overlap-add, prefilter and
        // preemphasis histories stay one continuous chain. Rendering to a
        // different output channel count happens *inside* each layer, where the
        // reference does it:
        //
        //   mono packet, stereo output (C=1, CC=2) — SILK emits the mid through
        //   both channels' resamplers; CELT re-runs its inverse MDCT per output
        //   channel off the single decoded spectrum.
        //
        //   stereo packet, mono output (C=2, CC=1) — SILK emits the mid, which
        //   *is* (L+R)/2 by construction, and never reconstructs L/R; CELT sums
        //   the two spectra before a single inverse MDCT.
        //
        // Both are `stream_channels` on the CELT decoder and `n_channels_internal`
        // on the SILK one; neither needs a second decoder.

        // One packet parser for the whole crate. `repacketizer::parse_packet`
        // is the port of libopus `opus_packet_parse_impl`, and `decode_fec` and
        // the repacketizer already went through it; the copy that used to live
        // here had drifted from it, accepting frames past the 1275-byte limit
        // of RFC 6716 §3.4 and rejecting the zero-length DTX frames of a code 1
        // packet that libopus accepts.
        let (_, frame_payloads, _) = repacketizer::parse_packet(input, false)?;
        let frame_count = frame_payloads.len();

        // libopus opus_decoder.c opus_decode_native:
        //   if (count*packet_frame_size > frame_size)
        //      return OPUS_BUFFER_TOO_SMALL;
        // The packet's own TOC duration must fit the caller's frame_size. We split
        // the caller's buffer as sub_frame_size = frame_size / frame_count, so a
        // malformed multi-frame packet (large frame count vs. a small caller
        // buffer) would otherwise make sub_frame_size smaller than the 2.5/5 ms
        // redundancy-fade region — the fuzzer-found out-of-bounds/underflow panics
        // in redundancy_fade_start/redundancy_fade_end. C rejects such packets
        // here; so do we.
        let packet_frame_samples =
            repacketizer::samples_per_frame(toc, self.sampling_rate) as usize;
        if frame_count * packet_frame_samples > frame_size {
            return Err(Error::buffer_too_small(
                frame_count * packet_frame_samples,
                frame_size,
            ));
        }

        // From here on, work in the packet's own duration rather than the
        // caller's buffer size. libopus does the same: `frame_size` is the room
        // available in `output`, and `opus_decode` returns however many samples
        // the packet actually held. Treating it as the exact output length
        // instead stretched a 20 ms packet across whatever buffer it was given.
        let frame_size = frame_count * packet_frame_samples;

        self.frame_size = frame_size;
        self.stream_channels = packet_channels;
        // libopus sets `st->end` from the packet's bandwidth (opus_decoder.c:546)
        // *after* the mode-switch concealment runs, so that concealment still
        // sees the band range the outgoing mode was decoded with.
        let prev_celt_end_band = self.celt_end_band;
        self.celt_end_band = celt_endband_for_bandwidth(bandwidth);

        let sub_frame_size = frame_size / frame_count;
        let sub_output_len = sub_frame_size * self.channels;

        // libopus opus_decoder.c:374 — a SILK<->CELT switch leaves the layer
        // taking over with no overlap-add history, so its first samples start
        // from silence. Unless the encoder bridged the seam with a redundant
        // frame, libopus conceals 5 ms of the outgoing mode and cross-fades it
        // over the head of this frame. Only the packet's first Opus frame can
        // be a transition: after it, the previous mode is this one.
        let transition = match self.prev_mode {
            Some(prev) => {
                (mode == OpusMode::CeltOnly && prev != OpusMode::CeltOnly && !self.prev_redundancy)
                    || (mode != OpusMode::CeltOnly && prev == OpusMode::CeltOnly)
            }
            None => false,
        };

        match mode {
            OpusMode::SilkOnly => {
                let internal_sample_rate = match bandwidth {
                    Bandwidth::Narrowband => 8000,
                    Bandwidth::Mediumband => 12000,
                    Bandwidth::Wideband => 16000,
                    _ => 16000,
                };
                let internal_frame_size =
                    (frame_duration_ms * internal_sample_rate / 1000) as usize;

                // Initialised even where the rates already match: the copy
                // path still applies the resampler's input delay, and
                // `render_silk_frame` now routes every SILK frame through it.
                if internal_sample_rate != self.prev_internal_rate {
                    self.silk_resampler
                        .init(internal_sample_rate, self.sampling_rate);
                    self.silk_resampler_r
                        .init(internal_sample_rate, self.sampling_rate);
                    self.prev_internal_rate = internal_sample_rate;
                }
                self.silk_internal_rate = internal_sample_rate;
                self.reset_silk_after_celt_only();

                // Pure-SILK stereo (both stream and output are 2ch): reconstruct
                // true L/R via SILK MS->LR instead of duplicating the mono mid.
                let silk_lr = self.setup_silk_channels(packet_channels);

                // A 40/60 ms Opus frame carries 2/3 internal 20 ms SILK frames;
                // 10/20 ms carry one. libopus calls silk_Decode once per internal
                // frame (continuing the same range coder within the payload). We
                // must too — decoding only the first internal frame leaves the
                // rest of a 40/60 ms packet silent (the "collapse" bug).
                let n_silk = match frame_duration_ms {
                    40 => 2,
                    60 => 3,
                    _ => 1,
                };
                let internal_sub_frame_size = internal_frame_size / n_silk;
                // Per-FRAME previous mode (libopus updates prev_mode per frame; for
                // payloads after the first, the previous frame is this same packet).
                let mut prev_mode_frame = self.prev_mode;

                for (fi, payload) in frame_payloads.iter().enumerate() {
                    let mut rc = RangeCoder::new_decoder(payload);
                    let pcm_i16_len = internal_sub_frame_size * self.channels;
                    // A malformed packet can imply a frame larger than our scratch
                    // buffer; reject it gracefully instead of slicing out of bounds
                    // (a decode-path DoS on attacker-controlled input).
                    if pcm_i16_len + 2 > self.w_pcm_i16.len() {
                        return Err(Error::InvalidPacket("opus: SILK frame size exceeds buffer"));
                    }
                    let out_start = fi * sub_output_len;
                    let mut silk_off = 0usize; // output samples/ch within this Opus frame

                    for sf in 0..n_silk {
                        let s_mid = self.silk_s_mid;
                        let ret = {
                            let (silk_dec, pcm_i16) = (&mut self.silk_dec, &mut self.w_pcm_i16);
                            // Prepend the previous frame's last two samples (sMid) at
                            // [0..2] and decode at offset 2, matching libopus's
                            // samplesOut1_tmp[n][2] layout.
                            pcm_i16[0] = s_mid[0];
                            pcm_i16[1] = s_mid[1];
                            silk_dec.decode(
                                &mut rc,
                                &mut pcm_i16[2..pcm_i16_len + 2],
                                silk::decode_frame::FLAG_DECODE_NORMAL,
                                sf == 0,
                                frame_duration_ms,
                                internal_sample_rate,
                            )
                        };

                        if ret < 0 {
                            return Err(Error::Internal("SILK decoding failed"));
                        }

                        let decoded_samples = ret as usize;
                        // Carry the last two decoded samples as next frame's sMid.
                        if decoded_samples >= 2 {
                            self.silk_s_mid[0] = self.w_pcm_i16[decoded_samples];
                            self.silk_s_mid[1] = self.w_pcm_i16[decoded_samples + 1];
                        }
                        let base = out_start + silk_off * self.channels;

                        // Stereo SILK: L in silk_dec.l_out, R in silk_dec.r_out,
                        // both already in the 1-sample-delay-line layout. Resample
                        // each channel through its own resampler.
                        let out_len = self.render_silk_frame(
                            output,
                            base,
                            decoded_samples,
                            silk_lr,
                            internal_sample_rate,
                        );
                        silk_off += out_len;
                    }

                    // --- Opus redundancy layer (opus_decoder.c:420-580) ---
                    // A SILK-only frame carries IMPLICIT CELT redundancy: if >= 17
                    // bits remain after SILK, the trailing bytes ARE a 5 ms CELT
                    // frame (no flag) used to smooth mode/bandwidth transitions.
                    let mut redundant_rng = 0u32;
                    let mut redundancy = false;
                    let mut celt_to_silk = false;
                    let plen = payload.len();
                    let f5 = (self.sampling_rate / 200) as usize;
                    let f2_5 = f5 / 2;
                    let red_end_band = celt_endband_for_bandwidth(bandwidth);
                    let mut red_buf = [0.0f32; 480]; // F5 * <=2ch, planar
                    let mut red_bytes = 0usize;
                    if self.sampling_rate == 48000 && rc.tell() + 17 <= (plen as i32) * 8 {
                        redundancy = true;
                        celt_to_silk = rc.decode_bit_logp(1);
                        red_bytes = plen - (((rc.tell() + 7) >> 3) as usize);
                        if red_bytes < 2 || red_bytes >= plen {
                            redundancy = false;
                            red_bytes = 0;
                        }
                    }
                    // A redundant frame already bridges the seam, so it replaces
                    // the cross-fade rather than stacking with it
                    // (opus_decoder.c:532). Conceal before the redundant frame
                    // below advances the CELT state it reads.
                    let fade_transition = transition && fi == 0 && !redundancy;
                    if fade_transition {
                        self.fill_transition(sub_frame_size, prev_celt_end_band)?;
                    }
                    // CELT->SILK: the redundant frame continues the prior CELT
                    // state (a fade-out of the previous CELT mode). Decode BEFORE
                    // the hybrid->SILK silence frame to keep libopus state order.
                    if redundancy && celt_to_silk {
                        redundant_rng = self.decode_redundant_celt(
                            &payload[plen - red_bytes..],
                            false,
                            packet_channels,
                            red_end_band,
                            &mut red_buf[..f5 * self.channels],
                        );
                    }
                    // Hybrid->SILK transition: let the CELT MDCT fade out by
                    // decoding a 2-byte silence frame; its 2.5 ms overlap tail is
                    // ADDED to the output (libopus decodes it into pcm before the
                    // SILK sum).
                    if self.sampling_rate == 48000
                        && prev_mode_frame == Some(OpusMode::Hybrid)
                        && !(redundancy && celt_to_silk && self.prev_redundancy)
                    {
                        let silence = [0xFFu8, 0xFF];
                        let mut sil_buf = [0.0f32; 240]; // F2_5 * <=2ch
                        self.celt_dec.set_stream_channels(packet_channels);
                        let mut src = RangeCoder::new_decoder(&silence);
                        self.celt_dec.decode_from_range_coder_with_band_range(
                            &mut src,
                            16,
                            f2_5,
                            &mut sil_buf[..f2_5 * self.channels],
                            0,
                            red_end_band,
                        );
                        let region = &mut output[out_start..out_start + sub_output_len];
                        for (o, s) in region.iter_mut().zip(&sil_buf[..f2_5 * self.channels]) {
                            *o += *s;
                        }
                    }
                    // SILK->CELT: reset, then decode — this PRIMES the CELT state
                    // for the upcoming CELT-mode frames (which is why the next mode
                    // change skips its reset when prev_redundancy is set).
                    if redundancy && !celt_to_silk {
                        redundant_rng = self.decode_redundant_celt(
                            &payload[plen - red_bytes..],
                            true,
                            packet_channels,
                            red_end_band,
                            &mut red_buf[..f5 * self.channels],
                        );
                    }
                    if redundancy {
                        let window = celt::modes::default_mode().window;
                        let region = &mut output[out_start..out_start + sub_output_len];
                        if celt_to_silk {
                            redundancy_fade_start(region, &red_buf, f2_5, self.channels, window);
                        } else {
                            redundancy_fade_end(
                                region,
                                sub_frame_size,
                                &red_buf,
                                f2_5,
                                self.channels,
                                window,
                            );
                        }
                    }
                    if fade_transition {
                        self.apply_transition(
                            &mut output[out_start..out_start + sub_output_len],
                            sub_frame_size,
                        );
                    }
                    self.prev_redundancy = redundancy && !celt_to_silk;
                    prev_mode_frame = Some(OpusMode::SilkOnly);
                    self.last_range = rc.rng ^ redundant_rng;
                }
                self.prev_mode = Some(OpusMode::SilkOnly);
                Ok(frame_size)
            }

            OpusMode::CeltOnly => {
                let celt_end_band = self.celt_end_band_from_toc(toc);
                // Conceal the outgoing SILK/hybrid mode BEFORE the reset below
                // wipes the state it reads (opus_decoder.c:388).
                if transition {
                    self.fill_transition(sub_frame_size, prev_celt_end_band)?;
                }
                // libopus opus_decoder.c:515 — discard CELT state on a mode change
                // unless the previous frame's SILK->CELT redundant frame already
                // primed it.
                if let Some(pm) = self.prev_mode
                    && pm != OpusMode::CeltOnly
                    && !self.prev_redundancy
                {
                    self.celt_dec.reset();
                }
                self.prev_redundancy = false;
                // Mono packet in a stereo stream => C=1, CC=2 (continuous state).
                self.celt_dec.set_stream_channels(packet_channels);

                for (fi, payload) in frame_payloads.iter().enumerate() {
                    let mut rc = RangeCoder::new_decoder(payload);
                    let total_bits = (payload.len() * 8) as i32;
                    let needed = sub_frame_size * self.channels;
                    let out_start = fi * needed;
                    let out_end = (out_start + needed).min(output.len());

                    if output.len() < out_end {
                        return Err(Error::buffer_too_small(out_end, output.len()));
                    }

                    // No clamping: libopus's float API returns the
                    // reconstruction as-is, and codec ringing legitimately
                    // overshoots slightly. Clamping here also made this path
                    // disagree with the SILK path, which never did.
                    self.celt_dec.decode_from_range_coder_with_band_range(
                        &mut rc,
                        total_bits,
                        sub_frame_size,
                        &mut output[out_start..out_end],
                        0,
                        celt_end_band,
                    );
                    self.last_range = rc.rng;
                }
                if transition {
                    self.apply_transition(&mut output[..sub_output_len], sub_frame_size);
                }
                self.prev_mode = Some(OpusMode::CeltOnly);
                Ok(frame_size)
            }

            OpusMode::Hybrid => {
                let internal_sample_rate = 16000;
                let internal_frame_size =
                    (frame_duration_ms * internal_sample_rate / 1000) as usize;
                let celt_end_band = self.celt_end_band_from_toc(toc);

                // Initialised even where the rates already match: the copy
                // path still applies the resampler's input delay, and
                // `render_silk_frame` now routes every SILK frame through it.
                if internal_sample_rate != self.prev_internal_rate {
                    self.silk_resampler
                        .init(internal_sample_rate, self.sampling_rate);
                    self.silk_resampler_r
                        .init(internal_sample_rate, self.sampling_rate);
                    self.prev_internal_rate = internal_sample_rate;
                }
                self.silk_internal_rate = internal_sample_rate;
                self.reset_silk_after_celt_only();

                // Same SILK stereo/channel handling as the SilkOnly arm: true L/R
                // low band via MS->LR for stereo packets; per-packet internal
                // channel switch with side-channel/stereo-state resets.
                let silk_lr = self.setup_silk_channels(packet_channels);

                for (fi, payload) in frame_payloads.iter().enumerate() {
                    let mut rc = RangeCoder::new_decoder(payload);
                    let pcm_silk_i16_len = internal_frame_size * self.channels;
                    if pcm_silk_i16_len + 2 > self.w_pcm_i16.len() {
                        return Err(Error::InvalidPacket("opus: SILK frame size exceeds buffer"));
                    }

                    // Prepend the previous frame's last two samples (sMid) and
                    // decode at offset 2, matching libopus's samplesOut1_tmp[n][2]
                    // layout — the resampler is fed from offset 1 (the 1-sample
                    // delay line), keeping the SILK low band aligned with the CELT
                    // high band exactly as in the reference.
                    let s_mid = self.silk_s_mid;
                    let ret = {
                        let (silk_dec, pcm_i16) = (&mut self.silk_dec, &mut self.w_pcm_i16);
                        pcm_i16[0] = s_mid[0];
                        pcm_i16[1] = s_mid[1];
                        silk_dec.decode(
                            &mut rc,
                            &mut pcm_i16[2..pcm_silk_i16_len + 2],
                            silk::decode_frame::FLAG_DECODE_NORMAL,
                            true,
                            frame_duration_ms,
                            internal_sample_rate,
                        )
                    };

                    if ret < 0 {
                        return Err(Error::Internal("SILK decoding failed"));
                    }

                    let silk_out_len = sub_frame_size * self.channels;
                    self.w_silk_out[..silk_out_len].fill(0.0);
                    if ret > 0 {
                        let decoded_samples = ret as usize;
                        if decoded_samples >= 2 {
                            self.silk_s_mid[0] = self.w_pcm_i16[decoded_samples];
                            self.silk_s_mid[1] = self.w_pcm_i16[decoded_samples + 1];
                        }
                        let ratio = self.sampling_rate as f64 / internal_sample_rate as f64;
                        let out_len =
                            ((decoded_samples as f64 * ratio) as usize).min(sub_frame_size);
                        debug_assert!(out_len <= self.w_pcm_resampled.len());
                        if silk_lr {
                            // Stereo low band: L/R from dec_api (already in the
                            // 1-sample-delay layout), each through its own resampler.
                            self.silk_resampler.process(
                                &mut self.w_pcm_resampled[..out_len],
                                &self.silk_dec.l_out[..decoded_samples],
                                decoded_samples as i32,
                            );
                            for i in 0..out_len {
                                self.w_silk_out[i * 2] = self.w_pcm_resampled[i] as f32 / 32768.0;
                            }
                            self.silk_resampler_r.process(
                                &mut self.w_pcm_resampled[..out_len],
                                &self.silk_dec.r_out[..decoded_samples],
                                decoded_samples as i32,
                            );
                            for i in 0..out_len {
                                self.w_silk_out[i * 2 + 1] =
                                    self.w_pcm_resampled[i] as f32 / 32768.0;
                            }
                        } else {
                            self.silk_resampler.process(
                                &mut self.w_pcm_resampled[..out_len],
                                &self.w_pcm_i16[1..1 + decoded_samples],
                                decoded_samples as i32,
                            );
                            for i in 0..out_len {
                                let v = self.w_pcm_resampled[i] as f32 / 32768.0;
                                for ch in 0..self.channels {
                                    self.w_silk_out[i * self.channels + ch] = v;
                                }
                            }
                            // Mono packet, stereo output: keep the right-channel
                            // resampler continuous (libopus dec_API.c:351-355).
                            if self.channels == 2 {
                                self.silk_resampler_r.process(
                                    &mut self.w_pcm_resampled[..out_len],
                                    &self.w_pcm_i16[1..1 + decoded_samples],
                                    decoded_samples as i32,
                                );
                                for i in 0..out_len {
                                    self.w_silk_out[i * 2 + 1] =
                                        self.w_pcm_resampled[i] as f32 / 32768.0;
                                }
                            }
                        }
                    }

                    // --- Opus redundancy layer, hybrid form (opus_decoder.c) ---
                    // redundancy = bit(12); if set: celt_to_silk = bit(1),
                    // redundancy_bytes = uint(256)+2 taken from the END of the
                    // packet — the MAIN CELT layer still decodes, but with the
                    // range coder's storage shrunk by those bytes (this changes
                    // its raw-bit region and tell budget).
                    let plen = payload.len();
                    let mut redundancy = false;
                    let mut celt_to_silk = false;
                    let mut red_bytes = 0usize;
                    let mut effective_len = plen;
                    #[cfg(feature = "probe")]
                    {
                        self.probe_silk_bits = rc.tell();
                        self.probe_total_bits = (plen as i32) * 8;
                    }
                    if rc.tell() + 37 <= (plen as i32) * 8 {
                        redundancy = rc.decode_bit_logp(12);
                        if redundancy {
                            celt_to_silk = rc.decode_bit_logp(1);
                            red_bytes = rc.dec_uint(256) as usize + 2;
                            if red_bytes <= effective_len {
                                effective_len -= red_bytes;
                            } else {
                                red_bytes = 0;
                                redundancy = false;
                            }
                            if redundancy && (effective_len as i32) * 8 < rc.tell() {
                                effective_len = plen;
                                red_bytes = 0;
                                redundancy = false;
                            }
                            if redundancy {
                                rc.storage -= red_bytes as u32;
                            }
                        }
                    }
                    let f5 = (self.sampling_rate / 200) as usize;
                    let f2_5 = f5 / 2;
                    let red_end_band = celt_endband_for_bandwidth(bandwidth);
                    let mut red_buf = [0.0f32; 480];
                    let mut redundant_rng = 0u32;
                    let do_red = redundancy && self.sampling_rate == 48000;
                    // As in the SILK-only arm: a redundant frame replaces the
                    // mode-switch cross-fade (opus_decoder.c:532), and the
                    // concealment must run before the redundant frame or the
                    // CELT reset below disturbs the state it reads.
                    let fade_transition = transition && fi == 0 && !redundancy;
                    if fade_transition {
                        self.fill_transition(sub_frame_size, prev_celt_end_band)?;
                    }
                    // CELT->SILK: redundant frame decodes BEFORE the main CELT,
                    // continuing the prior CELT state (fade-out of previous CELT).
                    if do_red && celt_to_silk {
                        redundant_rng = self.decode_redundant_celt(
                            &payload[plen - red_bytes..],
                            false,
                            packet_channels,
                            red_end_band,
                            &mut red_buf[..f5 * self.channels],
                        );
                    }

                    // Main CELT high band. libopus opus_decoder.c:515 — reset CELT
                    // on a mode change unless primed by prior SILK->CELT redundancy.
                    if fi == 0
                        && let Some(pm) = self.prev_mode
                        && pm != OpusMode::Hybrid
                        && !self.prev_redundancy
                    {
                        self.celt_dec.reset();
                    }
                    self.celt_dec.set_stream_channels(packet_channels);
                    let total_bits = (effective_len * 8) as i32;
                    {
                        let (celt_dec, celt_out) = (&mut self.celt_dec, &mut self.w_celt_out);
                        celt_dec.decode_from_range_coder_with_band_range(
                            &mut rc,
                            total_bits,
                            sub_frame_size,
                            &mut celt_out[..silk_out_len],
                            17,
                            celt_end_band,
                        );
                    }

                    let out_start = fi * silk_out_len;
                    let total = silk_out_len.min(output.len() - out_start);
                    for j in 0..total {
                        output[out_start + j] = self.w_silk_out[j] + self.w_celt_out[j];
                    }

                    // SILK->CELT: reset + decode the redundant frame AFTER the main
                    // decode; it primes the CELT state for the upcoming CELT mode.
                    if do_red && !celt_to_silk {
                        redundant_rng = self.decode_redundant_celt(
                            &payload[plen - red_bytes..],
                            true,
                            packet_channels,
                            red_end_band,
                            &mut red_buf[..f5 * self.channels],
                        );
                    }
                    if do_red {
                        let window = celt::modes::default_mode().window;
                        let region = &mut output[out_start..out_start + silk_out_len];
                        if celt_to_silk {
                            redundancy_fade_start(region, &red_buf, f2_5, self.channels, window);
                        } else {
                            redundancy_fade_end(
                                region,
                                sub_frame_size,
                                &red_buf,
                                f2_5,
                                self.channels,
                                window,
                            );
                        }
                    }
                    if fade_transition {
                        self.apply_transition(
                            &mut output[out_start..out_start + silk_out_len],
                            sub_frame_size,
                        );
                    }
                    self.prev_redundancy = redundancy && !celt_to_silk;
                    self.last_range = rc.rng ^ redundant_rng;
                }
                self.prev_mode = Some(OpusMode::Hybrid);
                Ok(frame_size)
            }
        }
    }
}

impl OpusDecoder {
    #[inline(always)]
    fn celt_end_band_from_toc(&self, toc: u8) -> usize {
        let mode = celt::modes::default_mode();
        let top = mode.eff_ebands;
        if mode_from_toc(toc) == OpusMode::CeltOnly && toc >= 0x80 {
            const FROM_OPUS_TABLE: [u8; 16] = [
                0x80, 0x88, 0x90, 0x98, 0x40, 0x48, 0x50, 0x58, 0x20, 0x28, 0x30, 0x38, 0x00, 0x08,
                0x10, 0x18,
            ];
            let idx = ((toc >> 3) - 16) as usize;
            let data0 = FROM_OPUS_TABLE[idx] | (toc & 0x7);
            let trim = (data0 >> 5) as usize;
            return top.saturating_sub(2 * trim).max(1);
        }
        // Hybrid: libopus maps the packet bandwidth to a CELT end band
        // (opus_decoder.c: SWB -> 19, FB -> 21). Decoding SWB hybrid with 21
        // reads two bands the encoder never coded -> range desync every packet.
        if mode_from_toc(toc) == OpusMode::Hybrid
            && bandwidth_from_toc(toc) == Bandwidth::Superwideband
        {
            return 19.min(top);
        }
        top
    }

    /// Decode a redundant CELT frame (opus_decoder.c "5 ms redundant frame"):
    /// start band 0, end band from the packet bandwidth, 5 ms, its own range
    /// decoder. Returns the redundant final range; PLANAR output in `buf`
    /// (F5 samples per state channel). Only valid at 48 kHz output.
    fn decode_redundant_celt(
        &mut self,
        red: &[u8],
        reset_first: bool,
        packet_channels: usize,
        end_band: usize,
        buf: &mut [f32],
    ) -> u32 {
        if reset_first {
            self.celt_dec.reset();
        }
        self.celt_dec.set_stream_channels(packet_channels);
        let f5 = (self.sampling_rate / 200) as usize;
        let mut rrc = RangeCoder::new_decoder(red);
        let total_bits = (red.len() * 8) as i32;
        self.celt_dec
            .decode_from_range_coder_with_band_range(&mut rrc, total_bits, f5, buf, 0, end_band);
        rrc.rng
    }
}

/// First CELT band the hybrid layer codes; below it the SILK layer owns the
/// spectrum (libopus `start_band = 17`).
const HYBRID_START_BAND: usize = 17;

/// libopus opus_decoder.c bandwidth -> CELT end band for the packet.
/// smooth_fade cross-fades (w = window[i]^2, 48 kHz inc=1) applied to the
/// interleaved output region of one frame. `red` is interleaved too.
/// celt_to_silk: redundant frame occupies the START of the frame — first 2.5 ms
/// copied verbatim, next 2.5 ms fades redundant -> main.
///
/// Indexing invariant: `out.len() >= f5 * channels` (writes reach sample
/// f5-1 = 2*f2_5-1). A malformed multi-frame packet used to violate this (a
/// hostile frame count made the per-frame region tinier than F5, fuzzer-found
/// OOB panics here); decode() now rejects such packets up front exactly as C
/// libopus does (opus_decode_native's count*packet_frame_size > frame_size ->
/// OPUS_BUFFER_TOO_SMALL, and the 120 ms cap of opus_packet_parse_impl), so a
/// redundant frame always has >= 10 ms of frame to fade into, as in C.
fn redundancy_fade_start(
    out: &mut [f32],
    red: &[f32],
    f2_5: usize,
    channels: usize,
    window: &[f32],
) {
    out[..f2_5 * channels].copy_from_slice(&red[..f2_5 * channels]);
    for i in 0..f2_5 {
        let w = window[i] * window[i];
        for c in 0..channels {
            let idx = (f2_5 + i) * channels + c;
            out[idx] = (1.0 - w) * red[idx] + w * out[idx];
        }
    }
}

/// SILK->CELT: redundant frame occupies the END of the frame — the last 2.5 ms
/// fades main -> redundant (second half of the redundant frame).
///
/// Indexing invariant: `frame_samples >= f2_5` and `out.len() >=
/// frame_samples * channels` (the index `frame_samples - f2_5 + i` would
/// otherwise underflow). A malformed multi-frame packet used to violate this
/// (fuzzer-found subtract-with-overflow panic here); decode() now rejects such
/// packets up front exactly as C libopus does (opus_decode_native's
/// count*packet_frame_size > frame_size -> OPUS_BUFFER_TOO_SMALL, plus the
/// 120 ms cap of opus_packet_parse_impl), so redundancy only ever runs on
/// frames of >= 10 ms, as in C.
fn redundancy_fade_end(
    out: &mut [f32],
    frame_samples: usize,
    red: &[f32],
    f2_5: usize,
    channels: usize,
    window: &[f32],
) {
    for i in 0..f2_5 {
        let w = window[i] * window[i];
        for c in 0..channels {
            let idx = (frame_samples - f2_5 + i) * channels + c;
            out[idx] = (1.0 - w) * out[idx] + w * red[(f2_5 + i) * channels + c];
        }
    }
}

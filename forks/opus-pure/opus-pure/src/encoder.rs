//! The Opus encoder.

use crate::analysis;
use crate::celt::{self, CeltEncoder};
use crate::config::{Application, Bandwidth, OpusMode, RateControl, Signal};
use crate::hp_cutoff::hp_cutoff;
use crate::range_coder::RangeCoder;
use crate::silk::{
    self, control_codec::silk_control_encoder, enc_api::silk_encode,
    init_encoder::silk_init_encoder, macros::*, structs::SilkEncoder,
};
use crate::soft_clip::i16_to_float;
use crate::toc::{celt_endband_for_bandwidth, frame_rate_from_params, gen_toc};
use crate::{Error, Result};

/// An Opus encoder: PCM in, Opus packets out.
///
/// One encoder handles one stream, and carries state between packets, so the
/// same instance has to be fed the whole stream in order. The settings below
/// are public fields rather than setters, and every one of them may be changed
/// between packets; only the sample rate, channel count and
/// [`Application`] are fixed at construction.
///
/// The encoder decides per packet which of the three Opus layers to use, what
/// audio [`Bandwidth`] to code, and how many bits to spend. Those decisions are
/// what [`bitrate_bps`](Self::bitrate_bps), [`complexity`](Self::complexity)
/// and the rest steer.
///
/// ```
/// use opus_pure::{Application, OpusEncoder};
///
/// let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
/// encoder.bitrate_bps = 96_000;
///
/// let pcm = vec![0.0f32; 960 * 2];        // 20 ms of stereo
/// let mut packet = vec![0u8; 4000];
/// let n = encoder.encode(&pcm, 960, &mut packet)?;
/// assert!(n > 0);
/// # Ok::<(), opus_pure::Error>(())
/// ```
pub struct OpusEncoder {
    celt_enc: CeltEncoder,
    silk_enc: Box<SilkEncoder>,
    application: Application,
    sampling_rate: i32,
    channels: usize,
    bandwidth: Bandwidth,
    /// Target bitrate in bits per second, across all channels. Default 64000.
    ///
    /// This is a target rather than a cap: the default is variable-rate, so an
    /// individual packet is as large as its content needs and the rate is met
    /// on average. See [`rate_control`](Self::rate_control) to make it a per-packet size
    /// instead.
    ///
    /// It is also the single strongest input to the encoder's own decisions.
    /// Coding mode and audio bandwidth are both chosen from it, so lowering it
    /// does not simply degrade the same signal: below roughly 20 kb/s the
    /// encoder moves to SILK and narrows the bandwidth, because spending the
    /// remaining bits on a smaller spectrum sounds better than spreading them
    /// over all of it.
    pub bitrate_bps: i32,
    /// How much CPU the encoder may spend, from 0 to 10. Default 9.
    ///
    /// Lower settings take shortcuts in pitch analysis and quantisation, and
    /// below 7 the content analysis is skipped entirely, which is what
    /// otherwise informs the speech/music decision. It is not purely a speed
    /// control: the encoder scales its own idea of the bitrate by
    /// `(90 + complexity) / 100` when choosing a mode and bandwidth, so a lower
    /// complexity also codes a narrower band at the same rate.
    pub complexity: i32,
    /// How much the size of each packet may vary. Default
    /// [`ConstrainedVbr`](RateControl::ConstrainedVbr), which is libopus's.
    ///
    /// [`Cbr`](RateControl::Cbr) pads every packet to the same size, which is
    /// what a fixed-capacity channel wants and what a file does not: it spends
    /// bits on silence that VBR would have given to the difficult passages. It
    /// also costs quality at a given rate, and the encoder accounts for that by
    /// discounting its working bitrate by a twelfth when deciding mode and
    /// bandwidth.
    ///
    /// Note this is the opposite polarity from libopus's `OPUS_SET_VBR`: the
    /// default here is named for what it does rather than for what it is not.
    pub rate_control: RateControl,

    /// Code a low-bitrate copy of the *previous* frame into each packet, so one
    /// lost packet can be partly recovered from the next. Default `false`.
    ///
    /// The redundant copy costs bits that would otherwise go to the current
    /// frame, so this is only worth enabling when loss is actually expected,
    /// and the encoder decides per packet whether to spend them, using
    /// [`packet_loss_perc`](Self::packet_loss_perc) as its estimate of how
    /// likely that is. FEC needs SILK, so it has no effect on packets the
    /// encoder codes as CELT.
    ///
    /// A decoder recovers the copy with
    /// [`OpusDecoder::decode_fec`](crate::OpusDecoder::decode_fec), which must
    /// be called on the packet *after* the missing one.
    pub use_inband_fec: bool,

    /// Discontinuous transmission: after enough consecutive inactive frames,
    /// emit a 1-byte (TOC-only) packet so the decoder runs comfort-noise/PLC.
    pub use_dtx: bool,
    /// Consecutive inactive milliseconds, in Q1 (opus_encoder.c nb_no_activity).
    nb_no_activity_ms_q1: i32,
    /// Final range-coder state of the last packet (0 for DTX/PLC packets, which
    /// carry no coded range — opus_encoder.c st->rangeFinal).
    range_final: u32,

    /// Expected packet loss, 0 to 100 percent. Default 0.
    ///
    /// This is what tells the encoder how defensively to code. A non-zero value
    /// makes the quantiser less reliant on inter-frame prediction, so a lost
    /// packet corrupts less of what follows, and it is the input
    /// [`use_inband_fec`](Self::use_inband_fec) uses to decide whether a
    /// redundant copy is worth its bits. Both cost quality on the packets that
    /// do arrive, so an estimate far above the real loss rate is not a safe
    /// default.
    pub packet_loss_perc: i32,
    /// Whether the current packet codes in-band FEC. Decided per packet by
    /// [`decide_fec`], which needs the previous answer for its hysteresis.
    lbrr_coded: bool,
    silk_initialized: bool,
    prev_enc_mode: Option<OpusMode>,

    variable_hp_smth2_q15: i32,
    /// Rate-dependent automatic bandwidth (libopus auto_bandwidth), stored as the
    /// Bandwidth discriminant (1101 NB .. 1105 FB). Hysteresis state.
    auto_bandwidth: i32,
    first_frame: bool,
    /// Overrides automatic bandwidth selection when set (OPUS_SET_BANDWIDTH).
    pub force_bandwidth: Option<Bandwidth>,
    /// OPUS_SET_SIGNAL: force the voice/music bias (None = auto from analysis).
    pub signal_type: Option<Signal>,
    /// OPUS_SET_MAX_BANDWIDTH: cap the automatically-selected bandwidth.
    pub max_bandwidth: Bandwidth,
    /// Tonality/music/bandwidth analysis (libopus src/analysis.c); runs when
    /// complexity >= 7 and the API rate is >= 16 kHz.
    /// Bit depth in force for the packet being coded: the lesser of what the
    /// entry point implies and what the caller asked for, which is libopus's
    /// `lsb_depth = IMIN(lsb_depth, st->lsb_depth)` at the top of
    /// `opus_encode_native`. It is per-call rather than a setting because the
    /// float and 16-bit entry points imply different depths, so the two cannot
    /// share one stored value.
    coded_lsb_depth: i32,
    tonality: analysis::TonalityAnalysisState,
    analysis_kfft: Option<celt::kiss_fft::KissFftState>,
    /// Input bit depth assumed by the analysis noise floors. The float API
    /// default is 24; set 16 for s16-sourced content (opus_demo parity).
    pub lsb_depth: i32,
    /// 0..100 voice probability from the analysis (-1 = unknown), C voice_ratio.
    voice_ratio: i32,
    detected_bandwidth: i32,
    hp_mem: Vec<i32>,

    /// Holds the converted input for [`OpusEncoder::encode_s16`], so a caller
    /// feeding integer PCM does not pay an allocation per packet.
    buf_from_s16: Vec<f32>,

    buf_filtered: Vec<i16>,
    buf_silk_input: Vec<i16>,
    buf_stereo_mid: Vec<i16>,
    buf_stereo_side: Vec<i16>,
    buf_celt_input: Vec<f32>,
    down_fir_l: Option<silk::resampler::SilkEncoderResampler>,
    down_fir_r: Option<silk::resampler::SilkEncoderResampler>,
    /// Last 10 ms of API-rate mono input, for the SILK prefill after a
    /// CELT-only -> SILK/hybrid transition (opus_encoder.c:1449 prefill=1).
    silk_prefill_tail: Vec<i16>,
    silk_prefill_pending: bool,
    buf_left: Vec<i16>,
    buf_right: Vec<i16>,
    /// Last 2.5 ms of input before the next CELT frame begins (planar), for the
    /// CELT prefill after a mode-transition reset (opus_encoder.c:2060).
    celt_prefill_tail: Vec<f32>,
    /// Input the CELT layer has not consumed yet: the most recent
    /// [`celt_delay_samples`] per channel, planar, oldest first.
    ///
    /// This is libopus's `delay_buffer` narrowed to what CELT actually reads
    /// from it. It advances on *every* frame, SILK-only ones included, because
    /// it is a position on the input timeline rather than a coder state; letting
    /// it stall through a SILK run would step CELT forward by that much when the
    /// mode came back.
    celt_delay: Vec<f32>,
    /// The spare half of the CELT delay line's double buffer. Swapped with
    /// `celt_delay` each frame so neither allocation is dropped.
    celt_delay_next: Vec<f32>,

    rc: RangeCoder<'static>,
}

/// Input bit depth the analysis noise floors assume (opus_encoder.c float-API
/// default). The floor is `(5.7e-4 / 2^(lsb_depth-8))^2`, so s16-sourced material
/// wants 16, which is what [`OpusEncoder::encode_s16`] uses.
const DEFAULT_LSB_DEPTH: i32 = 24;

/// Precision the float entry point declares its input to have (libopus
/// `MAX_ENCODING_DEPTH`). The 16-bit entry point declares 16 instead.
pub(crate) const MAX_ENCODING_DEPTH: i32 = 24;

/// Force `mode` to one that can code a packet of `packet_rate` packets per
/// second at all.
///
/// The mode decision looks at bitrate, bandwidth and the content analysis, none
/// of which know the caller's frame size, so it can pick a mode RFC 6716 §3.1
/// has no configuration for at that duration. Only the two shortest durations
/// need forcing: 2.5 and 5 ms exist for CELT alone, and no amount of splitting
/// produces something SILK can code. Every duration longer than 20 ms that a
/// mode cannot code as one frame is reached by [`PacketDuration::layout`]
/// splitting the packet into frames the mode *can* code, which is what libopus
/// does (`opus_encoder.c` `enc_frame_size`).
fn coerce_mode_for_packet_rate(mode: OpusMode, packet_rate: i32) -> OpusMode {
    match packet_rate {
        400 | 200 => OpusMode::CeltOnly,
        _ => mode,
    }
}

/// One of the nine packet durations RFC 6716 admits, in tenths of a millisecond
/// so that 2.5 ms is an integer.
///
/// A duration is not a frame size: 80, 100 and 120 ms have no single-frame
/// configuration at all, and 40 and 60 ms have one only for SILK. What a
/// duration becomes on the wire is [`PacketDuration::layout`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PacketDuration(i32);

impl PacketDuration {
    /// Recognise `frame_size` samples per channel as a packet duration, or
    /// reject it.
    ///
    /// The relations are written as exact integer products, the way
    /// `opus_encoder.c` writes them, so that the durations no sample rate
    /// divides evenly (60 and 120 ms at every rate, 100 ms at none) need no
    /// special case.
    fn classify(sampling_rate: i32, frame_size: usize) -> Option<Self> {
        let fs = i32::try_from(frame_size).ok()?;
        if fs <= 0 {
            return None;
        }
        let tenths_ms = if 400 * fs == sampling_rate {
            25
        } else if 200 * fs == sampling_rate {
            50
        } else if 100 * fs == sampling_rate {
            100
        } else if 50 * fs == sampling_rate {
            200
        } else if 25 * fs == sampling_rate {
            400
        } else if 50 * fs == 3 * sampling_rate {
            600
        } else if 25 * fs == 2 * sampling_rate {
            800
        } else if 10 * fs == sampling_rate {
            1000
        } else if 25 * fs == 3 * sampling_rate {
            1200
        } else {
            return None;
        };
        Some(PacketDuration(tenths_ms))
    }

    /// How this duration is laid out as coded frames when the packet is coded in
    /// `mode` (`opus_encoder.c:1698`).
    ///
    /// Only SILK has configurations past 20 ms, and the CELT transform has no
    /// frame longer than 20 ms outright, so anything else becomes several frames
    /// sharing one TOC (RFC 6716 §3.2).
    fn layout(self, sampling_rate: i32, mode: OpusMode) -> PacketLayout {
        let ms20 = (sampling_rate / 50) as usize;
        let ms40 = (sampling_rate / 25) as usize;
        let ms60 = (3 * sampling_rate / 50) as usize;

        let frame_size = (sampling_rate as i64 * self.0 as i64 / 10_000) as usize;

        let silk = mode == OpusMode::SilkOnly;
        let enc_frame_size = match self.0 {
            // 20 ms and shorter is one frame, whatever the mode.
            d if d <= 200 => frame_size,
            // SILK keeps its long frames whole: 40 and 60 ms as themselves,
            // 80 ms as two 40s and 120 ms as two 60s.
            400 | 800 if silk => ms40,
            600 | 1200 if silk => ms60,
            // Everything else is 20 ms frames, including 100 ms in every mode,
            // because no legal duration halves it.
            _ => ms20,
        };

        PacketLayout {
            enc_frame_size,
            nb_frames: frame_size / enc_frame_size,
            frame_rate: frame_rate_from_params(sampling_rate, enc_frame_size)
                .expect("every frame size `layout` produces is a coded frame duration"),
        }
    }
}

/// How one `encode` call's audio is laid out as coded frames in one packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PacketLayout {
    /// Samples per channel in each coded frame.
    enc_frame_size: usize,
    /// Frames in the packet. 1 means the frame is coded straight into the
    /// caller's buffer, with no repacketizing.
    nb_frames: usize,
    /// Frames per second of `enc_frame_size`, as the TOC codes it.
    frame_rate: i32,
}

/// Worst-case bytes the framing of an `nb_frames`-frame packet can cost
/// (`opus_encoder.c` `max_header_bytes`): a code 2 packet for two frames, and a
/// code 3 VBR packet, whose per-frame length fields run to two bytes, above that.
fn max_header_bytes(nb_frames: usize) -> usize {
    if nb_frames == 2 {
        3
    } else {
        2 + (nb_frames - 1) * 2
    }
}

/// Whether the CELT layer can code a frame of `frame_size` samples per channel.
///
/// The CELT layer always runs the 48 kHz mode regardless of the API rate, so the
/// only frame sizes it can transform are `SHORT_MDCT_SIZE << lm` for
/// `lm` in `0..=max_lm` — 120, 240, 480 and 960 samples. Frame sizes the Opus API
/// otherwise allows (2.5 ms at 12 kHz is 30 samples, 5 ms at 24 kHz is 120 but
/// 2.5 ms is 60) have no matching `lm`.
/// The widest bandwidth an input at `sampling_rate` can actually carry: coding
/// above the input's Nyquist rate asks the encoder for bands that hold no
/// signal. Mirrors the final clamp in `opus_encoder.c`.
fn bandwidth_from_i32(v: i32) -> Bandwidth {
    match v {
        x if x == Bandwidth::Narrowband as i32 => Bandwidth::Narrowband,
        x if x == Bandwidth::Mediumband as i32 => Bandwidth::Mediumband,
        x if x == Bandwidth::Superwideband as i32 => Bandwidth::Superwideband,
        x if x == Bandwidth::Fullband as i32 => Bandwidth::Fullband,
        _ => Bandwidth::Wideband,
    }
}

fn clamp_bandwidth_to_rate(bw: Bandwidth, sampling_rate: i32) -> Bandwidth {
    let max = match sampling_rate {
        r if r <= 8000 => Bandwidth::Narrowband,
        r if r <= 12000 => Bandwidth::Mediumband,
        r if r <= 16000 => Bandwidth::Wideband,
        r if r <= 24000 => Bandwidth::Superwideband,
        _ => Bandwidth::Fullband,
    };
    if (bw as i32) > (max as i32) { max } else { bw }
}

fn celt_can_code_frame(frame_size_48k: usize) -> bool {
    let blocks = frame_size_48k / celt::modes::SHORT_MDCT_SIZE;
    frame_size_48k.is_multiple_of(celt::modes::SHORT_MDCT_SIZE)
        && blocks.is_power_of_two()
        && blocks <= 1 << celt::modes::MAX_LM
}

/// 48 kHz samples per input sample at `sampling_rate` (libopus
/// `resampling_factor`). The CELT layer only has the 48 kHz mode, so a lower
/// API rate is coded by zero-stuffing up to 48 kHz.
fn celt_upsample(sampling_rate: i32) -> usize {
    (48_000 / sampling_rate) as usize
}

/// Input samples per channel used to prefill a freshly-reset CELT encoder: one
/// short MDCT block, 2.5 ms, expressed at the API rate. Scaled by
/// [`celt_upsample`] it is always exactly `SHORT_MDCT_SIZE` at 48 kHz, which is
/// the only frame size a fresh CELT encoder can transform.
fn celt_prefill_samples(sampling_rate: i32) -> usize {
    celt::modes::SHORT_MDCT_SIZE / celt_upsample(sampling_rate)
}

/// Samples the CELT layer's input trails the caller's, `st->delay_compensation`
/// in libopus (`opus_encoder.c:313`).
///
/// CELT has a shorter algorithmic delay than SILK, so an encoder that fed both
/// the same samples would emit a stream whose timeline jumps by the difference
/// every time the mode changes. libopus closes that by handing CELT input that
/// lags by 4 ms: it builds `pcm_buf` as this much history followed by the new
/// frame, gives SILK the new frame (`:2211`) and CELT the buffer from the start
/// (`:2493`). The two layers then line up at the decoder, and the constant total
/// delay is what [`crate::ogg::OpusHead::RECOMMENDED_PRE_SKIP`] counts.
///
/// Zero for [`Application::RestrictedLowDelay`], which trades the mode switch
/// away for the lower delay (`opus_encoder.c:1904`).
fn celt_delay_samples(sampling_rate: i32, application: Application) -> usize {
    match application {
        Application::RestrictedLowDelay => 0,
        _ => (sampling_rate / 250) as usize,
    }
}

/// The largest packet [`OpusEncoder::encode`] can produce, and therefore the
/// output buffer size that never costs you bitrate.
///
/// This has to be exact rather than merely generous. [`OpusEncoder::encode`]
/// takes the output slice's length as the packet's byte budget, the way libopus
/// takes `max_data_bytes`, so a buffer smaller than this does not fail — it
/// quietly codes a smaller packet, and the stream comes out under the rate you
/// asked for.
///
/// RFC 6716 §3.4 caps a coded frame at 1275 bytes. The longest duration one call
/// can ask for is 120 ms, which the encoder lays out as six 20 ms frames sharing
/// a TOC byte, a frame-count byte and a two-byte length for all but the last.
///
/// ```
/// # use opus_pure::{Application, OpusEncoder, MAX_PACKET_BYTES};
/// let mut packet = vec![0u8; MAX_PACKET_BYTES];
/// # let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
/// # let n = encoder.encode(&vec![0.0; 960 * 2], 960, &mut packet)?;
/// # Ok::<(), opus_pure::Error>(())
/// ```
pub const MAX_PACKET_BYTES: usize = 6 * 1275 + 2 + 5 * 2;

/// Lowest bitrate the encoder will act on, matching libopus's `OPUS_SET_BITRATE`
/// floor. Anything positive below it is raised to it.
const MIN_BITRATE_BPS: i32 = 500;

/// Highest bitrate per channel, matching libopus's `OPUS_SET_BITRATE` ceiling.
/// Anything above `MAX_BITRATE_BPS_PER_CHANNEL * channels` is lowered to it.
const MAX_BITRATE_BPS_PER_CHANNEL: i32 = 300_000;

/// Largest packet that can hold its payload as a single unframed Opus frame.
///
/// RFC 6716 §3.4 caps one frame at 1275 bytes, so a code 0 packet tops out at
/// 1276 including the TOC. libopus applies the same bound in
/// `opus_encode_frame_native` (`max_data_bytes = IMIN(orig_max_data_bytes, 1276)`).
const MAX_ONE_FRAME_PACKET: usize = 1276;

/// Write `frame` into `output` as a complete one-frame packet of `target_total`
/// bytes, returning how many bytes were written.
///
/// A CBR target can be larger than a frame is allowed to be: 60 ms stereo above
/// roughly 170 kbps asks for more than 1275 bytes. The surplus becomes code 3
/// padding rather than an over-long frame, which is how libopus reconciles its
/// packet target with the frame limit (`opus_packet_pad` at the end of
/// `opus_encode_frame_native`).
fn emit_one_frame_packet(output: &mut [u8], toc: u8, frame: &[u8], target_total: usize) -> usize {
    let target_total = target_total.min(output.len());

    // The frame already fills the target: plain code 0, no framing overhead.
    if frame.len() + 1 >= target_total {
        output[0] = toc;
        let copy_len = frame.len().min(target_total - 1);
        output[1..1 + copy_len].copy_from_slice(&frame[..copy_len]);
        return copy_len + 1;
    }

    // Code 3, CBR, one frame. The frame-count byte absorbs the single spare byte.
    output[0] = toc | 0x03;
    if frame.len() + 2 >= target_total {
        output[1] = 0x01;
        output[2..2 + frame.len()].copy_from_slice(frame);
        return target_total;
    }

    // Code 3 with padding. `pad_amount` counts the length bytes themselves: a
    // 255 stands for 254 further padding bytes and demands another length byte,
    // so `nb_255s * 255 + 1 + last` bytes are accounted for (RFC 6716 §3.2.5).
    // The padding data itself goes after the frame, at the end of the packet.
    output[1] = 0x41;
    let pad_amount = target_total - frame.len() - 2;
    let nb_255s = (pad_amount - 1) / 255;
    let mut ptr = 2;
    for _ in 0..nb_255s {
        output[ptr] = 255;
        ptr += 1;
    }
    output[ptr] = (pad_amount - 255 * nb_255s - 1) as u8;
    ptr += 1;

    output[ptr..ptr + frame.len()].copy_from_slice(frame);
    ptr += frame.len();
    output[ptr..target_total].fill(0);

    target_total
}

// libopus opus_encoder.c bandwidth thresholds: (threshold, hysteresis) pairs for
// NB<->MB, MB<->WB, WB<->SWB, SWB<->FB, interpolated voice<->music by voice_est^2.
const MONO_VOICE_BANDWIDTH_THRESHOLDS: [i32; 8] = [9000, 700, 9000, 700, 13500, 1000, 14000, 2000];
const MONO_MUSIC_BANDWIDTH_THRESHOLDS: [i32; 8] = [9000, 700, 9000, 700, 11000, 1000, 12000, 2000];
const STEREO_VOICE_BANDWIDTH_THRESHOLDS: [i32; 8] =
    [9000, 700, 9000, 700, 13500, 1000, 14000, 2000];
const STEREO_MUSIC_BANDWIDTH_THRESHOLDS: [i32; 8] =
    [9000, 700, 9000, 700, 11000, 1000, 12000, 2000];

/// Port of libopus `decide_fec` (src/opus_encoder.c).
///
/// In-band FEC is not free: the redundant copy has to come out of the same
/// budget as the primary stream, so below a bitrate threshold enabling it costs
/// more quality than the loss it insures against. libopus keeps a per-bandwidth
/// threshold with hysteresis, scales it down as the reported loss rises (at high
/// loss FEC is worth more), and above 5% loss will narrow the coded bandwidth to
/// find room rather than give FEC up — which is why `bandwidth` is in/out.
///
/// Returns whether LBRR should be coded.
fn decide_fec(
    use_inband_fec: bool,
    packet_loss_perc: i32,
    last_fec: bool,
    mode: OpusMode,
    bandwidth: &mut i32,
    rate: i32,
) -> bool {
    /// `(threshold_bps, hysteresis_bps)` per bandwidth, narrowband first.
    const FEC_THRESHOLDS: [(i32, i32); 5] = [
        (12000, 1000), // NB
        (14000, 1000), // MB
        (16000, 1000), // WB
        (20000, 1000), // SWB
        (22000, 1000), // FB
    ];
    if !use_inband_fec || packet_loss_perc == 0 || mode == OpusMode::CeltOnly {
        return false;
    }
    let nb = Bandwidth::Narrowband as i32;
    let orig_bandwidth = *bandwidth;
    loop {
        let idx = ((*bandwidth - nb) as usize).min(FEC_THRESHOLDS.len() - 1);
        let (thres, hysteresis) = FEC_THRESHOLDS[idx];
        let mut lbrr_rate_thres_bps = if last_fec {
            thres - hysteresis
        } else {
            thres + hysteresis
        };
        lbrr_rate_thres_bps =
            silk_smulwb(lbrr_rate_thres_bps * (125 - packet_loss_perc.min(25)), 655);
        if rate > lbrr_rate_thres_bps {
            return true;
        } else if packet_loss_perc <= 5 {
            return false;
        } else if *bandwidth > nb {
            *bandwidth -= 1;
        } else {
            break;
        }
    }
    // No bandwidth left that makes FEC affordable; keep what was asked for.
    *bandwidth = orig_bandwidth;
    false
}

fn compute_equiv_rate(
    bitrate: i32,
    channels: usize,
    frame_rate: i32,
    vbr: bool,
    complexity: i32,
    loss: i32,
) -> i32 {
    let mut equiv = bitrate;
    if frame_rate > 50 {
        equiv -= (40 * channels as i32 + 20) * (frame_rate - 50);
    }
    if !vbr {
        equiv -= equiv / 12;
    }
    equiv = equiv * (90 + complexity) / 100;
    if loss > 0 {
        equiv -= equiv * loss / (12 * loss + 20);
    }
    equiv
}

fn compute_mode_threshold(
    application: Application,
    channels: usize,
    prev_was_celt: bool,
    has_prev_mode: bool,
    voice_est: i32,
) -> i32 {
    let mode_voice = if channels == 1 { 64000 } else { 44000 };
    let mode_music = 10000;

    let diff = mode_voice - mode_music;
    let offset = (voice_est * voice_est * diff) >> 14;
    let mut threshold = mode_music + offset;

    if application == Application::Voip {
        threshold += 8000;
    }

    if has_prev_mode {
        if prev_was_celt {
            threshold -= 4000;
        } else {
            threshold += 4000;
        }
    }

    if application == Application::RestrictedLowDelay {
        threshold = 0;
    }

    threshold
}

/// `celt.h`. The factor of six carries the one frame duration whose rate is not
/// an integer: at 60 ms there are 16.67 frames per second, and `6*Fs/frame_size`
/// keeps that exact where a plain division would not.
fn bits_to_bitrate(bits: i32, fs: i32, frame_size: i32) -> i32 {
    ((bits as i64 * (6 * fs / frame_size) as i64) / 6) as i32
}

fn bitrate_to_bits(bitrate: i32, fs: i32, frame_size: i32) -> i32 {
    ((bitrate as i64 * 6) / (6 * fs / frame_size) as i64) as i32
}

/// The SILK share of a hybrid packet's rate (`opus_encoder.c`).
///
/// The allocation is per channel: the total is divided down, the table is read
/// at the single-channel rate, and the result is scaled back up. Reading the
/// table at the *total* rate lands in a higher row and hands SILK far more than
/// its share of a stereo packet.
fn compute_silk_rate_for_hybrid(
    rate_bps: i32,
    bandwidth: Bandwidth,
    frame20ms: bool,
    vbr: bool,
    fec: bool,
    channels: usize,
) -> i32 {
    // total, then the SILK share at (10 ms, 20 ms) without FEC and with it.
    // FEC costs SILK real bits, so it is given a wider share to spend.
    #[rustfmt::skip]
    const RATE_TABLE: &[(i32, i32, i32, i32, i32)] = &[
        (    0,     0,     0,     0,     0),
        (12000, 10000, 10000, 11000, 11000),
        (16000, 13500, 13500, 15000, 15000),
        (20000, 16000, 16000, 18000, 18000),
        (24000, 18000, 18000, 21000, 21000),
        (32000, 22000, 22000, 28000, 28000),
        (64000, 38000, 38000, 50000, 50000),
    ];
    let share = |row: &(i32, i32, i32, i32, i32)| match (fec, frame20ms) {
        (false, false) => row.1,
        (false, true) => row.2,
        (true, false) => row.3,
        (true, true) => row.4,
    };

    // Per channel, and scaled back up at the end.
    let rate_bps = rate_bps / channels as i32;
    let n = RATE_TABLE.len();
    let mut i = 1;
    while i < n && RATE_TABLE[i].0 <= rate_bps {
        i += 1;
    }
    let mut silk_rate = if i == n {
        let last = &RATE_TABLE[n - 1];
        // Above the table, SILK takes half of whatever the rate adds.
        share(last) + (rate_bps - last.0) / 2
    } else {
        let (lo_row, hi_row) = (&RATE_TABLE[i - 1], &RATE_TABLE[i]);
        let (x0, x1) = (lo_row.0, hi_row.0);
        (share(lo_row) * (x1 - rate_bps) + share(hi_row) * (rate_bps - x0)) / (x1 - x0)
    };
    // C tail adjustments (opus_encoder.c:789): tiny SILK boost for CBR, and
    // +300 for SWB hybrid (the CELT part starts at band 17 either way but
    // covers less spectrum, so SILK earns a bigger share).
    if !vbr {
        silk_rate += 100;
    }
    if bandwidth == Bandwidth::Superwideband {
        silk_rate += 300;
    }
    silk_rate *= channels as i32;
    // Small stereo adjustment, calibrated in the reference at 32 kb/s. `rate_bps`
    // is the per-channel rate by this point, as it is in the C.
    if channels == 2 && rate_bps >= 12000 {
        silk_rate -= 1000;
    }
    silk_rate
}

/// Shows the encoder's configuration and omits its coding state.
///
/// A derived `Debug` here would print every filter history and analysis buffer
/// the encoder carries, which is tens of kilobytes of numbers that mean nothing
/// without the codec beside them. What a caller wants from `dbg!` is the
/// settings, so that is what this prints; the `..` stands for the rest.
impl std::fmt::Debug for OpusEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpusEncoder")
            .field("sampling_rate", &self.sampling_rate)
            .field("channels", &self.channels)
            .field("application", &self.application)
            .field("bitrate_bps", &self.bitrate_bps)
            .field("complexity", &self.complexity)
            .field("rate_control", &self.rate_control)
            .field("use_inband_fec", &self.use_inband_fec)
            .field("use_dtx", &self.use_dtx)
            .field("packet_loss_perc", &self.packet_loss_perc)
            .field("force_bandwidth", &self.force_bandwidth)
            .field("max_bandwidth", &self.max_bandwidth)
            .field("signal_type", &self.signal_type)
            .field("lsb_depth", &self.lsb_depth)
            .finish_non_exhaustive()
    }
}

impl OpusEncoder {
    /// Create an encoder for `sampling_rate` Hz and `channels` channels.
    ///
    /// The rate must be one of 8000, 12000, 16000, 24000 or 48000, and the
    /// channel count 1 or 2; anything else is
    /// [`Error::InvalidArgument`]. These three
    /// arguments are the only settings fixed for the encoder's life. See
    /// [`Application`] for which one to pass, and the fields on this type for
    /// everything that can be changed afterwards.
    ///
    /// The rate is the rate of the PCM handed to [`encode`](Self::encode), not
    /// a property of the packets produced: Opus always codes internally at one
    /// of its own rates and every packet's duration is counted at 48 kHz
    /// regardless. Passing 48000 avoids a resampling step on the way in.
    ///
    /// For more than two channels, see
    /// [`OpusMSEncoder`](crate::OpusMSEncoder).
    pub fn new(sampling_rate: i32, channels: usize, application: Application) -> Result<Self> {
        if ![8000, 12000, 16000, 24000, 48000].contains(&sampling_rate) {
            return Err(Error::InvalidArgument("Invalid sampling rate"));
        }
        if ![1, 2].contains(&channels) {
            return Err(Error::InvalidArgument("Invalid number of channels"));
        }

        let mode = celt::modes::default_mode();
        let mut celt_enc = CeltEncoder::new(mode, channels);
        celt_enc.set_upsample(celt_upsample(sampling_rate));

        let mut silk_enc = Box::new(SilkEncoder::default());
        if silk_init_encoder(&mut silk_enc.state[0], 0) != 0 {
            return Err(Error::Internal("SILK encoder initialization failed"));
        }

        // Only the starting bandwidth is kept: `encode()` re-derives the coding
        // mode from bitrate, frame size and signal on every call, so an initial
        // mode here would never be read.
        let bw = match application {
            Application::Voip => match sampling_rate {
                8000 => Bandwidth::Narrowband,
                12000 => Bandwidth::Mediumband,
                16000 => Bandwidth::Wideband,
                24000 => Bandwidth::Superwideband,
                48000 => Bandwidth::Fullband,
                _ => Bandwidth::Narrowband,
            },
            Application::RestrictedLowDelay => match sampling_rate {
                8000 => Bandwidth::Narrowband,
                12000 => Bandwidth::Mediumband,
                16000 => Bandwidth::Wideband,
                24000 => Bandwidth::Superwideband,
                _ => Bandwidth::Fullband,
            },
            Application::Audio => {
                if sampling_rate <= 16000 {
                    match sampling_rate {
                        8000 => Bandwidth::Narrowband,
                        12000 => Bandwidth::Mediumband,
                        _ => Bandwidth::Wideband,
                    }
                } else {
                    match sampling_rate {
                        24000 => Bandwidth::Superwideband,
                        _ => Bandwidth::Fullband,
                    }
                }
            }
        };

        let variable_hp_smth2_q15 = silk_lin2log(60) << 8;

        Ok(Self {
            celt_enc,
            silk_enc,
            application,
            sampling_rate,
            channels,
            bandwidth: bw,
            bitrate_bps: 64000,
            complexity: 9,
            rate_control: RateControl::ConstrainedVbr,
            use_inband_fec: false,
            use_dtx: false,
            nb_no_activity_ms_q1: 0,
            range_final: 0,
            packet_loss_perc: 0,
            lbrr_coded: false,
            silk_initialized: false,
            prev_enc_mode: None,
            variable_hp_smth2_q15,
            auto_bandwidth: 0,
            first_frame: true,
            force_bandwidth: None,
            signal_type: None,
            max_bandwidth: Bandwidth::Fullband,
            tonality: analysis::TonalityAnalysisState::new(sampling_rate),
            analysis_kfft: celt::kiss_fft::KissFftState::new(480),
            lsb_depth: DEFAULT_LSB_DEPTH,
            coded_lsb_depth: DEFAULT_LSB_DEPTH,
            voice_ratio: -1,
            detected_bandwidth: 0,
            hp_mem: vec![0; channels * 2],

            buf_from_s16: Vec::new(),
            buf_filtered: Vec::new(),
            buf_silk_input: Vec::new(),
            buf_stereo_mid: Vec::new(),
            buf_stereo_side: Vec::new(),
            buf_celt_input: Vec::new(),
            down_fir_l: None,
            down_fir_r: None,
            silk_prefill_tail: Vec::new(),
            silk_prefill_pending: false,
            buf_left: Vec::new(),
            buf_right: Vec::new(),
            celt_prefill_tail: Vec::new(),
            celt_delay: vec![0.0; celt_delay_samples(sampling_rate, application) * channels],
            celt_delay_next: Vec::new(),
            rc: RangeCoder::new_encoder(1),
        })
    }

    /// Final range-coder state of the last encoded packet (libopus
    /// OPUS_GET_FINAL_RANGE). Stored in opus_demo `.bit` framing so the reference
    /// decoder can verify encoder/decoder range-coder agreement per packet.
    pub fn final_range(&self) -> u32 {
        self.range_final
    }

    /// The sample rate this encoder was created with, in Hz.
    pub fn sample_rate(&self) -> i32 {
        self.sampling_rate
    }

    /// The channel count this encoder was created with.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The [`Application`] this encoder was created with.
    pub fn application(&self) -> Application {
        self.application
    }

    /// Samples per channel of algorithmic delay, at this encoder's sample rate
    /// (libopus `OPUS_GET_LOOKAHEAD`).
    ///
    /// The encoder's output trails its input by this much, so a decoder should
    /// discard this many samples from the start to line the two up again. It is
    /// not a constant: [`Application::RestrictedLowDelay`] gives up the 4 ms
    /// the other two spend keeping SILK and CELT aligned, so it is 120 samples
    /// at 48 kHz where `Audio` and `Voip` are 312.
    ///
    /// For an Ogg stream this is what
    /// [`OpusHead::pre_skip`](crate::OpusHead::pre_skip) must carry, expressed
    /// at 48 kHz — which [`OpusHead::for_encoder`](crate::OpusHead::for_encoder)
    /// does for you, and is the reason to prefer it over
    /// [`OpusHead::new`](crate::OpusHead::new).
    pub fn lookahead(&self) -> usize {
        celt_prefill_samples(self.sampling_rate)
            + celt_delay_samples(self.sampling_rate, self.application)
    }

    /// Discard everything the encoder has learned, keeping its settings.
    ///
    /// Equivalent to building a new encoder with the same sample rate, channel
    /// count and [`Application`], then re-applying every setting you had
    /// changed — which is what it does. Use it to encode an unrelated second
    /// stream through the same instance, so that nothing from the first one
    /// (the filter histories, the bandwidth hysteresis, the speech/music
    /// decision) carries across and colours its opening frames.
    ///
    /// This is libopus's `OPUS_RESET_STATE`. Note it re-initialises the coding
    /// state rather than merely rewinding it, so it is not free; it is cheaper
    /// and far less error-prone than trying to keep a used encoder honest by
    /// hand, but a caller counting allocations should know it makes some.
    pub fn reset_state(&mut self) -> Result<()> {
        let mut fresh = Self::new(self.sampling_rate, self.channels, self.application)?;
        fresh.bitrate_bps = self.bitrate_bps;
        fresh.complexity = self.complexity;
        fresh.rate_control = self.rate_control;
        fresh.use_inband_fec = self.use_inband_fec;
        fresh.use_dtx = self.use_dtx;
        fresh.packet_loss_perc = self.packet_loss_perc;
        fresh.force_bandwidth = self.force_bandwidth;
        fresh.signal_type = self.signal_type;
        fresh.max_bandwidth = self.max_bandwidth;
        fresh.lsb_depth = self.lsb_depth;
        *self = fresh;
        Ok(())
    }

    /// opus_encoder.c:1296 voice_est ladder: forced by `signal_type` when set,
    /// else analysis-driven when voice_ratio is known, else application defaults.
    fn compute_voice_est(&self) -> i32 {
        match self.signal_type {
            Some(Signal::Voice) => return 127,
            Some(Signal::Music) => return 0,
            None => {}
        }
        if self.voice_ratio >= 0 {
            let mut v = (self.voice_ratio * 327) >> 8;
            // For AUDIO, never be more than 90% confident of having speech.
            if self.application == Application::Audio {
                v = v.min(115);
            }
            v
        } else {
            match self.application {
                Application::Voip => 115,
                Application::Audio => 48,
                Application::RestrictedLowDelay => 0,
            }
        }
    }

    /// Encode `frame_size` samples per channel into one Opus packet, returning
    /// how many bytes of `output` it filled.
    ///
    /// `input` is interleaved and must hold `frame_size * channels` samples.
    /// `frame_size` is one of the nine durations Opus defines — 2.5, 5, 10, 20,
    /// 40, 60, 80, 100 or 120 ms at this encoder's sample rate — and anything
    /// else is an `InvalidArgument`. Whether the packet ends up holding one
    /// coded frame or several is the encoder's to decide: only SILK has
    /// configurations past 20 ms, so a longer packet in any other mode is
    /// several frames sharing one TOC byte. A caller sees the difference only in
    /// the packet's framing, never in its duration.
    ///
    /// `output.len()` is the packet's **byte budget**, not merely a capacity.
    /// This is libopus's `max_data_bytes`, and it means a short buffer does not
    /// produce an error — the encoder codes a smaller packet to fit it, and the
    /// stream quietly comes out under the bitrate you asked for. Pass
    /// [`MAX_PACKET_BYTES`](crate::MAX_PACKET_BYTES) unless you are deliberately
    /// capping the instantaneous rate, for instance to a network MTU. The one
    /// size that is refused outright is a buffer under two bytes, which cannot
    /// hold any packet at all.
    ///
    /// Samples outside ±1 are coded rather than rejected, so a caller working
    /// in float can drive the encoder past full scale. If your source is
    /// integer PCM, prefer [`encode_s16`](Self::encode_s16), which is the same
    /// encoder told the truth about its input's precision.
    pub fn encode(&mut self, input: &[f32], frame_size: usize, output: &mut [u8]) -> Result<usize> {
        self.encode_native(input, frame_size, output, MAX_ENCODING_DEPTH)
    }

    /// Encode `frame_size` samples per channel of 16-bit PCM into one packet,
    /// returning how many bytes of `output` it filled.
    ///
    /// The same encoder as [`encode`](Self::encode) in every respect but one:
    /// it knows the input came from 16 bits. That matters because the encoder
    /// treats anything below the source's own noise floor as digital silence
    /// and codes it as such, and the floor sits `2^-depth` from full scale. Told
    /// 24 bits when the input has 16, it holds detail no 16-bit source could
    /// carry and spends bits on the dither in the bottom bits; told 16, it
    /// drops out where the source does. libopus draws the same distinction
    /// between `opus_encode` and `opus_encode_float`, and this matches it,
    /// including honouring a lower [`lsb_depth`](Self::lsb_depth) if the caller
    /// has set one.
    ///
    /// Conversion is `sample / 32768`, which is exact — the scale is a power of
    /// two — so this differs from converting by hand and calling
    /// [`encode`](Self::encode) only in the depth, never in the samples.
    ///
    /// ```
    /// # use opus_pure::{Application, OpusEncoder};
    /// let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio)?;
    /// let pcm = vec![0i16; 960 * 2];               // 20 ms of stereo at 48 kHz
    /// let mut packet = vec![0u8; 4000];
    /// let n = encoder.encode_s16(&pcm, 960, &mut packet)?;
    /// # assert!(n > 0);
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn encode_s16(
        &mut self,
        input: &[i16],
        frame_size: usize,
        output: &mut [u8],
    ) -> Result<usize> {
        let wanted = frame_size * self.channels;
        // Checked here as well as in `encode_native`, because the conversion
        // below reads the whole span before the encoder ever sees it.
        if input.len() < wanted {
            return Err(Error::InvalidArgument(
                "input is shorter than frame_size * channels",
            ));
        }
        let mut converted = std::mem::take(&mut self.buf_from_s16);
        converted.clear();
        converted.extend(input[..wanted].iter().copied().map(i16_to_float));
        let result = self.encode_native(&converted, frame_size, output, 16);
        self.buf_from_s16 = converted;
        result
    }

    /// Bring every setting into the range the encoder can honour, and reject
    /// the one value that cannot mean anything.
    ///
    /// The settings are public fields, so a caller can put any `i32` in them
    /// between one call and the next, and several of them go on to index a
    /// table or feed a shift. Clamping here keeps that in one place instead of
    /// at every read, and clamping the field itself rather than a private copy
    /// means [`Debug`] afterwards reports what the encoder is actually doing
    /// rather than what was asked for.
    ///
    /// A rate of zero or less is rejected rather than clamped. libopus spells
    /// "decide for me" and "as high as it goes" as the negative sentinels
    /// `OPUS_AUTO` (-1000) and `OPUS_BITRATE_MAX` (-1), and this crate has no
    /// sentinels — [`bitrate_bps`](Self::bitrate_bps) is always a rate, and
    /// "decide for me" is spelled by leaving the default alone. Quietly
    /// treating -1000 as half a kilobit would hand somebody porting from C a
    /// stream of two-byte packets and no reason why.
    fn normalize_settings(&mut self) -> Result<()> {
        if self.bitrate_bps <= 0 {
            return Err(Error::InvalidArgument(
                "bitrate_bps must be a positive rate; this crate has no \
                 OPUS_AUTO/OPUS_BITRATE_MAX sentinel",
            ));
        }
        self.bitrate_bps = self.bitrate_bps.clamp(
            MIN_BITRATE_BPS,
            MAX_BITRATE_BPS_PER_CHANNEL * self.channels as i32,
        );
        self.complexity = self.complexity.clamp(0, 10);
        self.packet_loss_perc = self.packet_loss_perc.clamp(0, 100);
        self.lsb_depth = self.lsb_depth.clamp(8, MAX_ENCODING_DEPTH);
        Ok(())
    }

    /// The body both entry points share. `api_lsb_depth` is the precision the
    /// entry point itself implies, which the caller's own setting can lower but
    /// not raise (libopus `opus_encode_native`).
    pub(crate) fn encode_native(
        &mut self,
        input: &[f32],
        frame_size: usize,
        output: &mut [u8],
        api_lsb_depth: i32,
    ) -> Result<usize> {
        self.normalize_settings()?;
        self.coded_lsb_depth = api_lsb_depth.min(self.lsb_depth);
        if output.len() < 2 {
            return Err(Error::buffer_too_small(2, output.len()));
        }

        // Reject an impossible duration before anything below advances the
        // analysis, the bandwidth hysteresis or any other encoder state.
        let duration = PacketDuration::classify(self.sampling_rate, frame_size).ok_or(
            Error::InvalidArgument("Invalid frame size for sampling rate"),
        )?;
        // C takes the input on trust because it is a bare pointer. Here the
        // slice knows its own length, and every layer below reads
        // `frame_size * channels` samples from it unconditionally, so a short
        // one has to be an error rather than an index out of bounds.
        if input.len() < frame_size * self.channels {
            return Err(Error::InvalidArgument(
                "input is shorter than frame_size * channels",
            ));
        }
        // The rate the *packet* repeats at, which is what the bitrate, mode and
        // bandwidth decisions below are scaled by (`opus_encoder.c`
        // `frame_rate = st->Fs/frame_size`). The rate a single coded frame
        // repeats at is only known once the split is decided, and is what the
        // TOC carries.
        let packet_rate = self.sampling_rate / frame_size as i32;

        // ---- Tonality analysis (opus_encoder.c:1123) ----
        // Runs over the whole packet, and `analysis_info` is the whole-packet
        // view the decisions below are made from. `analysis_at` is where the
        // ring stood before it was consumed: a split packet rewinds to it so
        // each frame can pull the slice covering the audio it actually codes
        // (`opus_encoder.c` analysis_read_pos_bak). `None` means the analysis
        // did not run, so there is nothing in the ring for a frame to read and
        // nothing to rewind.
        let mut analysis_at = None;
        let mut analysis_info = analysis::AnalysisInfo::default();
        if self.complexity >= 7 && self.sampling_rate >= 16000 {
            if let Some(kfft) = &self.analysis_kfft {
                analysis_at = Some(self.tonality.read_position());
                analysis_info = analysis::run_analysis(
                    &mut self.tonality,
                    kfft,
                    input,
                    frame_size,
                    frame_size,
                    self.channels,
                    self.sampling_rate,
                    self.coded_lsb_depth,
                );
            }
        } else if self.tonality.initialized() {
            self.tonality.reset();
        }

        // voice_ratio / detected_bandwidth from the analysis (opus_encoder.c:1154).
        // This is the whole packet's silence; a split packet re-tests each frame
        // on its own, so a silent stretch inside a long packet still reaches DTX.
        let is_silence = self.is_digital_silence(&input[..frame_size * self.channels]);
        if !is_silence {
            self.voice_ratio = -1;
        }
        // The classifier's verdict is used from the first frame it reports one,
        // as libopus does. This port used to discount the first ten, on the
        // theory that libopus's analysis lookahead left it converged by frame 0
        // — but libopus spends about twenty frames climbing from "voice" to
        // "music" on musical input, so the early hybrid run that guard removed
        // was the reference's own behaviour rather than a defect. Trusting the
        // verdict brings the mode decision to within one packet of libopus at
        // every rate and application measured; the guard put it as much as
        // twenty-one packets out.
        self.detected_bandwidth = 0;
        if analysis_info.valid {
            // Auto path (signal_type override applies later in compute_voice_est):
            // pick the hysteresis-correct probability.
            let prob = if self.prev_enc_mode.is_none() {
                analysis_info.music_prob
            } else if self.prev_enc_mode == Some(OpusMode::CeltOnly) {
                analysis_info.music_prob_max
            } else {
                analysis_info.music_prob_min
            };
            self.voice_ratio = (0.5 + 100.0 * (1.0 - prob)).floor() as i32;
            let ab = analysis_info.bandwidth;
            self.detected_bandwidth = if ab <= 12 {
                Bandwidth::Narrowband as i32
            } else if ab <= 14 {
                Bandwidth::Mediumband as i32
            } else if ab <= 16 {
                Bandwidth::Wideband as i32
            } else if ab <= 18 {
                Bandwidth::Superwideband as i32
            } else {
                Bandwidth::Fullband as i32
            };
        }

        // Mode selection: match C's opus_encode_native() behavior.
        // C reference auto-selects between SILK_ONLY and CELT_ONLY; Hybrid is
        // produced afterwards by bandwidth overrides (SILK-only + FB/SWB → Hybrid).
        let mut mode = if self.application == Application::RestrictedLowDelay {
            OpusMode::CeltOnly
        } else {
            let equiv = compute_equiv_rate(
                self.bitrate_bps,
                self.channels,
                packet_rate,
                !self.rate_control.is_cbr(),
                self.complexity,
                self.packet_loss_perc,
            );
            let prev_was_celt = self.prev_enc_mode == Some(OpusMode::CeltOnly);
            let has_prev_mode = self.prev_enc_mode.is_some();
            let voice_est = self.compute_voice_est();
            let threshold = compute_mode_threshold(
                self.application,
                self.channels,
                prev_was_celt,
                has_prev_mode,
                voice_est,
            );
            // libopus compares the equivalent rate against the threshold and
            // nothing else (opus_encoder.c). This port carried an extra
            // `&& self.sampling_rate >= 24000` from the crate it was forked
            // from, where CELT below 48 kHz was broken — 24 kHz decoded to
            // full-scale noise. That was fixed here when CELT learned to code
            // lower rates the way libopus does, but the guard outlived it and
            // pinned 8, 12 and 16 kHz to SILK whatever the content or bitrate.
            // At 16 kHz that cost a third of the requested bitrate, because
            // SILK saturates at wideband and simply cannot spend the rest.
            if equiv >= threshold {
                OpusMode::CeltOnly
            } else {
                OpusMode::SilkOnly
            }
        };

        // ---- Automatic rate-dependent bandwidth selection (opus_encoder.c:1456) ----
        // Walk down from FB; stop at the first bandwidth whose hysteresis-adjusted
        // threshold the equivalent rate meets. Thresholds interpolate voice<->music
        // by voice_est^2. Without the tonality analysis we cannot do
        // detected-bandwidth reduction, so this reproduces libopus's
        // complexity-0 choices (measured: WB @16k, SWB @20k, FB @24k+ voip mono).
        {
            let equiv = compute_equiv_rate(
                self.bitrate_bps,
                self.channels,
                packet_rate,
                !self.rate_control.is_cbr(),
                self.complexity,
                self.packet_loss_perc,
            );
            let voice_est: i32 = self.compute_voice_est();
            let (vt, mt) = if self.channels == 2 {
                (
                    &STEREO_VOICE_BANDWIDTH_THRESHOLDS,
                    &STEREO_MUSIC_BANDWIDTH_THRESHOLDS,
                )
            } else {
                (
                    &MONO_VOICE_BANDWIDTH_THRESHOLDS,
                    &MONO_MUSIC_BANDWIDTH_THRESHOLDS,
                )
            };
            let mut th = [0i32; 8];
            for i in 0..8 {
                th[i] = mt[i] + ((voice_est * voice_est * (vt[i] - mt[i])) >> 14);
            }
            const NB: i32 = Bandwidth::Narrowband as i32; // 1101
            const MB: i32 = Bandwidth::Mediumband as i32; // 1102
            const FB: i32 = Bandwidth::Fullband as i32; // 1105
            let mut bw = FB;
            while bw > NB {
                let idx = (2 * (bw - MB)) as usize;
                let mut threshold = th[idx];
                let hysteresis = th[idx + 1];
                if !self.first_frame {
                    if self.auto_bandwidth >= bw {
                        threshold -= hysteresis;
                    } else {
                        threshold += hysteresis;
                    }
                }
                if equiv >= threshold {
                    break;
                }
                bw -= 1;
            }
            // Mediumband is no longer used by libopus's selector.
            if bw == MB {
                bw = Bandwidth::Wideband as i32;
            }
            self.auto_bandwidth = bw;
            // Hybrid at unsafe CBR rates starves SILK: cap at WB below 15 kb/s.
            if mode != OpusMode::CeltOnly && self.rate_control.is_cbr() && self.bitrate_bps < 15000
            {
                bw = bw.min(Bandwidth::Wideband as i32);
            }
            // NB/MB SILK-internal rates (8/12 kHz) aren't wired for >16 kHz API
            // input yet (no 48k->8k/12k encode resamplers); clamp to WB.
            if mode != OpusMode::CeltOnly && self.sampling_rate > 16000 {
                bw = bw.max(Bandwidth::Wideband as i32);
            }
            // Never code above the input's Nyquist (opus_encoder.c:1516).
            if self.sampling_rate <= 24000 {
                bw = bw.min(Bandwidth::Superwideband as i32);
            }
            if self.sampling_rate <= 16000 {
                bw = bw.min(Bandwidth::Wideband as i32);
            }
            if self.sampling_rate <= 12000 {
                bw = bw.min(Bandwidth::Mediumband as i32);
            }
            if self.sampling_rate <= 8000 {
                bw = bw.min(Bandwidth::Narrowband as i32);
            }
            // (MB remap above may have been undone by the caps; keep WB floor
            // only where the API rate allows it.)
            if bw == Bandwidth::Mediumband as i32 && self.sampling_rate > 12000 {
                bw = Bandwidth::Wideband as i32;
            }
            // Use the detected bandwidth to reduce the coded bandwidth
            // (opus_encoder.c:1526), conservatively floored by rate. (For
            // CELT-only this is currently undone below — no end-band support.)
            // For CELT-only, hold the detected-bandwidth narrowing until the
            // leak_boost dynalloc lands: decisions already match libopus
            // frame-for-frame (64k st music: 27:704/31:680/23:90 both), but our
            // dynalloc lacks C's leakage compensation at the spectral cut, so
            // the same narrowing costs 0.25 ODG more than C pays (PEAQ-gated
            // out). Hybrid/SILK caps (incl. hybrid SWB) stay live.
            // CELT-only keeps FULL bandwidth by choice: C's detected-bandwidth
            // narrowing costs PEAQ universally (libopus's own -2.11 at 64k st
            // IS its narrowed score; our FB encode scores -1.65 on the same
            // clip). leak_boost did NOT change this verdict (tested 2026-07-09
            // with the full dynalloc live: narrowing still -2.37). Hybrid/SILK
            // caps stay (they pick coding MODE, not spectral truncation).
            if self.detected_bandwidth != 0
                && self.force_bandwidth.is_none()
                && mode != OpusMode::CeltOnly
            {
                let ch = self.channels as i32;
                let equiv2 = equiv; // same 20-ms equivalent rate as the walk
                let min_det = if equiv2 <= 18000 * ch && mode == OpusMode::CeltOnly {
                    NB
                } else if equiv2 <= 24000 * ch && mode == OpusMode::CeltOnly {
                    MB
                } else if equiv2 <= 30000 * ch {
                    Bandwidth::Wideband as i32
                } else if equiv2 <= 44000 * ch {
                    Bandwidth::Superwideband as i32
                } else {
                    FB
                };
                bw = bw.min(self.detected_bandwidth.max(min_det));
            }
            // Cap by OPUS_SET_MAX_BANDWIDTH before the force override
            // (opus_encoder.c: bandwidth = IMIN(bandwidth, max_bandwidth)), but
            // keep the WB floor for non-CELT >16 kHz input — NB/MB SILK from
            // 48 kHz needs the 48->8/12k encode resamplers we don't have, so a
            // max_bandwidth of NB/MB there would emit an uncodeable config.
            let mut max_bw = self.max_bandwidth as i32;
            if mode != OpusMode::CeltOnly && self.sampling_rate > 16000 {
                max_bw = max_bw.max(Bandwidth::Wideband as i32);
            }
            bw = bw.min(max_bw);
            // The CELT TOC has no mediumband config; C maps MB down to NB.
            if mode == OpusMode::CeltOnly && bw == MB {
                bw = NB;
            }
            self.bandwidth = match self.force_bandwidth {
                Some(f) => f,
                None => match bw {
                    x if x == NB => Bandwidth::Narrowband,
                    x if x == MB => Bandwidth::Mediumband,
                    x if x == Bandwidth::Wideband as i32 => Bandwidth::Wideband,
                    x if x == Bandwidth::Superwideband as i32 => Bandwidth::Superwideband,
                    x if x == FB => Bandwidth::Fullband,
                    _ => Bandwidth::Wideband,
                },
            };
            self.first_frame = false;
        }

        // Never code a band the input cannot contain. `force_bandwidth` is a
        // user override that bypasses the selection above, so this clamp has to
        // sit after it — libopus applies the same one last
        // (opus_encoder.c, "prevents Opus from wasting bits on frequencies that
        // are above the Nyquist rate of the input signal"). Without it, asking
        // for e.g. mediumband at 8 kHz emits a config the rest of the encoder
        // cannot honour, and the result is not merely mis-tuned: it decodes to
        // full-scale noise.
        self.bandwidth = clamp_bandwidth_to_rate(self.bandwidth, self.sampling_rate);

        // Whether this packet carries in-band FEC. libopus settles this after
        // the bandwidth is chosen and lets it narrow the bandwidth further, so
        // it has to run here rather than alongside the other SILK settings.
        {
            let equiv = compute_equiv_rate(
                self.bitrate_bps,
                self.channels,
                packet_rate,
                !self.rate_control.is_cbr(),
                self.complexity,
                self.packet_loss_perc,
            );
            let mut bw = self.bandwidth as i32;
            self.lbrr_coded = decide_fec(
                self.use_inband_fec,
                self.packet_loss_perc.clamp(0, 100),
                self.lbrr_coded,
                mode,
                &mut bw,
                equiv,
            );
            if bw != self.bandwidth as i32 {
                self.bandwidth = bandwidth_from_i32(bw);
            }
        }

        if mode == OpusMode::SilkOnly
            && matches!(
                self.bandwidth,
                Bandwidth::Superwideband | Bandwidth::Fullband
            )
        {
            mode = OpusMode::Hybrid;
        }
        if mode == OpusMode::Hybrid
            && matches!(
                self.bandwidth,
                Bandwidth::Narrowband | Bandwidth::Mediumband | Bandwidth::Wideband
            )
        {
            mode = OpusMode::SilkOnly;
        }

        // Stereo hybrid is now CONFORMANT (the CELT intensity-clamp fix), but
        // our FIXED-point stereo SILK executes it worse than plain CELT-FB above
        // ~28 kb/s: PEAQ on stereo speech (ODG) measured hybrid −2.196/−2.193 vs
        // CELT-FB −2.136/−2.057 at 32k/48k (CELT-FB wins), while at 24k hybrid
        // −2.198 beats CELT-FB −2.240. libopus's FLOAT stereo SILK hybrid beats
        // both everywhere — the gap is fixed-vs-float, not a bug. So route
        // stereo hybrid to CELT-FB except at the low rates where it wins. (Force
        // via OPUS_SET_BANDWIDTH if the true hybrid path is wanted.) The clean
        // fix is float stereo SILK — a large port, tracked in the roadmap.
        if self.channels == 2 && mode == OpusMode::Hybrid && self.bitrate_bps > 28000 {
            mode = OpusMode::CeltOnly;
            self.bandwidth = Bandwidth::Fullband;
        }

        mode = coerce_mode_for_packet_rate(mode, packet_rate);

        // CELT has no mediumband configuration. libopus widens to wideband
        // (opus_encoder.c) rather than letting the TOC fall into the narrowband
        // slot, which would halve the coded bandwidth for a caller who asked for
        // more than narrowband.
        if mode == OpusMode::CeltOnly && self.bandwidth == Bandwidth::Mediumband {
            self.bandwidth = Bandwidth::Wideband;
        }

        // ---- The packet is decided; code it (opus_encoder.c:1698) ----
        // Everything above ran once for the whole packet, because every frame in
        // a packet shares one TOC byte and so must agree on mode, bandwidth and
        // duration. `layout` says how many frames those decisions imply. This is
        // libopus's split between `opus_encode_native` and
        // `opus_encode_frame_native`.
        let layout = duration.layout(self.sampling_rate, mode);
        if layout.nb_frames == 1 {
            return self.encode_frame(
                input,
                frame_size,
                output,
                mode,
                layout.frame_rate,
                &analysis_info,
                is_silence,
            );
        }
        self.encode_split_packet(input, output, mode, layout, analysis_at)
    }

    /// Code one packet as `layout.nb_frames` frames sharing a TOC.
    ///
    /// The frames are coded into scratch buffers and assembled by the
    /// repacketizer, exactly as `opus_encode_native` does — there is no separate
    /// multi-frame writer. Each frame gets its own slice of the packet's byte
    /// budget and its own slice of the tonality analysis.
    fn encode_split_packet(
        &mut self,
        input: &[f32],
        output: &mut [u8],
        mode: OpusMode,
        layout: PacketLayout,
        analysis_at: Option<analysis::AnalysisReadPos>,
    ) -> Result<usize> {
        let PacketLayout {
            enc_frame_size,
            nb_frames,
            frame_rate,
        } = layout;

        // Under CBR the packet owes the caller an exact size, so the frame
        // budget is carved out of that; under VBR the only ceiling is the
        // caller's buffer.
        let frame_size = enc_frame_size * nb_frames;
        let target_bits =
            (self.bitrate_bps as i64 * frame_size as i64 / self.sampling_rate as i64) as i32;
        let repacketize_len = if self.rate_control.is_cbr() {
            (((target_bits + 4) / 8) as usize).min(output.len())
        } else {
            output.len()
        };
        // Each coded frame carries a TOC byte the repacketizer strips, so the
        // frames may spend `nb_frames` bytes more between them than the packet
        // will hold. The framing itself is charged up front at its worst case.
        let header = max_header_bytes(nb_frames);
        if repacketize_len + nb_frames <= header {
            return Err(Error::buffer_too_small(header + 1, repacketize_len));
        }
        let max_len_sum = nb_frames + repacketize_len - header;
        // One frame's share of the configured bitrate. Both this and an equal
        // division of the packet bound each frame, so no frame can spend the
        // packet's whole budget however the mode decision inside it goes.
        let per_frame_bitrate_bytes = ((self.bitrate_bps as i64 * enc_frame_size as i64
            / self.sampling_rate as i64)
            / 8) as usize;

        let mut rp = crate::repacketizer::Repacketizer::new();

        // Not enough of a packet to seat every frame. libopus answers this the
        // same way (`opus_encoder.c:1340`, "If the space is too low to do
        // something useful, emit 'PLC' frames"): write the framing and no
        // payload, so the packet still announces its duration and the decoder
        // conceals it. Coding some frames and starving the rest would desync
        // the stream instead.
        if max_len_sum < 3 * nb_frames {
            let toc = gen_toc(mode, frame_rate, self.bandwidth, self.channels);
            for _ in 0..nb_frames {
                rp.cat(&[toc])?;
            }
            self.range_final = 0;
            self.prev_enc_mode = Some(mode);
            let pad_to = self.rate_control.is_cbr().then_some(repacketize_len);
            return Self::emit(rp, nb_frames, pad_to, output);
        }

        let mut scratch = vec![0u8; max_len_sum];
        let mut tot_size = 0usize;
        let mut dtx_count = 0usize;

        // Rewind the tonality ring to where it stood before `run_analysis`
        // consumed the whole packet's worth of it.
        if let Some(at) = analysis_at {
            self.tonality.set_read_position(at);
        }

        for i in 0..nb_frames {
            let curr_max = per_frame_bitrate_bytes
                .min(max_len_sum / nb_frames)
                // A frame is a TOC byte plus the two a range coder needs to say
                // anything at all, so a bitrate share below that is raised
                // rather than handed over as an impossible budget.
                .max(3)
                // What is actually left, which is the one bound that cannot be
                // relaxed: the frames have to fit the buffer they are coded into.
                .min(max_len_sum - tot_size);
            let base = i * enc_frame_size * self.channels;
            let frame_input = &input[base..base + enc_frame_size * self.channels];

            let analysis_info = match analysis_at {
                Some(_) => analysis::tonality_get_info(&mut self.tonality, enc_frame_size),
                None => analysis::AnalysisInfo::default(),
            };
            let is_silence = self.is_digital_silence(frame_input);

            let len = self.encode_frame(
                frame_input,
                enc_frame_size,
                &mut scratch[tot_size..tot_size + curr_max],
                mode,
                frame_rate,
                &analysis_info,
                is_silence,
            )?;
            if len == 1 {
                dtx_count += 1;
            }
            rp.cat(&scratch[tot_size..tot_size + len])?;
            tot_size += len;
        }

        // CBR still owes the caller an exact packet, unless every frame was
        // dropped to DTX — padding a packet that says "nothing was sent" would
        // put the bytes back that DTX exists to save.
        let pad_to =
            (self.rate_control.is_cbr() && dtx_count != nb_frames).then_some(repacketize_len);
        Self::emit(rp, nb_frames, pad_to, output)
    }

    /// Assemble the held frames into `output` (`opus_repacketizer_out_range_impl`
    /// plus C's copy into the caller's buffer).
    fn emit(
        rp: crate::repacketizer::Repacketizer,
        nb_frames: usize,
        pad_to: Option<usize>,
        output: &mut [u8],
    ) -> Result<usize> {
        let packet = rp.out_range_impl(0, nb_frames, pad_to)?;
        if packet.len() > output.len() {
            return Err(Error::buffer_too_small(packet.len(), output.len()));
        }
        output[..packet.len()].copy_from_slice(&packet);
        Ok(packet.len())
    }

    /// Whether every sample is at or below the quantiser's own noise floor
    /// (libopus `is_digital_silence`).
    fn is_digital_silence(&self, input: &[f32]) -> bool {
        let thresh = 1.0f32 / (1i64 << self.coded_lsb_depth) as f32;
        input.iter().fold(0.0f32, |m, &v| m.max(v.abs())) <= thresh
    }

    /// Code one frame — one TOC's worth of audio — into `output`.
    ///
    /// libopus `opus_encode_frame_native`. Everything the mode and bandwidth
    /// decision settled is already in `self`; what varies frame to frame within
    /// one packet is the audio, its analysis and its byte budget.
    #[allow(clippy::too_many_arguments)]
    fn encode_frame(
        &mut self,
        input: &[f32],
        frame_size: usize,
        output: &mut [u8],
        mode: OpusMode,
        frame_rate: i32,
        analysis_info: &analysis::AnalysisInfo,
        is_silence: bool,
    ) -> Result<usize> {
        if output.len() < 2 {
            return Err(Error::buffer_too_small(2, output.len()));
        }
        let curr_bw = self.bandwidth;

        // The frame durations each mode has a TOC configuration for
        // (RFC 6716 §3.1). `PacketDuration::layout` only ever produces frame
        // sizes that satisfy these, so reaching one of these errors means the
        // mode decision and the split disagree.
        let codable = match mode {
            OpusMode::CeltOnly => matches!(frame_rate, 400 | 200 | 100 | 50),
            OpusMode::Hybrid => matches!(frame_rate, 100 | 50),
            // 60 ms is 16 because the frame rate is truncated; see
            // `frame_rate_from_params`.
            OpusMode::SilkOnly => matches!(frame_rate, 100 | 50 | 25 | 16),
        };
        if !codable {
            return Err(Error::Internal("frame size is not codable in this mode"));
        }

        // CELT can only transform frame sizes its 48 kHz mode has an `lm` for.
        // Reject the combination rather than letting it reach the MDCT, which
        // would fall back to `lm = 0` and read past the output spectrum.
        if mode != OpusMode::SilkOnly
            && !celt_can_code_frame(frame_size * celt_upsample(self.sampling_rate))
        {
            return Err(Error::Internal(
                "frame size is not codable by the CELT layer at this sampling rate",
            ));
        }

        // Voice-activity flag for DTX (opus_encoder.c:1160). Silence is always
        // inactive; with analysis, use the VAD probability; without it, assume
        // active (conservative — never DTX away real audio). We skip the
        // peak-energy SNR fallback, which only ever ADDS activity.
        let activity = if is_silence {
            false
        } else if analysis_info.valid {
            analysis_info.activity_probability >= 0.1
        } else {
            true
        };

        // ---- Mode-transition resets (opus_encoder.c:1449 + 2054) ----
        // The decoder resets its CELT state on ANY mode change (when there is
        // no redundancy) and its SILK state when leaving CELT-only; the
        // encoder must mirror both or the streams desync from that frame on.
        if let Some(prev) = self.prev_enc_mode
            && prev != mode
        {
            if mode != OpusMode::SilkOnly {
                let ch = self.channels;
                self.celt_enc = CeltEncoder::new(celt::modes::default_mode(), ch);
                // Prefill one CELT block so the fresh state has real
                // preemph/overlap history instead of a hard edge
                // (opus_encoder.c:2060). Skipped when the previous frame was
                // shorter than a block and no tail was captured; that only
                // costs a transition artifact, never decoder sync.
                self.celt_enc
                    .set_upsample(celt_upsample(self.sampling_rate));
                let prefill = celt_prefill_samples(self.sampling_rate);
                if self.celt_prefill_tail.len() == prefill * ch {
                    let mut dummy = RangeCoder::new_encoder(2);
                    let tail = std::mem::take(&mut self.celt_prefill_tail);
                    self.celt_enc
                        .encode_with_budget(&tail, prefill, &mut dummy, 0, 21, 16);
                    self.celt_prefill_tail = tail;
                }
            }
            if mode != OpusMode::CeltOnly && prev == OpusMode::CeltOnly {
                self.silk_initialized = false;
                self.silk_prefill_pending = true;
            }
        }

        // SILK prefill tail: last 10 ms of API-rate mono input.
        if self.channels == 1 {
            let n10 = (self.sampling_rate / 100) as usize;
            if frame_size >= n10 {
                self.silk_prefill_tail.resize(n10, 0);
                for i in 0..n10 {
                    self.silk_prefill_tail[i] =
                        (input[frame_size - n10 + i] * 32768.0).clamp(-32768.0, 32767.0) as i16;
                }
            }
        }

        // ---- Advance the CELT input timeline (opus_encoder.c:1967, 2301, 2304) ----
        // CELT reads `delay` samples behind the caller, so everything it touches
        // comes off one virtual timeline: `celt_delay` (the samples it has not
        // reached yet) followed by this frame's `input`. Index `i` on that
        // timeline is `i - delay` on the caller's.
        //
        // All three reads happen here, before the delay buffer moves, and before
        // any early return below — DTX included. The timeline is a position in
        // the input, not a coder state, so it has to advance on every frame
        // whatever the mode does.
        {
            let ch = self.channels;
            let delay = celt_delay_samples(self.sampling_rate, self.application);
            debug_assert_eq!(self.celt_delay.len(), delay * ch);
            let timeline = |celt_delay: &[f32], c: usize, i: usize| -> f32 {
                if i < delay {
                    celt_delay[c * delay + i]
                } else {
                    input[(i - delay) * ch + c]
                }
            };

            // What CELT codes this frame: `frame_size` samples from the start of
            // the timeline, i.e. ending `delay` short of the newest input.
            if mode != OpusMode::SilkOnly {
                self.buf_celt_input.resize(frame_size * ch, 0.0);
                for c in 0..ch {
                    for i in 0..frame_size {
                        self.buf_celt_input[c * frame_size + i] = timeline(&self.celt_delay, c, i);
                    }
                }
            }

            // The 2.5 ms immediately before the *next* CELT frame, kept for a
            // prefill if the next mode transition needs one. On the timeline
            // that block ends exactly where the new delay buffer begins. A frame
            // shorter than one CELT block leaves the tail empty, which the
            // prefill treats as "skip".
            let prefill = celt_prefill_samples(self.sampling_rate);
            if frame_size >= prefill {
                self.celt_prefill_tail.resize(prefill * ch, 0.0);
                for c in 0..ch {
                    for i in 0..prefill {
                        self.celt_prefill_tail[c * prefill + i] =
                            timeline(&self.celt_delay, c, frame_size - prefill + i);
                    }
                }
            } else {
                self.celt_prefill_tail.clear();
            }

            // Slide the timeline forward by one frame, into the spare buffer
            // and back, rather than allocating a fresh one every frame and
            // dropping the old one. `timeline` reads `celt_delay`, so the two
            // cannot be the same buffer, and swapping is what keeps both
            // allocations alive across calls.
            if delay > 0 {
                let mut next = std::mem::take(&mut self.celt_delay_next);
                next.clear();
                next.resize(delay * ch, 0.0);
                for c in 0..ch {
                    for i in 0..delay {
                        next[c * delay + i] = timeline(&self.celt_delay, c, frame_size + i);
                    }
                }
                self.celt_delay_next = std::mem::replace(&mut self.celt_delay, next);
            }
        }

        let toc = gen_toc(mode, frame_rate, self.bandwidth, self.channels);
        output[0] = toc;

        // ---- DTX decision (opus_encoder.c:2137 decide_dtx_mode) ----
        // After enough consecutive inactive frames, emit a TOC-only 1-byte
        // packet: the decoder sees an empty payload and runs comfort-noise /
        // PLC. We decide before the (skipped) SILK/CELT encode — SILK's own DTX
        // likewise stops coding, so the encoder state simply doesn't advance;
        // the codecs resync on the next active frame.
        if self.use_dtx && (analysis_info.valid || is_silence) {
            let frame_ms_q1 = 2 * 1000 * frame_size as i32 / self.sampling_rate;
            let dtx = if !activity {
                self.nb_no_activity_ms_q1 += frame_ms_q1;
                const LO: i32 = silk::define::NB_SPEECH_FRAMES_BEFORE_DTX * 20 * 2; // 400
                const HI: i32 = (silk::define::NB_SPEECH_FRAMES_BEFORE_DTX
                    + silk::define::MAX_CONSECUTIVE_DTX)
                    * 20
                    * 2; // 1200
                if self.nb_no_activity_ms_q1 > LO {
                    if self.nb_no_activity_ms_q1 <= HI {
                        true
                    } else {
                        self.nb_no_activity_ms_q1 = LO;
                        false
                    }
                } else {
                    false
                }
            } else {
                self.nb_no_activity_ms_q1 = 0;
                false
            };
            if dtx {
                self.prev_enc_mode = Some(mode);
                self.range_final = 0;
                return Ok(1);
            }
        } else {
            self.nb_no_activity_ms_q1 = 0;
        }

        let target_bits =
            (self.bitrate_bps as i64 * frame_size as i64 / self.sampling_rate as i64) as i32;
        let cbr_bytes = ((target_bits + 4) / 8) as usize;
        let max_data_bytes = output.len();

        // CBR: the packet is exactly the target size. VBR: the packet ends
        // wherever the coded frame ends — SILK-only packets stop at whatever
        // SILK produced, and the CELT layer picks its own frame size
        // (compute_vbr) and shrinks the coder to it — so the only bound that
        // matters is the caller's buffer.
        let n_bytes = if self.rate_control.is_cbr() {
            cbr_bytes.min(max_data_bytes).max(1)
        } else {
            max_data_bytes.max(3)
        };

        // `n_bytes` is the packet the caller asked for; `frame_bytes` is what a
        // single coded frame is allowed to occupy. They diverge only when a CBR
        // target exceeds the frame limit, and the surplus then leaves `encode`
        // as code 3 padding instead of an over-long frame that libopus would
        // reject as OPUS_INVALID_PACKET. Mirrors libopus's split between
        // `orig_max_data_bytes` and `max_data_bytes`.
        let frame_bytes = n_bytes.min(MAX_ONE_FRAME_PACKET);

        self.rc.reset_for_encode((frame_bytes - 1) as u32);

        let mut hybrid_silk_rate = 0i32;
        if mode == OpusMode::SilkOnly || mode == OpusMode::Hybrid {
            // SILK's internal rate follows the coded bandwidth, exactly as
            // libopus derives `maxInternalSampleRate` from it: narrowband is
            // coded at 8 kHz, mediumband at 12, everything else at 16. Deriving
            // it from the API rate instead made the TOC advertise a bandwidth
            // the encoder had not actually coded, and the decoder then ran SILK
            // at a different rate than the encoder did.
            let silk_fs_khz = if mode == OpusMode::Hybrid {
                16
            } else {
                let by_bandwidth = match self.bandwidth {
                    Bandwidth::Narrowband => 8,
                    Bandwidth::Mediumband => 12,
                    _ => 16,
                };
                by_bandwidth.min(self.sampling_rate / 1000)
            };
            let silk_fs_hz = silk_fs_khz * 1000;

            let frame_ms = (frame_size as i32 * 1000) / self.sampling_rate;
            // A stream that codes stereo needs its second channel built and the
            // mid/side smoothers primed before the first frame that uses them.
            let n_channels_internal = self.channels as i32;
            let channels_changed = n_channels_internal != self.silk_enc.n_channels_internal;
            if n_channels_internal > self.silk_enc.n_channels_internal {
                silk_init_encoder(&mut self.silk_enc.state[1], 0);
                self.silk_enc.stereo.reset_for_stereo();
                // Both resamplers are rebuilt below, so the two channels start
                // from the same state: were they to start at different points,
                // the first stereo frame would carry a phase difference that is
                // purely an artefact of the filters and not of the image.
            }
            self.silk_enc.n_channels_internal = n_channels_internal;

            // libopus reconfigures SILK on every call (silk_Encode ->
            // silk_control_encoder), so do the same rather than gating on a
            // hand-maintained list of what might have changed. The frame
            // duration is one of the inputs: 20 -> 40 ms leaves `frame_length`
            // alone but doubles `n_frames_per_packet`, and skipping the call
            // left SILK coding one frame into a packet whose TOC announced two.
            // What must stay gated is the *resampler*, whose filter memory is
            // real audio history: rebuilding it every frame would restart the
            // filter mid-stream.
            let resampler_stale = !self.silk_initialized
                || self.silk_enc.state[0].s_cmn.fs_khz != silk_fs_khz
                || channels_changed;
            for ch in 0..n_channels_internal as usize {
                silk_control_encoder(
                    &mut self.silk_enc.state[ch],
                    silk_fs_khz,
                    frame_ms,
                    self.complexity,
                );
                self.silk_enc.state[ch].s_cmn.use_cbr =
                    if self.rate_control.is_cbr() { 1 } else { 0 };
            }
            if resampler_stale {
                self.silk_initialized = true;
                self.down_fir_l =
                    silk::resampler::SilkEncoderResampler::new(self.sampling_rate, silk_fs_hz);
                self.down_fir_r =
                    silk::resampler::SilkEncoderResampler::new(self.sampling_rate, silk_fs_hz);
            }

            // SILK prefill after CELT-only (opus_encoder.c prefill=1): run 10 ms
            // of the previous audio through the fresh resampler + SILK warmup
            // path so the first coded SILK frame has real LTP/shape history.
            if self.silk_prefill_pending {
                self.silk_prefill_pending = false;
                let n10 = (self.sampling_rate / 100) as usize;
                if self.channels == 1 && self.silk_prefill_tail.len() == n10 {
                    let need = silk_fs_khz as usize * 10;
                    let mut resampled = vec![0i16; need];
                    if let Some(r) = &mut self.down_fir_l {
                        r.process(&mut resampled, &self.silk_prefill_tail);
                    } else {
                        resampled.copy_from_slice(&self.silk_prefill_tail[..need]);
                    }
                    let (state, stereo) = (&mut self.silk_enc.state[0], &mut self.silk_enc.stereo);
                    silk::enc_api::silk_encode_prefill(state, stereo, &resampled, 0);
                }
            }

            for ch in 0..n_channels_internal as usize {
                let cmn = &mut self.silk_enc.state[ch].s_cmn;
                cmn.packet_loss_perc = self.packet_loss_perc.clamp(0, 100);

                // libopus silk_setup_LBRR. The gain bump is what makes the
                // redundant copy cheap; it shrinks as loss rises, so LBRR stays
                // decodable when it is most needed. A stream that had no LBRR in
                // the previous packet gets the full 7: that packet was coded at
                // the higher rate FEC-off allows, so there is more to give back.
                let lbrr_in_previous_packet = cmn.lbrr_enabled != 0;
                cmn.lbrr_enabled = if self.lbrr_coded { 1 } else { 0 };
                if cmn.lbrr_enabled != 0 {
                    cmn.lbrr_gain_increases = if !lbrr_in_previous_packet {
                        7
                    } else {
                        (7 - silk_smulwb(cmn.packet_loss_perc, 13107)).max(3)
                    };
                }
            }

            let hp_freq_smth1 = if mode == OpusMode::CeltOnly {
                silk_lin2log(60) << 8
            } else {
                self.silk_enc.state[0].s_cmn.variable_hp_smth1_q15
            };

            const VARIABLE_HP_SMTH_COEF2_Q16: i32 = 984;
            self.variable_hp_smth2_q15 = silk_smlawb(
                self.variable_hp_smth2_q15,
                hp_freq_smth1 - self.variable_hp_smth2_q15,
                VARIABLE_HP_SMTH_COEF2_Q16,
            );

            let cutoff_hz = silk_log2lin(silk_rshift(self.variable_hp_smth2_q15, 8));

            let required_size = frame_size * self.channels;
            self.buf_filtered.resize(required_size, 0);
            if self.application == Application::Voip {
                hp_cutoff(
                    input,
                    cutoff_hz,
                    &mut self.buf_filtered,
                    &mut self.hp_mem,
                    frame_size,
                    self.channels,
                    self.sampling_rate,
                );
            } else {
                for (i, &x) in input.iter().enumerate() {
                    self.buf_filtered[i] = (x * 32768.0).clamp(-32768.0, 32767.0) as i16;
                }
            }

            let input_i16 = &self.buf_filtered;

            let (silk_left, silk_right): (&[i16], &[i16]) = if self.channels == 2 {
                // Stereo SILK/hybrid: deinterleave and resample EACH channel to
                // the SILK-internal rate with its own filter state, then hand
                // both to silk_encode. The mid/side conversion happens in there,
                // not here, because how wide the image is coded depends on the
                // frame's bit budget and on state that advances with the coder.
                let frame_length = input_i16.len() / 2;
                self.buf_left.resize(frame_length, 0);
                self.buf_right.resize(frame_length, 0);
                for i in 0..frame_length {
                    self.buf_left[i] = input_i16[2 * i];
                    self.buf_right[i] = input_i16[2 * i + 1];
                }
                let ds_len =
                    frame_length * silk_fs_khz as usize / (self.sampling_rate as usize / 1000);
                self.buf_stereo_mid.resize(ds_len, 0);
                self.buf_stereo_side.resize(ds_len, 0);
                if let (Some(rl), Some(rr)) = (&mut self.down_fir_l, &mut self.down_fir_r) {
                    rl.process(&mut self.buf_stereo_mid, &self.buf_left);
                    rr.process(&mut self.buf_stereo_side, &self.buf_right);
                }
                self.buf_left.resize(ds_len, 0);
                self.buf_right.resize(ds_len, 0);
                self.buf_left
                    .copy_from_slice(&self.buf_stereo_mid[..ds_len]);
                self.buf_right
                    .copy_from_slice(&self.buf_stereo_side[..ds_len]);
                (&self.buf_left[..ds_len], &self.buf_right[..ds_len])
            } else {
                // Mono. When the API rate is above SILK's internal rate this
                // is a direct FIR for the whole ratio, never a chain of
                // halvings: the old down2 + down2_3 pair aliased badly (a 1 kHz
                // sine came back with a 7 kHz mirror at a third of its
                // amplitude). When the rates match it is the pass-through, which
                // still runs so the delay comes out the same either way.
                let silk_frame_size =
                    frame_size * silk_fs_khz as usize / (self.sampling_rate as usize / 1000);
                self.buf_silk_input.resize(silk_frame_size, 0);
                if let Some(r) = &mut self.down_fir_l {
                    r.process(&mut self.buf_silk_input, input_i16);
                } else {
                    self.buf_silk_input
                        .copy_from_slice(&input_i16[..silk_frame_size]);
                }
                (&self.buf_silk_input[..], &[][..])
            };

            let mut pn_bytes = 0;

            // The frames-per-second math below divides by silk_input.len(), which is
            // at the SILK-INTERNAL rate — so the rate here must be internal too.
            // Using the API rate at 48 kHz told SILK to target 3x the real budget
            // with a hard max_bits cap -> the gain loop crushed every frame to fit
            // -> near-silent output (only worked at 16 kHz API where they coincide).
            let silk_rate_for_calc = silk_fs_hz;
            let silk_frame_len = silk_left.len();

            let silk_bitrate = if mode == OpusMode::Hybrid {
                let frame_duration_ms = frame_size as i32 * 1000 / self.sampling_rate;
                let frame20ms = frame_duration_ms >= 20;
                // libopus distributes the *frame's* budget, not the configured
                // bitrate: capped by the caller's buffer, less the TOC byte
                // (`opus_encoder.c`, `bits_target`). At 20 kb/s and 20 ms that
                // is 19600 rather than 20000, which is a whole table row's
                // worth of interpolation.
                let fs = self.sampling_rate;
                let bits_target = (8 * frame_bytes as i32).min(bitrate_to_bits(
                    self.bitrate_bps,
                    fs,
                    frame_size as i32,
                )) - 8;
                let r = compute_silk_rate_for_hybrid(
                    bits_to_bitrate(bits_target, fs, frame_size as i32),
                    curr_bw,
                    frame20ms,
                    !self.rate_control.is_cbr(),
                    self.lbrr_coded,
                    self.channels,
                );
                hybrid_silk_rate = r;
                r
            } else if self.rate_control.is_cbr() {
                (8i64 * (n_bytes - 1) as i64 * silk_rate_for_calc as i64 / silk_frame_len as i64)
                    as i32
            } else {
                // VBR: n_bytes is only the buffer cap; target the configured rate.
                self.bitrate_bps
            };
            let silk_max_bits = if mode == OpusMode::Hybrid {
                let total_max_bits = ((frame_bytes - 1) * 8) as i32;
                if self.rate_control.is_cbr() {
                    let silk_bits = (silk_bitrate as i64 * silk_frame_len as i64
                        / silk_rate_for_calc as i64) as i32;
                    let other_bits = 0i32.max(total_max_bits - silk_bits);
                    0i32.max(total_max_bits - other_bits * 3 / 4)
                } else {
                    let frame_duration_ms = frame_size as i32 * 1000 / self.sampling_rate;
                    let frame20ms = frame_duration_ms >= 20;
                    let max_bit_rate = compute_silk_rate_for_hybrid(
                        bits_to_bitrate(total_max_bits, self.sampling_rate, frame_size as i32),
                        curr_bw,
                        frame20ms,
                        !self.rate_control.is_cbr(),
                        self.lbrr_coded,
                        self.channels,
                    );
                    max_bit_rate * frame_size as i32 / self.sampling_rate
                }
            } else {
                ((frame_bytes - 1) * 8) as i32
            };
            let silk_use_cbr = if mode == OpusMode::Hybrid && self.rate_control.is_cbr() {
                0
            } else if self.rate_control.is_cbr() {
                1
            } else {
                0
            };
            let ret = silk_encode(
                &mut self.silk_enc,
                silk_left,
                silk_right,
                &mut self.rc,
                &mut pn_bytes,
                silk_bitrate,
                silk_max_bits,
                silk_use_cbr,
                1,
            );
            if ret != 0 {
                return Err(Error::Internal("SILK encoding failed"));
            }

            // What SILK just coded, handed to CELT the way `opus_encoder.c` hands
            // it over (`CELT_SET_SILK_INFO`, hybrid only). The high band's rate,
            // its temporal resolution and its transient decision all key off it.
            let idx = &self.silk_enc.state[0].s_cmn.indices;
            self.celt_enc.silk_signal_type = idx.signal_type as i32;
            self.celt_enc.silk_offset = crate::silk::tables::SILK_QUANTIZATION_OFFSETS_Q10
                [(idx.signal_type >> 1) as usize][idx.quant_offset_type as usize]
                as i32;
        }

        // The hybrid redundancy flag is only present when >=37 bits remain
        // (opus_encoder.c: ec_tell+17+20 <= 8*(max_data_bytes-1)); the decoder
        // gates its read identically. Writing it unconditionally desynced every
        // frame where SILK left fewer than 37 bits (starved low-rate hybrid).
        if mode == OpusMode::Hybrid && self.rc.tell() + 37 <= ((frame_bytes - 1) * 8) as i32 {
            self.rc.encode_bit_logp(false, 12); // redundancy = 0
        }

        if mode == OpusMode::Hybrid {
            let nb_compr_bytes = (frame_bytes - 1) as u32;
            self.rc.shrink(nb_compr_bytes);
        }

        let silk_ret_bytes = if mode == OpusMode::SilkOnly {
            ((self.rc.tell() + 7) >> 3) as usize
        } else {
            0
        };

        if mode == OpusMode::CeltOnly || mode == OpusMode::Hybrid {
            self.celt_enc.analysis = celt::AnalysisInfo {
                valid: analysis_info.valid,
                tonality: analysis_info.tonality,
                tonality_slope: analysis_info.tonality_slope,
                noisiness: analysis_info.noisiness,
                activity: analysis_info.activity,
                music_prob: analysis_info.music_prob,
                music_prob_min: analysis_info.music_prob_min,
                music_prob_max: analysis_info.music_prob_max,
                bandwidth: analysis_info.bandwidth,
                activity_probability: analysis_info.activity_probability,
                max_pitch_ratio: analysis_info.max_pitch_ratio,
                leak_boost: analysis_info.leak_boost,
            };
            self.celt_enc.complexity = self.complexity;
            self.celt_enc.lsb_depth = self.coded_lsb_depth;
            // Census 2026-08-07 fix: loss_rate was never assigned, so CELT's
            // prefilter loss ladder (celt.rs) and coarse-energy intra bias were
            // dead even with OPUS_SET_PACKET_LOSS_PERC set. Default 0 = no
            // change on the default path (libopus opus_encoder.c parity).
            self.celt_enc.loss_rate = self.packet_loss_perc;
            let start_band = if mode == OpusMode::Hybrid { 17 } else { 0 };
            let end_band = celt_endband_for_bandwidth(self.bandwidth);
            let total_packet_bits = ((frame_bytes - 1) * 8) as i32;
            // VBR: hand CELT the target in eighth-bits per frame; it picks the
            // frame's size (compute_vbr) and shrinks the range coder to it. The
            // hybrid target covers the whole packet (CELT adds back the SILK
            // bits via `target += tell`).
            // libopus turns constrained VBR *off* for the hybrid high band
            // (`opus_encoder.c`: `OPUS_SET_VBR_CONSTRAINT(0)` beside the
            // bitrate ctl) and leaves it on for CELT-only. The constrained path
            // caps the frame against a reservoir sized for the whole packet,
            // which in hybrid is mostly SILK's bits, so leaving it on starves
            // the high band of the little rate it was given.
            // Hybrid has always run unconstrained here, matching libopus.
            // `RateControl::Vbr` extends that to every mode; the other two
            // variants keep exactly the behaviour this line had before it took
            // the setting into account.
            self.celt_enc.constrained_vbr =
                mode != OpusMode::Hybrid && self.rate_control != RateControl::Vbr;
            self.celt_enc.vbr_rate = if self.rate_control.is_cbr() {
                0
            } else {
                let den = self.sampling_rate >> 3; // Fs >> BITRES
                // In hybrid, CELT codes only the high band, so its target is
                // what SILK did not take (`opus_encoder.c`: `OPUS_SET_BITRATE
                // (st->bitrate_bps - st->silk_mode.bitRate)`). Handing it the
                // whole packet's rate and then adding the SILK bits back via
                // `target += tell` counts the low band twice.
                let rate = self.bitrate_bps - hybrid_silk_rate;
                ((rate as i64 * frame_size as i64 + (den >> 1) as i64) / den as i64) as i32
            };

            // Filled above from the delayed timeline, planar, for every mode
            // that reaches CELT.
            let celt_input: &[f32] = &self.buf_celt_input;

            if self.rc.tell() <= total_packet_bits {
                self.celt_enc
                    .set_upsample(celt_upsample(self.sampling_rate));
                self.celt_enc.encode_with_budget(
                    celt_input,
                    frame_size,
                    &mut self.rc,
                    start_band,
                    end_band,
                    total_packet_bits,
                );
            }
        }

        self.rc.done();
        self.range_final = self.rc.rng;

        // Payload actually coded. SILK reports its own length; CELT and hybrid
        // fill the coder under CBR, or shrank it to the size compute_vbr chose.
        let payload_len = if mode == OpusMode::SilkOnly {
            let mut len = silk_ret_bytes.min(self.rc.storage as usize);
            // Trailing zero bytes carry no information, and dropping them keeps
            // a short frame from being framed as if it filled the budget.
            while len > 2 && self.rc.buf[len - 1] == 0 {
                len -= 1;
            }
            len
        } else if self.rate_control.is_cbr() {
            frame_bytes - 1
        } else {
            (self.rc.storage as usize).min(frame_bytes - 1)
        };

        // CBR owes the caller a packet of exactly `n_bytes`; VBR emits only what
        // was coded. Where the two differ by more than the frame limit allows,
        // `emit_one_frame_packet` makes up the difference with code 3 padding.
        let target_total = if self.rate_control.is_cbr() {
            n_bytes
        } else {
            payload_len + 1
        };

        self.prev_enc_mode = Some(mode);
        Ok(emit_one_frame_packet(
            output,
            toc,
            &self.rc.buf[..payload_len],
            target_total,
        ))
    }
}

#[cfg(test)]
mod silk_rate_tests {
    use super::compute_silk_rate_for_hybrid;
    use crate::Bandwidth;

    /// Mono, no FEC: read the table straight off.
    fn mono(rate: i32) -> i32 {
        compute_silk_rate_for_hybrid(rate, Bandwidth::Fullband, true, true, false, 1)
    }

    #[test]
    fn test_reference_table_exact_entries() {
        assert_eq!(mono(12000), 10000);
        assert_eq!(mono(16000), 13500);
        assert_eq!(mono(20000), 16000);
        assert_eq!(mono(24000), 18000);
        assert_eq!(mono(32000), 22000);
        assert_eq!(mono(64000), 38000);
    }

    #[test]
    fn test_32kbps_gives_22kbps_silk() {
        assert_eq!(mono(32000), 22000);
    }

    #[test]
    fn test_interpolation_between_table_entries() {
        assert_eq!(mono(18000), 14750);
    }

    #[test]
    fn test_above_table_max_gives_half_extra() {
        assert_eq!(mono(72000), 38000 + (72000 - 64000) / 2);
    }

    /// FEC costs SILK real bits, so the reference gives it a wider share at the
    /// same total rate rather than letting the redundant copy squeeze the frame.
    #[test]
    fn fec_widens_the_silk_share() {
        for rate in [12000, 16000, 20000, 24000, 32000, 64000] {
            let no_fec =
                compute_silk_rate_for_hybrid(rate, Bandwidth::Fullband, true, false, false, 1);
            let fec = compute_silk_rate_for_hybrid(rate, Bandwidth::Fullband, true, false, true, 1);
            assert!(
                fec > no_fec,
                "{rate}: FEC should widen SILK's share, got {fec} against {no_fec}"
            );
        }
        // The reference's own numbers at two table rows.
        assert_eq!(
            compute_silk_rate_for_hybrid(20000, Bandwidth::Fullband, true, true, true, 1),
            18000
        );
        assert_eq!(
            compute_silk_rate_for_hybrid(64000, Bandwidth::Fullband, true, true, true, 1),
            50000
        );
    }

    /// The allocation is per channel: the total is divided down, the table is
    /// read at the single-channel rate, and the result scaled back up. Reading
    /// the table at the *total* rate lands in a higher row and hands SILK far
    /// more than its share, which is what this port used to do.
    #[test]
    fn stereo_allocates_per_channel() {
        // 32 kb/s stereo is 16 kb/s per channel: 13500 each, less the 1000 the
        // reference trims from stereo above 12 kb/s per channel.
        assert_eq!(
            compute_silk_rate_for_hybrid(32000, Bandwidth::Fullband, true, true, false, 2),
            13500 * 2 - 1000
        );
        // Below the trim threshold there is no trim: 20 kb/s stereo is 10 kb/s
        // per channel, under the 12 kb/s the reference tests against.
        let per_channel =
            compute_silk_rate_for_hybrid(10000, Bandwidth::Fullband, true, true, false, 1);
        assert_eq!(
            compute_silk_rate_for_hybrid(20000, Bandwidth::Fullband, true, true, false, 2),
            per_channel * 2
        );
        // And a stereo packet never gets the mono answer for the same total.
        assert_ne!(
            compute_silk_rate_for_hybrid(32000, Bandwidth::Fullband, true, true, false, 2),
            compute_silk_rate_for_hybrid(32000, Bandwidth::Fullband, true, true, false, 1)
        );
    }

    /// Superwideband hybrid gives SILK a little more, because CELT starts at the
    /// same band either way but covers less spectrum above it.
    #[test]
    fn superwideband_adds_to_the_silk_share() {
        assert_eq!(
            compute_silk_rate_for_hybrid(20000, Bandwidth::Superwideband, true, true, false, 1),
            16000 + 300
        );
        // Per channel first, then scaled: the bonus is per channel too.
        assert_eq!(
            compute_silk_rate_for_hybrid(40000, Bandwidth::Superwideband, true, true, false, 2),
            (16000 + 300) * 2 - 1000
        );
    }

    /// CBR gets a small boost, as in the reference.
    #[test]
    fn cbr_boosts_the_silk_share() {
        assert_eq!(
            compute_silk_rate_for_hybrid(20000, Bandwidth::Fullband, true, false, false, 1),
            16000 + 100
        );
    }
}

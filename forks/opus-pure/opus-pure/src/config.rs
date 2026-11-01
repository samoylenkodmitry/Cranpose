//! Encoder and decoder configuration types.

/// What the encoder is being asked to optimise for, fixed when it is created.
///
/// This is the one setting that cannot be changed afterwards, because it
/// decides which coding layers the encoder is allowed to use at all. It biases
/// the SILK/CELT decision rather than dictating it: [`Audio`](Self::Audio) and
/// [`Voip`](Self::Voip) both reach all three modes, and speech still codes as
/// SILK under `Audio` when the content analysis says so.
///
/// The discriminants are libopus's `OPUS_APPLICATION_*` values, so a caller
/// bridging to a C API can cast between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Application {
    /// Speech over a network. Shifts the mode threshold 8 kHz toward SILK
    /// (`opus_encoder.c`), so borderline content codes as speech, and enables
    /// the speech-oriented machinery: the analysis-driven bandwidth cap, DTX,
    /// and in-band FEC being worth spending bits on.
    Voip = 2048,
    /// General audio, and the right default when the content is unknown or
    /// mixed. Favours reproducing the input over making speech intelligible at
    /// low rates.
    Audio = 2049,
    /// Lowest achievable latency, at a cost in quality.
    ///
    /// Forces CELT for every frame and removes the 2.5 ms CELT lookahead that
    /// the other two spend to keep the layers aligned across a mode switch
    /// (`opus_encoder.c:1904`), since with no mode switching there is nothing to
    /// align. That lookahead is part of what
    /// [`OpusHead::RECOMMENDED_PRE_SKIP`](crate::ogg::OpusHead::RECOMMENDED_PRE_SKIP)
    /// counts, so a stream encoded this way has less delay to skip.
    RestrictedLowDelay = 2051,
}

/// How the encoder is allowed to vary the size of each packet.
///
/// Opus is a variable-rate codec, and the bitrate a caller sets is an average
/// the encoder spends around rather than a size it emits every time. This
/// chooses how much it is allowed to deviate.
///
/// [`ConstrainedVbr`](Self::ConstrainedVbr) is the default, and matches
/// libopus. The difference from [`Vbr`](Self::Vbr) only shows up on content
/// whose difficulty changes quickly: constrained VBR keeps a reservoir so that
/// any window of packets stays near the target, which is what a network with a
/// fixed budget needs, while unconstrained VBR spends whatever a frame is
/// worth. For encoding a file, where nothing downstream is metering the rate,
/// unconstrained is usually the better picture per byte — it is what `opusenc`
/// uses by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateControl {
    /// Spend what each frame is worth, with no reservoir. Best quality per byte
    /// over a whole file; the instantaneous rate can wander a long way from the
    /// target.
    Vbr,
    /// Vary per packet, but hold a reservoir so any short window stays near the
    /// target. The default, and libopus's.
    ConstrainedVbr,
    /// One size for every packet. Costs roughly a twelfth of the working
    /// bitrate against the other two, because the encoder can no longer move
    /// bits from an easy frame to a hard one; pick it only when something
    /// downstream genuinely needs a fixed packet size.
    Cbr,
}

impl RateControl {
    /// Whether this is [`Cbr`](Self::Cbr), which is the distinction almost
    /// every decision inside the encoder actually turns on.
    pub(crate) fn is_cbr(self) -> bool {
        matches!(self, RateControl::Cbr)
    }
}

/// OPUS_SET_SIGNAL hint: bias mode selection toward speech or music. `None` =
/// OPUS_AUTO (let the analysis decide).
///
/// Setting this pins the answer the content analysis would otherwise reach, so
/// it also makes the analysis cheap to skip. That matters for
/// [`encode_parallel`](crate::encode_parallel), where every worker would
/// otherwise have to re-derive it from its own warm-up audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Speech: code it as SILK, or as hybrid where the rate allows.
    Voice,
    /// Music: code it as CELT.
    Music,
}

/// The audio bandwidth a packet carries, which is what Opus varies instead of
/// the sample rate.
///
/// An Opus decoder always produces audio at the rate it was created with; a
/// narrowband packet is not a slower stream, it is one whose upper spectrum was
/// never coded. Each name gives the audio bandwidth, and the sample rate that
/// would be needed to represent it: 4 kHz of audio needs 8 kHz of sampling.
///
/// The encoder chooses this per packet from the bitrate, and a caller normally
/// leaves it alone. To constrain it, prefer
/// [`max_bandwidth`](crate::OpusEncoder::max_bandwidth), which caps the
/// automatic choice, over
/// [`force_bandwidth`](crate::OpusEncoder::force_bandwidth), which overrides it
/// and can spend bits on spectrum the rate cannot afford.
///
/// The discriminants are libopus's `OPUS_BANDWIDTH_*` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bandwidth {
    /// 4 kHz of audio, as an 8 kHz sample rate would carry.
    Narrowband = 1101,
    /// 6 kHz of audio (12 kHz sampling). Retained because it appears in the
    /// bitstream and a decoder must handle it, but libopus's encoder no longer
    /// selects it automatically and neither does this one.
    Mediumband = 1102,
    /// 8 kHz of audio (16 kHz sampling), the usual top of speech coding.
    Wideband = 1103,
    /// 12 kHz of audio (24 kHz sampling). Hybrid and CELT only.
    Superwideband = 1104,
    /// 20 kHz of audio (48 kHz sampling), the full audible range. Hybrid and
    /// CELT only.
    Fullband = 1105,
}

/// Which coding layers a packet actually used.
///
/// Opus is two codecs behind one bitstream, and the TOC byte says which of them
/// coded a given packet. A caller normally does not care — the decoder handles
/// all three — but the layer decides what some other operations mean. In-band
/// FEC, in particular, only exists in SILK and hybrid packets, so
/// [`OpusDecoder::decode_fec`](crate::OpusDecoder::decode_fec) on a
/// [`CeltOnly`](Self::CeltOnly) packet can only conceal.
///
/// RFC 6716 §3.1 fixes these three, so the set will not grow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusMode {
    /// SILK alone: speech, up to wideband, 10 ms and longer.
    SilkOnly,
    /// SILK below and CELT above: speech at super-wideband or fullband.
    Hybrid,
    /// CELT alone: music, any bandwidth, and every frame shorter than 10 ms.
    CeltOnly,
}

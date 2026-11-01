//! The two mandatory Opus header packets, `OpusHead` and `OpusTags`
//! (RFC 7845 §5).

use crate::{ChannelLayout, Error, OpusDecoder, OpusEncoder, OpusMSEncoder, Result};

/// Magic signature of the identification header.
pub(crate) const OPUS_HEAD_MAGIC: &[u8; 8] = b"OpusHead";
/// Magic signature of the comment header.
pub(crate) const OPUS_TAGS_MAGIC: &[u8; 8] = b"OpusTags";

/// Opus is always framed at 48 kHz in Ogg, whatever rate the encoder ran at:
/// granule positions and pre-skip are counted in 48 kHz samples (RFC 7845 §4).
pub const GRANULE_RATE: u32 = 48_000;

/// The identification header — RFC 7845 §5.1.
///
/// This is the first packet of the stream and must sit alone on the first page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusHead {
    /// Channels in the decoded stream, 1..=255.
    pub channel_count: u8,
    /// Samples at 48 kHz to discard from the decoder's output — the encoder's
    /// algorithmic delay. See [`OpusHead::RECOMMENDED_PRE_SKIP`].
    pub pre_skip: u16,
    /// Sample rate of the *original* input, purely informational: it does not
    /// change how the stream decodes. 0 means unspecified.
    pub input_sample_rate: u32,
    /// Gain to apply on playback, in Q7.8 dB. 0 = unity.
    pub output_gain_q8: i16,
    /// Channel mapping family: 0 = mono/stereo, 1 = Vorbis surround order,
    /// 255 = discrete channels.
    pub mapping_family: u8,
    /// Opus streams in the packet. Always 1 for family 0.
    pub stream_count: u8,
    /// How many of those streams are coupled (stereo) pairs.
    pub coupled_count: u8,
    /// Which decoded channel feeds each output channel. Empty for family 0,
    /// where the mapping is implicit.
    pub channel_mapping: Vec<u8>,
}

impl OpusHead {
    /// libopus's algorithmic delay at 48 kHz, for an encoder using
    /// [`Application::Audio`](crate::Application::Audio) or
    /// [`Voip`](crate::Application::Voip).
    ///
    /// It is **not** right for every encoder.
    /// [`RestrictedLowDelay`](crate::Application::RestrictedLowDelay) gives up
    /// the 4 ms the other two spend keeping SILK and CELT aligned, and its real
    /// delay is 120 samples rather than 312 — so a header built with this
    /// constant over that encoder tells players to discard 192 samples of
    /// genuine audio. Prefer [`for_encoder`](Self::for_encoder), which asks the
    /// encoder instead of assuming.
    pub const RECOMMENDED_PRE_SKIP: u16 = 312;

    /// A mono or stereo header (mapping family 0) with the recommended pre-skip.
    ///
    /// `input_sample_rate` is recorded for information only; pass the rate the
    /// PCM you encoded was sampled at, or 0 if you would rather not say.
    ///
    /// This is the right constructor when you are muxing packets you did not
    /// encode yourself. When you do have the encoder, use
    /// [`for_encoder`](Self::for_encoder): it takes the pre-skip from the
    /// encoder's own delay rather than from
    /// [`RECOMMENDED_PRE_SKIP`](Self::RECOMMENDED_PRE_SKIP), which is wrong for
    /// [`RestrictedLowDelay`](crate::Application::RestrictedLowDelay).
    pub fn new(channel_count: u8, input_sample_rate: u32) -> Result<Self> {
        if channel_count == 0 || channel_count > 2 {
            return Err(Error::InvalidArgument(
                "mapping family 0 supports only 1 or 2 channels",
            ));
        }
        Ok(OpusHead {
            channel_count,
            pre_skip: Self::RECOMMENDED_PRE_SKIP,
            input_sample_rate,
            output_gain_q8: 0,
            mapping_family: 0,
            stream_count: 1,
            coupled_count: channel_count - 1,
            channel_mapping: Vec::new(),
        })
    }

    /// A header describing what `encoder` produces, with the pre-skip taken
    /// from the encoder's own algorithmic delay.
    ///
    /// This is the constructor to reach for when you are encoding and muxing in
    /// one pass. [`new`](Self::new) assumes
    /// [`RECOMMENDED_PRE_SKIP`](Self::RECOMMENDED_PRE_SKIP), which is correct
    /// for two of the three [`Application`](crate::Application)s and four
    /// milliseconds wrong for the third; this one asks
    /// [`OpusEncoder::lookahead`] and scales it to the 48 kHz that
    /// [`pre_skip`](Self::pre_skip) is counted in.
    ///
    /// ```
    /// use opus_pure::{Application, OggOpusWriter, OpusEncoder, OpusHead};
    ///
    /// let encoder = OpusEncoder::new(48_000, 2, Application::RestrictedLowDelay)?;
    /// let head = OpusHead::for_encoder(&encoder, 48_000);
    /// assert_eq!(head.pre_skip, 120); // not the 312 a fixed constant would give
    ///
    /// let writer = OggOpusWriter::new(Vec::new(), head)?;
    /// # let _ = writer;
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn for_encoder(encoder: &OpusEncoder, input_sample_rate: u32) -> Self {
        let channel_count = encoder.channels() as u8;
        OpusHead {
            channel_count,
            pre_skip: pre_skip_48k(encoder.lookahead(), encoder.sample_rate()),
            input_sample_rate,
            output_gain_q8: 0,
            mapping_family: 0,
            stream_count: 1,
            coupled_count: channel_count - 1,
            channel_mapping: Vec::new(),
        }
    }

    /// A header describing what a multistream `encoder` produces.
    ///
    /// The surround counterpart of [`for_encoder`](Self::for_encoder), and the
    /// missing half of writing a surround `.opus` file: it takes the stream
    /// count, the coupled count and the channel mapping from the encoder's own
    /// [`ChannelLayout`], so they cannot disagree with what is actually in the
    /// packets.
    ///
    /// ```
    /// use opus_pure::{Application, OggOpusWriter, OpusHead, OpusMSEncoder};
    ///
    /// let encoder = OpusMSEncoder::new(48_000, 6, 1, Application::Audio)?;
    /// let head = OpusHead::for_ms_encoder(&encoder, 48_000);
    /// assert_eq!(head.channel_count, 6);
    /// assert_eq!(head.stream_count, 4);
    /// assert_eq!(head.coupled_count, 2);
    ///
    /// let writer = OggOpusWriter::new(Vec::new(), head)?;
    /// # let _ = writer;
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn for_ms_encoder(encoder: &OpusMSEncoder, input_sample_rate: u32) -> Self {
        let stream = &encoder.streams()[0];
        let mut head = Self::for_layout(encoder.layout(), input_sample_rate);
        head.pre_skip = pre_skip_48k(stream.lookahead(), stream.sample_rate());
        head
    }

    /// A header for a channel `layout`, with the recommended pre-skip.
    ///
    /// Use it when you have a layout but not the encoder that goes with it —
    /// remuxing, say. With an encoder in hand,
    /// [`for_ms_encoder`](Self::for_ms_encoder) is exact about the pre-skip
    /// where this is only conventional.
    pub fn for_layout(layout: &ChannelLayout, input_sample_rate: u32) -> Self {
        OpusHead {
            channel_count: layout.nb_channels as u8,
            pre_skip: Self::RECOMMENDED_PRE_SKIP,
            input_sample_rate,
            output_gain_q8: 0,
            mapping_family: layout.mapping_family,
            stream_count: layout.nb_streams as u8,
            coupled_count: layout.nb_coupled_streams as u8,
            // Family 0's mapping is implicit and the header carries none.
            channel_mapping: if layout.mapping_family == 0 {
                Vec::new()
            } else {
                layout.mapping.clone()
            },
        }
    }

    /// A decoder configured for this stream, at `sample_rate`.
    ///
    /// Three facts have to travel from the header into the decoder, and only
    /// one of them announces itself when it is missed. The channel count is
    /// checked — a decoder built for the wrong one fails or interleaves
    /// visibly. The pre-skip is what [`Trim`](super::Trim) needs, and it takes
    /// the header directly. [`output_gain_q8`](Self::output_gain_q8) is the
    /// silent one: RFC 7845 §5.1 says a player SHOULD apply it, and a stream
    /// that carries a non-zero gain plays at the wrong level with no other
    /// symptom if it does not. This constructor carries all three of those
    /// decisions so none of them is a thing to remember.
    ///
    /// `sample_rate` is the rate you want *out*, not one stored in the file:
    /// Opus decodes to whichever of 8/12/16/24/48 kHz you ask for.
    ///
    /// ```no_run
    /// use opus_pure::OggOpusReader;
    ///
    /// let mut reader = OggOpusReader::new(std::fs::File::open("in.opus")?)?;
    /// let mut decoder = reader.head().decoder(48_000)?;   // gain already set
    /// # let _ = &mut decoder;
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    ///
    /// Only mapping family 0 (mono and stereo) has a plain [`OpusDecoder`]
    /// behind it; a surround stream needs
    /// [`OpusMSDecoder`](crate::OpusMSDecoder), built from
    /// [`channel_count`](Self::channel_count) and
    /// [`mapping_family`](Self::mapping_family), and this reports that rather
    /// than quietly decoding one stream of several. Rendering a stream to a
    /// channel count that is not its own is likewise a case for
    /// [`OpusDecoder::new`] plus setting
    /// [`gain_q8`](OpusDecoder::gain_q8) by hand.
    pub fn decoder(&self, sample_rate: i32) -> Result<OpusDecoder> {
        if self.mapping_family != 0 {
            return Err(Error::InvalidArgument(
                "mapping family is not 0; a surround stream needs an OpusMSDecoder",
            ));
        }
        let mut decoder = OpusDecoder::new(sample_rate, self.channel_count as usize)?;
        decoder.gain_q8 = i32::from(self.output_gain_q8);
        Ok(decoder)
    }

    /// Serialize to the wire format.
    pub(crate) fn to_packet(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(19 + self.channel_mapping.len());
        v.extend_from_slice(OPUS_HEAD_MAGIC);
        v.push(1); // version
        v.push(self.channel_count);
        v.extend_from_slice(&self.pre_skip.to_le_bytes());
        v.extend_from_slice(&self.input_sample_rate.to_le_bytes());
        v.extend_from_slice(&self.output_gain_q8.to_le_bytes());
        v.push(self.mapping_family);
        if self.mapping_family != 0 {
            v.push(self.stream_count);
            v.push(self.coupled_count);
            v.extend_from_slice(&self.channel_mapping);
        }
        v
    }

    /// Parse the wire format, rejecting anything RFC 7845 forbids.
    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 19 || &data[..8] != OPUS_HEAD_MAGIC {
            return Err(Error::InvalidStream("not an OpusHead packet"));
        }
        // §5.1: "the major version number is the upper four bits"; a decoder must
        // reject major versions above 0 and tolerate unknown minor versions.
        if data[8] >> 4 != 0 {
            return Err(Error::InvalidStream("unsupported OpusHead version"));
        }
        let channel_count = data[9];
        if channel_count == 0 {
            return Err(Error::InvalidStream("OpusHead declares zero channels"));
        }
        let mapping_family = data[18];

        let (stream_count, coupled_count, channel_mapping) = if mapping_family == 0 {
            if channel_count > 2 {
                return Err(Error::InvalidStream(
                    "mapping family 0 allows at most 2 channels",
                ));
            }
            (1u8, channel_count - 1, Vec::new())
        } else {
            let need = 21 + channel_count as usize;
            if data.len() < need {
                return Err(Error::InvalidStream(
                    "OpusHead channel mapping is truncated",
                ));
            }
            let stream_count = data[19];
            let coupled_count = data[20];
            if stream_count == 0 {
                return Err(Error::InvalidStream("OpusHead declares zero streams"));
            }
            if coupled_count > stream_count {
                return Err(Error::InvalidStream(
                    "OpusHead couples more streams than it declares",
                ));
            }
            // Every output channel must name a decoded channel that exists, or
            // 255 for silence (§5.1.1).
            let decoded = stream_count as usize + coupled_count as usize;
            let mapping = data[21..need].to_vec();
            if mapping.iter().any(|&m| m != 255 && (m as usize) >= decoded) {
                return Err(Error::InvalidStream(
                    "OpusHead channel mapping references a stream that does not exist",
                ));
            }
            (stream_count, coupled_count, mapping)
        };

        Ok(OpusHead {
            channel_count,
            pre_skip: u16::from_le_bytes(data[10..12].try_into().unwrap()),
            input_sample_rate: u32::from_le_bytes(data[12..16].try_into().unwrap()),
            output_gain_q8: i16::from_le_bytes(data[16..18].try_into().unwrap()),
            mapping_family,
            stream_count,
            coupled_count,
            channel_mapping,
        })
    }

    /// Playback gain in dB, from the Q7.8 fixed-point field.
    pub fn output_gain_db(&self) -> f32 {
        self.output_gain_q8 as f32 / 256.0
    }
}

/// The comment header — RFC 7845 §5.2.
///
/// Comments are held exactly as they appear on the wire, so a stream survives a
/// read/write round trip byte-for-byte even when it carries entries this crate
/// does not understand. Use [`OpusTags::get`] and [`OpusTags::push`] for the
/// usual `NAME=value` access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusTags {
    /// Identifies the encoder that produced the stream.
    pub vendor: String,
    /// Raw `NAME=value` entries, in file order.
    pub comments: Vec<String>,
}

impl Default for OpusTags {
    fn default() -> Self {
        OpusTags {
            vendor: concat!("opus-pure ", env!("CARGO_PKG_VERSION")).to_string(),
            comments: Vec::new(),
        }
    }
}

impl OpusTags {
    /// Empty tags carrying this crate's vendor string.
    pub fn new() -> Self {
        Self::default()
    }

    /// First value for `name`, matched case-insensitively as the spec requires.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.get_all(name).next()
    }

    /// Every value stored under `name`, in file order.
    ///
    /// A Vorbis comment name may legitimately repeat, so this is the honest
    /// form of [`get`](Self::get) for anything that can have more than one
    /// value — `ARTIST`, `PERFORMER`, `GENRE`.
    pub fn get_all<'a, 'n>(&'a self, name: &'n str) -> impl Iterator<Item = &'a str> + use<'a, 'n> {
        self.comments.iter().filter_map(move |c| {
            let (k, v) = c.split_once('=')?;
            k.eq_ignore_ascii_case(name).then_some(v)
        })
    }

    /// Append a `NAME=value` entry.
    ///
    /// Named `push` rather than `insert` because that is what it does: Vorbis
    /// comments allow a name to appear more than once — several `ARTIST` lines
    /// are how a collaboration is spelled — so this adds an entry and never
    /// replaces one. [`get`](Self::get) then returns the first;
    /// [`get_all`](Self::get_all) returns every one.
    ///
    /// Comment names are ASCII 0x20..=0x7D excluding `=`; a name outside that set
    /// is rejected rather than written out to produce a file no other tool reads.
    pub fn push(&mut self, name: &str, value: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| (0x20..=0x7d).contains(&b) && b != b'=')
        {
            return Err(Error::InvalidArgument(
                "comment name must be ASCII 0x20..=0x7D excluding '='",
            ));
        }
        self.comments.push(format!("{name}={value}"));
        Ok(())
    }

    pub(crate) fn to_packet(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(OPUS_TAGS_MAGIC);
        v.extend_from_slice(&(self.vendor.len() as u32).to_le_bytes());
        v.extend_from_slice(self.vendor.as_bytes());
        v.extend_from_slice(&(self.comments.len() as u32).to_le_bytes());
        for c in &self.comments {
            v.extend_from_slice(&(c.len() as u32).to_le_bytes());
            v.extend_from_slice(c.as_bytes());
        }
        v
    }

    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 12 || &data[..8] != OPUS_TAGS_MAGIC {
            return Err(Error::InvalidStream("not an OpusTags packet"));
        }
        let mut pos = 8;

        let take_len = |pos: &mut usize| -> Result<usize> {
            if data.len() < *pos + 4 {
                return Err(Error::InvalidStream("OpusTags is truncated"));
            }
            let n = u32::from_le_bytes(data[*pos..*pos + 4].try_into().unwrap()) as usize;
            *pos += 4;
            // Check against the buffer, not just as a length: a hostile 4 GiB
            // length must not become an allocation.
            if data.len() < *pos + n {
                return Err(Error::InvalidStream("OpusTags string runs past the packet"));
            }
            Ok(n)
        };

        let vlen = take_len(&mut pos)?;
        let vendor = String::from_utf8_lossy(&data[pos..pos + vlen]).into_owned();
        pos += vlen;

        let count = take_len(&mut pos)?;
        // Each remaining comment needs at least its own 4-byte length prefix, so
        // a count that cannot fit is malformed regardless of the strings.
        if count.saturating_mul(4) > data.len() - pos {
            return Err(Error::InvalidStream(
                "OpusTags comment count exceeds the packet",
            ));
        }
        let mut comments = Vec::with_capacity(count);
        for _ in 0..count {
            let n = take_len(&mut pos)?;
            comments.push(String::from_utf8_lossy(&data[pos..pos + n]).into_owned());
            pos += n;
        }
        Ok(OpusTags { vendor, comments })
    }
}

/// An encoder's lookahead, expressed in the 48 kHz samples
/// [`OpusHead::pre_skip`] is counted in (RFC 7845 §5.1).
fn pre_skip_48k(lookahead: usize, sample_rate: i32) -> u16 {
    let scaled = lookahead as u64 * GRANULE_RATE as u64 / sample_rate as u64;
    scaled.min(u16::MAX as u64) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_round_trips_family_0() {
        let head = OpusHead::new(2, 48_000).unwrap();
        assert_eq!(OpusHead::parse(&head.to_packet()).unwrap(), head);
    }

    #[test]
    fn head_round_trips_family_1_surround() {
        let head = OpusHead {
            channel_count: 6,
            pre_skip: 312,
            input_sample_rate: 48_000,
            output_gain_q8: -512,
            mapping_family: 1,
            stream_count: 4,
            coupled_count: 2,
            channel_mapping: vec![0, 4, 1, 2, 3, 5],
        };
        let parsed = OpusHead::parse(&head.to_packet()).unwrap();
        assert_eq!(parsed, head);
        assert_eq!(parsed.output_gain_db(), -2.0);
    }

    #[test]
    fn head_rejects_malformed_input() {
        assert!(matches!(
            OpusHead::parse(b"nope"),
            Err(Error::InvalidStream(_))
        ));

        let mut p = OpusHead::new(2, 48_000).unwrap().to_packet();
        p[8] = 0x10; // major version 1
        assert!(matches!(OpusHead::parse(&p), Err(Error::InvalidStream(_))));

        let mut p = OpusHead::new(2, 48_000).unwrap().to_packet();
        p[9] = 0; // zero channels
        assert!(matches!(OpusHead::parse(&p), Err(Error::InvalidStream(_))));

        let mut p = OpusHead::new(2, 48_000).unwrap().to_packet();
        p[9] = 3; // family 0 caps at 2
        assert!(matches!(OpusHead::parse(&p), Err(Error::InvalidStream(_))));
    }

    /// An unknown *minor* version must still parse — §5.1 requires forward
    /// compatibility within major version 0.
    #[test]
    fn head_accepts_unknown_minor_version() {
        let mut p = OpusHead::new(1, 16_000).unwrap().to_packet();
        p[8] = 0x0f;
        assert!(OpusHead::parse(&p).is_ok());
    }

    #[test]
    fn head_rejects_mapping_that_names_a_missing_stream() {
        let head = OpusHead {
            channel_count: 2,
            pre_skip: 312,
            input_sample_rate: 48_000,
            output_gain_q8: 0,
            mapping_family: 1,
            stream_count: 1,
            coupled_count: 0,
            channel_mapping: vec![0, 7], // only decoded channel 0 exists
        };
        assert!(matches!(
            OpusHead::parse(&head.to_packet()),
            Err(Error::InvalidStream(_))
        ));
    }

    #[test]
    fn head_rejects_more_coupled_than_streams() {
        let head = OpusHead {
            channel_count: 2,
            pre_skip: 312,
            input_sample_rate: 48_000,
            output_gain_q8: 0,
            mapping_family: 1,
            stream_count: 1,
            coupled_count: 2,
            channel_mapping: vec![0, 1],
        };
        assert!(matches!(
            OpusHead::parse(&head.to_packet()),
            Err(Error::InvalidStream(_))
        ));
    }

    #[test]
    fn tags_round_trip() {
        let mut tags = OpusTags::new();
        tags.push("TITLE", "A Song").unwrap();
        tags.push("ARTIST", "Someone").unwrap();
        let parsed = OpusTags::parse(&tags.to_packet()).unwrap();
        assert_eq!(parsed, tags);
        assert_eq!(parsed.get("title"), Some("A Song"));
        assert_eq!(parsed.get("MISSING"), None);
    }

    #[test]
    fn tags_reject_invalid_names() {
        let mut tags = OpusTags::new();
        assert!(tags.push("BAD=NAME", "x").is_err());
        assert!(tags.push("", "x").is_err());
        assert!(tags.push("nul\0", "x").is_err());
        assert!(tags.push("Ünicode", "x").is_err());
        assert!(tags.comments.is_empty());
    }

    /// A length field must be validated against the buffer before it is used to
    /// size an allocation.
    #[test]
    fn tags_reject_oversized_lengths() {
        let mut p = OpusTags::new().to_packet();
        p[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(OpusTags::parse(&p), Err(Error::InvalidStream(_))));

        let mut p = OpusTags::new().to_packet();
        let vlen = u32::from_le_bytes(p[8..12].try_into().unwrap()) as usize;
        let at = 12 + vlen;
        p[at..at + 4].copy_from_slice(&1_000_000u32.to_le_bytes());
        assert!(matches!(OpusTags::parse(&p), Err(Error::InvalidStream(_))));
    }

    #[test]
    fn tags_preserve_unknown_entries_verbatim() {
        let tags = OpusTags {
            vendor: "someone else".into(),
            comments: vec!["WEIRD".into(), "X=y=z".into(), "=empty-name".into()],
        };
        assert_eq!(OpusTags::parse(&tags.to_packet()).unwrap(), tags);
    }
}

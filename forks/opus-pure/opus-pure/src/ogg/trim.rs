//! Turning a decoded Ogg Opus stream back into exactly the audio that went in.

use super::header::{GRANULE_RATE, OpusHead};
use super::reader::OggPacket;
use crate::{Error, Result};
use std::ops::Range;

/// Trims an Ogg Opus stream's decoded output back to the original audio.
///
/// An Opus stream decodes to more samples than were encoded, at both ends, and
/// RFC 7845 describes both corrections:
///
/// - **Pre-skip** (§4.2). The first [`OpusHead::pre_skip`] samples are the
///   encoder's algorithmic delay, not audio, and are discarded.
/// - **End-trim** (§4.4). The final page's granule position may claim *fewer*
///   samples than its packets decode to. Opus codes whole frames, so a clip
///   that is not a whole number of frames long is padded on the way in, and the
///   short final granule is how the encoder says where the audio actually
///   stopped. Every file `opusenc` writes carries one.
///
/// Both are driven by what the container says rather than by counting packets:
/// a page's granule position is the number of samples decodable through the
/// last packet completed on it, so holding the output to it is an equality on
/// every page but the last, and the end-trim on the last.
///
/// Skipping the first is conspicuous: the stream starts a few milliseconds
/// late. Skipping the second is not. It appends up to one packet — 120 ms at
/// the limit, typically the tail of a 20 ms frame — of padding past the end of
/// the audio, which is inaudible on a clip played once and an audible gap at
/// every seam of a loop.
///
/// Both corrections are a few lines of arithmetic that every consumer of the
/// container would otherwise write, and both fail silently when they are
/// written wrong. `Trim` is those lines, once.
///
/// # Using it
///
/// Construct it from the header, hand it each packet with the samples that
/// packet decoded to, and write out what comes back.
///
/// ```no_run
/// use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, Trim};
///
/// let rate = 48_000;
/// let mut reader = OggOpusReader::new(std::fs::File::open("in.opus")?)?;
/// let channels = reader.head().channel_count as usize;
/// let mut decoder = reader.head().decoder(rate)?;
/// let mut trim = Trim::new(reader.head(), rate, channels)?;
///
/// let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
/// let mut pcm = Vec::new();
/// for packet in reader.packets() {
///     let packet = packet?;
///     let n = decoder.decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)?;
///     pcm.extend_from_slice(trim.keep(&packet, &block[..n * channels]));
/// }
/// # Ok::<(), opus_pure::Error>(())
/// ```
///
/// The decoder still has to decode the trimmed samples: they are part of the
/// packet, and the ones at the start are what brings its state up to date. Only
/// the output is trimmed, which is why this sits after `decode` rather than
/// filtering the packets going in.
///
/// # Scope
///
/// One logical bitstream, decoded from its first packet and starting from
/// granule zero, as [`OggOpusReader`](super::OggOpusReader) yields it. It
/// counts what it has been given and compares that against an absolute granule
/// position, so a `Trim` cannot be joined to a stream already in progress, and
/// a stream restarted from the beginning wants a fresh one alongside the
/// decoder's [`reset_state`](crate::OpusDecoder::reset_state).
///
/// It trusts the granule positions, which is the only thing it can do with a
/// forward-only reader. A file whose granules go backwards or under-claim
/// somewhere other than the end is malformed, and this keeps whichever of the
/// two — audio or granule — promises less, so such a file loses audio rather
/// than gaining silence.
#[derive(Debug, Clone)]
pub struct Trim {
    /// Interleaved channels in the PCM handed to [`keep`](Trim::keep). Not
    /// necessarily the header's: a stereo stream may be rendered to mono.
    channels: usize,
    /// Granule ticks per output sample, `48000 / sample_rate`. Every rate Opus
    /// decodes at divides 48 000, so converting either way is exact.
    ticks: u64,
    /// The header's pre-skip, at 48 kHz, as the granule positions count it.
    pre_skip_48k: u64,
    /// Samples per channel of encoder delay still to discard from the front,
    /// at the decode rate.
    skip_remaining: u64,
    /// Samples per channel handed back so far.
    emitted: u64,
}

impl Trim {
    /// Build a trimmer for a stream, decoded at `sample_rate` into `channels`
    /// interleaved channels.
    ///
    /// `sample_rate` is the rate being decoded *to*, which Opus lets a caller
    /// choose (8/12/16/24/48 kHz) and which need not be the rate the file was
    /// made at; the header's counts are at 48 kHz and are converted here.
    /// `channels` is what the decoder was built for rather than what the header
    /// declares, because the two differ when a stream is rendered to a channel
    /// count that is not its own.
    pub fn new(head: &OpusHead, sample_rate: i32, channels: usize) -> Result<Self> {
        if channels == 0 {
            return Err(Error::InvalidArgument("channels must be at least 1"));
        }
        crate::packet::check_rate(sample_rate)?;
        // Every rate Opus decodes at divides 48 000, so this is exact.
        let ticks = u64::from(GRANULE_RATE) / sample_rate as u64;
        let pre_skip_48k = u64::from(head.pre_skip);
        Ok(Trim {
            channels,
            ticks,
            pre_skip_48k,
            // Rounded *up*, and that is load-bearing rather than a taste in
            // rounding. A pre-skip that is not a whole number of output samples
            // has to become one, and the granule clamp below measures the same
            // quantity by flooring `(granule - pre_skip) / ticks`: round down
            // here and the two disagree by one sample on every page, so the
            // clamp fires on the *first* page and takes that sample out of the
            // middle of the audio instead of off the front. Rounding up makes
            // the two identities equal — `floor((n·ticks - P)/ticks)` is
            // `n - ceil(P/ticks)` — and errs toward discarding delay rather
            // than playing it.
            skip_remaining: pre_skip_48k.div_ceil(ticks),
            emitted: 0,
        })
    }

    /// The part of one packet's decoded PCM that is audio.
    ///
    /// `decoded` is interleaved output for a single packet, already cut to the
    /// sample count [`decode`](crate::OpusDecoder::decode) returned:
    /// `&block[..n * channels]`. The slice that comes back is a subrange of it,
    /// and is empty while the pre-skip is still being consumed or once the
    /// end-trim has been reached.
    ///
    /// Call it for every packet, in order, including the ones it returns
    /// nothing for: the counting is cumulative, and a packet passed over leaves
    /// the end-trim measuring from the wrong place.
    ///
    /// A caller that has to hold its position *between* calls wants
    /// [`keep_range`](Trim::keep_range), which returns the same cut as indices
    /// into `decoded`.
    ///
    /// ```
    /// use opus_pure::{OggPacket, OpusHead, Trim};
    ///
    /// // A stream of two 20 ms mono packets at 48 kHz, 312 samples of
    /// // pre-skip, whose final granule claims 1500 of the 1920 samples the two
    /// // packets decode to.
    /// let mut head = OpusHead::new(1, 48_000)?;
    /// head.pre_skip = 312;
    /// let mut trim = Trim::new(&head, 48_000, 1)?;
    /// let block = vec![0.0f32; 960];
    ///
    /// let first = OggPacket::new(vec![0xfc], 960, false);
    /// assert_eq!(trim.keep(&first, &block).len(), 960 - 312);
    ///
    /// let last = OggPacket::new(vec![0xfc], 1500, true);
    /// assert_eq!(trim.keep(&last, &block).len(), (1500 - 312) - (960 - 312));
    /// assert_eq!(trim.samples_emitted(), 1500 - 312);
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn keep<'a, T>(&mut self, packet: &OggPacket, decoded: &'a [T]) -> &'a [T] {
        let audio = self.keep_range(packet, decoded.len());
        &decoded[audio]
    }

    /// The same cut as [`keep`](Trim::keep), as indices into the decoded PCM.
    ///
    /// `decoded_len` is the length of one packet's interleaved output — what
    /// would be handed to `keep` — and the range indexes that buffer directly,
    /// so `&decoded[trim.keep_range(&packet, decoded.len())]` is exactly what
    /// `keep` returns. Both bounds count interleaved values rather than sample
    /// frames, and both land on a frame boundary.
    ///
    /// Reach for this when the position has to outlive the call — a playback
    /// path draining one packet across several buffer fills cannot hold a slice
    /// beside the buffer it borrows, and the trimmed length alone does not say
    /// where the audio starts, because the pre-skip shortens it from the front
    /// and the end-trim from the back.
    ///
    /// The same rule as `keep` applies: call it for every packet, in order,
    /// including the ones it returns an empty range for.
    ///
    /// ```
    /// use opus_pure::{OggPacket, OpusHead, Trim};
    ///
    /// // One 20 ms mono packet at 48 kHz, 312 samples of pre-skip, on a page
    /// // whose granule ends the audio early. Both ends are cut, and the range
    /// // says where — 388 values long, starting at 312.
    /// let mut head = OpusHead::new(1, 48_000)?;
    /// head.pre_skip = 312;
    /// let mut trim = Trim::new(&head, 48_000, 1)?;
    ///
    /// let only = OggPacket::new(vec![0xfc], 700, true);
    /// assert_eq!(trim.keep_range(&only, 960), 312..700);
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    pub fn keep_range(&mut self, packet: &OggPacket, decoded_len: usize) -> Range<usize> {
        debug_assert_eq!(
            decoded_len % self.channels,
            0,
            "decoded PCM is not a whole number of {}-channel frames",
            self.channels
        );
        let total = (decoded_len / self.channels) as u64;

        // Front: whatever is left of the encoder delay.
        let start = self.skip_remaining.min(total);
        self.skip_remaining -= start;
        let mut end = total;

        // Back: never hand out more audio than the page's own granule accounts
        // for. A page's granule *is* the count of samples decodable through the
        // last packet completed on it, so on every page but the final one this
        // is an equality and clamps nothing; the final page is the one allowed
        // to under-claim, and that under-claim is the end-trim.
        //
        // The rule is the granule rather than `end_of_stream` because the flag
        // is the narrower signal: a muxer that ends with a bare EOS page
        // carrying no packet leaves no packet flagged at all, and gating on the
        // flag would silently keep that file's padding. A page that completed
        // no packet reports -1 and says nothing. A granule claiming *more* than
        // the packets carry is a broken file, and `playable < after` does not
        // fire for it — the audio that exists wins over the promise.
        if packet.page_granule >= 0 {
            let playable =
                (packet.page_granule as u64).saturating_sub(self.pre_skip_48k) / self.ticks;
            end = start + playable.saturating_sub(self.emitted).min(end - start);
        }

        self.emitted += end - start;
        start as usize * self.channels..end as usize * self.channels
    }

    /// Samples per channel handed back so far.
    ///
    /// After the last packet this is the stream's true length, the number the
    /// audio had before it was encoded — which is what makes it worth asserting
    /// on in a round-trip test.
    pub fn samples_emitted(&self) -> u64 {
        self.emitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A head with a chosen pre-skip; nothing else here reads the other fields.
    fn head(pre_skip: u16, channels: u8) -> OpusHead {
        let mut h = OpusHead::new(channels, 48_000).unwrap();
        h.pre_skip = pre_skip;
        h
    }

    /// `n` samples per channel of interleaved PCM, each value its own index so a
    /// trimmed slice can be checked for *which* samples survived, not just how
    /// many.
    fn pcm(n: usize, channels: usize) -> Vec<f32> {
        (0..n * channels).map(|i| i as f32).collect()
    }

    #[test]
    fn pre_skip_is_taken_from_the_front_of_the_first_packet() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        assert_eq!(kept.len(), 648);
        // The samples kept are the *last* 648, not the first.
        assert_eq!(kept[0], 312.0);
        assert_eq!(t.samples_emitted(), 648);
    }

    #[test]
    fn a_pre_skip_longer_than_one_packet_spans_packets() {
        // 3120 samples of delay is more than three 960-sample packets.
        let mut t = Trim::new(&head(3120, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        for i in 1..=3 {
            let p = OggPacket::new(vec![0xfc], 960 * i, false);
            assert_eq!(t.keep(&p, &block).len(), 0, "packet {i}");
        }
        let kept = t.keep(&OggPacket::new(vec![0xfc], 3840, false), &block);
        assert_eq!(kept.len(), 960 - 240);
        assert_eq!(kept[0], 240.0);
    }

    #[test]
    fn the_final_granule_trims_the_end() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        // 1500 - 312 = 1188 playable, of which 648 have already been handed out.
        let kept = t.keep(&OggPacket::new(vec![0xfc], 1500, true), &block);
        assert_eq!(kept.len(), 1188 - 648);
        assert_eq!(
            kept[0], 0.0,
            "the end-trim takes from the tail, not the head"
        );
        assert_eq!(t.samples_emitted(), 1188);
    }

    #[test]
    fn an_exact_final_granule_trims_nothing() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 1920, true), &block);
        assert_eq!(kept.len(), 960);
        assert_eq!(t.samples_emitted(), 1920 - 312);
    }

    /// A granule *above* the decodable count is a broken file. Trimming to it
    /// would mean inventing samples, so the packets win.
    #[test]
    fn a_granule_that_over_claims_does_not_extend_the_audio() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 99_999, true), &block);
        assert_eq!(kept.len(), 648);
    }

    /// The end-trim can land inside a packet that is entirely padding.
    #[test]
    fn a_final_packet_can_be_trimmed_away_completely() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 960, true), &block);
        assert_eq!(kept.len(), 0);
        assert_eq!(t.samples_emitted(), 648);
    }

    /// A stream whose granule never gets past the pre-skip carries no audio at
    /// all, and must not underflow its way into a huge count.
    #[test]
    fn a_stream_shorter_than_its_own_pre_skip_yields_nothing() {
        let mut t = Trim::new(&head(3120, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 960, true), &block);
        assert_eq!(kept.len(), 0);
        assert_eq!(t.samples_emitted(), 0);
    }

    /// A page's granule is the decodable count through its last packet, so on
    /// every page but the final one it matches exactly and clamps nothing —
    /// including when several packets share one page and the granule therefore
    /// runs ahead of the packets before the last.
    #[test]
    fn an_ordinary_page_granule_clamps_nothing() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        // Four packets completing on one page, whose granule is the count after
        // the fourth.
        for i in 0..4 {
            let p = OggPacket::new(vec![0xfc], 3840, false);
            let kept = t.keep(&p, &block);
            assert_eq!(kept.len(), if i == 0 { 648 } else { 960 }, "packet {i}");
        }
        assert_eq!(t.samples_emitted(), 3840 - 312);
    }

    /// The end-trim follows the granule, not the end-of-stream flag: a muxer
    /// that ends with a bare EOS page leaves no packet flagged, and the trim on
    /// the last page that did carry one still has to be honoured.
    #[test]
    fn the_trim_is_taken_from_an_unflagged_final_page() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 1500, false), &block);
        assert_eq!(kept.len(), (1500 - 312) - 648);
        assert_eq!(t.samples_emitted(), 1500 - 312);
    }

    /// A page that completed no packet reports -1, which says nothing about
    /// where the audio ends.
    #[test]
    fn an_absent_granule_trims_nothing() {
        let mut t = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let block = pcm(960, 1);
        let kept = t.keep(&OggPacket::new(vec![0xfc], -1, true), &block);
        assert_eq!(kept.len(), 648);
    }

    /// What `keep_range` exists for: a packet cut at both ends hands back a
    /// slice whose length says how much survived and not which part, and here
    /// 388 could equally be the first 388 samples or the last.
    #[test]
    fn keep_range_locates_a_cut_that_a_length_cannot() {
        let mut by_range = Trim::new(&head(312, 1), 48_000, 1).unwrap();
        let mut by_slice = by_range.clone();
        let block = pcm(960, 1);
        let only = OggPacket::new(vec![0xfc], 700, true);

        let audio = by_range.keep_range(&only, block.len());
        assert_eq!(audio, 312..700);
        assert_eq!(by_slice.keep(&only, &block), &block[audio]);
        assert_eq!(by_range.samples_emitted(), by_slice.samples_emitted());
    }

    /// The range indexes interleaved values, so it drops straight into the
    /// buffer that was decoded into.
    #[test]
    fn keep_range_indexes_values_not_frames() {
        let mut t = Trim::new(&head(312, 2), 48_000, 2).unwrap();
        assert_eq!(
            t.keep_range(&OggPacket::new(vec![0xfc], 960, false), 960 * 2),
            624..1920
        );
    }

    /// Both counts are per channel; the slicing is interleaved.
    #[test]
    fn stereo_counts_sample_frames_not_values() {
        let mut t = Trim::new(&head(312, 2), 48_000, 2).unwrap();
        let block = pcm(960, 2);
        let kept = t.keep(&OggPacket::new(vec![0xfc], 960, false), &block);
        assert_eq!(kept.len(), 648 * 2);
        assert_eq!(
            kept[0], 624.0,
            "trimmed at a frame boundary, not a value one"
        );
        assert_eq!(t.samples_emitted(), 648);
    }

    /// Granule positions stay at 48 kHz however the caller decodes; a 16 kHz
    /// decode gets a third as many samples for the same granule.
    #[test]
    fn a_lower_decode_rate_scales_both_ends() {
        let mut t = Trim::new(&head(312, 1), 16_000, 1).unwrap();
        let block = pcm(320, 1); // 20 ms at 16 kHz
        assert_eq!(
            t.keep(&OggPacket::new(vec![0xfc], 960, false), &block)
                .len(),
            216
        );
        // 1500 - 312 = 1188 at 48 kHz is 396 at 16 kHz.
        let kept = t.keep(&OggPacket::new(vec![0xfc], 1500, true), &block);
        assert_eq!(kept.len(), 396 - 216);
        assert_eq!(t.samples_emitted(), 396);
    }

    /// A pre-skip that is not a whole number of samples at the decode rate must
    /// still leave every page's granule an exact match.
    ///
    /// This is the case that made the rounding in `new` matter. `pre_skip` is
    /// counted at 48 kHz, so at 8 kHz it converts through a factor of six and
    /// 313 is not a whole number of output samples. Rounding it *down* leaves
    /// the front skip one sample short of what flooring `(granule - pre_skip)`
    /// asks for, the clamp fires on the very first page, and the sample comes
    /// out of the middle of the audio as a one-sample discontinuity — while the
    /// total length stays right, so a round-trip test sees nothing.
    #[test]
    fn a_pre_skip_that_does_not_divide_the_rate_still_clamps_nowhere() {
        for (rate, pre_skip) in [
            (8_000i32, 313u16),
            (8_000, 317),
            (16_000, 316),
            (24_000, 317),
            (12_000, 313),
        ] {
            let ticks = 48_000 / rate as u64;
            let n = (rate / 50) as usize; // 20 ms at the decode rate
            let mut t = Trim::new(&head(pre_skip, 1), rate, 1).unwrap();
            let block = pcm(n, 1);
            let skip = u64::from(pre_skip).div_ceil(ticks) as usize;

            for k in 1..=5u64 {
                let p = OggPacket::new(vec![0xfc], (k * 960) as i64, k == 5);
                let kept = t.keep(&p, &block);
                let label = format!("{rate} Hz, pre-skip {pre_skip}: packet {k}");

                // Length alone proves nothing here: flooring the pre-skip and
                // then losing a sample to the clamp gives the *same* count as
                // rounding up and losing none, and the same total at the end.
                // What separates them is which samples came back. `pcm` fills
                // each sample with its own index, so the last value says whether
                // the tail of the packet survived, and the first says where the
                // front skip landed.
                assert_eq!(
                    *kept.last().unwrap(),
                    (n - 1) as f32,
                    "{label} was clamped at the tail"
                );
                let want_first = if k == 1 { skip } else { 0 };
                assert_eq!(
                    kept[0], want_first as f32,
                    "{label} starts in the wrong place"
                );
                assert_eq!(kept.len(), n - want_first, "{label} length");
            }
            assert_eq!(t.samples_emitted() as usize, 5 * n - skip);
        }
    }

    #[test]
    fn a_rate_opus_cannot_decode_at_is_rejected() {
        assert!(Trim::new(&head(312, 1), 44_100, 1).is_err());
        assert!(Trim::new(&head(312, 1), 48_000, 0).is_err());
    }
}

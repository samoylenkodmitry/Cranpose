//! Ogg Opus muxer.

use std::io::Write;

use super::header::{GRANULE_RATE, OpusHead, OpusTags};
use super::page::{HeaderType, MAX_SEGMENTS, lacing_values, write_page};
use crate::{Error, Result};

/// Payload bytes to accumulate before flushing a page.
///
/// RFC 3533 allows up to 65025, but pages that large cost a lot of audio on a
/// single CRC failure and delay the first decodable output. ~4 KiB is what the
/// reference `opusenc` settles on, and it keeps per-page overhead near 0.7%.
const DEFAULT_PAGE_TARGET: usize = 4096;

/// Writes Opus packets into an Ogg stream (RFC 7845).
///
/// The two header pages are written by the constructor, so a stream is well
/// formed from the first call. Audio packets accumulate into pages and are
/// flushed on size; [`finish`](OggOpusWriter::finish) writes the trailing page
/// with the end-of-stream flag and **must** be called — dropping the writer
/// leaves the stream without its EOS page.
///
/// ```no_run
/// use opus_pure::{OggOpusWriter, OpusHead};
///
/// let file = std::fs::File::create("out.opus")?;
/// let mut w = OggOpusWriter::new(file, OpusHead::new(2, 48_000)?)?;
/// // w.write_packet(&packet)?;  // duration read from the packet itself
/// w.finish()?;
/// # Ok::<(), opus_pure::Error>(())
/// ```
///
/// # Ending the file where the audio ends
///
/// Opus codes whole frames, so a clip that is not a whole number of frames long
/// is padded with silence on the way in and decodes longer than it started.
/// RFC 7845 §4.4 fixes that with an end-trim: the final granule position
/// deliberately claims fewer samples than the packets carry, and a player stops
/// there. [`write_packet_with_duration`](OggOpusWriter::write_packet_with_duration)
/// is how this writer states it, and [`examples/encode.rs`][ex] works the
/// arithmetic through. Without it a round trip comes back up to one frame long,
/// with the encoder delay lost off the end instead — audible at the seam of a
/// loop, and the reason the padding is worth getting right.
///
/// [ex]: https://github.com/stephenberry/opus-pure/blob/main/examples/encode.rs
pub struct OggOpusWriter<W: Write> {
    /// `None` only after `finish` has taken it.
    sink: Option<W>,
    serial: u32,
    sequence: u32,
    page_target: usize,

    /// Lacing values for the page being assembled.
    segments: Vec<u8>,
    /// Payload bytes matching `segments`.
    payload: Vec<u8>,
    /// Running granule position: the number of 48 kHz samples a decoder can
    /// produce from every packet written so far. The pre-skip samples are the
    /// first `pre_skip` of those, so they are already counted here and must
    /// **not** be added on top (RFC 7845 §4).
    granule: i64,
    /// Granule of the last packet that *completed* on the page being assembled.
    /// `None` when no packet has completed here, which the page must report
    /// as `-1`.
    page_granule: Option<i64>,
    /// The page being assembled resumes a packet started on the previous one.
    continued: bool,
    finished: bool,
    /// Scratch, reused so steady-state muxing does not allocate.
    scratch: Vec<u8>,
}

/// Shows how much of the stream has been written, without requiring
/// `W: Debug`, for the reason given on [`OggOpusReader`](super::OggOpusReader).
impl<W: Write> std::fmt::Debug for OggOpusWriter<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OggOpusWriter")
            .field("serial", &format_args!("{:#010x}", self.serial))
            .field("pages_written", &self.sequence)
            .field("granule", &self.granule)
            .field("pending_bytes", &self.payload.len())
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl<W: Write> OggOpusWriter<W> {
    /// Start a stream, writing the `OpusHead` and a default `OpusTags` page.
    pub fn new(sink: W, head: OpusHead) -> Result<Self> {
        Self::with_tags(sink, head, OpusTags::new())
    }

    /// Start a stream with caller-supplied tags.
    ///
    /// The serial number is derived from the header content, so encoding the
    /// same audio twice produces byte-identical files. Multiplexing several
    /// logical streams into one physical stream needs distinct serials —
    /// construct with [`with_serial`](OggOpusWriter::with_serial).
    pub fn with_tags(sink: W, head: OpusHead, tags: OpusTags) -> Result<Self> {
        let serial = derive_serial(&head, &tags);
        Self::with_serial(sink, head, tags, serial)
    }

    /// Start a stream with an explicit serial number.
    pub fn with_serial(sink: W, head: OpusHead, tags: OpusTags, serial: u32) -> Result<Self> {
        let mut w = OggOpusWriter {
            sink: Some(sink),
            serial,
            sequence: 0,
            page_target: DEFAULT_PAGE_TARGET,
            segments: Vec::with_capacity(MAX_SEGMENTS),
            payload: Vec::with_capacity(DEFAULT_PAGE_TARGET + 255),
            // A page's granule counts decoder output samples, and the pre-skip
            // samples are the first of those — they arrive in the first packets
            // and are counted as those packets are written. Seeding the counter
            // with `pre_skip` would claim `pre_skip` samples that no packet
            // carries, leaving the final granule past the end of the audio.
            granule: 0,
            page_granule: None,
            continued: false,
            finished: false,
            scratch: Vec::new(),
        };

        // §5: OpusHead sits alone on the first page, and OpusTags starts a new
        // page. Both carry granule position 0.
        w.push_packet_bytes(&head.to_packet())?;
        w.flush_page(HeaderType::BOS, 0)?;
        w.push_packet_bytes(&tags.to_packet())?;
        w.flush_page(0, 0)?;
        Ok(w)
    }

    /// The serial number identifying this logical stream, as written into every
    /// page header. Derived from the header content unless the writer was built
    /// with [`with_serial`](OggOpusWriter::with_serial).
    pub fn serial(&self) -> u32 {
        self.serial
    }

    /// The granule position the stream has reached: the 48 kHz samples every
    /// packet written so far decodes to, pre-skip included (RFC 7845 §4).
    ///
    /// This is what makes an end-trim a subtraction rather than a derivation.
    /// The final granule a gapless file wants is the header's pre-skip plus the
    /// audio; read this immediately before writing the last packet and the
    /// difference is that packet's duration:
    ///
    /// ```no_run
    /// # use opus_pure::{OggOpusWriter, OpusHead};
    /// # let (head, packet, audio_48k) = (OpusHead::new(1, 48_000)?, &[0xfcu8][..], 96_000u64);
    /// # let mut writer = OggOpusWriter::new(Vec::new(), head.clone())?;
    /// let final_granule = u64::from(head.pre_skip) + audio_48k;
    /// let duration = final_granule - writer.granule() as u64;
    /// writer.write_packet_with_duration(packet, duration as u32)?;
    /// # Ok::<(), opus_pure::Error>(())
    /// ```
    ///
    /// Computing the same number from a frame count instead would work only for
    /// a stream whose packets were all the same duration, which Opus does not
    /// require and which one call to
    /// [`write_packet`](Self::write_packet) with a different frame size breaks.
    pub fn granule(&self) -> i64 {
        self.granule
    }

    /// Payload bytes to accumulate before a page is flushed, clamped to what a
    /// page can hold. Larger pages mean less framing overhead and coarser
    /// recovery from a corrupt page.
    pub fn set_page_target(&mut self, bytes: usize) {
        self.page_target = bytes.clamp(1, super::page::MAX_PAGE_PAYLOAD);
    }

    /// Append one Opus packet, taking its duration from the packet itself.
    ///
    /// The duration advances the granule position, which is what a player uses
    /// to seek and to know when the stream ends. It is not something a caller
    /// should have to supply: an Opus packet states its own duration in its TOC
    /// byte, and this reads it with [`packet::samples_48k`](crate::packet::samples_48k).
    ///
    /// Use [`write_packet_with_duration`](Self::write_packet_with_duration)
    /// only when the granule must deliberately differ from the audio — the
    /// end-trim of RFC 7845 §4.4, where the final page reports less audio than
    /// its packets carry.
    pub fn write_packet(&mut self, packet: &[u8]) -> Result<()> {
        let samples_48k = crate::packet::samples_48k(packet)? as u32;
        self.write_packet_with_duration(packet, samples_48k)
    }

    /// Append one Opus packet, stating its duration explicitly.
    ///
    /// `samples_48k` is the packet's duration in 48 kHz samples — 960 for a
    /// 20 ms frame — regardless of the rate the encoder ran at. Prefer
    /// [`write_packet`](Self::write_packet), which reads the same number out of
    /// the packet and cannot get it wrong; a value that disagrees with the
    /// audio produces a file that decodes correctly but seeks and reports its
    /// duration wrongly. The reason to use this one is the end-trim of RFC 7845
    /// §4.4: a final granule deliberately short of the audio, so a clip that is
    /// not a whole number of frames ends where it should.
    pub fn write_packet_with_duration(&mut self, packet: &[u8], samples_48k: u32) -> Result<()> {
        if self.finished {
            return Err(Error::InvalidArgument("writer has already been finished"));
        }
        if packet.is_empty() {
            return Err(Error::InvalidArgument("Opus packets cannot be empty"));
        }
        // A packet may carry at most 120 ms, and the granule must stay in range
        // for the i64 the page header holds.
        if samples_48k > 120 * GRANULE_RATE / 1000 {
            return Err(Error::InvalidArgument(
                "an Opus packet cannot exceed 120 ms (5760 samples at 48 kHz)",
            ));
        }

        // Flush a full page *before* the next packet rather than after the last,
        // so `finish` always has something to put on its EOS page.
        if self.payload.len() >= self.page_target || self.segments.len() == MAX_SEGMENTS {
            let g = self.page_granule_or_none();
            self.flush_page(0, g)?;
        }

        self.push_packet_bytes(packet)?;
        self.granule += i64::from(samples_48k);
        self.page_granule = Some(self.granule);
        Ok(())
    }

    /// The sink being written to.
    pub fn get_ref(&self) -> Option<&W> {
        self.sink.as_ref()
    }

    /// The sink being written to, mutably.
    ///
    /// Writing to it directly corrupts the stream; this is for the sink's own
    /// controls, like asking a file for its handle.
    pub fn get_mut(&mut self) -> Option<&mut W> {
        self.sink.as_mut()
    }

    /// Flush the final page with the end-of-stream flag and return the sink.
    pub fn finish(mut self) -> Result<W> {
        self.finish_in_place()?;
        self.sink
            .take()
            .ok_or(Error::Internal("ogg writer sink taken twice"))
    }

    fn finish_in_place(&mut self) -> Result<()> {
        if self.finished || self.sink.is_none() {
            return Ok(());
        }
        self.finished = true;
        if self.segments.is_empty() {
            // No audio was written. The stream still needs an EOS page, and a
            // page must carry at least one lacing value; a single zero-length
            // segment is the conventional empty page.
            self.segments.push(0);
        }
        let g = self.page_granule_or_none();
        self.flush_page(HeaderType::EOS, g)?;
        if let Some(sink) = self.sink.as_mut() {
            sink.flush()?;
        }
        Ok(())
    }

    fn page_granule_or_none(&self) -> i64 {
        self.page_granule.unwrap_or(-1)
    }

    /// Append a packet's segments and bytes, flushing whenever the 255-value
    /// segment table fills mid-packet — the remainder then continues on the
    /// next page.
    fn push_packet_bytes(&mut self, packet: &[u8]) -> Result<()> {
        let mut off = 0usize;
        for lace in lacing_values(packet.len()) {
            if self.segments.len() == MAX_SEGMENTS {
                // The packet is unfinished, so this page completes nothing new:
                // report the granule of the last packet that *did* complete.
                let g = self.page_granule_or_none();
                self.flush_page(0, g)?;
                self.continued = true;
            }
            self.segments.push(lace);
            self.payload
                .extend_from_slice(&packet[off..off + lace as usize]);
            off += lace as usize;
        }
        Ok(())
    }

    fn flush_page(&mut self, flags: u8, granule: i64) -> Result<()> {
        if self.segments.is_empty() {
            return Ok(());
        }
        let header_type = flags
            | if self.continued {
                HeaderType::CONTINUED
            } else {
                0
            };

        self.scratch.clear();
        write_page(
            header_type,
            granule,
            self.serial,
            self.sequence,
            &self.segments,
            &self.payload,
            &mut self.scratch,
        );
        match self.sink.as_mut() {
            Some(sink) => sink.write_all(&self.scratch)?,
            None => return Err(Error::Internal("ogg writer used after finish")),
        }

        self.sequence += 1;
        self.segments.clear();
        self.payload.clear();
        self.page_granule = None;
        self.continued = false;
        Ok(())
    }
}

/// A deterministic serial derived from the header packets.
///
/// RFC 3533 wants serials that are unlikely to collide when streams are
/// multiplexed; it does not require randomness. Deriving them keeps output
/// reproducible, which is worth more here than collision resistance a caller can
/// get explicitly from [`OggOpusWriter::with_serial`].
fn derive_serial(head: &OpusHead, tags: &OpusTags) -> u32 {
    // FNV-1a, 32-bit.
    let mut h: u32 = 0x811c_9dc5;
    for b in head.to_packet().iter().chain(tags.to_packet().iter()) {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    // 0 is legal but conventionally avoided as a "unset" sentinel.
    if h == 0 { 1 } else { h }
}

impl<W: Write> Drop for OggOpusWriter<W> {
    fn drop(&mut self) {
        // Best-effort: a stream missing its EOS page is still mostly playable,
        // and `finish` is the supported way to learn whether the write worked.
        let _ = self.finish_in_place();
    }
}

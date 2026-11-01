//! Ogg Opus demuxer.

use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;

use super::header::{OpusHead, OpusTags};
use super::page::{
    CAPTURE_PATTERN, HEADER_LEN, MAX_PAGE_PAYLOAD, MAX_SEGMENTS, PageHeader, verify_crc,
};
use crate::{Error, Result};

/// Largest packet the reader will reassemble from continued pages, 16 MiB.
///
/// A packet's own framing bounds its frames but not its padding, so nothing in
/// the format stops a chain of continued pages from growing the reader's
/// packet buffer for as long as the source keeps delivering. This is far above
/// any packet an encoder produces (a 120 ms packet at the highest rate is under
/// 8 KiB) and exists only so a hostile stream cannot make the reader allocate
/// without limit.
pub(crate) const MAX_OGG_PACKET_BYTES: usize = 16 * 1024 * 1024;

/// One Opus packet recovered from the container.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OggPacket {
    /// The packet, ready to hand to a decoder.
    pub data: Vec<u8>,
    /// Granule position of the page this packet completed on, or `-1` if that
    /// page completed no packet. This is a *page* property: several packets
    /// completing on one page all report the same value, which is the granule
    /// after the last of them.
    pub page_granule: i64,
    /// The packet completed the final page of the stream.
    pub end_of_stream: bool,
}

/// An empty packet, for [`read_packet_into`](OggOpusReader::read_packet_into)
/// to fill: no data, a `page_granule` of `-1` (no page read yet), and not the
/// end of the stream.
impl Default for OggPacket {
    fn default() -> Self {
        OggPacket::new(Vec::new(), -1, false)
    }
}

impl OggPacket {
    /// Build a packet directly, without a container to read it out of.
    ///
    /// The reader produces these; this exists so code that *consumes* them can
    /// be tested without muxing a file first. The interesting logic on the
    /// consuming side is what a caller does with `page_granule` and
    /// `end_of_stream` — the end-trim arithmetic of RFC 7845 §4.4, which
    /// [`Trim`](super::Trim) implements — and a test for it should be able to
    /// state the two edge cases directly rather than construct a stream that
    /// happens to produce them.
    ///
    /// ```
    /// use opus_pure::OggPacket;
    ///
    /// // The last packet of a stream whose final granule trims 160 samples.
    /// let packet = OggPacket::new(vec![0xfc], 48_000, true);
    /// assert!(packet.end_of_stream);
    /// ```
    pub fn new(data: Vec<u8>, page_granule: i64, end_of_stream: bool) -> Self {
        OggPacket {
            data,
            page_granule,
            end_of_stream,
        }
    }
}

/// Reads Opus packets out of an Ogg stream (RFC 7845).
///
/// The constructor consumes the two header packets, so [`head`](Self::head) and
/// [`tags`](Self::tags) are available immediately and
/// [`read_packet`](Self::read_packet) yields audio from the first call.
///
/// Pages failing their CRC are an error rather than a silent skip: a truncated
/// or corrupt file should not decode as if it were fine.
///
/// ```no_run
/// use opus_pure::OggOpusReader;
///
/// let file = std::fs::File::open("in.opus")?;
/// let mut r = OggOpusReader::new(file)?;
/// println!("{} channels, pre-skip {}", r.head().channel_count, r.head().pre_skip);
/// while let Some(packet) = r.read_packet()? {
///     // decoder.decode(&packet.data, ...)
///     let _ = packet;
/// }
/// # Ok::<(), opus_pure::Error>(())
/// ```
///
/// [`head().decoder(rate)`](OpusHead::decoder) builds a decoder configured for
/// the stream, and [`Trim`](super::Trim) turns its output back into the audio
/// that was encoded. Decoding without that second step leaves the encoder delay
/// on the front and the final page's end-trim on the back.
///
/// # Reading without allocating
///
/// [`read_packet`](Self::read_packet) hands out a new `Vec` per packet.
/// [`read_packet_into`](Self::read_packet_into) fills one the caller keeps
/// instead, and the reader reuses its own buffers, so once the largest page
/// and packet have been seen a playback loop reads without touching the heap.
///
/// # Reading forward
///
/// This reads forward from the first audio packet and does not seek to an
/// arbitrary point. Playing a stream again means going back to the start:
/// [`rewind`](Self::rewind) does that for a source that can seek, without
/// reading the header pages again.
///
/// ```no_run
/// use opus_pure::{OggOpusReader, OggPacket};
///
/// let mut reader = OggOpusReader::new(std::fs::File::open("in.opus")?)?;
/// let mut packet = OggPacket::default();
/// while reader.read_packet_into(&mut packet)? {
///     // decoder.decode(&packet.data, ...)
/// }
/// reader.rewind()?;   // back at the first audio packet
/// # Ok::<(), opus_pure::Error>(())
/// ```
///
/// A decoder carried across that boundary needs
/// [`reset_state`](crate::OpusDecoder::reset_state), and the
/// [`Trim`](super::Trim) needs replacing, or the second pass begins with the
/// first one's state and counts.
pub struct OggOpusReader<R: Read> {
    source: Counted<R>,
    head: OpusHead,
    tags: OpusTags,
    serial: u32,

    /// The packets completed on the last page read, back to back, followed by
    /// the start of any packet that continues onto the next page. Reused for
    /// the whole stream: a page is read straight into its tail.
    packets: Vec<u8>,
    /// Where each completed packet in `packets` ends, oldest first. A page
    /// completes at most one packet per segment, and this is reserved for that
    /// many up front.
    ends: Vec<usize>,
    /// How many of `ends` have been handed out.
    taken: usize,
    /// Granule position of the last page read.
    page_granule: i64,
    /// The last packet in `ends` completed the final page of the stream.
    last_is_eos: bool,
    /// The last page carried the end-of-stream flag.
    saw_eos: bool,
    /// The source returned EOF.
    exhausted: bool,
    /// Where in the source the first audio page starts, which is where
    /// [`rewind`](Self::rewind) goes back to.
    audio_start: u64,
    /// `saw_eos` as it stood there: set when the header pages ended the
    /// stream, so a rewind does not read on past its end.
    audio_start_eos: bool,
}

/// Shows where the reader has got to in the stream.
///
/// Deliberately does not require `R: Debug`: the source is a file or a socket
/// far more often than it is something printable, and requiring it would leave
/// most readers with no `Debug` at all.
impl<R: Read> std::fmt::Debug for OggOpusReader<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OggOpusReader")
            .field("head", &self.head)
            .field("serial", &format_args!("{:#010x}", self.serial))
            .field("packets_ready", &(self.ends.len() - self.taken))
            .field("saw_eos", &self.saw_eos)
            .field("exhausted", &self.exhausted)
            .finish_non_exhaustive()
    }
}

impl<R: Read> OggOpusReader<R> {
    /// Read the header packets and position the reader at the first audio
    /// packet.
    ///
    /// Refuses a stream whose comment header does not finish its page: RFC
    /// 7845 §3 requires the audio to start on a page of its own, and
    /// libopusfile refuses the same streams.
    pub fn new(source: R) -> Result<Self> {
        let mut r = OggOpusReader {
            source: Counted {
                inner: source,
                position: 0,
            },
            head: OpusHead::new(1, 0)?,
            tags: OpusTags::new(),
            serial: 0,
            packets: Vec::new(),
            ends: Vec::with_capacity(MAX_SEGMENTS),
            taken: 0,
            page_granule: -1,
            last_is_eos: false,
            saw_eos: false,
            exhausted: false,
            audio_start: 0,
            audio_start_eos: false,
        };

        let first = r
            .next_packet()?
            .ok_or(Error::InvalidStream("stream ends before OpusHead"))?;
        r.head = OpusHead::parse(&r.packets[first])?;

        let second = r
            .next_packet()?
            .ok_or(Error::InvalidStream("stream ends before OpusTags"))?;
        r.tags = OpusTags::parse(&r.packets[second])?;

        // RFC 7845 §3: the comment header finishes the page it completes on,
        // so audio starts on a page of its own. That page boundary is what
        // `rewind` returns to, and libopusfile refuses a stream without it.
        if r.taken < r.ends.len() || r.packets.len() > r.partial_start() {
            return Err(Error::InvalidStream(
                "comment header does not finish its page",
            ));
        }
        r.audio_start = r.source.position;
        r.audio_start_eos = r.saw_eos;

        Ok(r)
    }

    /// The stream's identification header.
    pub fn head(&self) -> &OpusHead {
        &self.head
    }

    /// The stream's comment header.
    pub fn tags(&self) -> &OpusTags {
        &self.tags
    }

    /// Serial number of the logical bitstream being read.
    pub fn serial(&self) -> u32 {
        self.serial
    }

    /// The next audio packet, or `None` at end of stream.
    ///
    /// Each packet is a new `Vec`; [`read_packet_into`](Self::read_packet_into)
    /// reuses one instead.
    pub fn read_packet(&mut self) -> Result<Option<OggPacket>> {
        let Some(range) = self.next_packet()? else {
            return Ok(None);
        };
        Ok(Some(OggPacket {
            data: self.packets[range].to_vec(),
            page_granule: self.page_granule,
            end_of_stream: self.completed_eos(),
        }))
    }

    /// Read the next audio packet into `packet`, reusing its `data` buffer.
    ///
    /// Returns `false` at end of stream. `packet` is overwritten only when this
    /// returns `true`, and its `data` grows only when a packet is longer than
    /// any it has held, so a caller keeping one `OggPacket` for a whole stream
    /// reads it without allocating once the longest packet has passed.
    pub fn read_packet_into(&mut self, packet: &mut OggPacket) -> Result<bool> {
        let Some(range) = self.next_packet()? else {
            return Ok(false);
        };
        packet.data.clear();
        packet.data.extend_from_slice(&self.packets[range]);
        packet.page_granule = self.page_granule;
        packet.end_of_stream = self.completed_eos();
        Ok(true)
    }

    /// The remaining audio packets, as an iterator.
    ///
    /// The same packets [`read_packet`](Self::read_packet) yields, in a form
    /// that composes: `for`, `take_while`, `filter`, or
    /// `collect::<Result<Vec<_>>>()` to stop at the first error.
    ///
    /// ```
    /// # use opus_pure::{OggOpusReader, Result};
    /// # fn f(bytes: &[u8]) -> Result<()> {
    /// let mut reader = OggOpusReader::new(std::io::Cursor::new(bytes))?;
    /// for packet in reader.packets() {
    ///     let packet = packet?;
    ///     // ... decode packet.data
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The iterator ends at the first error as well as at end of stream, so a
    /// truncated file stops rather than looping.
    pub fn packets(&mut self) -> Packets<'_, R> {
        Packets {
            reader: self,
            done: false,
        }
    }

    /// The underlying reader, giving up the ability to read further packets.
    pub fn into_inner(self) -> R {
        self.source.inner
    }

    /// The underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.source.inner
    }

    /// The next packet as a range of `packets`, reading pages until one
    /// completes or the stream ends.
    fn next_packet(&mut self) -> Result<Option<Range<usize>>> {
        loop {
            if self.taken < self.ends.len() {
                let start = if self.taken == 0 {
                    0
                } else {
                    self.ends[self.taken - 1]
                };
                let end = self.ends[self.taken];
                self.taken += 1;
                return Ok(Some(start..end));
            }
            if self.saw_eos || self.exhausted {
                // A packet still pending was cut off by the end of the stream;
                // report that rather than returning it as if complete.
                if self.packets.len() > self.partial_start() {
                    self.packets.clear();
                    self.ends.clear();
                    self.taken = 0;
                    return Err(Error::InvalidStream(
                        "stream ends in the middle of a packet",
                    ));
                }
                return Ok(None);
            }
            self.read_page()?;
        }
    }

    /// Where the packet still being reassembled starts in `packets`.
    fn partial_start(&self) -> usize {
        self.ends.last().copied().unwrap_or(0)
    }

    /// The packet [`next_packet`](Self::next_packet) last returned completed
    /// the final page of the stream.
    fn completed_eos(&self) -> bool {
        self.last_is_eos && self.taken == self.ends.len()
    }

    /// Read one page and split it into packets, replacing the handed-out ones
    /// in `packets` and `ends`.
    fn read_page(&mut self) -> Result<()> {
        debug_assert_eq!(self.taken, self.ends.len());
        // Every completed packet has been handed out; keep only the one still
        // being reassembled, moved to the front.
        self.packets.drain(..self.partial_start());
        self.ends.clear();
        self.taken = 0;
        self.last_is_eos = false;

        let Some(raw) = self.read_page_header()? else {
            self.exhausted = true;
            return Ok(());
        };
        let header = PageHeader::parse(&raw)?;

        if header.segment_count == 0 {
            return Err(Error::InvalidStream("page has an empty segment table"));
        }

        let mut segments_arr = [0u8; MAX_SEGMENTS];
        let segments = &mut segments_arr[..header.segment_count as usize];
        read_exact(&mut self.source, segments)?;
        let payload_len: usize = segments.iter().map(|&s| s as usize).sum();
        debug_assert!(payload_len <= MAX_PAGE_PAYLOAD);

        // The payload goes straight after the pending bytes: a page's packets
        // are its payload cut at the terminating segments, so it needs no
        // copying once read.
        let base = self.packets.len();
        self.packets.resize(base + payload_len, 0);
        if let Err(e) = read_exact(&mut self.source, &mut self.packets[base..]) {
            self.packets.truncate(base);
            return Err(e);
        }
        if !verify_crc(&raw, segments, &self.packets[base..], header.crc) {
            self.packets.truncate(base);
            return Err(Error::InvalidStream("page CRC mismatch"));
        }

        if header.is_bos() {
            self.serial = header.serial;
        } else if header.serial != self.serial {
            // Multiplexed or chained streams are out of scope: a second logical
            // stream would need its own decoder state and pre-skip.
            self.packets.truncate(base);
            return Err(Error::InvalidStream(
                "stream contains more than one logical bitstream",
            ));
        }

        // A page that claims to continue a packet when none is pending — or that
        // starts fresh while one is pending — means pages were lost or reordered.
        if header.is_continued() == (base == 0) {
            self.packets.clear();
            return Err(Error::InvalidStream(
                "page continuation flag does not match the pending packet",
            ));
        }

        self.saw_eos = header.is_eos();
        self.page_granule = header.granule_position;

        let mut start = 0usize;
        let mut end = base;
        for (i, &lace) in segments.iter().enumerate() {
            end += lace as usize;
            if end - start > MAX_OGG_PACKET_BYTES {
                self.packets.truncate(start);
                return Err(Error::InvalidStream("packet exceeds maximum allowed size"));
            }
            if lace < 255 {
                // Terminating segment: the packet is complete. An empty one is
                // dropped: muxers emit one as the payload of a bare EOS page,
                // and it is not decodable audio.
                if end > start {
                    self.ends.push(end);
                    self.last_is_eos = self.saw_eos && i + 1 == segments.len();
                }
                start = end;
            }
        }
        Ok(())
    }

    /// Find and read the next page header, resynchronising on the capture
    /// pattern if the stream does not sit on a page boundary.
    fn read_page_header(&mut self) -> Result<Option<[u8; HEADER_LEN]>> {
        let mut buf = [0u8; HEADER_LEN];
        match read_exact_or_eof(&mut self.source, &mut buf)? {
            0 => return Ok(None),
            n if n < HEADER_LEN => {
                return Err(Error::InvalidStream("stream ends inside a page header"));
            }
            _ => {}
        }
        if &buf[0..4] == CAPTURE_PATTERN {
            return Ok(Some(buf));
        }

        // Resync: slide a one-byte window until the capture pattern appears.
        // Bounded so a stream of garbage terminates instead of spinning.
        const RESYNC_LIMIT: usize = 1 << 20;
        for _ in 0..RESYNC_LIMIT {
            buf.copy_within(1..HEADER_LEN, 0);
            let mut b = [0u8; 1];
            if read_exact_or_eof(&mut self.source, &mut b)? == 0 {
                return Err(Error::InvalidStream("stream ends without a valid page"));
            }
            buf[HEADER_LEN - 1] = b[0];
            if &buf[0..4] == CAPTURE_PATTERN {
                return Ok(Some(buf));
            }
        }
        Err(Error::InvalidStream(
            "no Ogg page found while resynchronising",
        ))
    }
}

impl<R: Read + Seek> OggOpusReader<R> {
    /// Go back to the first audio packet, as if the reader had just been
    /// constructed.
    ///
    /// This is how a stream loops: it seeks the source back to where the audio
    /// pages start, so the header pages are neither read nor parsed again and
    /// nothing is allocated. The seek is relative to where the reader has got
    /// to, so a stream that starts partway into its source rewinds to its own
    /// start rather than the source's.
    ///
    /// A decoder carried across the rewind needs
    /// [`reset_state`](crate::OpusDecoder::reset_state), and the
    /// [`Trim`](super::Trim) needs replacing, as the type-level docs describe.
    /// If the seek fails the reader is left where it was.
    pub fn rewind(&mut self) -> Result<()> {
        let back = self.source.position - self.audio_start;
        let back = i64::try_from(back).map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "stream too long to seek back over",
            ))
        })?;
        self.source.inner.seek(SeekFrom::Current(-back))?;
        self.source.position = self.audio_start;
        self.packets.clear();
        self.ends.clear();
        self.taken = 0;
        self.last_is_eos = false;
        self.saw_eos = self.audio_start_eos;
        self.exhausted = false;
        Ok(())
    }
}

/// A source that counts the bytes read from it, so the reader knows where its
/// audio pages start without requiring `Seek` of every source.
struct Counted<R> {
    inner: R,
    position: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.position += n as u64;
        Ok(n)
    }
}

fn read_exact<R: Read>(source: &mut R, buf: &mut [u8]) -> Result<()> {
    if read_exact_or_eof(source, buf)? < buf.len() {
        return Err(Error::InvalidStream("stream ends inside a page"));
    }
    Ok(())
}

/// Fill `buf`, returning how many bytes were read; short only at EOF.
fn read_exact_or_eof<R: Read>(source: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(filled)
}

/// Iterator over an [`OggOpusReader`]'s remaining packets, from
/// [`OggOpusReader::packets`].
#[derive(Debug)]
pub struct Packets<'a, R: Read> {
    reader: &'a mut OggOpusReader<R>,
    /// Set once the stream has ended or errored, so a caller who keeps polling
    /// gets `None` rather than the same error for ever.
    done: bool,
}

impl<R: Read> Iterator for Packets<'_, R> {
    type Item = Result<OggPacket>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.reader.read_packet() {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

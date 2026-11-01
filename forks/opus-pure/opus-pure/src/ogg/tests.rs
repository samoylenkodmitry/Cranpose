//! Container-level round-trip and robustness tests.
//!
//! These exercise the framing on its own — synthetic packet payloads, no codec —
//! so a failure points at the muxer/demuxer rather than at the encoder.

use super::page::{CAPTURE_PATTERN, HEADER_LEN};
use super::*;
use crate::Error;

/// Deterministic pseudo-random bytes; `Math.random`-free so failures reproduce.
fn pseudo_packet(seed: u32, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (s >> 24) as u8
        })
        .collect()
}

fn mux(head: OpusHead, tags: OpusTags, packets: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let mut w = OggOpusWriter::with_tags(Vec::new(), head, tags).unwrap();
    for (p, dur) in packets {
        w.write_packet_with_duration(p, *dur).unwrap();
    }
    w.finish().unwrap()
}

fn demux(bytes: &[u8]) -> (OpusHead, OpusTags, Vec<OggPacket>) {
    let mut r = OggOpusReader::new(std::io::Cursor::new(bytes)).unwrap();
    let head = r.head().clone();
    let tags = r.tags().clone();
    let mut out = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        out.push(p);
    }
    (head, tags, out)
}

#[test]
fn round_trips_a_simple_stream() {
    let head = OpusHead::new(2, 48_000).unwrap();
    let mut tags = OpusTags::new();
    tags.push("TITLE", "test").unwrap();

    let packets: Vec<(Vec<u8>, u32)> = (0..50)
        .map(|i| (pseudo_packet(i, 40 + i as usize), 960))
        .collect();

    let bytes = mux(head.clone(), tags.clone(), &packets);
    let (got_head, got_tags, got) = demux(&bytes);

    assert_eq!(got_head, head);
    assert_eq!(got_tags, tags);
    assert_eq!(got.len(), packets.len());
    for (g, (p, _)) in got.iter().zip(&packets) {
        assert_eq!(&g.data, p);
    }
    assert!(got.last().unwrap().end_of_stream);
}

/// A page's granule position counts the samples a decoder can produce from the
/// packets completed on it. The pre-skip samples are the first of those, so
/// they must be counted once, by the packets that carry them — never added on
/// top. A granule past the end of the audio makes players report a duration
/// they cannot deliver.
#[test]
fn granule_counts_decodable_samples_not_pre_skip_plus_them() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..10).map(|i| (pseudo_packet(i, 60), 960)).collect();

    let bytes = mux(head, OpusTags::new(), &packets);
    let (_, _, got) = demux(&bytes);

    // Everything fits one page here, so all packets report the final granule.
    let expected = 960 * 10;
    assert_eq!(got.last().unwrap().page_granule, expected);
    assert!(got.iter().all(|p| p.page_granule == expected));
}

#[test]
fn granule_advances_across_multiple_pages() {
    let head = OpusHead::new(1, 48_000).unwrap();
    // 1 KiB packets against the 4 KiB default target: several pages.
    let packets: Vec<(Vec<u8>, u32)> = (0..40).map(|i| (pseudo_packet(i, 1024), 960)).collect();

    let bytes = mux(head, OpusTags::new(), &packets);
    let (_, _, got) = demux(&bytes);

    assert_eq!(got.len(), 40);
    let granules: Vec<i64> = got.iter().map(|p| p.page_granule).collect();
    assert!(
        granules.windows(2).all(|w| w[1] >= w[0]),
        "granule must not go backwards"
    );
    assert_eq!(*granules.last().unwrap(), 960 * 40);
    assert!(
        granules.first() < granules.last(),
        "expected more than one page"
    );
}

/// A packet longer than one page's 255 segments must span pages, with the
/// continuation flag set — the case a naive one-page-per-packet muxer gets wrong.
#[test]
fn packets_spanning_pages_reassemble() {
    let head = OpusHead::new(2, 48_000).unwrap();
    // 70000 > 255*255 = 65025, so this needs more than one page on its own.
    let big = pseudo_packet(7, 70_000);
    let packets = vec![
        (pseudo_packet(1, 100), 960),
        (big.clone(), 960),
        (pseudo_packet(2, 100), 960),
    ];

    let bytes = mux(head, OpusTags::new(), &packets);
    let (_, _, got) = demux(&bytes);

    assert_eq!(got.len(), 3);
    assert_eq!(got[1].data, big);
    assert_eq!(got[0].data.len(), 100);
    assert_eq!(got[2].data.len(), 100);
}

/// A packet whose length is an exact multiple of 255 needs a trailing
/// zero-length lacing value, or the demuxer runs it into the next packet.
#[test]
fn packet_lengths_that_are_multiples_of_255_round_trip() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = [255usize, 510, 765, 1275]
        .iter()
        .enumerate()
        .map(|(i, &n)| (pseudo_packet(i as u32, n), 960))
        .collect();

    let bytes = mux(head, OpusTags::new(), &packets);
    let (_, _, got) = demux(&bytes);

    assert_eq!(got.len(), packets.len());
    for (g, (p, _)) in got.iter().zip(&packets) {
        assert_eq!(g.data.len(), p.len());
        assert_eq!(&g.data, p);
    }
}

#[test]
fn header_pages_are_separate_and_flagged() {
    let bytes = mux(
        OpusHead::new(2, 48_000).unwrap(),
        OpusTags::new(),
        &[(vec![0xfc; 8], 960)],
    );

    // First page: BOS, one packet, OpusHead.
    assert_eq!(&bytes[0..4], CAPTURE_PATTERN);
    assert_eq!(bytes[5] & 0x02, 0x02, "first page must set BOS");
    let seg_count = bytes[26] as usize;
    assert_eq!(seg_count, 1, "OpusHead must sit alone on its page");
    let head_len = bytes[HEADER_LEN] as usize;
    let head_start = HEADER_LEN + seg_count;
    assert_eq!(&bytes[head_start..head_start + 8], b"OpusHead");

    // Second page starts right after and carries OpusTags.
    let second = head_start + head_len;
    assert_eq!(&bytes[second..second + 4], CAPTURE_PATTERN);
    assert_eq!(bytes[second + 5] & 0x02, 0, "only the first page sets BOS");
    let tags_start = second + HEADER_LEN + bytes[second + 26] as usize;
    assert_eq!(&bytes[tags_start..tags_start + 8], b"OpusTags");
}

#[test]
fn last_page_is_flagged_end_of_stream() {
    let bytes = mux(
        OpusHead::new(1, 48_000).unwrap(),
        OpusTags::new(),
        &[(vec![1, 2, 3], 960)],
    );
    // Walk the pages; only the final one may carry EOS.
    let mut off = 0usize;
    let mut eos_pages = 0;
    let mut last_off = 0;
    while off + HEADER_LEN <= bytes.len() {
        assert_eq!(&bytes[off..off + 4], CAPTURE_PATTERN);
        let segs = bytes[off + 26] as usize;
        let payload: usize = bytes[off + HEADER_LEN..off + HEADER_LEN + segs]
            .iter()
            .map(|&b| b as usize)
            .sum();
        if bytes[off + 5] & 0x04 != 0 {
            eos_pages += 1;
            last_off = off;
        }
        off += HEADER_LEN + segs + payload;
    }
    assert_eq!(off, bytes.len(), "pages must tile the file exactly");
    assert_eq!(eos_pages, 1);
    assert_eq!(
        last_off + HEADER_LEN + bytes[last_off + 26] as usize + 3,
        bytes.len()
    );
}

#[test]
fn a_corrupt_page_is_an_error_not_silent_truncation() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..30).map(|i| (pseudo_packet(i, 300), 960)).collect();
    let mut bytes = mux(head, OpusTags::new(), &packets);

    // Flip a byte deep in the audio payload, past both header pages.
    let at = bytes.len() * 3 / 4;
    bytes[at] ^= 0xff;

    let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut err = None;
    loop {
        match r.read_packet() {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    assert!(
        matches!(err, Some(Error::InvalidStream(_))),
        "corrupt payload must surface as an error, got {err:?}"
    );
}

/// The cap on a reassembled packet is what stops a chain of continued pages
/// from growing the reader's packet buffer without limit. It sits far above any real packet, so
/// exactly the cap still reads back and one byte more is refused.
#[test]
fn a_packet_past_the_size_cap_is_refused() {
    use super::reader::MAX_OGG_PACKET_BYTES;
    let head = OpusHead::new(1, 48_000).unwrap();
    for (len, accepted) in [
        (MAX_OGG_PACKET_BYTES, true),
        (MAX_OGG_PACKET_BYTES + 1, false),
    ] {
        let pkt = pseudo_packet(7, len);
        let bytes = mux(head.clone(), OpusTags::new(), &[(pkt.clone(), 960)]);
        let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes)).unwrap();
        let got = r.read_packet();
        if accepted {
            assert_eq!(got.unwrap().unwrap().data, pkt);
        } else {
            assert!(matches!(got, Err(Error::InvalidStream(_))), "{got:?}");
        }
    }
}

#[test]
fn truncated_stream_is_rejected() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..20).map(|i| (pseudo_packet(i, 500), 960)).collect();
    let bytes = mux(head, OpusTags::new(), &packets);

    let cut = &bytes[..bytes.len() - 200];
    let mut r = OggOpusReader::new(std::io::Cursor::new(cut)).unwrap();
    let mut saw_err = false;
    loop {
        match r.read_packet() {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                saw_err = true;
                break;
            }
        }
    }
    assert!(
        saw_err,
        "a truncated final page must not read as a clean end of stream"
    );
}

#[test]
fn rejects_a_stream_that_is_not_ogg() {
    let err = OggOpusReader::new(std::io::Cursor::new(b"this is not an ogg file".as_slice()));
    assert!(matches!(err, Err(Error::InvalidStream(_))));
}

#[test]
fn rejects_ogg_that_is_not_opus() {
    // A well-formed page whose first packet is not OpusHead.
    let mut page = Vec::new();
    super::page::write_page(0x02, 0, 1, 0, &[8], b"NotOpus!", &mut page);
    assert!(matches!(
        OggOpusReader::new(std::io::Cursor::new(page)),
        Err(Error::InvalidStream(_))
    ));
}

#[test]
fn writer_rejects_impossible_packets() {
    let mut w = OggOpusWriter::new(Vec::new(), OpusHead::new(1, 48_000).unwrap()).unwrap();
    assert!(matches!(
        w.write_packet_with_duration(&[], 960),
        Err(Error::InvalidArgument(_))
    ));
    // 120 ms is the ceiling for a single Opus packet.
    assert!(w.write_packet_with_duration(&[0xfc], 5760).is_ok());
    assert!(matches!(
        w.write_packet_with_duration(&[0xfc], 5761),
        Err(Error::InvalidArgument(_))
    ));
}

#[test]
fn output_is_reproducible() {
    let packets: Vec<(Vec<u8>, u32)> = (0..20).map(|i| (pseudo_packet(i, 200), 960)).collect();
    let a = mux(OpusHead::new(2, 48_000).unwrap(), OpusTags::new(), &packets);
    let b = mux(OpusHead::new(2, 48_000).unwrap(), OpusTags::new(), &packets);
    assert_eq!(a, b, "same input must produce a byte-identical file");
}

#[test]
fn distinct_headers_get_distinct_serials() {
    let mono = mux(
        OpusHead::new(1, 48_000).unwrap(),
        OpusTags::new(),
        &[(vec![0xfc], 960)],
    );
    let stereo = mux(
        OpusHead::new(2, 48_000).unwrap(),
        OpusTags::new(),
        &[(vec![0xfc], 960)],
    );
    assert_ne!(mono[14..18], stereo[14..18]);
}

#[test]
fn explicit_serial_is_honoured() {
    let w = OggOpusWriter::with_serial(
        Vec::new(),
        OpusHead::new(1, 48_000).unwrap(),
        OpusTags::new(),
        0xabcd_1234,
    )
    .unwrap();
    let bytes = w.finish().unwrap();
    assert_eq!(
        u32::from_le_bytes(bytes[14..18].try_into().unwrap()),
        0xabcd_1234
    );
    let r = OggOpusReader::new(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(r.serial(), 0xabcd_1234);
}

#[test]
fn page_target_controls_page_count() {
    let packets: Vec<(Vec<u8>, u32)> = (0..60).map(|i| (pseudo_packet(i, 200), 960)).collect();

    let count_pages = |target: usize| {
        let mut w = OggOpusWriter::new(Vec::new(), OpusHead::new(1, 48_000).unwrap()).unwrap();
        w.set_page_target(target);
        for (p, d) in &packets {
            w.write_packet_with_duration(p, *d).unwrap();
        }
        let bytes = w.finish().unwrap();
        bytes.windows(4).filter(|c| *c == CAPTURE_PATTERN).count()
    };

    assert!(count_pages(1000) > count_pages(60_000));
}

/// A stream with headers but no audio is still structurally valid.
#[test]
fn empty_stream_round_trips() {
    let bytes = mux(OpusHead::new(1, 48_000).unwrap(), OpusTags::new(), &[]);
    let (_, _, got) = demux(&bytes);
    assert!(got.is_empty());
}

/// The reader must not build an unbounded packet out of a page that never
/// terminates it.
#[test]
fn resyncs_past_leading_garbage() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let good = mux(head, OpusTags::new(), &[(vec![0xfc, 1, 2, 3], 960)]);
    let mut bytes = vec![0x00u8; 64];
    bytes.extend_from_slice(&good);

    let (_, _, got) = demux(&bytes);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].data, vec![0xfc, 1, 2, 3]);
}

/// Packets of every framing the reader reassembles: several to a page, one
/// spanning pages, lengths that are multiples of 255, and a final page flagged
/// end of stream.
fn varied_stream() -> Vec<u8> {
    let head = OpusHead::new(2, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..40)
        .map(|i| {
            let len = match i % 5 {
                0 => 255,
                1 => 510,
                2 => 70_000,
                _ => 40 + i as usize * 3,
            };
            (pseudo_packet(i, len), 960)
        })
        .collect();
    mux(head, OpusTags::new(), &packets)
}

fn all_packets(r: &mut OggOpusReader<std::io::Cursor<&[u8]>>) -> Vec<OggPacket> {
    let mut out = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        out.push(p);
    }
    out
}

#[test]
fn read_packet_into_yields_what_read_packet_does() {
    let bytes = varied_stream();
    let want = demux(&bytes).2;
    assert!(want.last().unwrap().end_of_stream);

    let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes[..])).unwrap();
    let mut packet = OggPacket::default();
    let mut got = Vec::new();
    while r.read_packet_into(&mut packet).unwrap() {
        got.push(packet.clone());
    }
    assert_eq!(got, want);

    // End of stream leaves the caller's packet as the last one read.
    assert!(!r.read_packet_into(&mut packet).unwrap());
    assert_eq!(&packet, want.last().unwrap());
}

/// A rewind from any point, including between two packets of one page and
/// partway through a packet that spans pages, replays the whole stream.
#[test]
fn rewind_replays_the_stream_from_the_first_audio_packet() {
    let bytes = varied_stream();
    let want = demux(&bytes).2;
    for read_first in [0, 1, 2, 3, 7, want.len()] {
        let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes[..])).unwrap();
        for _ in 0..read_first {
            r.read_packet().unwrap().unwrap();
        }
        r.rewind().unwrap();
        assert_eq!(all_packets(&mut r), want, "rewound after {read_first}");
    }
}

/// A stream that stops at a page boundary without an end-of-stream page ends
/// on the source running dry rather than on the flag, and rewinds all the same.
#[test]
fn rewind_replays_a_stream_with_no_end_of_stream_page() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..60).map(|i| (pseudo_packet(i, 400), 960)).collect();
    let mut bytes = mux(head, OpusTags::new(), &packets);
    // Cut the final page, which is the only one flagged end of stream.
    let last_page = bytes
        .windows(4)
        .rposition(|w| w == CAPTURE_PATTERN)
        .unwrap();
    assert_ne!(bytes[last_page + 5] & 0x04, 0);
    bytes.truncate(last_page);

    let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes[..])).unwrap();
    let want = all_packets(&mut r);
    assert!(!want.is_empty() && !want.iter().any(|p| p.end_of_stream));
    r.rewind().unwrap();
    assert_eq!(all_packets(&mut r), want);
}

/// The rewind goes back to the audio pages, not to the start of the source:
/// the header pages are not read again, and a stream that starts partway into
/// its source rewinds to its own start.
#[test]
fn rewind_returns_to_the_first_audio_page_of_a_stream_inside_its_source() {
    let head = OpusHead::new(1, 48_000).unwrap();
    let mut tags = OpusTags::new();
    // Tags large enough to span pages.
    tags.push("COMMENT", &"x".repeat(200_000)).unwrap();
    let packets: Vec<(Vec<u8>, u32)> = (0..12).map(|i| (pseudo_packet(i, 300), 960)).collect();
    // The stream sits after bytes the caller has already read past.
    let mut bytes = vec![0x5au8; 1000];
    bytes.extend_from_slice(&mux(head, tags, &packets));
    let mut source = std::io::Cursor::new(&bytes[..]);
    source.set_position(1000);

    let mut r = OggOpusReader::new(source).unwrap();
    let audio_start = r.get_ref().position();
    let want = all_packets(&mut r);
    assert_eq!(want.len(), packets.len());
    r.rewind().unwrap();
    assert_eq!(r.get_ref().position(), audio_start);
    assert_eq!(all_packets(&mut r), want);
}

/// RFC 7845 §3 has the comment header finish its page. A stream whose tags
/// share a page with audio has no page boundary for a rewind to return to.
#[test]
fn a_comment_header_sharing_its_page_with_audio_is_refused() {
    use super::page::{HeaderType, lacing_values, write_page};
    let head = OpusHead::new(1, 48_000).unwrap().to_packet();
    let tags = OpusTags::new().to_packet();
    let audio = pseudo_packet(1, 100);

    let mut bytes = Vec::new();
    let head_lacing: Vec<u8> = lacing_values(head.len()).collect();
    write_page(HeaderType::BOS, 0, 7, 0, &head_lacing, &head, &mut bytes);
    let lacing: Vec<u8> = lacing_values(tags.len())
        .chain(lacing_values(audio.len()))
        .collect();
    let payload = [&tags[..], &audio[..]].concat();
    write_page(HeaderType::EOS, 960, 7, 1, &lacing, &payload, &mut bytes);

    let got = OggOpusReader::new(std::io::Cursor::new(&bytes));
    assert!(
        matches!(got, Err(Error::InvalidStream(_))),
        "{:?}",
        got.map(|_| ())
    );
}

/// A stream whose header pages end it has no audio, and a rewind keeps it
/// ended rather than reading on into whatever follows it in the source.
#[test]
fn rewind_keeps_a_stream_that_ends_on_its_headers_ended() {
    use super::page::{HeaderType, lacing_values, write_page};
    let head = OpusHead::new(1, 48_000).unwrap().to_packet();
    let tags = OpusTags::new().to_packet();

    let mut bytes = Vec::new();
    let lacing: Vec<u8> = lacing_values(head.len()).collect();
    write_page(HeaderType::BOS, 0, 7, 0, &lacing, &head, &mut bytes);
    let lacing: Vec<u8> = lacing_values(tags.len()).collect();
    write_page(HeaderType::EOS, 0, 7, 1, &lacing, &tags, &mut bytes);
    // A second stream chained after it.
    bytes.extend_from_slice(&mux(
        OpusHead::new(2, 48_000).unwrap(),
        OpusTags::new(),
        &[(vec![0xfc, 1, 2, 3], 960)],
    ));

    let mut r = OggOpusReader::new(std::io::Cursor::new(&bytes[..])).unwrap();
    assert!(r.read_packet().unwrap().is_none());
    r.rewind().unwrap();
    assert!(r.read_packet().unwrap().is_none());
}

/// A source whose seeks can be made to fail.
struct FlakySeek<'a> {
    inner: std::io::Cursor<&'a [u8]>,
    fail: bool,
}

impl std::io::Read for FlakySeek<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Seek for FlakySeek<'_> {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        if self.fail {
            return Err(std::io::Error::other("seek refused"));
        }
        self.inner.seek(to)
    }
}

#[test]
fn a_failed_rewind_leaves_the_reader_where_it_was() {
    let bytes = varied_stream();
    let want = demux(&bytes).2;
    let source = FlakySeek {
        inner: std::io::Cursor::new(&bytes[..]),
        fail: true,
    };
    let mut r = OggOpusReader::new(source).unwrap();
    for _ in 0..3 {
        r.read_packet().unwrap().unwrap();
    }
    assert!(matches!(r.rewind(), Err(Error::Io(_))));
    let mut rest = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        rest.push(p);
    }
    assert_eq!(rest, want[3..]);
}

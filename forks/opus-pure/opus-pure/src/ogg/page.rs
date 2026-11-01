//! Ogg page framing (RFC 3533 §6).
//!
//! A page is a 27-byte fixed header, a segment table of 1..=255 lacing values,
//! and the payload those values describe. Packets are split into 255-byte
//! segments; a value below 255 terminates a packet, so a packet whose length is
//! an exact multiple of 255 ends with a 0-length segment. A packet that does not
//! terminate before the segment table fills continues on the next page, which
//! sets [`HeaderType::CONTINUED`].

use super::crc::{Crc32, crc32};
use crate::{Error, Result};

/// Every page starts with this.
pub(crate) const CAPTURE_PATTERN: &[u8; 4] = b"OggS";

/// Bytes in a page header before the segment table.
pub(crate) const HEADER_LEN: usize = 27;

/// Lacing values a single page can carry.
pub(crate) const MAX_SEGMENTS: usize = 255;

/// Largest payload a single page can describe (255 segments × 255 bytes).
pub(crate) const MAX_PAGE_PAYLOAD: usize = MAX_SEGMENTS * 255;

/// Byte offset of the CRC field within the header.
const CRC_OFFSET: usize = 22;

/// `header_type` flags.
pub(crate) struct HeaderType;

impl HeaderType {
    /// The first packet on this page continues one started on the previous page.
    pub(crate) const CONTINUED: u8 = 0x01;
    /// Beginning of stream: the first page of a logical bitstream.
    pub(crate) const BOS: u8 = 0x02;
    /// End of stream: the last page of a logical bitstream.
    pub(crate) const EOS: u8 = 0x04;
}

/// A parsed page header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageHeader {
    pub(crate) header_type: u8,
    /// Samples at 48 kHz decodable through the last packet *completing* on this
    /// page, including pre-skip; `-1` when no packet completes here.
    pub(crate) granule_position: i64,
    pub(crate) serial: u32,
    pub(crate) sequence: u32,
    pub(crate) crc: u32,
    pub(crate) segment_count: u8,
}

impl PageHeader {
    pub(crate) fn is_continued(self) -> bool {
        self.header_type & HeaderType::CONTINUED != 0
    }

    pub(crate) fn is_bos(self) -> bool {
        self.header_type & HeaderType::BOS != 0
    }

    pub(crate) fn is_eos(self) -> bool {
        self.header_type & HeaderType::EOS != 0
    }

    /// Parse a 27-byte header. Does not validate the CRC — the payload is not
    /// available yet at this point.
    pub(crate) fn parse(buf: &[u8; HEADER_LEN]) -> Result<Self> {
        if &buf[0..4] != CAPTURE_PATTERN {
            return Err(Error::InvalidStream(
                "page is missing the OggS capture pattern",
            ));
        }
        if buf[4] != 0 {
            return Err(Error::InvalidStream("unsupported Ogg page version"));
        }
        Ok(PageHeader {
            header_type: buf[5],
            granule_position: i64::from_le_bytes(buf[6..14].try_into().unwrap()),
            serial: u32::from_le_bytes(buf[14..18].try_into().unwrap()),
            sequence: u32::from_le_bytes(buf[18..22].try_into().unwrap()),
            crc: u32::from_le_bytes(buf[22..26].try_into().unwrap()),
            segment_count: buf[26],
        })
    }

    fn write_into(self, out: &mut [u8; HEADER_LEN]) {
        out[0..4].copy_from_slice(CAPTURE_PATTERN);
        out[4] = 0;
        out[5] = self.header_type;
        out[6..14].copy_from_slice(&self.granule_position.to_le_bytes());
        out[14..18].copy_from_slice(&self.serial.to_le_bytes());
        out[18..22].copy_from_slice(&self.sequence.to_le_bytes());
        out[22..26].copy_from_slice(&[0; 4]); // CRC is computed over a zeroed field
        out[26] = self.segment_count;
    }
}

/// Serialize a complete page and stamp its CRC.
///
/// The checksum covers the whole page with the CRC field itself read as four
/// zero bytes, so it is computed before the field is filled in.
pub(crate) fn write_page(
    header_type: u8,
    granule_position: i64,
    serial: u32,
    sequence: u32,
    segments: &[u8],
    payload: &[u8],
    out: &mut Vec<u8>,
) {
    debug_assert!(!segments.is_empty() && segments.len() <= MAX_SEGMENTS);
    debug_assert_eq!(
        segments.iter().map(|&s| s as usize).sum::<usize>(),
        payload.len(),
        "segment table must account for exactly the payload"
    );

    let header = PageHeader {
        header_type,
        granule_position,
        serial,
        sequence,
        crc: 0,
        segment_count: segments.len() as u8,
    };
    let mut raw = [0u8; HEADER_LEN];
    header.write_into(&mut raw);

    let start = out.len();
    out.extend_from_slice(&raw);
    out.extend_from_slice(segments);
    out.extend_from_slice(payload);

    let crc = crc32(&out[start..]);
    out[start + CRC_OFFSET..start + CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
}

/// Verify a page's stored CRC against its contents.
pub(crate) fn verify_crc(
    raw_header: &[u8; HEADER_LEN],
    segments: &[u8],
    payload: &[u8],
    expected: u32,
) -> bool {
    let mut c = Crc32::new();
    c.update(&raw_header[..CRC_OFFSET]);
    c.update_zeros(4);
    c.update(&raw_header[CRC_OFFSET + 4..]);
    c.update(segments);
    c.update(payload);
    c.finish() == expected
}

/// Lacing values encoding a packet of `len` bytes.
///
/// Always ends in a value below 255, adding a 0-length terminator when `len` is
/// an exact multiple of 255 — without it a decoder cannot tell the packet ended.
pub(crate) fn lacing_values(len: usize) -> impl Iterator<Item = u8> {
    let full = len / 255;
    (0..full)
        .map(|_| 255u8)
        .chain(std::iter::once((len % 255) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lacing_terminates_below_255() {
        for len in [0usize, 1, 254, 255, 256, 509, 510, 511, 65024] {
            let v: Vec<u8> = lacing_values(len).collect();
            assert_eq!(
                v.iter().map(|&x| x as usize).sum::<usize>(),
                len,
                "len {len}"
            );
            assert!(*v.last().unwrap() < 255, "len {len} must end below 255");
            assert!(v[..v.len() - 1].iter().all(|&x| x == 255), "len {len}");
        }
    }

    /// A 255-multiple needs the explicit zero terminator, which is the classic
    /// off-by-one in a hand-rolled muxer.
    #[test]
    fn exact_multiples_get_a_zero_terminator() {
        assert_eq!(lacing_values(255).collect::<Vec<_>>(), vec![255, 0]);
        assert_eq!(lacing_values(510).collect::<Vec<_>>(), vec![255, 255, 0]);
        assert_eq!(lacing_values(0).collect::<Vec<_>>(), vec![0]);
    }

    #[test]
    fn header_round_trips() {
        let mut out = Vec::new();
        write_page(
            HeaderType::BOS | HeaderType::EOS,
            0x0123_4567_89ab_cdef,
            0xdead_beef,
            42,
            &[4, 0],
            b"data",
            &mut out,
        );
        let raw: [u8; HEADER_LEN] = out[..HEADER_LEN].try_into().unwrap();
        let h = PageHeader::parse(&raw).unwrap();
        assert_eq!(h.header_type, HeaderType::BOS | HeaderType::EOS);
        assert_eq!(h.granule_position, 0x0123_4567_89ab_cdef);
        assert_eq!(h.serial, 0xdead_beef);
        assert_eq!(h.sequence, 42);
        assert_eq!(h.segment_count, 2);
        assert!(h.is_bos() && h.is_eos() && !h.is_continued());
        assert!(verify_crc(
            &raw,
            &out[HEADER_LEN..HEADER_LEN + 2],
            b"data",
            h.crc
        ));
    }

    #[test]
    fn crc_rejects_a_flipped_payload_byte() {
        let mut out = Vec::new();
        write_page(0, 960, 1, 0, &[4, 0], b"data", &mut out);
        let raw: [u8; HEADER_LEN] = out[..HEADER_LEN].try_into().unwrap();
        let crc = PageHeader::parse(&raw).unwrap().crc;
        let mut payload = b"data".to_vec();
        payload[0] ^= 0x01;
        assert!(!verify_crc(
            &raw,
            &out[HEADER_LEN..HEADER_LEN + 2],
            &payload,
            crc
        ));
    }

    #[test]
    fn rejects_bad_capture_pattern_and_version() {
        let mut buf = [0u8; HEADER_LEN];
        buf[0..4].copy_from_slice(b"OggZ");
        assert!(matches!(
            PageHeader::parse(&buf),
            Err(Error::InvalidStream(_))
        ));

        buf[0..4].copy_from_slice(CAPTURE_PATTERN);
        buf[4] = 1;
        assert!(matches!(
            PageHeader::parse(&buf),
            Err(Error::InvalidStream(_))
        ));
    }
}

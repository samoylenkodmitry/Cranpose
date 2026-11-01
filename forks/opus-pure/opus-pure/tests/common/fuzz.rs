//! The bodies of the fuzz targets in [`fuzz/`][f].
//!
//! They live here, in the tracked test helpers, rather than in the fuzz crate,
//! for the same reason the reference harnesses include this directory rather
//! than copying it: a body that only the fuzzer can run is a body no ordinary
//! test can replay. `fuzz_corpus.rs` runs every one of these over the tracked
//! corpus on `cargo test`, so an input that once crashed the decoder keeps
//! being checked long after the fuzzing session that found it is over, and on
//! platforms the fuzzer does not run on.
//!
//! Each body decides what its input *means* — which sample rate, where one
//! packet ends and the next begins — from the bytes themselves, so that the
//! fuzzer's mutations reach the decoder's configuration as well as its
//! bitstream. The rule is that no input may panic, hang, or produce a
//! non-finite sample; beyond that, each body asserts the contracts a caller is
//! entitled to rely on, because a decoder that quietly disagrees with
//! [`packet::samples`] about a packet's duration is a defect even though it
//! never crashes.
//!
//! [f]: https://github.com/stephenberry/opus-pure/tree/main/fuzz

// Each fuzz target includes this file on its own and calls one of the bodies,
// so from inside the fuzz crate the other two always look unused.
#![allow(dead_code)]

use opus_pure::{OggOpusReader, OpusDecoder, OpusMSDecoder, packet, repacketizer};

/// The rates a fuzzed configuration byte can select, one per value of its low
/// two bits. 12 kHz is left out only because two bits hold four values; it is
/// covered by `packet_shape`, which asks every rate.
const RATES: [i32; 4] = [8_000, 16_000, 24_000, 48_000];

/// The longest packet Opus can describe, in milliseconds (RFC 6716 §3.2).
const MAX_PACKET_MS: usize = 120;

/// Decode `data` as a whole stream through one decoder.
///
/// The first byte configures the decoder and the rest is a sequence of
/// length-prefixed packets. The prefix is two bytes: its top bit asks for the
/// in-band FEC entry point instead of the ordinary one, and the remaining
/// fifteen are the packet's length, big-endian. Fifteen bits rather than eight
/// because a CELT packet at an ordinary bitrate is several hundred bytes, and a
/// framing that could not express one would have left the fuzzer unable to
/// reach CELT at all. A length of zero is a lost packet, which puts the decoder
/// through concealment; a length past the end of the input is clamped to it,
/// which is how a truncated final packet gets decoded.
///
/// One decoder for the whole input is the point. A harness that builds a fresh
/// decoder per packet only ever exercises a decoder that has just been reset,
/// and almost everything interesting in this codec is carried between packets:
/// the LTP and LPC histories, the overlap-add buffer, the resampler, which
/// layer coded the previous frame, how many channels it had. The defects this
/// crate has already found in that machinery — a mode switch with no
/// cross-fade, a stereo-to-mono path that resumed from stale history — were all
/// reachable only from a decoder with a past.
pub fn decode_stream(data: &[u8]) {
    let Some((&cfg, mut rest)) = data.split_first() else {
        return;
    };
    let rate = RATES[(cfg & 0b11) as usize];
    let channels = 1 + ((cfg >> 2) & 1) as usize;
    let Ok(mut dec) = OpusDecoder::new(rate, channels) else {
        return;
    };

    // Sized for the longest packet Opus can describe, so `frame_size` is never
    // what limits a decode and a short return means the packet was short.
    let frame_size = rate as usize / 1000 * MAX_PACKET_MS;
    let mut pcm = vec![0.0f32; frame_size * channels];
    let mut pcm_s16 = vec![0i16; frame_size * channels];
    let integer_api = (cfg >> 3) & 1 == 1;

    while rest.len() >= 2 {
        let (prefix, tail) = rest.split_at(2);
        let fec = prefix[0] & 0x80 != 0;
        let len = (((prefix[0] & 0x7f) as usize) << 8) | prefix[1] as usize;
        let (pkt, next) = tail.split_at(len.min(tail.len()));
        rest = next;

        let produced = match (fec, integer_api) {
            (false, false) => dec.decode(pkt, frame_size, &mut pcm),
            (false, true) => dec.decode_s16(pkt, frame_size, &mut pcm_s16),
            (true, false) => dec.decode_fec(pkt, frame_size, &mut pcm),
            (true, true) => dec.decode_fec_s16(pkt, frame_size, &mut pcm_s16),
        };
        let Ok(n) = produced else { continue };

        assert!(
            n <= frame_size,
            "decode reported {n} samples into a buffer holding {frame_size}"
        );
        if !integer_api {
            assert!(
                pcm[..n * channels].iter().all(|s| s.is_finite()),
                "decode accepted a packet and produced a non-finite sample"
            );
        }

        // A muxer asks `packet::samples` how long a packet is and never decodes
        // it — `OggOpusWriter::write_packet` takes that number for the granule
        // position. If the decoder accepts a packet the two have to agree about
        // it, or a remuxed stream claims audio it does not carry. FEC is
        // excluded because it reconstructs the *previous* frame, whose duration
        // this packet does not describe.
        if !fec && !pkt.is_empty() {
            assert_eq!(
                packet::samples(pkt, rate).ok(),
                Some(n),
                "decoded {n} samples from a packet `packet::samples` reads as \
                 {:?}",
                packet::samples(pkt, rate)
            );
        }
    }
}

/// Read `data` as an Ogg Opus file and decode every packet it yields.
///
/// This is the path a caller reaches by handing the crate a file, so it runs
/// the container parser and the codec together: the page headers, the segment
/// table and the lacing decide what the decoder is handed, and the `OpusHead`
/// decides which decoder it is.
pub fn ogg_read(data: &[u8]) {
    let data = reseal_pages(data);
    let Ok(mut reader) = OggOpusReader::new(std::io::Cursor::new(&data)) else {
        return;
    };
    let head = reader.head().clone();
    let channels = head.channel_count as usize;

    let mut mono = None;
    let mut multi = None;
    if head.mapping_family == 0 {
        match OpusDecoder::new(48_000, channels) {
            Ok(d) => mono = Some(d),
            Err(_) => return,
        }
    } else {
        match OpusMSDecoder::new(48_000, channels, head.mapping_family) {
            Ok(d) => multi = Some(d),
            Err(_) => return,
        }
    }

    let frame_size = 48 * MAX_PACKET_MS;
    let mut pcm = vec![0.0f32; frame_size * channels];

    // A packet needs at least one lacing byte to exist, and every lacing byte
    // comes out of the input, so the reader cannot honestly produce more
    // packets than `data` has bytes. Tripping this means it is yielding without
    // consuming, which a fuzzer would otherwise report only as a timeout.
    let ceiling = data.len() + 16;
    let mut packets = 0usize;

    while let Ok(Some(pkt)) = reader.read_packet() {
        packets += 1;
        assert!(
            packets <= ceiling,
            "the reader produced {packets} packets from {} bytes of input",
            data.len()
        );
        let produced = match (mono.as_mut(), multi.as_mut()) {
            (Some(d), _) => d.decode(&pkt.data, frame_size, &mut pcm),
            (_, Some(d)) => d.decode(&pkt.data, frame_size, &mut pcm),
            _ => unreachable!("one of the two decoders was built above"),
        };
        let Ok(n) = produced else { continue };
        assert!(
            n <= frame_size,
            "decode reported {n} samples into a buffer holding {frame_size}"
        );
        assert!(
            pcm[..n * channels].iter().all(|s| s.is_finite()),
            "a packet from the container decoded to a non-finite sample"
        );
    }
}

/// Rewrite every page checksum in `data`, so that a mutation reaches the parser.
///
/// Each Ogg page carries a CRC over its own bytes and the reader rejects a page
/// whose checksum does not match. That is correct, and it is also a wall a
/// fuzzer cannot climb: any mutation of a valid file breaks the checksum, so
/// every input that would have been interesting is turned away at the check
/// rather than at what it actually says. It is not a wall for an attacker
/// either, who computes the checksum like anyone else. Resealing puts the
/// fuzzer on the same footing as the threat. Rejecting a bad checksum is still
/// covered, by `ogg_end_to_end.rs::corrupted_container_is_reported`.
///
/// A page whose header does not parse is left exactly as it is, so the reader
/// still sees truncated headers, impossible segment tables and pages that run
/// off the end of the file.
fn reseal_pages(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    let mut i = 0usize;
    while i + 27 <= out.len() {
        if &out[i..i + 4] != b"OggS" {
            i += 1;
            continue;
        }
        let segments = out[i + 26] as usize;
        let table = i + 27;
        if table + segments > out.len() {
            break;
        }
        let payload: usize = out[table..table + segments]
            .iter()
            .map(|&b| b as usize)
            .sum();
        let Some(end) = table
            .checked_add(segments)
            .and_then(|t| t.checked_add(payload))
        else {
            break;
        };
        if end > out.len() {
            break;
        }
        // The checksum covers the whole page with its own field zeroed.
        out[i + 22..i + 26].fill(0);
        let crc = ogg_crc32(&out[i..end]);
        out[i + 22..i + 26].copy_from_slice(&crc.to_le_bytes());
        i = end;
    }
    out
}

/// Ogg's CRC-32 (RFC 3533 §6): the Ethernet polynomial, run unreflected with no
/// initial or final inversion, which is why a stock `crc32` gives a different
/// answer. Written out a bit at a time because this is a harness and the
/// crate's table-driven version is private; the crate's own tests check that
/// one against this same bitwise form.
fn ogg_crc32(data: &[u8]) -> u32 {
    const POLY: u32 = 0x04c1_1db7;
    let mut crc: u32 = 0;
    for &b in data {
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ POLY
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Read `data` as a packet with every tool that inspects one without decoding.
///
/// These are what a muxer, a remuxer and a jitter buffer call, so they see
/// hostile bytes as readily as the decoder does, and they are cheap enough that
/// the fuzzer gets orders of magnitude more executions here than it does
/// through a decode.
pub fn packet_shape(data: &[u8]) {
    for rate in [8_000, 12_000, 16_000, 24_000, 48_000] {
        if let Ok(n) = packet::samples(data, rate) {
            let longest = MAX_PACKET_MS * rate as usize / 1000;
            assert!(n <= longest, "packet claims {n} samples, past {longest}");
            assert!(
                packet::frame_count(data).is_ok(),
                "a packet with a readable duration had an unreadable frame count"
            );
            assert!(
                packet::channels(data).is_ok(),
                "a packet with a readable duration had an unreadable channel count"
            );
        }
    }

    // The self-delimited framing has no public parser of its own — it is an
    // interior detail of a multistream packet — so it is reached the way a
    // caller reaches it: a 3-channel family 1 layout is two streams, and the
    // first of them is self-delimited, so this puts `data` through that parser.
    if let Ok(mut ms) = OpusMSDecoder::new(48_000, 3, 1) {
        let mut out = vec![0.0f32; 48 * MAX_PACKET_MS * 3];
        if let Ok(n) = ms.decode(data, 48 * MAX_PACKET_MS, &mut out) {
            assert!(
                out[..n * 3].iter().all(|s| s.is_finite()),
                "a multistream decode accepted a packet and produced a non-finite sample"
            );
        }
    }

    let mut rp = repacketizer::Repacketizer::new();
    if rp.cat(data).is_ok() {
        // Whatever it accepted, it must be able to write back out, and the
        // result must parse as the packet it claims to be.
        if let Ok(out) = rp.out() {
            assert!(
                packet::frame_count(&out).is_ok(),
                "the repacketizer emitted a packet it cannot itself read"
            );
        }
        let _ = rp.out_self_delimited();
        let _ = rp.out_range(0, rp.nb_frames());
    }

    let _ = repacketizer::unpad_packet(data);
    let mut padded = data.to_vec();
    let target = data.len() + 7;
    if repacketizer::pad_packet(&mut padded, target).is_ok() {
        assert_eq!(padded.len(), target, "pad_packet did not reach its target");
    }
}

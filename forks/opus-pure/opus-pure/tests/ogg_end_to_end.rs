//! Real Opus audio through the Ogg container and back.
//!
//! `src/ogg/tests.rs` covers the framing with synthetic payloads; this covers
//! the seam between the codec and the container — pre-skip, granule positions
//! derived from real frame durations, and the audio surviving the trip.

mod common;
use common::*;
use opus_pure::{Application, OggOpusReader, OggOpusWriter, OpusDecoder, OpusHead, OpusTags};

/// Encode `pcm` into a complete Ogg Opus stream in memory.
fn encode_to_ogg(rate: i32, channels: usize, bitrate: i32, pcm: &[f32]) -> Vec<u8> {
    let mut c = Codec::new(rate, channels, Application::Audio).bitrate(bitrate);
    let packets = c.encode_all(pcm);

    let head = OpusHead::new(channels as u8, rate as u32).unwrap();
    let mut tags = OpusTags::new();
    tags.push("ENCODER", "opus-pure integration test").unwrap();
    let mut w = OggOpusWriter::with_tags(Vec::new(), head, tags).unwrap();

    for pkt in &packets {
        w.write_packet(pkt).unwrap();
    }
    w.finish().unwrap()
}

fn decode_from_ogg(bytes: &[u8], rate: i32) -> (OpusHead, OpusTags, Vec<f32>) {
    let mut r = OggOpusReader::new(std::io::Cursor::new(bytes)).unwrap();
    let head = r.head().clone();
    let tags = r.tags().clone();
    let channels = head.channel_count as usize;
    let mut packets = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        packets.push(p.data);
    }
    let pcm = Codec::new(rate, channels, Application::Audio).decode_all(&packets);
    (head, tags, pcm)
}

#[test]
fn audio_survives_the_container_round_trip() {
    for &(rate, channels, bitrate) in &[
        (48_000i32, 1usize, 96_000i32),
        (48_000, 2, 128_000),
        (16_000, 1, 24_000),
        (24_000, 2, 48_000),
    ] {
        let mono = music_like(rate, rate as usize * 2);
        let src = if channels == 2 {
            interleave(&[mono.clone(), mono.clone()])
        } else {
            mono.clone()
        };

        let file = encode_to_ogg(rate, channels, bitrate, &src);
        let (head, tags, decoded) = decode_from_ogg(&file, rate);

        assert_eq!(head.channel_count as usize, channels);
        assert_eq!(head.input_sample_rate, rate as u32);
        assert_eq!(tags.get("ENCODER"), Some("opus-pure integration test"));
        assert_eq!(
            decoded.len(),
            src.len(),
            "{rate} Hz {channels}ch: sample count changed"
        );

        // Compare against a direct encode/decode with no container in between:
        // the container must be lossless with respect to the packets.
        let skip = (rate / 50) as usize * 10 * channels;
        let ch0 = deinterleave(&decoded[skip..], channels, 0);
        let src0 = deinterleave(&src[skip..], channels, 0);
        let (corr, _) = aligned_correlation(&ch0, &src0, (rate / 50) as usize);
        assert!(
            corr > 0.98,
            "{rate} Hz {channels}ch: correlation {corr:.4} after the round trip"
        );
    }
}

/// The packets recovered from the container must be byte-identical to the ones
/// the encoder produced. Anything less means the framing is lossy.
#[test]
fn packets_survive_byte_for_byte() {
    let (rate, channels) = (48_000i32, 2usize);
    let mono = music_like(rate, rate as usize * 2);
    let src = interleave(&[mono.clone(), mono]);

    let original = Codec::new(rate, channels, Application::Audio)
        .bitrate(128_000)
        .encode_all(&src);
    let mut w = OggOpusWriter::new(Vec::new(), OpusHead::new(2, 48_000).unwrap()).unwrap();
    for pkt in &original {
        w.write_packet(pkt).unwrap();
    }
    let file = w.finish().unwrap();

    let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
    let mut recovered = Vec::new();
    while let Some(p) = r.read_packet().unwrap() {
        recovered.push(p.data);
    }
    assert_eq!(recovered.len(), original.len(), "packet count changed");
    for (i, (a, b)) in recovered.iter().zip(&original).enumerate() {
        assert_eq!(a, b, "packet {i} changed passing through the container");
    }
}

/// The final granule position must equal the number of 48 kHz samples the
/// written packets actually decode to — never more. A player computes the
/// playable length as `final granule - pre_skip`, so an over-claim by even one
/// sample promises audio the file does not contain.
#[test]
fn final_granule_position_matches_the_duration() {
    for &(rate, secs) in &[(48_000i32, 2usize), (16_000, 2), (24_000, 3)] {
        let src = music_like(rate, rate as usize * secs);
        let file = encode_to_ogg(rate, 1, 64_000, &src);

        let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
        let pre_skip = i64::from(r.head().pre_skip);
        let mut last = -1i64;
        let mut packets = 0;
        while let Some(p) = r.read_packet().unwrap() {
            last = p.page_granule;
            packets += 1;
        }
        let frames = (rate as usize * secs) / (rate as usize / 50);
        assert_eq!(packets, frames, "{rate} Hz: packet count");
        assert_eq!(
            last,
            (frames as i64) * 960,
            "{rate} Hz: final granule position"
        );
        assert!(
            last >= pre_skip,
            "{rate} Hz: stream must carry at least the pre-skip"
        );
    }
}

/// A stereo file must declare stereo in `OpusHead`, and the reader must be able
/// to configure a decoder from the header alone.
#[test]
fn header_describes_the_stream_well_enough_to_decode_it() {
    let src = interleave(&[music_like(48_000, 48_000), music_like(48_000, 48_000)]);
    let file = encode_to_ogg(48_000, 2, 128_000, &src);

    let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
    let head = r.head().clone();
    assert_eq!(head.channel_count, 2);
    assert_eq!(head.mapping_family, 0);
    assert_eq!(head.stream_count, 1);
    assert_eq!(head.coupled_count, 1);
    assert_eq!(head.pre_skip, OpusHead::RECOMMENDED_PRE_SKIP);

    // Drive the decoder purely from the header.
    let mut dec = OpusDecoder::new(48_000, head.channel_count as usize).unwrap();
    let mut total = 0;
    while let Some(p) = r.read_packet().unwrap() {
        let mut out = vec![0.0f32; 960 * head.channel_count as usize];
        total += dec.decode(&p.data, 960, &mut out).unwrap();
    }
    assert_eq!(total, 50 * 960);
}

/// Files this crate writes must be stable across runs: the same audio in gives
/// the same bytes out, container included.
#[test]
fn container_output_is_reproducible() {
    let src = music_like(48_000, 48_000);
    assert_eq!(
        encode_to_ogg(48_000, 1, 64_000, &src),
        encode_to_ogg(48_000, 1, 64_000, &src)
    );
}

/// Corruption anywhere in the audio must surface as an error from the reader
/// rather than silently truncating the stream.
#[test]
fn corrupted_container_is_reported() {
    let src = music_like(48_000, 48_000 * 2);
    let mut file = encode_to_ogg(48_000, 1, 96_000, &src);
    let at = file.len() * 2 / 3;
    file[at] ^= 0xff;

    let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
    let mut errored = false;
    loop {
        match r.read_packet() {
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(_) => {
                errored = true;
                break;
            }
        }
    }
    assert!(errored, "a corrupted page decoded as a clean end of stream");
}

/// The muxer may never claim more audio than it wrote.
///
/// A page's granule position counts the 48 kHz samples decodable from the
/// packets completed on it, and the pre-skip samples are the *first* of those —
/// they arrive inside the first packets. Seeding the counter with `pre_skip`
/// therefore claims samples no packet carries: players compute the playable
/// length as `final granule - pre_skip` and then run off the end of the audio.
///
/// This checks the final granule against the durations in the packets' own TOC
/// bytes, so it audits the muxer rather than trusting its arithmetic. An
/// over-claim is always a bug; an under-claim is legal end-trimming, which
/// `write_packet` never produces on its own — `tests/ogg_gapless.rs` covers the
/// streams that ask for one through `write_packet_with_duration`.
#[test]
fn granule_never_claims_more_audio_than_the_packets_carry() {
    for &(rate, channels) in &[(48_000i32, 1usize), (48_000, 2), (16_000, 1), (8_000, 1)] {
        let src = music_like(rate, rate as usize * 2);
        let pcm = if channels == 1 {
            src
        } else {
            interleave(&[src.clone(), src])
        };
        let file = encode_to_ogg(rate, channels, 64_000, &pcm);

        let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
        let pre_skip = i64::from(r.head().pre_skip);
        let mut decodable = 0i64;
        let mut final_granule = -1i64;
        while let Some(p) = r.read_packet().unwrap() {
            decodable += packet_samples_48k(&p.data) as i64;
            if p.page_granule >= 0 {
                final_granule = p.page_granule;
            }
        }

        assert_eq!(
            final_granule,
            decodable,
            "{rate} Hz/{channels}ch: final granule must equal the samples the \
             packets decode to (over-claim = {})",
            final_granule - decodable
        );
        assert!(
            decodable > pre_skip,
            "{rate} Hz/{channels}ch: stream is shorter than its own pre-skip"
        );
    }
}

/// 60 ms packets must be muxed and demuxed as faithfully as 20 ms ones.
///
/// 60 ms is the one Opus duration that is not a whole number of packets per
/// second, so it is the one where a granule position computed from a packet
/// count rather than from the packets themselves goes wrong: 2880 samples at
/// 48 kHz, from a source that may be running at 8 kHz.
#[test]
fn sixty_ms_packets_survive_the_container() {
    for &(rate, channels) in &[(48_000i32, 2usize), (16_000, 1), (8_000, 1)] {
        let frame = rate as usize * 60 / 1000;
        let n = frame * 25;
        let mono = music_like(rate, n);
        let pcm = if channels == 1 {
            mono.clone()
        } else {
            interleave(&[mono.clone(), speech_like(rate, n)])
        };

        let mut c = Codec::new(rate, channels, Application::Audio)
            .frame_samples(frame)
            .bitrate(64_000);
        let coded = c.encode_all(&pcm);
        let written = coded.len();
        let head = OpusHead::new(channels as u8, rate as u32).unwrap();
        let mut w = OggOpusWriter::new(Vec::new(), head).unwrap();
        for pkt in &coded {
            // 60 ms is 2880 samples at 48 kHz whatever the encoder's own rate.
            assert_eq!(packet_samples_48k(pkt), 2880);
            w.write_packet(pkt).unwrap();
        }
        let file = w.finish().unwrap();

        let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
        let (mut decodable, mut final_granule) = (0i64, -1i64);
        let mut demuxed = Vec::new();
        while let Some(p) = r.read_packet().unwrap() {
            decodable += packet_samples_48k(&p.data) as i64;
            if p.page_granule >= 0 {
                final_granule = p.page_granule;
            }
            demuxed.push(p.data);
        }
        let read = demuxed.len();
        let decoded = c.decode_all(&demuxed);

        let label = format!("{rate} Hz {channels}ch");
        assert_eq!(read, written, "{label}: packet count through the container");
        assert_eq!(
            decodable,
            written as i64 * 2880,
            "{label}: packets decode to a different duration than they claim"
        );
        assert_eq!(
            final_granule, decodable,
            "{label}: final granule against the audio the packets carry"
        );

        for c in 0..channels {
            let s = deinterleave(&pcm, channels, c);
            let d = deinterleave(&decoded, channels, c);
            let skip = frame * 2;
            let (corr, _) = aligned_correlation(&d[skip..], &s[skip..], rate as usize / 50);
            assert!(
                corr >= 0.95,
                "{label} ch{c}: correlation {corr:.4} after the container round trip"
            );
        }
    }
}

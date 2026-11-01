//! Coverage for the public API beyond the core encoder/decoder pair.
//!
//! These exist because the port narrowed the public surface deliberately: every
//! item the crate still exports should have at least one test proving it is
//! reachable and works, so nothing ships as an untested promise.
//!
//! Chunk-parallel encoding used to be covered from here by one whole-clip SNR
//! comparison, which is the wrong instrument: what goes wrong when a clip is
//! split across threads is confined to a few frames per boundary and to whole
//! chunks coded in the wrong mode, and an average over the clip sees neither.
//! It has its own file, `tests/parallel.rs`.

mod common;
use common::*;
use opus_pure::{
    Application, Bandwidth, Error, OpusDecoder, OpusEncoder, OpusMSDecoder, OpusMSEncoder,
    RateControl, Signal,
};

/// One 20 ms frame at 48 kHz. A function rather than a `const` so
/// `chunks_exact` is not handed a compile-time constant, which clippy would
/// rather see written as `as_chunks`.
fn frame_20ms() -> usize {
    960
}

/// 5.1 and 7.1 surround through the multistream encoder and decoder.
///
/// The 60 ms entry matters because the multistream layer forwards `frame_size`
/// to every stream encoder and sizes its own scratch from it; a duration that
/// no sample rate divides evenly is where that arithmetic goes wrong.
#[test]
fn multistream_surround_round_trips() {
    for &(channels, frame) in &[(6usize, 960usize), (8, 960), (6, 2880)] {
        let rate = 48_000i32;
        let mono = music_like(rate, frame * 10);
        let mut src = Vec::with_capacity(frame * 10 * channels);
        for &sample in mono.iter().take(frame * 10) {
            for c in 0..channels {
                src.push(sample * (1.0 - c as f32 * 0.1));
            }
        }

        let mut enc = OpusMSEncoder::new(rate, channels, 1, Application::Audio).unwrap();
        let mut dec = OpusMSDecoder::new(rate, channels, 1).unwrap();
        let mut decoded = Vec::new();
        let mut pkt = vec![0u8; 8000];
        for f in 0..10 {
            let len = enc
                .encode(
                    &src[f * frame * channels..(f + 1) * frame * channels],
                    frame,
                    &mut pkt,
                )
                .unwrap();
            assert!(len > 0);
            let mut out = vec![0.0f32; frame * channels];
            let n = dec.decode(&pkt[..len], frame, &mut out).unwrap();
            assert_eq!(n, frame);
            assert!(out.iter().all(|s| s.is_finite()));
            decoded.extend_from_slice(&out);
        }

        // Every channel must carry its own signal — a multistream bug that drops
        // or duplicates a channel shows up as a wrong per-channel energy.
        let skip = frame * 4 * channels;
        for c in 0..channels {
            let got = energy(&deinterleave(&decoded[skip..], channels, c));
            let want = energy(&deinterleave(&src[skip..], channels, c));
            let ratio = got / want.max(1e-12);
            assert!(
                (0.25..=4.0).contains(&ratio),
                "{channels}ch/{frame} samples: channel {c} energy is {ratio:.3}x the input's"
            );
        }
    }
}

/// A surround stream's declared output gain must be reachable, and must work.
///
/// RFC 7845 §5.1 says a player SHOULD apply the gain a file declares, and says
/// nothing about mapping family. `OpusHead::decoder` carries it on the mono and
/// stereo path; on the surround path `streams_mut` is the only route to it, and
/// without one the gain could not be applied at all — a file that plays at the
/// wrong level with no other symptom.
#[test]
fn a_surround_decoder_can_apply_the_headers_output_gain() {
    let (rate, channels, frame) = (48_000i32, 6usize, 960usize);
    let mono = music_like(rate, frame * 4);
    let mut src = Vec::with_capacity(frame * 4 * channels);
    for &sample in mono.iter().take(frame * 4) {
        for c in 0..channels {
            src.push(sample * (1.0 - c as f32 * 0.1));
        }
    }

    let mut enc = OpusMSEncoder::new(rate, channels, 1, Application::Audio).unwrap();
    let mut pkt = vec![0u8; 8000];
    let mut packets = Vec::new();
    for f in 0..4 {
        let len = enc
            .encode(
                &src[f * frame * channels..(f + 1) * frame * channels],
                frame,
                &mut pkt,
            )
            .unwrap();
        packets.push(pkt[..len].to_vec());
    }

    // -1536 in Q7.8 dB is -6 dB, an amplitude ratio of 10^(-6/20).
    let decode = |gain_q8: i32| {
        let mut dec = OpusMSDecoder::new(rate, channels, 1).unwrap();
        assert_eq!(dec.streams().len(), dec.nb_streams());
        for d in dec.streams_mut() {
            d.gain_q8 = gain_q8;
        }
        let mut out = Vec::new();
        let mut block = vec![0.0f32; frame * channels];
        for p in &packets {
            let n = dec.decode(p, frame, &mut block).unwrap();
            out.extend_from_slice(&block[..n * channels]);
        }
        out
    };

    let plain = decode(0);
    let quiet = decode(-1536);
    assert_eq!(plain.len(), quiet.len());

    // Compare energies rather than samples: the gain is applied per stream, so
    // every output channel must come back scaled, not just the coupled pair.
    let want = 10f32.powf(-6.0 / 20.0);
    for c in 0..channels {
        let (a, b) = (
            energy(&deinterleave(&plain, channels, c)),
            energy(&deinterleave(&quiet, channels, c)),
        );
        assert!(a > 1e-9, "channel {c} decoded to silence at unity gain");
        let ratio = (b / a).sqrt();
        assert!(
            (ratio - want).abs() < 0.01,
            "channel {c}: gain applied as {ratio:.4}x, expected {want:.4}x"
        );
    }
}

/// `Signal` must actually steer the mode decision, or it is a setting that
/// silently does nothing.
#[test]
fn signal_type_hint_steers_mode_selection() {
    let (rate, frame) = (48_000i32, 960usize);
    let src = speech_like(rate, frame * 40);

    let count_modes = |hint: Option<Signal>| {
        let mut c = Codec::new(rate, 1, Application::Audio)
            .frame_samples(frame)
            .bitrate(32_000);
        c.enc.signal_type = hint;
        c.encode_all(&src)
            .iter()
            .filter(|p| packet_mode(p) != "celt")
            .count()
    };

    let voice = count_modes(Some(Signal::Voice));
    let music = count_modes(Some(Signal::Music));
    assert!(
        voice > music,
        "the Voice hint produced {voice} SILK/hybrid frames and Music {music}; \
         the hint is not reaching the mode decision"
    );
}

/// `force_bandwidth` must be honoured in the emitted TOC.
#[test]
fn forced_bandwidth_reaches_the_bitstream() {
    let (rate, frame) = (48_000i32, 960usize);
    let src = music_like(rate, frame * 20);
    for &(bw, want_min_cfg, want_max_cfg) in &[
        // CELT configurations: 16-19 narrowband, 20-23 wideband, 24-27
        // superwideband, 28-31 fullband (RFC 6716 §3.1).
        (Bandwidth::Narrowband, 16u8, 19u8),
        (Bandwidth::Mediumband, 20, 23), // CELT has no mediumband; it widens
        (Bandwidth::Wideband, 20, 23),
        (Bandwidth::Superwideband, 24, 27),
        (Bandwidth::Fullband, 28, 31),
    ] {
        let packets = Codec::new(rate, 1, Application::Audio)
            .frame_samples(frame)
            .bitrate(64_000)
            .bandwidth(bw)
            .encode_all(&src);
        for pkt in &packets {
            let cfg = pkt[0] >> 3;
            assert!(
                (want_min_cfg..=want_max_cfg).contains(&cfg),
                "{bw:?}: TOC config {cfg} outside {want_min_cfg}..={want_max_cfg}"
            );
        }
    }
}

/// CBR must produce constant-size packets; VBR must not.
///
/// Both durations matter because the target size is `bitrate * frame_size /
/// rate` rounded to bytes: at 20 ms that division is exact, and at 60 ms it is
/// the one duration where it is not.
#[test]
fn cbr_and_vbr_differ_as_advertised() {
    for &frame in &[960usize, 2880] {
        let rate = 48_000i32;
        // Alternating loud and silent stretches: VBR should track that, CBR must not.
        let mut src = music_like(rate, frame * 30);
        for (i, s) in src.iter_mut().enumerate() {
            if (i / (frame * 5)) % 2 == 1 {
                *s = 0.0;
            }
        }

        let sizes = |cbr: bool| {
            let mut c = Codec::new(rate, 1, Application::Audio)
                .frame_samples(frame)
                .bitrate(64_000);
            c.enc.rate_control = if cbr {
                RateControl::Cbr
            } else {
                RateControl::ConstrainedVbr
            };
            c.encode_all(&src)
                .iter()
                .map(|p| p.len())
                .collect::<Vec<_>>()
        };

        let cbr = sizes(true);
        assert!(
            cbr.windows(2).all(|w| w[0] == w[1]),
            "{frame}-sample CBR packet sizes varied: {:?}",
            &cbr[..5]
        );
        // 64 kb/s over `frame` samples at 48 kHz, in whole bytes.
        let want = (64_000i64 * frame as i64 / 48_000 + 4) / 8;
        assert_eq!(
            cbr[0] as i64, want,
            "{frame}-sample CBR packets are {} bytes, not the {want} the bitrate asks for",
            cbr[0]
        );
        let vbr = sizes(false);
        assert!(
            vbr.windows(2).any(|w| w[0] != w[1]),
            "{frame}-sample VBR produced constant-size packets"
        );
    }
}

/// `final_range` is how a caller verifies encoder/decoder range-coder agreement.
#[test]
fn final_range_is_reported_and_changes_per_packet() {
    let (rate, frame) = (48_000i32, 960usize);
    let src = music_like(rate, frame * 10);
    let mut c = Codec::new(rate, 1, Application::Audio).bitrate(64_000);

    // Encoded one frame at a time: `final_range` is per-packet state, so it has
    // to be read between calls.
    let mut ranges = Vec::new();
    for chunk in src.chunks_exact(frame) {
        let mut pkt = vec![0u8; 4000];
        c.enc.encode(chunk, frame, &mut pkt).unwrap();
        ranges.push(c.enc.final_range());
    }
    assert!(ranges.iter().any(|&r| r != 0), "final_range is always zero");
    assert!(
        ranges.windows(2).any(|w| w[0] != w[1]),
        "final_range never changes"
    );
}

/// `frame_size` is the room available in the output buffer, not a demand for
/// exactly that many samples.
///
/// libopus documents the argument as "the number of samples per channel of
/// available space in pcm" and returns however many the packet held. Treating
/// it as an exact length made a 20 ms packet stretch across whatever buffer it
/// was handed, so the natural way to call a decoder — pass a generous buffer —
/// silently produced the wrong number of samples.
#[test]
fn decode_treats_frame_size_as_buffer_capacity() {
    let rate = 48_000i32;
    let frame = 960usize; // 20 ms
    let src = music_like(rate, frame * 20);
    let pkt = Codec::new(rate, 1, Application::Audio)
        .bitrate(64_000)
        .encode_all(&src[..frame])
        .remove(0);

    // A buffer six times the packet's duration must yield the packet's
    // duration, and exactly the same samples as an exactly-sized buffer.
    let mut exact = vec![0.0f32; frame];
    let mut roomy = vec![0.0f32; frame * 6];
    let got_exact = OpusDecoder::new(rate, 1)
        .unwrap()
        .decode(&pkt, frame, &mut exact)
        .unwrap();
    let got_roomy = OpusDecoder::new(rate, 1)
        .unwrap()
        .decode(&pkt, frame * 6, &mut roomy)
        .unwrap();

    assert_eq!(got_exact, frame, "exact buffer: wrong sample count");
    assert_eq!(got_roomy, frame, "roomy buffer: should still decode 20 ms");
    assert_eq!(
        exact[..frame],
        roomy[..frame],
        "the same packet must decode identically whatever the buffer size"
    );

    // Too little room is still an error rather than a truncated decode.
    let mut tiny = vec![0.0f32; frame / 2];
    assert!(matches!(
        OpusDecoder::new(rate, 1)
            .unwrap()
            .decode(&pkt, frame / 2, &mut tiny),
        Err(Error::BufferTooSmall { .. })
    ));

    // And a declared capacity the slice cannot back is an error too, rather
    // than a decode that quietly writes less than it promised.
    assert!(matches!(
        OpusDecoder::new(rate, 1)
            .unwrap()
            .decode(&pkt, frame * 6, &mut vec![0.0f32; frame]),
        Err(Error::BufferTooSmall { .. })
    ));
}

/// Every public type renders through `Debug`, and says something worth reading.
///
/// `#![deny(missing_debug_implementations)]` guarantees the impls exist, which
/// is not the same as their being useful: the codecs and the container types
/// carry hand-written ones so that `dbg!` prints configuration rather than the
/// tens of kilobytes of filter state a derived impl would dump. This checks
/// they name the type and report the settings a caller would be looking for.
///
/// It also pins the two impls that are generic. `OggOpusReader<R>` and
/// `OggOpusWriter<W>` deliberately do not require `R: Debug` or `W: Debug`,
/// because a source is usually a file or a socket and neither is printable; a
/// derive would silently take that requirement and leave most readers with no
/// `Debug` at all. The sink here is a `Vec<u8>`, which would satisfy a derive,
/// so the check that matters is `NotDebug` below.
#[test]
fn public_types_render_through_debug() {
    use opus_pure::{
        ChannelLayout, OggOpusReader, OggOpusWriter, OpusEncoder, OpusHead, ParallelConfig,
        Repacketizer,
    };

    let encoder = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    let rendered = format!("{encoder:?}");
    assert!(rendered.starts_with("OpusEncoder {"), "{rendered}");
    assert!(rendered.contains("bitrate_bps"), "{rendered}");
    assert!(rendered.contains("application: Audio"), "{rendered}");
    // The coding state is omitted, which `finish_non_exhaustive` marks.
    assert!(rendered.ends_with(".. }"), "{rendered}");
    assert!(rendered.len() < 400, "Debug dumped codec state: {rendered}");

    let decoder = OpusDecoder::new(48_000, 2).unwrap();
    let rendered = format!("{decoder:?}");
    assert!(rendered.starts_with("OpusDecoder {"), "{rendered}");
    assert!(rendered.contains("final_range"), "{rendered}");
    assert!(rendered.len() < 400, "Debug dumped codec state: {rendered}");

    let layout = ChannelLayout::surround(6, 1).unwrap();
    let rendered = format!("{layout:?}");
    assert!(rendered.contains("nb_coupled_streams"), "{rendered}");

    let ms_enc = OpusMSEncoder::new(48_000, 6, 1, Application::Audio).unwrap();
    assert!(format!("{ms_enc:?}").starts_with("OpusMSEncoder {"));
    let ms_dec = OpusMSDecoder::new(48_000, 6, 1).unwrap();
    assert!(format!("{ms_dec:?}").starts_with("OpusMSDecoder {"));

    let config = ParallelConfig::new(48_000, 2, Application::Audio);
    assert!(format!("{config:?}").contains("warmup_ms"));

    let rendered = format!("{:?}", Repacketizer::new());
    assert!(rendered.contains("nb_frames: 0"), "{rendered}");

    // A container round trip, so the reader has a real stream behind it.
    let mut writer = OggOpusWriter::new(Vec::new(), OpusHead::new(2, 48_000).unwrap()).unwrap();
    let rendered = format!("{writer:?}");
    assert!(rendered.starts_with("OggOpusWriter {"), "{rendered}");
    assert!(rendered.contains("granule: 0"), "{rendered}");

    let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    let mut packet = vec![0u8; 4000];
    let n = encoder
        .encode(&vec![0.0f32; 960 * 2], 960, &mut packet)
        .unwrap();
    writer.write_packet(&packet[..n]).unwrap();
    let file = writer.finish().unwrap();

    let reader = OggOpusReader::new(std::io::Cursor::new(file)).unwrap();
    let rendered = format!("{reader:?}");
    assert!(rendered.starts_with("OggOpusReader {"), "{rendered}");
    assert!(rendered.contains("channel_count: 2"), "{rendered}");
}

/// The container types do not require their `Read`/`Write` parameter to be
/// `Debug`. This is the check a derived impl would fail: `NotDebug` implements
/// neither, so if either impl ever gained a `R: Debug` bound this stops
/// compiling.
#[test]
fn container_debug_does_not_require_a_debug_source() {
    use opus_pure::{OggOpusWriter, OpusHead};

    struct NotDebug(Vec<u8>);
    impl std::io::Write for NotDebug {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let writer =
        OggOpusWriter::new(NotDebug(Vec::new()), OpusHead::new(1, 48_000).unwrap()).unwrap();
    assert!(format!("{writer:?}").starts_with("OggOpusWriter {"));
}

// ---------------------------------------------------------------------------
// The settings a caller can reach, and what happens when they are wrong.
//
// Every one of these was a real defect before it was a test: a writable field
// that did nothing, a public field that panicked, a helper that panicked where
// its sibling returned `Err`, and a header constant that discarded audio.
// ---------------------------------------------------------------------------

/// A multistream bitrate set through the public API reaches the streams.
///
/// `OpusMSEncoder` used to carry a `pub bitrate_bps` field alongside
/// `set_bitrate`, and `encode` read neither — it read each sub-encoder's own
/// setting. Assigning the field compiled, read back the value you asked for,
/// and changed nothing about the output, which is the worst shape a setting can
/// have. The field is private now and this pins the consequence rather than the
/// spelling: asking for a low rate has to produce smaller packets than asking
/// for a high one.
#[test]
fn multistream_bitrate_reaches_the_streams() {
    const CH: usize = 6;
    let pcm = interleave(
        &(0..CH)
            .map(|c| {
                music_like(48_000, 960)
                    .iter()
                    .map(|s| s * (1.0 - c as f32 * 0.05))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
    );

    let sizes = |total: i32| -> usize {
        let mut enc = OpusMSEncoder::new(48_000, CH, 1, Application::Audio).unwrap();
        enc.set_bitrate(total);
        let mut pkt = vec![0u8; 16_000];
        // Two frames: the first carries the encoder's start-up transient.
        let _ = enc.encode(&pcm, 960, &mut pkt).unwrap();
        enc.encode(&pcm, 960, &mut pkt).unwrap()
    };

    let low = sizes(96_000);
    let high = sizes(384_000);
    assert!(
        high > low * 2,
        "384 kb/s produced {high} bytes and 96 kb/s produced {low}: the total \
         bitrate is not reaching the per-stream encoders"
    );
    let mut enc = OpusMSEncoder::new(48_000, CH, 1, Application::Audio).unwrap();
    enc.set_bitrate(96_000);
    assert_eq!(
        enc.bitrate_bps(),
        96_000,
        "the getter disagrees with the setter"
    );
}

/// Every `OpusEncoder` setting is reachable through a multistream encoder.
#[test]
fn multistream_exposes_every_stream_setting() {
    let mut enc = OpusMSEncoder::new(48_000, 6, 1, Application::Audio).unwrap();
    for e in enc.streams_mut() {
        e.use_inband_fec = true;
        e.packet_loss_perc = 10;
        e.complexity = 3;
    }
    assert!(enc.streams().iter().all(|e| e.use_inband_fec));
    assert_eq!(enc.streams().len(), enc.nb_streams());
}

/// Settings outside their range are clamped rather than fed to a shift or a
/// table index, and a bitrate that is not a rate is refused.
///
/// `lsb_depth = -1` reached `1i64 << lsb_depth` and panicked in a debug build;
/// `bitrate_bps = i32::MAX` overflowed a multiply the same way. Neither is
/// reachable now, and the field shows the effective value afterwards.
#[test]
fn out_of_range_settings_are_clamped_not_obeyed() {
    let pcm = speech_like(48_000, 960);
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];

    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    enc.complexity = 100;
    enc.packet_loss_perc = 250;
    enc.lsb_depth = -1;
    enc.bitrate_bps = i32::MAX;
    enc.encode(&pcm, 960, &mut pkt).unwrap();
    assert_eq!(enc.complexity, 10);
    assert_eq!(enc.packet_loss_perc, 100);
    assert_eq!(
        enc.lsb_depth, 8,
        "clamped up to the floor, not down from the top"
    );
    assert_eq!(
        enc.bitrate_bps, 300_000,
        "one channel, so 300 kb/s is the ceiling"
    );

    let mut enc = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    enc.complexity = -100;
    enc.lsb_depth = 99;
    enc.bitrate_bps = 1;
    enc.encode(&speech_like(48_000, 1920), 960, &mut pkt)
        .unwrap();
    assert_eq!(enc.complexity, 0);
    assert_eq!(enc.lsb_depth, 24);
    assert_eq!(enc.bitrate_bps, 500, "the floor libopus uses");
}

/// libopus's negative bitrate sentinels are refused with a message, not
/// silently taken for a very small rate.
///
/// `OPUS_AUTO` is -1000 and `OPUS_BITRATE_MAX` is -1. Both used to encode two
/// byte packets — the audio simply gone — which is what somebody porting from C
/// would have hit first.
#[test]
fn libopus_bitrate_sentinels_are_refused() {
    let pcm = speech_like(48_000, 960);
    let mut pkt = vec![0u8; 4000];
    for sentinel in [-1000, -1, 0] {
        let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
        enc.bitrate_bps = sentinel;
        match enc.encode(&pcm, 960, &mut pkt) {
            Err(Error::InvalidArgument(m)) => {
                assert!(m.contains("bitrate_bps"), "unhelpful message: {m}")
            }
            other => panic!("bitrate_bps = {sentinel} gave {other:?}"),
        }
    }
}

/// `encode_parallel` reports a bad argument instead of panicking a worker.
///
/// It used to `.expect()` inside a scoped thread, so a frame size no coding
/// mode can carry took down four threads and then the caller, with the original
/// cause replaced by "opus parallel worker panicked".
#[test]
fn parallel_encode_reports_errors_rather_than_panicking() {
    use opus_pure::{ParallelConfig, encode_parallel};

    let cfg = ParallelConfig::new(48_000, 1, Application::Audio);
    let pcm = speech_like(48_000, 48_000);
    // 333 samples at 48 kHz is not one of the nine durations Opus defines.
    match encode_parallel(&cfg, &pcm, 333) {
        Err(Error::InvalidArgument(_)) => {}
        other => panic!("expected InvalidArgument, got {:?}", other.map(|v| v.len())),
    }
    // The serial fallback path has to agree.
    match encode_parallel(&cfg, &speech_like(48_000, 960), 333) {
        Err(Error::InvalidArgument(_)) => {}
        other => panic!(
            "serial path: expected InvalidArgument, got {:?}",
            other.map(|v| v.len())
        ),
    }
}

/// An `.opus` header built from an encoder carries that encoder's real delay.
///
/// `OpusHead::new` uses the conventional 312, which is right for two of the
/// three applications. `RestrictedLowDelay` gives up the 4 ms the others spend
/// keeping SILK and CELT aligned, so a 312-sample pre-skip over it tells every
/// player to discard 192 samples of genuine audio.
#[test]
fn header_pre_skip_follows_the_encoder_not_a_constant() {
    use opus_pure::OpusHead;

    for (app, expected) in [
        (Application::Audio, 312u16),
        (Application::Voip, 312),
        (Application::RestrictedLowDelay, 120),
    ] {
        let enc = OpusEncoder::new(48_000, 2, app).unwrap();
        let head = OpusHead::for_encoder(&enc, 48_000);
        assert_eq!(head.pre_skip, expected, "{app:?}");
        assert_eq!(head.pre_skip as usize, enc.lookahead(), "{app:?} at 48 kHz");
    }

    // pre_skip is counted at 48 kHz whatever rate the encoder runs at.
    let enc = OpusEncoder::new(16_000, 1, Application::Audio).unwrap();
    assert_eq!(OpusHead::for_encoder(&enc, 16_000).pre_skip, 312);
    assert_eq!(enc.lookahead(), 104, "312 at 48 kHz is 104 at 16 kHz");
}

/// A decoder built from the header carries the three facts the header holds,
/// including the one that is silent when it is missed.
///
/// A wrong channel count fails loudly and a wrong pre-skip shifts the audio
/// audibly, but a dropped `output_gain_q8` only plays the file at the wrong
/// level. RFC 7845 §5.1 says a player SHOULD apply it.
#[test]
fn a_decoder_built_from_the_header_carries_its_gain() {
    use opus_pure::OpusHead;

    let mut head = OpusHead::new(2, 48_000).unwrap();
    head.output_gain_q8 = -1536; // -6 dB
    let decoder = head.decoder(24_000).unwrap();
    assert_eq!(decoder.channels(), 2);
    assert_eq!(decoder.sample_rate(), 24_000);
    assert_eq!(decoder.gain_q8, -1536);
    assert!((head.output_gain_db() - -6.0).abs() < 1e-3);

    // A surround stream is not a plain `OpusDecoder`, and saying so beats
    // decoding one stream of several.
    let surround = OpusHead::for_layout(&opus_pure::ChannelLayout::surround(6, 1).unwrap(), 48_000);
    assert!(matches!(
        surround.decoder(48_000),
        Err(Error::InvalidArgument(_))
    ));
    assert!(matches!(
        head.decoder(44_100),
        Err(Error::InvalidArgument(_))
    ));
}

/// The decode-side buffer size, so no caller has to write `rate / 1000 * 120`.
///
/// `MAX_PACKET_SAMPLES` is the companion to `MAX_PACKET_BYTES` on the way in: a
/// buffer of that many samples per channel must take the longest packet Opus
/// can describe, at any rate.
#[test]
fn the_decode_buffer_size_is_a_constant_not_a_magic_number() {
    use opus_pure::{MAX_PACKET_SAMPLES, packet};

    assert_eq!(MAX_PACKET_SAMPLES, 5760);

    // 120 ms of CELT at 48 kHz is the longest packet there is.
    let mut enc = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
    let n = enc
        .encode(&music_like(48_000, 5760 * 2), 5760, &mut pkt)
        .unwrap();
    assert_eq!(packet::samples(&pkt[..n], 48_000).unwrap(), 5760);

    let mut dec = OpusDecoder::new(48_000, 2).unwrap();
    let mut out = vec![0.0f32; MAX_PACKET_SAMPLES * 2];
    assert_eq!(
        dec.decode(&pkt[..n], MAX_PACKET_SAMPLES, &mut out).unwrap(),
        5760
    );
}

/// A surround header can be built without re-deriving the layout by hand.
#[test]
fn surround_header_comes_from_the_encoder() {
    use opus_pure::{ChannelLayout, OpusHead};

    let enc = OpusMSEncoder::new(48_000, 6, 1, Application::Audio).unwrap();
    let head = OpusHead::for_ms_encoder(&enc, 48_000);
    let layout = ChannelLayout::surround(6, 1).unwrap();

    assert_eq!(head.channel_count, 6);
    assert_eq!(head.mapping_family, 1);
    assert_eq!(head.stream_count as usize, layout.nb_streams);
    assert_eq!(head.coupled_count as usize, layout.nb_coupled_streams);
    assert_eq!(head.channel_mapping, layout.mapping);
    // Family 0 carries no explicit mapping.
    assert!(
        OpusHead::for_layout(&ChannelLayout::surround(2, 0).unwrap(), 48_000)
            .channel_mapping
            .is_empty()
    );
}

/// Unconstrained VBR is reachable and is not the same encode as the default.
#[test]
fn rate_control_selects_three_distinct_behaviours() {
    let pcm = interleave(&[music_like(48_000, 960 * 8), speech_like(48_000, 960 * 8)]);
    let run = |rc: RateControl| -> Vec<usize> {
        let mut enc = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
        enc.bitrate_bps = 64_000;
        enc.rate_control = rc;
        let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
        pcm.chunks_exact(frame_20ms() * 2)
            .map(|c| enc.encode(c, 960, &mut pkt).unwrap())
            .collect()
    };

    let cbr = run(RateControl::Cbr);
    let constrained = run(RateControl::ConstrainedVbr);
    let free = run(RateControl::Vbr);

    assert!(
        cbr.windows(2).all(|w| w[0] == w[1]),
        "CBR packets vary in size: {cbr:?}"
    );
    assert!(
        constrained.windows(2).any(|w| w[0] != w[1]),
        "constrained VBR produced fixed-size packets"
    );
    assert_ne!(
        constrained, free,
        "Vbr and ConstrainedVbr produced identical output, so the constraint is \
         not reaching the CELT layer"
    );
    // The default must remain what it has always been.
    let default = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    assert_eq!(default.rate_control, RateControl::ConstrainedVbr);
}

/// `reset_state` makes a used codec behave like a fresh one.
#[test]
fn reset_state_returns_a_codec_to_its_starting_point() {
    let a = speech_like(48_000, 960 * 4);
    let b = music_like(48_000, 960 * 4);

    let encode_all = |enc: &mut OpusEncoder, pcm: &[f32]| -> Vec<Vec<u8>> {
        let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
        pcm.chunks_exact(frame_20ms())
            .map(|c| {
                let n = enc.encode(c, 960, &mut pkt).unwrap();
                pkt[..n].to_vec()
            })
            .collect()
    };

    let mut fresh = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let want = encode_all(&mut fresh, &b);

    let mut reused = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let _ = encode_all(&mut reused, &a);
    reused.reset_state().unwrap();
    let got = encode_all(&mut reused, &b);

    assert_eq!(got, want, "a reset encoder did not encode like a new one");

    // Settings survive the reset; coding state does not.
    let mut enc = OpusEncoder::new(48_000, 1, Application::Voip).unwrap();
    enc.bitrate_bps = 24_000;
    enc.complexity = 4;
    enc.use_inband_fec = true;
    enc.reset_state().unwrap();
    assert_eq!(enc.bitrate_bps, 24_000);
    assert_eq!(enc.complexity, 4);
    assert!(enc.use_inband_fec);
    assert_eq!(enc.application(), Application::Voip);

    let mut dec = OpusDecoder::new(48_000, 2).unwrap();
    dec.gain_q8 = 256;
    let [celt, ..] = packets_of_every_mode();
    let mut pcm = vec![0.0f32; 960 * 2];
    dec.decode(&celt[0], 960, &mut pcm).unwrap();
    assert_ne!(dec.final_range(), 0);
    assert_ne!(dec.last_packet_duration(), 0);
    dec.reset_state().unwrap();
    assert_eq!(dec.gain_q8, 256, "the gain is a setting, not coding state");
    assert_eq!(dec.final_range(), 0);
    assert_eq!(dec.last_packet_duration(), 0);
}

/// Twelve 20 ms packets of each coding mode: CELT stereo music,
/// narrowband SILK speech, and the SILK+CELT hybrid.
fn packets_of_every_mode() -> [Vec<Vec<u8>>; 3] {
    let run = |rate: i32, channels: usize, bitrate: i32, app: Application, pcm: Vec<f32>| {
        let mut enc = OpusEncoder::new(rate, channels, app).unwrap();
        enc.bitrate_bps = bitrate;
        let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
        let frame = rate as usize / 50;
        pcm.chunks_exact(frame * channels)
            .map(|c| {
                let n = enc.encode(c, frame, &mut pkt).unwrap();
                pkt[..n].to_vec()
            })
            .collect::<Vec<_>>()
    };
    let n = 960 * 12;
    [
        run(
            48_000,
            2,
            128_000,
            Application::Audio,
            interleave(&[music_like(48_000, n), speech_like(48_000, n)]),
        ),
        run(
            16_000,
            1,
            12_000,
            Application::Voip,
            speech_like(16_000, 320 * 12),
        ),
        run(
            48_000,
            1,
            24_000,
            Application::Audio,
            speech_like(48_000, n),
        ),
    ]
}

/// A reset decoder decodes exactly as a new one does, whatever it decoded
/// before, which is what `reset_state` promises now that it resets in place.
///
/// Most decoder state is also cleared by a switch between modes, so a reset
/// that missed some would still pass a test that switched modes straight
/// after it. Every pairing is tried instead: the used decoder last decoded
/// each mode, and decodes each mode first after the reset, on the float and the
/// 16-bit path, with and without a lost packet in front, over a spread of
/// output rates and channel counts. Some fields are overwritten before anything
/// reads them after a reset, so no output can see them; the exhaustive
/// destructure in `reset_state` is what keeps those right.
#[test]
fn a_reset_decoder_decodes_like_a_new_one() {
    let modes = packets_of_every_mode();

    for (rate, channels) in [
        (48_000, 2),
        (48_000, 1),
        (24_000, 2),
        (16_000, 1),
        (8_000, 2),
    ] {
        let frame_size = rate as usize * 120 / 1000;
        let mut float = vec![0.0f32; frame_size * channels];
        let mut s16 = vec![0i16; frame_size * channels];

        // One packet, `None` for a lost one, on the float or the 16-bit path.
        let play = |dec: &mut OpusDecoder, packets: &[Option<&[u8]>], as_s16: bool| {
            let mut float = vec![0.0f32; frame_size * channels];
            let mut s16 = vec![0i16; frame_size * channels];
            let mut out = Vec::new();
            for p in packets {
                let p = p.unwrap_or(&[]);
                if as_s16 {
                    let n = dec.decode_s16(p, frame_size, &mut s16).unwrap();
                    out.extend(s16[..n * channels].iter().map(|&s| s as u32));
                } else {
                    let n = dec.decode(p, frame_size, &mut float).unwrap();
                    out.extend(float[..n * channels].iter().map(|s| s.to_bits()));
                }
            }
            out
        };

        for first in 0..modes.len() {
            // The first mode, then the other two, then the first again.
            let mut after: Vec<Option<&[u8]>> = Vec::new();
            for m in [first, (first + 1) % 3, (first + 2) % 3, first] {
                after.extend(modes[m][..3].iter().map(|p| Some(p.as_slice())));
            }
            for lead_loss in [false, true] {
                let after = if lead_loss {
                    [&[None][..], &after].concat()
                } else {
                    after.clone()
                };
                for as_s16 in [false, true] {
                    let want = play(
                        &mut OpusDecoder::new(rate, channels).unwrap(),
                        &after,
                        as_s16,
                    );
                    assert!(want.iter().any(|&s| s != 0));

                    for last in 0..modes.len() {
                        let mut used = OpusDecoder::new(rate, channels).unwrap();
                        // Every mode, concealment and in-band FEC, then the
                        // last mode, ending on frames driven so far past full
                        // scale that the 16-bit path's soft clip is almost
                        // certainly mid-curve when the reset comes.
                        for m in [(last + 1) % 3, (last + 2) % 3, last] {
                            for (i, p) in modes[m][3..9].iter().enumerate() {
                                match i {
                                    1 => used.decode(&[], frame_size, &mut float),
                                    2 => used.decode_fec(p, frame_size, &mut float),
                                    _ => used.decode(p, frame_size, &mut float),
                                }
                                .unwrap();
                            }
                        }
                        used.gain_q8 = 256 * 24;
                        for p in &modes[last][9..] {
                            used.decode_s16(p, frame_size, &mut s16).unwrap();
                        }
                        used.gain_q8 = 0;

                        used.reset_state().unwrap();
                        let got = play(&mut used, &after, as_s16);
                        assert!(
                            got == want,
                            "{rate} Hz x{channels}: last decoded mode {last}, then mode {first} \
                             first{} on the {} path, decoded differently after a reset",
                            if lead_loss {
                                " behind a lost packet"
                            } else {
                                ""
                            },
                            if as_s16 { "16-bit" } else { "float" },
                        );
                    }
                }
            }
        }
    }
}

/// The decoder can apply the gain RFC 7845 puts in the container header.
#[test]
fn decoder_applies_output_gain() {
    let pcm = speech_like(48_000, 960 * 3);
    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
    let packets: Vec<Vec<u8>> = pcm
        .chunks_exact(frame_20ms())
        .map(|c| {
            let n = enc.encode(c, 960, &mut pkt).unwrap();
            pkt[..n].to_vec()
        })
        .collect();

    let rms = |gain_q8: i32| -> f64 {
        let mut dec = OpusDecoder::new(48_000, 1).unwrap();
        dec.gain_q8 = gain_q8;
        let mut out = vec![0.0f32; 960];
        let mut sum = 0.0f64;
        let mut n = 0usize;
        for p in &packets {
            let got = dec.decode(p, 960, &mut out).unwrap();
            sum += out[..got].iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
            n += got;
        }
        (sum / n as f64).sqrt()
    };

    let unity = rms(0);
    // +6.02 dB is a factor of two, and 6.02 dB in Q8 is 6.02 * 256.
    let doubled = rms((6.0206 * 256.0) as i32);
    assert!(
        (doubled / unity - 2.0).abs() < 0.02,
        "+6 dB of gain scaled the output by {:.3}, not 2.0",
        doubled / unity
    );
}

/// A packet says which coding layers it used and what bandwidth it carries.
#[test]
fn packet_reports_its_mode_and_bandwidth() {
    use opus_pure::{OpusMode, packet};

    let mut enc = OpusEncoder::new(48_000, 1, Application::Voip).unwrap();
    enc.bitrate_bps = 12_000;
    enc.force_bandwidth = Some(Bandwidth::Wideband);
    let mut pkt = vec![0u8; 4000];
    let n = enc
        .encode(&speech_like(48_000, 960), 960, &mut pkt)
        .unwrap();
    assert_eq!(packet::mode(&pkt[..n]).unwrap(), OpusMode::SilkOnly);
    assert_eq!(packet::bandwidth(&pkt[..n]).unwrap(), Bandwidth::Wideband);

    let mut enc = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
    enc.bitrate_bps = 256_000;
    let n = enc
        .encode(
            &interleave(&[music_like(48_000, 960), music_like(48_000, 960)]),
            960,
            &mut pkt,
        )
        .unwrap();
    assert_eq!(packet::mode(&pkt[..n]).unwrap(), OpusMode::CeltOnly);
    assert_eq!(packet::bandwidth(&pkt[..n]).unwrap(), Bandwidth::Fullband);

    assert!(packet::mode(&[]).is_err());
    assert!(packet::bandwidth(&[]).is_err());
}

/// Vorbis comments may repeat a name, so the accessor that admits it exists.
#[test]
fn tags_append_and_read_back_every_value() {
    use opus_pure::OpusTags;

    let mut tags = OpusTags::new();
    tags.push("ARTIST", "First").unwrap();
    tags.push("ARTIST", "Second").unwrap();
    tags.push("TITLE", "Only").unwrap();

    assert_eq!(tags.get("ARTIST"), Some("First"));
    assert_eq!(
        tags.get_all("ARTIST").collect::<Vec<_>>(),
        vec!["First", "Second"]
    );
    assert_eq!(
        tags.get_all("artist").count(),
        2,
        "names are case-insensitive"
    );
    assert_eq!(tags.get_all("MISSING").count(), 0);
    assert_eq!(tags.get("TITLE"), Some("Only"));
}

/// The writer takes a packet's duration from the packet.
#[test]
fn writer_derives_the_granule_from_the_packet() {
    use opus_pure::{OggOpusReader, OggOpusWriter, OpusHead, packet};

    for frame in [120usize, 960, 2880] {
        let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
        let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
        let head = OpusHead::for_encoder(&enc, 48_000);
        let mut w = OggOpusWriter::new(Vec::new(), head).unwrap();
        let pcm = speech_like(48_000, frame * 4);
        let mut expected = 0u64;
        for c in pcm.chunks_exact(frame) {
            let n = enc.encode(c, frame, &mut pkt).unwrap();
            expected += packet::samples_48k(&pkt[..n]).unwrap() as u64;
            w.write_packet(&pkt[..n]).unwrap();
        }
        let file = w.finish().unwrap();

        let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
        let last = r.packets().last().unwrap().unwrap();
        assert_eq!(
            last.page_granule as u64, expected,
            "frame {frame}: the granule does not match the audio written"
        );
    }
}

/// The reader is usable as an iterator, and stops at the first error.
#[test]
fn reader_iterates_and_stops_on_error() {
    use opus_pure::{OggOpusReader, OggOpusWriter, OpusHead};

    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
    let mut w = OggOpusWriter::new(Vec::new(), OpusHead::for_encoder(&enc, 48_000)).unwrap();
    for c in speech_like(48_000, 960 * 6).chunks_exact(frame_20ms()) {
        let n = enc.encode(c, 960, &mut pkt).unwrap();
        w.write_packet(&pkt[..n]).unwrap();
    }
    let file = w.finish().unwrap();

    let mut r = OggOpusReader::new(std::io::Cursor::new(&file)).unwrap();
    let packets = r.packets().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(packets.len(), 6);

    // Truncating mid-stream makes the iterator yield an error and then end,
    // rather than repeating the error for ever.
    let truncated = &file[..file.len() * 2 / 3];
    let mut r = OggOpusReader::new(std::io::Cursor::new(truncated)).unwrap();
    let mut saw_error = false;
    let mut count = 0;
    for item in r.packets() {
        count += 1;
        if item.is_err() {
            saw_error = true;
        }
        assert!(count < 1000, "the iterator did not terminate");
    }
    assert!(saw_error || count > 0);
}

/// A repacketizer can be reused, and can write into a caller's buffer.
#[test]
fn repacketizer_is_reusable_and_can_append() {
    use opus_pure::Repacketizer;

    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
    let packets: Vec<Vec<u8>> = speech_like(48_000, 960 * 4)
        .chunks_exact(frame_20ms())
        .map(|c| {
            let n = enc.encode(c, 960, &mut pkt).unwrap();
            pkt[..n].to_vec()
        })
        .collect();

    let mut rp = Repacketizer::new();
    let mut owned = Vec::new();
    for p in &packets[..3] {
        rp.cat(p).unwrap();
    }
    let merged = rp.out().unwrap();
    rp.out_into(&mut owned).unwrap();
    assert_eq!(merged, owned, "out_into disagrees with out");

    // Clearing and refilling gives the same answer as a fresh instance.
    rp.clear();
    assert_eq!(rp.nb_frames(), 0);
    for p in &packets[..2] {
        rp.cat(p).unwrap();
    }
    let after_clear = rp.out().unwrap();

    let mut fresh = Repacketizer::new();
    for p in &packets[..2] {
        fresh.cat(p).unwrap();
    }
    assert_eq!(after_clear, fresh.out().unwrap());

    // `_into` appends rather than replacing.
    let mut two = Vec::new();
    rp.out_into(&mut two).unwrap();
    let one_len = two.len();
    rp.out_into(&mut two).unwrap();
    assert_eq!(
        two.len(),
        one_len * 2,
        "out_into overwrote instead of appending"
    );
}

/// Codec configuration is readable back out, so a pool need not carry it.
#[test]
fn codecs_report_how_they_were_built() {
    let enc = OpusEncoder::new(24_000, 2, Application::Voip).unwrap();
    assert_eq!(enc.sample_rate(), 24_000);
    assert_eq!(enc.channels(), 2);
    assert_eq!(enc.application(), Application::Voip);

    let dec = OpusDecoder::new(16_000, 1).unwrap();
    assert_eq!(dec.sample_rate(), 16_000);
    assert_eq!(dec.channels(), 1);
    assert_eq!(dec.last_packet_duration(), 0);

    let ms = OpusMSEncoder::new(48_000, 6, 1, Application::Audio).unwrap();
    assert_eq!(ms.sample_rate(), 48_000);
    assert_eq!(ms.channels(), 6);
    assert_eq!(ms.layout().nb_channels, 6);

    let msd = OpusMSDecoder::new(48_000, 6, 1).unwrap();
    assert_eq!(msd.sample_rate(), 48_000);
    assert_eq!(msd.channels(), 6);
    assert_eq!(msd.nb_streams(), 4);
}

/// `last_packet_duration` reports what was actually decoded.
#[test]
fn decoder_reports_the_last_packet_duration() {
    let mut enc = OpusEncoder::new(48_000, 1, Application::Audio).unwrap();
    let mut dec = OpusDecoder::new(48_000, 1).unwrap();
    let mut pkt = vec![0u8; opus_pure::MAX_PACKET_BYTES];
    let mut out = vec![0.0f32; 5760];

    for frame in [480usize, 960, 1920] {
        let n = enc
            .encode(&speech_like(48_000, frame), frame, &mut pkt)
            .unwrap();
        dec.decode(&pkt[..n], 5760, &mut out).unwrap();
        assert_eq!(dec.last_packet_duration(), frame);
    }
}

# opus-pure

[![CI](https://github.com/stephenberry/opus-pure/actions/workflows/ci.yml/badge.svg)](https://github.com/stephenberry/opus-pure/actions/workflows/ci.yml)

Pure-Rust [Opus](https://opus-codec.org/) audio codec (RFC 6716) with Ogg encapsulation (RFC 7845).

It reads and writes `.opus` files, and it encodes and decodes raw Opus packets for RTP, WebRTC, or a container of your own. There is no C, no FFI, no `build.rs`, and no dependencies — adding the crate is the whole install, and it cross-compiles anywhere Rust does.

- **A complete codec.** Encoder *and* decoder for all three Opus modes — SILK for speech, CELT for music, and the hybrid of both — at 8, 12, 16, 24 and 48 kHz, mono and stereo, at every one of the nine Opus frame sizes.
- **A real container.** Ogg mux *and* demux, with `OpusHead`, `OpusTags`, page CRCs and correct granule positions. Files it writes pass `opusinfo` and decode with `opusdec`; files `opusenc` writes read back through this crate.
- **Checked against the C library.** Byte-exact with libopus 1.6.1 everywhere the format allows it, and measured against it everywhere it does not. See [Correctness](#correctness).
- **Fast.** Encoding runs 1.1–1.8x libopus on the same machine and the same audio; decoding runs at 0.84–1.02x of it. See [Speed](#speed).

**Contents** — [Install](#install) · [Quick start](#quick-start) · [Choosing settings](#choosing-settings) · [Beyond the basics](#beyond-the-basics) · [What's here](#whats-here) · [Correctness](#correctness) · [Speed](#speed) · [Known limitations](#known-limitations)

## Install

```toml
[dependencies]
opus-pure = "0.2"
```

Requires Rust 1.88 or newer. Nothing else: there are no optional features to turn on for anything below, and no system libopus needs to be installed. Full API documentation is on [docs.rs](https://docs.rs/opus-pure).

## Quick start

Four things worth knowing before the code:

- **Samples are interleaved.** Stereo PCM is `[L, R, L, R, …]`, so a buffer holds `frame_size * channels` values.
- **`frame_size` is per channel.** 20 ms is the usual choice, which is 960 samples per channel at 48 kHz (`sample_rate / 50`).
- **Float samples run from -1.0 to 1.0**, nominally, and the decoder's output may drift slightly past that. See [Known limitations](#known-limitations); if you convert to `i16` with `decode_s16` it is already handled.
- **Opus decodes to whatever rate you ask for**, not the one the file was made at. 48 kHz is always a safe answer.

Every fallible call returns `opus_pure::Result<T>`, an alias for `Result<T, opus_pure::Error>`, and `Error` implements `std::error::Error`.

### Encode PCM to an `.opus` file

```rust
use opus_pure::{Application, MAX_PACKET_BYTES, OggOpusWriter, OpusEncoder, OpusHead, Result};

/// Write interleaved f32 samples to a playable `.opus` file.
fn encode_to_file(pcm: &[f32], rate: i32, channels: usize, path: &str) -> Result<()> {
    let frame = (rate / 50) as usize;                 // 20 ms per packet

    let mut encoder = OpusEncoder::new(rate, channels, Application::Audio)?;
    encoder.bitrate_bps = 96_000;

    // `for_encoder` takes the header's pre-skip from this encoder's real delay
    // rather than from a constant, so a player trims exactly the right amount.
    let head = OpusHead::for_encoder(&encoder, rate as u32);
    let file = std::fs::File::create(path)?;
    let mut writer = OggOpusWriter::new(std::io::BufWriter::new(file), head)?;

    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    for block in pcm.chunks_exact(frame * channels) {
        let n = encoder.encode(block, frame, &mut packet)?;
        writer.write_packet(&packet[..n])?;      // duration read from the packet
    }
    writer.finish()?;                            // writes the end-of-stream page
    Ok(())
}
```

`finish` must be called. Dropping the writer flushes on a best-effort basis but cannot report an I/O failure.

`chunks_exact` drops a trailing partial frame, so this writes a whole number of frames and loses whatever is left over. That is fine for a stream and not fine for a file. See [Ending the file exactly](#ending-the-file-exactly).

### Ending the file exactly

Opus codes whole frames and runs a few milliseconds behind its input, so a file needs two things to come back as the audio that went into it. Both are in [`examples/encode.rs`](examples/encode.rs); this is the arithmetic on its own.

```rust
use opus_pure::{Application, MAX_PACKET_BYTES, OggOpusWriter, OpusEncoder, OpusHead, Result};

/// Write `pcm` so that decoding it returns exactly these samples — no padding
/// on the end, and none of the tail missing.
fn encode_gapless(pcm: &[f32], rate: i32, channels: usize, path: &str) -> Result<()> {
    let frame = (rate / 50) as usize;
    let mut encoder = OpusEncoder::new(rate, channels, Application::Audio)?;
    let head = OpusHead::for_encoder(&encoder, rate as u32);

    // Every Opus rate divides 48 kHz, so these conversions are exact.
    let ticks = 48_000 / rate as usize;   // granule ticks per encoder-rate sample
    let total = pcm.len() / channels;     // sample frames of real audio

    // 1. The encoder is `pre_skip` samples behind, so its last samples are still
    //    inside it when the input runs out. Feeding that much extra silence is
    //    what flushes them; then round up, since Opus has no partial frames.
    let frames = (total + (head.pre_skip as usize).div_ceil(ticks)).div_ceil(frame);
    // 2. That padding decodes as real output, so the file has to say where the
    //    audio stopped: the final granule is the pre-skip plus the audio and
    //    nothing else (RFC 7845 §4.4). The last packet carries the difference
    //    between that and what the writer has counted so far.
    let final_granule = u64::from(head.pre_skip) + (total * ticks) as u64;

    let file = std::fs::File::create(path)?;
    let mut writer = OggOpusWriter::new(std::io::BufWriter::new(file), head)?;
    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    let per_frame = frame * channels;
    let mut block = vec![0.0f32; per_frame];

    for i in 0..frames {
        let start = (i * per_frame).min(pcm.len());
        let end = (start + per_frame).min(pcm.len());
        block[..end - start].copy_from_slice(&pcm[start..end]);
        block[end - start..].fill(0.0);         // silence past the audio

        let n = encoder.encode(&block, frame, &mut packet)?;
        if i + 1 == frames {
            let duration = final_granule - writer.granule() as u64;
            writer.write_packet_with_duration(&packet[..n], duration as u32)?;
        } else {
            writer.write_packet(&packet[..n])?;
        }
    }
    writer.finish()?;
    Ok(())
}
```

`write_packet_with_duration` is the only reason to state a duration by hand; `write_packet` reads the right one out of every other packet. Skip the end-trim and the file plays up to a frame of padding past its own end — inaudible once, an audible gap at every seam of a loop. Skip the extra silence and the last few milliseconds of the audio never leave the encoder.

### Read one back

```rust
use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, Result, Trim};

/// Decode an `.opus` file to interleaved f32 at `rate` (8/12/16/24/48 kHz).
fn decode_from_file(path: &str, rate: i32) -> Result<(Vec<f32>, usize)> {
    let file = std::fs::File::open(path)?;
    let mut reader = OggOpusReader::new(std::io::BufReader::new(file))?;
    let channels = reader.head().channel_count as usize;

    // `decoder` carries the channel count and the header's output gain, which
    // RFC 7845 §5.1 says a player should apply.
    let mut decoder = reader.head().decoder(rate)?;
    // `trim` takes the encoder delay off the front and the end-trim off the
    // back, so what comes out is the audio and nothing else.
    let mut trim = Trim::new(reader.head(), rate, channels)?;

    // Sized for the longest packet Opus allows, so the loop does not care what
    // frame size the file was made with. `decode` returns what it produced.
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];

    let mut pcm = Vec::new();
    for packet in reader.packets() {
        let packet = packet?;
        let n = decoder.decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)?;
        pcm.extend_from_slice(trim.keep(&packet, &block[..n * channels]));
    }
    Ok((pcm, channels))
}
```

A decoded Opus stream is longer than the audio that went into it at both ends, and dropping either correction is silent. [`Trim`](https://docs.rs/opus-pure/latest/opus_pure/struct.Trim.html) applies both: the pre-skip at the front (RFC 7845 §4.2, the encoder's algorithmic delay) and the end-trim at the back (§4.4, a final granule position deliberately short of the audio). Every file `opusenc` writes carries an end-trim, and so does every file the recipe above writes.

### Playing it back a buffer at a time

A file read all at once can take `Trim::keep` and its slice. A player cannot: the audio device asks for however many samples it wants, whenever it wants them, and what is left of the last packet has to survive until the next call. [`keep_range`](https://docs.rs/opus-pure/latest/opus_pure/struct.Trim.html#method.keep_range) is `keep` as indices for exactly that. The trimmed length alone does not say where the audio starts, because the pre-skip cuts the front and the end-trim the back.

```rust
use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, OggPacket, OpusDecoder, Result, Trim};
use std::io::Read;
use std::ops::Range;

struct Playback<R: Read> {
    reader: OggOpusReader<R>,
    decoder: OpusDecoder,
    trim: Trim,
    channels: usize,
    /// One packet, read. Kept so each read reuses its buffer.
    packet: OggPacket,
    /// One packet, decoded.
    block: Vec<f32>,
    /// The part of `block` that is audio and has not been handed out yet.
    /// Indices rather than a slice: this outlives the call that produced it.
    pending: Range<usize>,
}

impl<R: Read> Playback<R> {
    fn new(source: R, rate: i32) -> Result<Self> {
        let reader = OggOpusReader::new(source)?;
        let channels = reader.head().channel_count as usize;
        Ok(Self {
            decoder: reader.head().decoder(rate)?,
            trim: Trim::new(reader.head(), rate, channels)?,
            packet: OggPacket::default(),
            block: vec![0.0f32; MAX_PACKET_SAMPLES * channels],
            channels,
            pending: 0..0,
            reader,
        })
    }

    /// Fills `out` with interleaved audio, decoding as it runs out, and pads
    /// with silence at the end of the stream. Returns how much of it is audio.
    fn fill(&mut self, out: &mut [f32]) -> Result<usize> {
        let mut written = 0;
        while written < out.len() {
            while self.pending.is_empty() {
                // A packet can trim to nothing — the pre-skip covers the whole
                // of it, or the end-trim already fell — so this loops.
                if !self.reader.read_packet_into(&mut self.packet)? {
                    out[written..].fill(0.0); // the device's buffer arrives dirty
                    return Ok(written);
                }
                let n = self
                    .decoder
                    .decode(&self.packet.data, MAX_PACKET_SAMPLES, &mut self.block)?;
                self.pending = self.trim.keep_range(&self.packet, n * self.channels);
            }
            let take = (out.len() - written).min(self.pending.len());
            let src = self.pending.start..self.pending.start + take;
            out[written..written + take].copy_from_slice(&self.block[src]);
            self.pending.start += take;
            written += take;
        }
        Ok(written)
    }
}
```

Nothing is staged in between: the decode buffer is the staging buffer, and each sample is written once, straight into the device's buffer. Nothing is allocated either, once the stream's largest page and packet have been through: `read_packet_into` reuses the kept packet, which matters on an audio thread, where an allocation can take a lock. To loop over a source that can seek (`R: Read + Seek`), call `reader.rewind()` at the end of the stream, give the decoder a `reset_state()` and the `Trim` a fresh instance; none of the three allocates, so a loop that has played through once allocates nothing on later passes.

### Raw packets, without the container

If the framing comes from somewhere else — RTP, WebRTC, your own file format — use the encoder and decoder on their own. Nothing above is required.

```rust
use opus_pure::{Application, MAX_PACKET_BYTES, OpusDecoder, OpusEncoder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (rate, channels, frame) = (48_000, 1, 960);      // 20 ms of mono

    let mut encoder = OpusEncoder::new(rate, channels, Application::Voip)?;
    encoder.bitrate_bps = 24_000;
    let mut decoder = OpusDecoder::new(rate, channels)?;

    let pcm = vec![0.0f32; frame * channels];
    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    let mut out = vec![0.0f32; frame * channels];

    let n = encoder.encode(&pcm, frame, &mut packet)?;   // n bytes to send
    let samples = decoder.decode(&packet[..n], frame, &mut out)?;
    assert_eq!(samples, frame);

    // A packet that never arrived: decode an empty slice to run concealment.
    let samples = decoder.decode(&[], frame, &mut out)?;
    assert_eq!(samples, frame);
    Ok(())
}
```

`MAX_PACKET_BYTES` is large enough for any packet at any setting. A smaller buffer is not an error: like libopus's `max_data_bytes`, `output.len()` is the encoder's *byte budget* for this packet, and a tight one produces a smaller packet rather than a failure.

### If your samples are `i16`

Most audio in flight is 16-bit, so both directions have an integer entry point that takes and returns `i16` directly:

```rust
use opus_pure::{Application, MAX_PACKET_BYTES, OpusDecoder, OpusEncoder};

let mut encoder = OpusEncoder::new(48_000, 2, Application::Audio).unwrap();
let mut decoder = OpusDecoder::new(48_000, 2).unwrap();

let pcm = vec![0i16; 960 * 2];
let mut packet = vec![0u8; MAX_PACKET_BYTES];
let mut out = vec![0i16; 960 * 2];

let n = encoder.encode_s16(&pcm, 960, &mut packet).unwrap();
decoder.decode_s16(&packet[..n], 960, &mut out).unwrap();
```

These are not wrappers over the float ones, any more than libopus's are. `encode_s16` declares 16 bits of input precision where the float entry point declares 24, which moves the floor below which the encoder calls a signal silent. `decode_s16` soft-clips before converting, because a codec rings and a track mastered near full scale comes back slightly over it: saturating there replaces a whole excursion with a flat plateau, and the corner at each end of that plateau is what spreads distortion across the spectrum. [`SoftClip`](https://docs.rs/opus-pure/latest/opus_pure/struct.SoftClip.html) is public on its own, for callers who take the float output and convert it downstream.

### Run it

Two complete programs, with nothing to install. `input.wav` is any 16-bit PCM WAV file:

```bash
cargo run --release --example encode -- input.wav output.opus 96000
cargo run --release --example decode -- output.opus roundtrip.wav
```

[`examples/encode.rs`](examples/encode.rs) and [`examples/decode.rs`](examples/decode.rs) are the code above with the loose ends tied off, and they are a matched pair: `roundtrip.wav` holds the same number of samples as `input.wav`, to the sample.

## Choosing settings

Settings are public fields on `OpusEncoder`, so there is nothing to build or unwrap: assign and keep encoding. Every one of them can change between packets.

```rust
use opus_pure::{Application, OpusEncoder, RateControl};

let mut encoder = OpusEncoder::new(48_000, 1, Application::Voip).unwrap();
encoder.bitrate_bps = 24_000;
encoder.rate_control = RateControl::Cbr;
encoder.use_inband_fec = true;
encoder.packet_loss_perc = 10;
```

The defaults are libopus's: 64 kb/s, complexity 9, constrained VBR, no FEC, no DTX. The two settings you should always consider are the `Application` you construct with and the bitrate.

| If you are… | Then |
|---|---|
| Carrying voice over a network | `Application::Voip`, 16–32 kb/s mono, `use_inband_fec = true`, and `packet_loss_perc` set to what the network actually loses |
| Encoding music or mixed content | `Application::Audio` (the default), 64–128 kb/s stereo |
| Chasing the lowest latency | `Application::RestrictedLowDelay` with a 2.5, 5 or 10 ms frame |
| Feeding a fixed-capacity channel | `RateControl::Cbr`, which pads every packet to the same size |
| Writing a file | `RateControl::Vbr`, which spends bits where the audio needs them |
| Short of CPU | Lower `complexity`; 8, 9 and 10 are within noise of each other, so savings start below 8 |

Bitrate is the single strongest input to the encoder's own decisions — it picks the coding mode and the audio bandwidth from it — so lowering it does not simply degrade the same signal. As a starting point: 16–24 kb/s for mono speech, 32–48 kb/s for good speech or low-rate music, 64–96 kb/s for stereo music, and 128 kb/s and up where quality matters more than size.

### Frame sizes

All nine Opus packet durations — 2.5, 5, 10, 20, 40, 60, 80, 100 and 120 ms — work at every sample rate, mono and stereo. Shorter frames mean lower latency and more per-packet overhead; longer frames mean better compression and more audio lost with each dropped packet. 20 ms is the usual compromise.

2.5 and 5 ms exist only for CELT, so the encoder overrides its mode decision there. Everything longer than 20 ms is chosen freely and then *framed* to suit: SILK keeps its 40 and 60 ms frames whole, while CELT and hybrid, which have no frame longer than 20 ms, get several frames sharing one TOC byte (RFC 6716 §3.2). 80, 100 and 120 ms are always several frames. This is what libopus does in `opus_encode_native`, and the packets come out the same size.

One encoder can change frame size between calls: pass a different `frame_size` and the stream carries on, so a caller adapting to network conditions does not have to start a new encoder.

## Beyond the basics

- **Surround.** `OpusMSEncoder` and `OpusMSDecoder` handle multistream audio up to 7.1, using the Vorbis channel orders RFC 7845 defines (mapping family 1). `ChannelLayout::surround` derives the stream and coupling counts, and `OpusHead::for_ms_encoder` writes a matching header. Both sides expose their per-stream codecs through `streams_mut`, which is where every `OpusEncoder` setting lives and, on the decoder, where the header's output gain has to be set.
- **Inspecting a packet without decoding it.** The [`packet`](https://docs.rs/opus-pure/latest/opus_pure/packet/) module reads duration, frame count, channel count, coding mode and bandwidth out of a packet without decoding it — what a jitter buffer or a muxer needs.
- **Repacketizing.** `Repacketizer` merges consecutive packets into one and splits them back apart, including the self-delimiting framing RFC 6716 Appendix B defines.
- **Packet loss.** `decode` on an empty slice runs concealment. If the encoder had `use_inband_fec` on, `decode_fec` recovers part of a lost frame from the packet *after* it.
- **Parallel encoding.** `encode_parallel` splits a clip across threads. It is an opt-in path with a documented cost; see [Known limitations](#known-limitations).
- **Looping a file.** `OggOpusReader::rewind` goes back to the first audio packet of a source that can seek, without reading the header pages again. Give the decoder a `reset_state()` and the `Trim` a fresh instance at the same time, or the seam carries the previous pass's state. See [Known limitations](#known-limitations) for why there is no `seek` to an arbitrary point.

## What's here

| | |
|---|---|
| Coding modes | SILK, CELT, hybrid, chosen automatically from bitrate and content |
| Rates | 8, 12, 16, 24, 48 kHz; mono and stereo |
| Stereo | Adaptive mid/side in SILK and hybrid, full stereo in CELT |
| Container | Ogg mux **and** demux — `OpusHead`, `OpusTags`, page CRCs, granule positions, multi-page packets |
| Frame sizes | 2.5 to 120 ms, packed as one frame or several per packet as the mode requires |
| Sample format | 32-bit float **and** 16-bit integer, both directions |
| Rate control | VBR, constrained VBR and CBR |
| Loss handling | Packet-loss concealment, and in-band FEC in SILK and hybrid |
| Also | Multistream/surround (up to 7.1), repacketizer, chunk-parallel encoding, DTX |

## Correctness

Opus is a bit-exact format on the decoder side and a defined-but-not-bit-exact one on the encoder side, so every claim below says which kind of agreement it is and what it was measured against. Everything is measured against C libopus 1.6.1 on the same audio.

| | Agreement |
|---|---|
| SILK encode | Byte-exact, across 22 configurations |
| The `i16` API, both directions | Byte-exact, including the soft clip |
| SILK decode, clean and through loss | Bit-exact, including a loss before the first packet |
| Multi-frame packet framing | Byte-exact |
| Encoder delay | Exact to within one sample, every mode at every rate |
| Whole streams, 440 configurations | Differ only inside the cross-fade at a mode switch; 433 agree to better than 100 dB SNR |
| CELT concealment | 83–112 dB, and cannot be exact — see [Known limitations](#known-limitations) |

`cargo test --release` runs all 334 tests in about thirty seconds, and needs nothing installed to do it.

### In detail

- **Byte-exact against C libopus** on 22 SILK configurations, 20 frames each — narrowband, mediumband and wideband at complexity 0/1/2/5/10, at every sample rate the encoder accepts. Eight of them run at 24 and 48 kHz, where SILK resamples the input down to its own internal rate, so the match covers the resampler and everything reading its output. See `tests/reference_vectors.rs`.
- **The integer API is byte-exact against libopus in both directions.** `encode_s16` reproduces `opus_encode` packet for packet, `decode_s16` reproduces `opus_decode` sample for sample — soft clipping and 16-bit conversion included — and the `opus_pcm_soft_clip` port matches bit for bit across frame boundaries. `tests/integer_pcm.rs`. That last comparison needs a libopus built with `-ffp-contract=off`: the curve is `x + a·x·x`, which a compiler may fuse into an FMA and Rust never does, and the fused and unfused results differ in the last bit. Measured, not assumed — 367 of 7680 samples differ by exactly one ULP with contraction on, and none with it off.
- **Verified against C libopus 1.6.1** (`opus-tools` 0.2) in both directions: our `.opus` files pass `opusinfo` and decode with `opusdec`, and `opusenc` files decode through this crate to within float32 rounding of `opusdec`. Across 440 encoder configurations — every sample rate, channel count, bandwidth, application and bitrate, at 20 ms and at 60 ms — **not one stream differs from libopus's own decode of it anywhere except inside the cross-fade at a mode switch**, and the widest such window measured is 4.0 ms against the 5 ms libopus fades over. 433 of the 440 agree to better than 100 dB SNR, and the 103 streams that are pure SILK are bit-identical, every one of them: SILK is fixed-point on both sides, so it agrees exactly or not at all. See [docs/interop-validation.md](docs/interop-validation.md).
- **Frozen-bitstream gate**, so a refactor cannot silently move the encoder — `tests/bitstream_stability.rs`. Most entries are byte-identical to the upstream crate this was forked from; the few that are not each carry a note saying which fix moved them and why.
- **Robustness sweep**: 600 encoder configurations, every single-bit flip and every truncation of a valid packet, and 8,000 random packets — none may panic or produce a non-finite sample. Beyond that, three coverage-guided fuzz targets in [`fuzz/`](fuzz/) cover the decoder across a whole stream, an Ogg file end to end, and a single packet through every inspector that reads one without decoding. They found a panic on the first run: concealing a lost packet longer than 20 ms walked off the CELT decode buffer, which needed no malformed input at all, only a lossy stream coded at 40 or 60 ms.
- **Container**: round-trip, corruption detection, multi-page packets, and lacing edge cases; output independently validated against a from-scratch RFC 3533/7845 parser.
- **Multi-frame packets match libopus byte for byte** at every duration that needs them — same packet size, same TOC, same packing code, on identical audio, mono and stereo. See [docs/interop-validation.md](docs/interop-validation.md#multi-frame-packets).
- **Packet-loss concealment is bit-exact in SILK.** A concealed frame, and every frame after it, is bit-identical to libopus 1.6.1's at 8, 12 and 16 kHz, mono and stereo, at every frame duration and for bursts as well as single losses — including a loss that arrives before the first packet. `tests/decoder_conformance.rs` pins nine loss patterns against the reference. The CELT layer conceals by the same algorithm and agrees to 83–157 dB. See [docs/interop-validation.md](docs/interop-validation.md#packet-loss-concealment).
- **The encoder's delay is asserted as a number, not searched for.** Every layer lands in the same place: CELT's input lags the caller's by `Fs/250` as libopus's does, and SILK's resampler applies `delay_matrix_enc` exactly once, so a stream that switches mode does not step at the seam and the pre-skip the Ogg header declares is the delay the encoder actually has. Measured against libopus, every mode at every sample rate agrees to within a sample. `tests/encoder_delay.rs` pins that: every other fidelity test measures through a lag search, which removes a constant offset by construction and hid three real defects. See [docs/interop-validation.md](docs/interop-validation.md#the-celt-input-delay).
- **The SILK/CELT decision matches libopus** across a 30-configuration sweep at 8, 12 and 16 kHz — both applications, 16 to 64 kb/s — with 27 agreeing exactly and the rest differing only in how many warm-up packets precede the switch. This crate used to pin every rate below 24 kHz to SILK, which cost a third of the requested bitrate at 16 kHz because SILK saturates at wideband. See [docs/interop-validation.md](docs/interop-validation.md#the-16-khz-mode-decision).
- **A hybrid packet's high band comes back at the level it left at.** Above 8 kHz a hybrid stream is CELT's, and at conversational rates it has too few bits to code a waveform there, so it fills the band with shaped noise instead. What can still be held is the level and the moving envelope, and both are: across 12 to 64 kb/s, mono and stereo, fullband and superwideband, the band returns within 0.6 dB of the source. This is not something a broadband SNR can see — the band is 30 to 40 dB down, so it moves a full-spectrum average by hundredths of a dB — and a band-limited SNR cannot see it either, because noise with the right spectrum and the wrong phase scores *worse than silence*: emitting nothing scores exactly 0 dB and both this crate and libopus score below it. `tests/highband.rs`, measured against libopus in [`reference/highband/`](reference/highband/), which is also where the finding came from that libopus quiets that band deliberately, by 4 dB where it can afford least, and this crate does not.
- **One decoder carries a stream whatever its channel count.** A stream may switch between mono and stereo at any packet, and the output channel count need not match either. Both layers merge the channels the way libopus does, upstream of synthesis — SILK emits the mid it already coded, CELT sums the two spectra before a single inverse MDCT — so one overlap-add, prefilter, preemphasis and resampler state carries the whole stream across every switch. Decoding a stereo stream to mono used to run a second, parallel decoder and average its output, and the two of them split the stream between them: it failed 6 of the 12 RFC 6716 vectors, and the error tracked how often the stream switched channel count. All 12 now pass at mono output as well as at stereo, and the three SILK configurations are bit-identical to libopus's own downmix, clean and through a concealed packet. See [docs/interop-validation.md](docs/interop-validation.md#rendering-to-a-different-channel-count).
- **Every SIMD kernel is pinned to its scalar definition** by a test that runs whichever path the host dispatches to, so an aarch64 build checks its NEON kernels and an x86-64 build whichever of SSE, AVX and AVX2 its CPU offers. The catch is in that last clause: a kernel the host does not select is not covered, and the test passes just the same. So the suite also reports which families it reached, and CI turns that report into an assertion per runner — a leg that exists to exercise AVX2 fails if it is handed a machine without it. That is not hypothetical: `cargo test --target x86_64-apple-darwin` runs under Rosetta, which offers SSE2 and nothing above it, so on this project's own hardware the AVX and AVX2 kernels compile on every x86 run and execute on none.
- **The test signals are bit-identical on every target**, so the frozen hashes above mean the same thing everywhere. They are generated from correctly-rounded arithmetic rather than the platform's libm, whose arm64 and x86_64 results differ in the last bits, and `tests/test_signals.rs` pins them. The whole suite passes on aarch64 and x86-64 alike, and the six SILK configurations in `tests/decoder_conformance.rs` are bit-identical to libopus 1.6.1's decode on both.

### Where it runs

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs every check above on four platforms: Linux x86-64, the only one that reaches the AVX2 kernels; Linux aarch64, which holds the frozen CELT hashes against a different libm than the one they were captured on; macOS aarch64; and Windows x86-64. Clippy runs on each rather than once, because half the vector code sits behind `#[cfg(target_arch)]` and a lint host never compiles the other architecture's kernels. A second job holds the formatting, the documentation, the declared MSRV of 1.88.0, and the crate as crates.io would receive it.

### Reproducing any of it

Every claim above was produced by a program in [`reference/`](reference/), and those programs are in the repository rather than described in it. `reference/build.sh` fetches libopus 1.6.1, builds it the three ways the comparisons need, and compiles the harnesses; each subdirectory holds one cross-check and names the test it backs. The frozen values are not copied by hand either — the tests that hold them carry `#[ignore]`d dumpers that write their own inputs where the C tools can reach them.

## Speed

On an Apple M1 Pro, 48 kHz stereo, 20 ms frames, at the default complexity, beside libopus 1.6.1 on the same machine and the same audio:

| | encode | libopus | decode | libopus |
|---|---|---|---|---|
| SILK, 32 kb/s | 67x realtime | 61x | 985x | 1084x |
| hybrid, 24 kb/s | 62x | 55x | 639x | 629x |
| CELT, 128 kb/s | 204x | 116x | 532x | 615x |

**Encode is faster than the C library and decode is close to it**, consistently enough across modes to be structural. The encode lead is widest in CELT, which is where this crate's hand-written SIMD kernels are, and narrowest in SILK at 1.09–1.12x. Decode runs at 0.84–1.02x, with stereo hybrid ahead of the reference. It used to be a flat 0.61–0.72x, and then 0.70–0.87x. Two rounds of profiling the two decoders over the same packets closed it: the first found the DSP kernels already at parity and the gap in four things around them, chiefly a de-emphasis loop carrying two integer divisions per sample and a saturation the reference compiles away in a float build; the second found SILK's LPC synthesis accumulating its sixteen taps newest first, which put every one of them on a recursion that only the first had any reason to wait for.

Mono runs 1.5 to 2x faster than stereo. The slowest configuration in the whole matrix still encodes 64 seconds of audio per second of CPU, so one core carries 64 real-time streams at the worst setting and several hundred at typical ones — as far as CPU is concerned. At that many streams the allocator is the other thing to watch: `encode` makes about ten short-lived allocations per frame, roughly 500 a second per stream, mostly inside the tonality analysis. That is fine for tens of streams and worth measuring before you plan on hundreds.

Two settings move it. **Complexity** is the large one: the default of 9 costs 3.4x complexity 0 in SILK and 2.6x in CELT, and 8 through 10 are within noise of each other, so everything worth saving is below 8. **Frame size** is the other: per-packet overhead is fixed, so 2.5 ms frames encode at 125x where 20 ms and longer sit at around 200x.

### Measuring it yourself

`cargo bench` measures encode and decode throughput across the three coding modes, all nine frame sizes and the complexity range. Like the crate, the harness has no dependencies, and the audio comes from the same bit-identical generators the tests use, so a number measured on one machine means the same thing on another. Every case names the mode it exists to exercise and the table reports the mode the encoder actually chose, so a row cannot quietly start measuring something other than what it is named for.

Speed is not pinned the way the bitstream is, but it can be compared against a recorded run:

```bash
cargo bench -- --save before.tsv
cargo bench -- --compare before.tsv     # after a change; positive means slower
```

Repeated runs at the default settings differ by 0.7% on average and 3% at worst, so a smaller change than that is noise, and `--reps` buys a tighter number. CI compiles the benchmark on all four platforms so it cannot rot, and times nothing there: shared runners vary far too much for the result to mean anything.

Those numbers, how the comparison is kept fair, why libopus needs two columns rather than one, and what the decode profiling turned up are in [`reference/speed/`](reference/speed/), which also runs it. The libopus comparison is held to the same methodology, transcribed rather than reimplemented, and checked against `cargo bench` to about 1% so it cannot drift from the instrument it mirrors. Reporting delivered bitrate beside the speed is what caught the hybrid rate split it describes: this crate was handing SILK a stereo packet's whole rate where libopus allocates per channel, and starving the CELT high band, which cost stereo hybrid 1.66 dB while spending more bits than the reference.

## Known limitations

- **The decoder's float output is not bounded by ±1**, matching libopus: codec ringing carries samples slightly past it. `decode_s16` handles this for you, and `SoftClip` is there for callers who want the float output and convert it themselves. Only a caller who takes the float and ignores both needs to do anything.
- **Chunk-parallel encoding is not the serial encode.** `encode_parallel` splits a clip across threads and primes each worker by re-encoding the audio before its chunk, which converges the encoder's state but never exactly: at a chunk boundary one packet was produced by an encoder that did not produce the packet before it, and the rate controllers on either side hold different state. Fully primed, the worst frame lands about 4 dB below a serial encode's; constant bitrate removes nearly all of it. Priming is also redundant work, so the worker count is capped at one per 8 s of audio — `ParallelConfig::plan` reports both before any encoding happens. It is an opt-in path for that reason. See [reference/parallel/](reference/parallel/).
- **The Ogg reader seeks only to the start.** It reads forward from the first packet, and `rewind` goes back to it cheaply, but seeking to an arbitrary point is not offered at all, rather than offered badly: RFC 7845 §4.2 wants roughly 80 ms decoded and discarded before the target, and a seek without that pre-roll lands on a cold decoder and audibly clicks, which is exactly the artifact a seeking player is trying to avoid.
- **CELT concealment is not bit-exact**, and cannot be: it extrapolates the last pitch period through a 24th-order LPC fit, and that fit turns the last-bit differences in a 1024-sample autocorrelation into coefficient differences a thousand times larger. Concealed CELT frames agree with libopus to 83–112 dB where an ordinary CELT frame agrees to 139. Concealment also feeds the 5 ms cross-fade at a mode switch, so a stream that changes mode differs there by the same amount.

## License

BSD-3-Clause. See [LICENSE](LICENSE) and [ATTRIBUTION.md](ATTRIBUTION.md).

# Changelog

Notable changes to this crate, newest first. Nothing is recorded here from before the first public release; what this crate changed relative to the fork it came from is described in [ATTRIBUTION.md](ATTRIBUTION.md).

## 0.2.2 — 2026-09-28

- **Reading and decoding a stream no longer allocate once warm**, including at a loop point. The range decoder borrows the packet instead of copying it, tonality analysis keeps its downmix scratch in the encoder state, the multistream decoder rebuilds each stream's packet into one reused buffer, a packet's frame table is held inline, and the Ogg reader reassembles packets in one reused buffer. `tests/ogg_playback_alloc.rs` holds a looping playback to zero allocations after its first pass.
- **`OggOpusReader::read_packet_into`** fills an `OggPacket` the caller keeps, where `read_packet` returns a new one. `OggPacket::default()` is the empty one to start from.
- **`OggOpusReader::rewind`** goes back to the first audio packet of a source that can seek, without reading the header pages again. It replaces rebuilding the reader to loop.
- **`OpusDecoder::reset_state` resets in place** rather than building a new decoder, so it no longer allocates. It still decodes exactly as a new decoder does.
- **The Ogg reader refuses a stream whose comment header does not finish its page**, as RFC 7845 §3 requires and libopusfile enforces. That page boundary is where `rewind` returns to.
- **The Ogg reader caps a reassembled packet at 16 MiB.** A hostile chain of continued pages could previously grow the reader's buffer without limit; it is now refused as an invalid stream.
- `RangeCoder::shrink` checks its size preconditions in release builds.

## 0.2.1 — 2026-08-31

- **`Trim::keep_range`** returns the same cut as `keep`, as indices into the decoded PCM, for a playback path that has to hold its position across buffer fills — a borrowed slice cannot, and the trimmed length alone does not say where the audio starts. The README shows the pattern.

## 0.2.0 — 2026-08-31

- **`Trim` applies RFC 7845's pre-skip and end-trim to a decoded stream.** The documented decode recipe took only the pre-skip, leaving up to a frame of padding past the end of the audio on any file that carries an end-trim — which every `opusenc` file does. README, crate docs and `examples/decode.rs` now use it.
- **The documented encode recipe writes a gapless file.** It flushes the encoder's delay and states the final granule with `write_packet_with_duration`, which existed but appeared in no example. Previously the last few milliseconds of a clip never left the encoder. `tests/ogg_gapless.rs` pins the round trip at every rate.
- `OpusHead::decoder` builds a decoder carrying the header's channel count and output gain, the one of the two that is silent when it is missed.
- `MAX_PACKET_SAMPLES` sizes a decode buffer, the companion to `MAX_PACKET_BYTES`.
- `OggOpusWriter::granule`, so stating an end-trim is a subtraction rather than a derivation from a frame count.
- `OggPacket::new`, so code that consumes packets can be tested without muxing a stream.
- `OpusMSDecoder::streams` and `streams_mut`, mirroring the encoder. Without them a surround stream's declared output gain could not be applied at all, which RFC 7845 §5.1 asks a player to do whatever the mapping family.
- The reader documents rewinding for playback loops. There is still no seek.

## 0.1.0 — 2026-08-26

First release.

- Opus encoder and decoder (RFC 6716) covering all three coding modes — SILK, CELT and the hybrid of both — at 8, 12, 16, 24 and 48 kHz, mono and stereo, at every one of the nine Opus frame sizes.
- Ogg mux and demux (RFC 7845), so the crate reads and writes `.opus` files rather than only raw packets: `OpusHead`, `OpusTags`, page CRCs and granule positions.
- Multistream encoding and decoding, a repacketizer, packet inspection without decoding, and parallel encoding.
- No C, no FFI, no `build.rs` and no dependencies. Requires Rust 1.88.
